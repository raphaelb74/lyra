//! What the controller plays: PCM decoded from WAV/FLAC, or DSD from DSF/DFF,
//! packed into the byte layout the device was opened with.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use dsd_pcm::Converter;
use format_dsd::dop::DopPacker;
use format_dsd::{DsdInfo, DsdReader, NativePacker};
use format_pcm::{DecodeError, Decoder, StreamInfo};

use crate::player::ConversionStats;
use crate::readahead::{Progress, ReadAhead};
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
                dop_confirmed: dsd.dop_confirmed,
            },
        }
    }
}

/// What [`Source::read_ahead`] reports.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ahead {
    /// The source frame up to which the file is in memory (approximate:
    /// byte offsets map to frames at the average rate).
    pub buffered_frame: Option<u64>,
    /// The rest of the file is in memory: opening the next one now costs
    /// this one nothing.
    pub complete: bool,
}

/// A file being decoded, read ahead on its own thread.
pub(crate) struct Source {
    decode: Decode,
    file: Progress,
    /// Where the audio data starts, about: the position after the headers.
    data_start: u64,
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source")
            .field("info", &self.info())
            .finish_non_exhaustive()
    }
}

enum Decode {
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
    /// Opens by content: DSF/DFF by their first bytes, otherwise WAV/FLAC.
    pub fn open(path: &Path) -> Result<Self, PlayerError> {
        let (mut file, progress) = ReadAhead::open(path).map_err(DecodeError::Io)?;
        let mut magic = [0; 4];
        let dsd = file.read_exact(&mut magic).is_ok() && format_dsd::is_dsd(&magic);
        file.seek(SeekFrom::Start(0)).map_err(DecodeError::Io)?;
        let decode = if dsd {
            Decode::Dsd {
                reader: DsdReader::from_reader(Box::new(file))?,
                dop: DopPacker::new(),
                native: NativePacker::new(),
                converter: None,
                bytes: Vec::new(),
                samples: Vec::new(),
                finished: false,
            }
        } else {
            let extension = path.extension().and_then(|extension| extension.to_str());
            Decode::Pcm {
                decoder: Decoder::from_reader(file, extension)?,
                samples: Vec::new(),
            }
        };
        let source = Self {
            decode,
            data_start: progress.window().pos,
            file: progress,
        };
        if let Some(rate) = source.byte_rate() {
            source.file.set_rate(rate);
        }
        Ok(source)
    }

    /// Average bytes per second of the audio data.
    fn byte_rate(&self) -> Option<u64> {
        let info = self.info();
        let frames = info.total_frames().filter(|&frames| frames > 0)?;
        let data = self.file.window().len.saturating_sub(self.data_start);
        let rate = u128::from(data) * u128::from(info.frame_rate()) / u128::from(frames);
        u64::try_from(rate).ok()
    }

    /// How far the file is in memory, from one look at the window.
    pub fn read_ahead(&self) -> Ahead {
        let window = self.file.window();
        let buffered_frame = self.info().total_frames().and_then(|total| {
            if window.complete {
                return Some(total);
            }
            let data = window
                .len
                .checked_sub(self.data_start)
                .filter(|&data| data > 0)?;
            let read = window.end.saturating_sub(self.data_start).min(data);
            u64::try_from(u128::from(total) * u128::from(read) / u128::from(data)).ok()
        });
        Ahead {
            buffered_frame,
            complete: window.complete,
        }
    }

    /// The decoder has read the file to its end. Symphonia's FLAC reader
    /// cannot seek back reliably from there (it fails with an unexpected
    /// end of file), so such a source is opened again instead.
    pub fn read_to_end(&self) -> bool {
        let window = self.file.window();
        window.pos >= window.len
    }

    /// What the converter has done so far, when this source is converted.
    pub fn conversion(&self) -> Option<ConversionStats> {
        match &self.decode {
            Decode::Dsd {
                converter: Some(converter),
                ..
            } => Some(ConversionStats {
                clipped: converter.clipped(),
                peak: converter.peak(),
            }),
            _ => None,
        }
    }

    pub fn info(&self) -> SourceInfo {
        match &self.decode {
            Decode::Pcm { decoder, .. } => SourceInfo::Pcm(*decoder.info()),
            Decode::Dsd { reader, .. } => SourceInfo::Dsd(*reader.info()),
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
        match &mut self.decode {
            Decode::Pcm { decoder, samples } => {
                let Encoding::Pcm(layout) = encoding else {
                    return Err(mismatch());
                };
                let more = decoder.decode_next(samples)?;
                layout.pack(samples, out);
                let frames = samples.len() / decoder.info().channels;
                Ok(more.then_some(frames as u64))
            }
            Decode::Dsd {
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
                                        f64::from(dsd.conversion_gain_db()),
                                    )
                                    .ok_or_else(no_converter)?,
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
            Decode::Dsd {
                reader,
                dop,
                converter,
                ..
            },
            Decode::Dsd {
                dop: before,
                converter: running,
                ..
            },
        ) = (&mut self.decode, &mut previous.decode)
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
            Decode::Dsd {
                converter: Some(converter),
                samples,
                ..
            },
            Encoding::Converted(layout),
        ) = (&mut self.decode, encoding)
        {
            samples.clear();
            converter.finish(samples);
            converter.reset();
            layout.pack(samples, out);
        }
    }

    pub fn seek(&mut self, frame: u64) -> Result<(), PlayerError> {
        match &mut self.decode {
            Decode::Pcm { decoder, .. } => Ok(decoder.seek(frame)?),
            Decode::Dsd {
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

/// Conversion was negotiated for a rate the converter does not take;
/// negotiation only offers the rates it does.
fn no_converter() -> PlayerError {
    PlayerError {
        kind: ErrorKind::Internal,
        message: "DSD to PCM conversion does not support this DSD rate".into(),
    }
}

/// The negotiated layout cannot carry this source; negotiation prevents it.
fn mismatch() -> PlayerError {
    PlayerError {
        kind: ErrorKind::Internal,
        message: "the output format does not match the source".into(),
    }
}
