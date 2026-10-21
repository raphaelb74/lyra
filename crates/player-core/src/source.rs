//! What the controller plays: PCM decoded from WAV/FLAC, or DSD from DSF/DFF,
//! packed into the byte layout the device was opened with.

use std::path::Path;

use dsd_pcm::Converter;
use format_dsd::dop::DopPacker;
use format_dsd::{DsdError, DsdInfo, DsdReader, NativePacker};
use format_pcm::{Decoder, StreamInfo};

use crate::sink::{DsdFallback, DsdPolicy, DsdTransport, Encoding, OutputFormat, StreamFormat};
use crate::status::{ErrorKind, PlayerError};

/// The file being played, for display and policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceInfo {
    Pcm(StreamInfo),
    Dsd(DsdInfo),
}

impl SourceInfo {
    /// Positions and seeks count these: PCM sample frames, or DSD bytes per
    /// channel (8 DSD samples).
    pub fn frame_rate(&self) -> u32 {
        match self {
            Self::Pcm(info) => info.sample_rate,
            Self::Dsd(info) => info.frame_rate(),
        }
    }

    pub fn total_frames(&self) -> Option<u64> {
        match self {
            Self::Pcm(info) => info.total_frames,
            Self::Dsd(info) => Some(info.total_frames),
        }
    }

    pub fn channels(&self) -> usize {
        match self {
            Self::Pcm(info) => info.channels,
            Self::Dsd(info) => info.channels,
        }
    }

    /// The stream the device must accept. `dsd` only applies to DSD.
    pub fn stream(&self, dsd: DsdPolicy) -> StreamFormat {
        match self {
            Self::Pcm(info) => StreamFormat::Pcm {
                sample_rate: info.sample_rate,
                channels: info.channels,
                bits_per_sample: info.bits_per_sample,
            },
            Self::Dsd(info) => StreamFormat::Dsd {
                dsd_rate: info.dsd_rate,
                channels: info.channels,
                transport: dsd.transport,
                convert: dsd.transport == DsdTransport::Auto
                    && matches!(dsd.fallback, DsdFallback::Convert { .. }),
            },
        }
    }
}

pub(crate) enum Source {
    Pcm {
        decoder: Decoder,
        /// Reused decode buffer.
        samples: Vec<i32>,
    },
    Dsd {
        reader: DsdReader,
        dop: DopPacker,
        native: NativePacker,
        /// Made on first use, when the device was opened for conversion.
        converter: Option<Box<Converter>>,
        /// Reused read buffer.
        bytes: Vec<u8>,
        /// Reused DoP sample buffer.
        samples: Vec<i32>,
        finished: bool,
    },
}

impl Source {
    /// Opens by content: DSF/DFF first (cheap magic check), then WAV/FLAC.
    pub fn open(path: &Path) -> Result<Self, PlayerError> {
        match DsdReader::open(path) {
            Ok(reader) => Ok(Self::Dsd {
                reader,
                dop: DopPacker::new(),
                native: NativePacker::new(),
                converter: None,
                bytes: Vec::new(),
                samples: Vec::new(),
                finished: false,
            }),
            Err(DsdError::NotDsd) => Ok(Self::Pcm {
                decoder: Decoder::open(path)?,
                samples: Vec::new(),
            }),
            Err(error) => Err(error.into()),
        }
    }

    pub fn info(&self) -> SourceInfo {
        match self {
            Self::Pcm { decoder, .. } => SourceInfo::Pcm(*decoder.info()),
            Self::Dsd { reader, .. } => SourceInfo::Dsd(*reader.info()),
        }
    }

