//! What the controller plays: PCM decoded from WAV/FLAC, or DSD from DSF/DFF,
//! packed into the byte layout the device was opened with.

use std::path::Path;

use format_dsd::dop::DopPacker;
use format_dsd::{DsdError, DsdInfo, DsdReader, NativePacker};
use format_pcm::{Decoder, StreamInfo};

use crate::sink::{DsdTransport, Encoding, StreamFormat};
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

    /// The stream the device must accept, exactly. `transport` only applies
    /// to DSD.
    pub fn stream(&self, transport: DsdTransport) -> StreamFormat {
        match self {
            Self::Pcm(info) => StreamFormat::Pcm {
                sample_rate: info.sample_rate,
                channels: info.channels,
                bits_per_sample: info.bits_per_sample,
            },
            Self::Dsd(info) => StreamFormat::Dsd {
                dsd_rate: info.dsd_rate,
                channels: info.channels,
                transport,
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

    /// Replaces `out` with the next audio in `encoding`. Returns the source
    /// frames read for it, or `None` at the end.
    pub fn next(
        &mut self,
        encoding: Encoding,
        out: &mut Vec<u8>,
    ) -> Result<Option<u64>, PlayerError> {
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
                bytes,
                samples,
                finished,
            } => {
                let channels = reader.info().channels;
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
                        Encoding::Pcm(_) => return Err(mismatch()),
                    }
                }
                Ok(Some(frames))
            }
        }
    }

    /// Picks up where `previous` ended on the same device: DoP marker phase
    /// carries over, so a gapless DSD boundary has no marker break.
    pub fn continue_from(&mut self, previous: &Self) {
        if let (Self::Dsd { dop, .. }, Self::Dsd { dop: before, .. }) = (self, previous) {
            dop.continue_from(before);
        }
    }

    pub fn seek(&mut self, frame: u64) -> Result<(), PlayerError> {
        match self {
            Self::Pcm { decoder, .. } => Ok(decoder.seek(frame)?),
            Self::Dsd {
                reader,
                dop,
                native,
                finished,
                ..
            } => {
                reader.seek(frame)?;
                dop.reset();
                native.reset();
                *finished = false;
                Ok(())
            }
        }
    }
}

/// The negotiated layout cannot carry this source; negotiation prevents it.
fn mismatch() -> PlayerError {
    PlayerError {
        kind: ErrorKind::Internal,
        message: "the output format does not match the source".into(),
    }
}
