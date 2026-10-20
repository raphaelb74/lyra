//! What the controller plays: PCM decoded from WAV/FLAC, or DSD from
//! DSF/DFF carried as DoP. Both come out as left-justified `i32` samples for
//! the PCM stream the device is opened with.

use std::path::Path;

use format_dsd::dop::{self, DopPacker};
use format_dsd::{DsdError, DsdInfo, DsdReader};
use format_pcm::{Decoder, StreamInfo};

use crate::sink::PcmStream;
use crate::status::PlayerError;

/// The file being played, for display and policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceInfo {
    Pcm(StreamInfo),
    Dsd(DsdInfo),
}

/// How the source reaches the DAC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Pcm,
    /// DSD in 24-bit PCM frames with alternating markers.
    Dop,
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

    pub fn transport(&self) -> Transport {
        match self {
            Self::Pcm(_) => Transport::Pcm,
            Self::Dsd(_) => Transport::Dop,
        }
    }

    /// The PCM stream the device must accept, exactly.
    pub fn pcm_stream(&self) -> PcmStream {
        match self {
            Self::Pcm(info) => PcmStream {
                sample_rate: info.sample_rate,
                channels: info.channels,
                bits_per_sample: info.bits_per_sample,
            },
            Self::Dsd(info) => PcmStream {
                sample_rate: dop::carrier_rate(info),
                channels: info.channels,
                bits_per_sample: 24,
            },
        }
    }
}

pub(crate) enum Source {
    Pcm(Decoder),
    Dop {
        reader: DsdReader,
        packer: DopPacker,
        /// Reused DSD read buffer.
        bytes: Vec<u8>,
        finished: bool,
    },
}

impl Source {
    /// Opens by content: DSF/DFF first (cheap magic check), then WAV/FLAC.
    pub fn open(path: &Path) -> Result<Self, PlayerError> {
        match DsdReader::open(path) {
            Ok(reader) => Ok(Self::Dop {
                reader,
                packer: DopPacker::new(),
                bytes: Vec::new(),
                finished: false,
            }),
            Err(DsdError::NotDsd) => Ok(Self::Pcm(Decoder::open(path)?)),
            Err(error) => Err(error.into()),
        }
    }

    pub fn info(&self) -> SourceInfo {
        match self {
            Self::Pcm(decoder) => SourceInfo::Pcm(*decoder.info()),
            Self::Dop { reader, .. } => SourceInfo::Dsd(*reader.info()),
        }
    }

    /// Replaces `out` with the next samples. Returns the source frames they
    /// cover, or `None` at the end.
    pub fn next(&mut self, out: &mut Vec<i32>) -> Result<Option<u64>, PlayerError> {
        match self {
            Self::Pcm(decoder) => {
                let more = decoder.decode_next(out)?;
                let frames = out.len() / decoder.info().channels;
                Ok(more.then_some(frames as u64))
            }
            Self::Dop {
                reader,
                packer,
                bytes,
                finished,
            } => {
                out.clear();
                let channels = reader.info().channels;
                while out.is_empty() {
                    if *finished {
                        return Ok(None);
                    }
                    if reader.read_next(bytes)? {
                        packer.pack(bytes, channels, out);
                        if !out.is_empty() {
                            return Ok(Some((bytes.len() / channels) as u64));
                        }
                    } else {
                        *finished = true;
                        packer.finish(channels, out);
                    }
                }
                Ok(Some(0))
            }
        }
    }

    pub fn seek(&mut self, frame: u64) -> Result<(), PlayerError> {
        match self {
            Self::Pcm(decoder) => Ok(decoder.seek(frame)?),
            Self::Dop {
                reader,
                packer,
                finished,
                ..
            } => {
                reader.seek(frame)?;
                packer.reset();
                *finished = false;
                Ok(())
            }
        }
    }
}