    /// Replaces `out` with the next audio for `output`. Returns the source
    /// frames read for it, or `None` at the end. Converted DSD keeps the
    /// filters' tail back: see [`Self::tail`].
    pub fn next(
        &mut self,
        output: &OutputFormat,
        dsd: DsdPolicy,
        out: &mut Vec<u8>,
    ) -> Result<Option<u64>, PlayerError> {
        let encoding = output.encoding;
        out.clear();
        match self {
            Self::Pcm { decoder, samples } => {
                let Encoding::Pcm(layout) = encoding else {
                    return Err(mismatch());
                };
                let more = decoder.decode_next(samples)?;
                layout.pack(samples, out);
                let frames = samples.len() / decoder.info().channels;
                Ok(more.then_some(frames as u64))
            }
            Self::Dsd {
                reader,
                dop,
                native,
                converter,
                bytes,
                samples,
                finished,
            } => {
                let info = *reader.info();
                let channels = info.channels;
                let mut frames = 0;
                // A pack can hold back a partial word; read until bytes come out.
                while out.is_empty() {
                    if *finished {
                        return Ok(None);
                    }
                    *finished = !reader.read_next(bytes)?;
                    frames += (bytes.len() / channels) as u64;
                    match encoding {
                        Encoding::Dop(layout) => {
                            samples.clear();
                            if *finished {
                                dop.finish(channels, samples);
                            } else {
                                dop.pack(bytes, channels, samples);
                            }
                            layout.pack(samples, out);
                        }
                        Encoding::Dsd(layout) => {
                            if *finished {
                                native.finish(layout, channels, out);
                            } else {
                                native.pack(layout, bytes, channels, out);
                            }
                        }
                        Encoding::Converted(layout) => {
                            let converter = match converter {
                                Some(converter)
                                    if converter.continues(
                                        info.dsd_rate,
                                        output.sample_rate,
                                        channels,
                                    ) =>
                                {
                                    converter
                                }
                                // Designs the filters: once per track, on
                                // the controller thread.
                                _ => converter.insert(Box::new(
                                    Converter::new(
                                        info.dsd_rate,
                                        output.sample_rate,
                                        channels,
                                        f64::from(gain_db(dsd)),
                                    )
                                    .ok_or_else(mismatch)?,
                                )),
                            };
                            samples.clear();
                            converter.push(bytes, samples);
                            layout.pack(samples, out);
                        }
                        Encoding::Pcm(_) => return Err(mismatch()),
                    }
                }
                Ok(Some(frames))
            }
        }
    }

    /// Picks up where `previous` ended on the same device: DoP marker phase
    /// carries over, so a gapless DSD boundary has no marker break, and so
    /// does the converter's filter state when it fits this track.
    pub fn continue_from(&mut self, previous: &mut Self) {
        if let (
            Self::Dsd {
                reader,
                dop,
                converter,
                ..
            },
            Self::Dsd {
                dop: before,
                converter: running,
                ..
            },
        ) = (self, previous)
        {
            dop.continue_from(before);
            let info = reader.info();
            if running.as_ref().is_some_and(|running| {
                running.continues(info.dsd_rate, running.output_rate(), info.channels)
            }) {
                *converter = running.take();
            }
        }
    }

    /// The end of converted audio still in the filters, for the track that
    /// ended without a converted continuation. Empty otherwise.
    pub fn tail(&mut self, encoding: Encoding, out: &mut Vec<u8>) {
        out.clear();
        if let (
            Self::Dsd {
                converter: Some(converter),
                samples,
                ..
            },
            Encoding::Converted(layout),
        ) = (self, encoding)
        {
            samples.clear();
            converter.finish(samples);
            converter.reset();
            layout.pack(samples, out);
        }
    }

    pub fn seek(&mut self, frame: u64) -> Result<(), PlayerError> {
        match self {
            Self::Pcm { decoder, .. } => Ok(decoder.seek(frame)?),
            Self::Dsd {
                reader,
                dop,
                native,
                converter,
                finished,
                ..
            } => {
                reader.seek(frame)?;
                dop.reset();
                native.reset();
                if let Some(converter) = converter {
                    converter.reset();
                }
                *finished = false;
                Ok(())
            }
        }
    }
}

fn gain_db(dsd: DsdPolicy) -> i8 {
    match dsd.fallback {
        DsdFallback::Convert { gain_db } => gain_db,
        DsdFallback::Refuse => 0,
    }
}

/// The negotiated layout cannot carry this source; negotiation prevents it.
fn mismatch() -> PlayerError {
    PlayerError {
        kind: ErrorKind::Internal,
        message: "the output format does not match the source".into(),
    }
}
