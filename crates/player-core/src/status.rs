use std::fmt;
use std::path::PathBuf;

use format_pcm::{DecodeError, StreamInfo};

use crate::sink::{HardwareVolume, OutputFormat, SinkError, SinkErrorKind};

/// Playback states. Every change is published as a [`PlayerStatus`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    /// Opening and probing the file.
    Loading,
    /// Opening the device with the file's exact format.
    Negotiating,
    /// Device open, first audio on its way.
    Buffering,
    Playing,
    Paused,
    Error,
}

/// Machine-readable error category (the IPC `code`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    DeviceBusy,
    DeviceNotFound,
    /// File or device cannot be played without conversion.
    Unsupported,
    InvalidFile,
    /// Output failed while playing (underrun recovery failed, unplugged…).
    Output,
    /// A player thread stopped unexpectedly.
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerError {
    pub kind: ErrorKind,
    pub message: String,
}

impl fmt::Display for PlayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PlayerError {}

impl From<SinkError> for PlayerError {
    fn from(error: SinkError) -> Self {
        let kind = match error.kind {
            SinkErrorKind::Busy => ErrorKind::DeviceBusy,
            SinkErrorKind::NotFound => ErrorKind::DeviceNotFound,
            SinkErrorKind::Unsupported => ErrorKind::Unsupported,
            SinkErrorKind::Failed => ErrorKind::Output,
        };
        Self {
            kind,
            message: error.message,
        }
    }
}

impl From<DecodeError> for PlayerError {
    fn from(error: DecodeError) -> Self {
        let kind = match error {
            DecodeError::Unsupported(_) => ErrorKind::Unsupported,
            DecodeError::Io(_) | DecodeError::Malformed(_) | DecodeError::SeekOutOfRange => {
                ErrorKind::InvalidFile
            }
        };
        Self {
            kind,
            message: error.to_string(),
        }
    }
}

/// The track being played and how it reaches the DAC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NowPlaying {
    pub path: PathBuf,
    pub device: String,
    pub source: StreamInfo,
    /// Set once the device accepted the stream.
    pub output: Option<OutputFormat>,
}

/// Whether the DAC receives the source samples unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BitPerfect {
    Yes,
    /// Something alters the samples.
    No {
        reason: String,
    },
    /// Lyra sends the exact samples but cannot see what the device does.
    Unverified {
        reason: String,
    },
}

impl NowPlaying {
    /// Verdict for the current output. The format path is exact by
    /// construction (same rate and channels, lossless layout, or the device
    /// is refused), so the remaining question is the device's own volume.
    pub fn bit_perfect(&self) -> Option<BitPerfect> {
        let output = self.output?;
        let exact = output.sample_rate == self.source.sample_rate
            && output.channels == self.source.channels
            && output.encoding.bits() >= self.source.bits_per_sample;
        if !exact {
            return Some(BitPerfect::No {
                reason: "output format differs from the source".into(),
            });
        }
        Some(match output.hardware_volume {
            HardwareVolume::None | HardwareVolume::Unity => BitPerfect::Yes,
            HardwareVolume::Attenuated { centibels } => BitPerfect::No {
                reason: format!("DAC volume is {} dB, not 0 dB", format_db(centibels)),
            },
            HardwareVolume::Unknown => BitPerfect::Unverified {
                reason: "the DAC volume level cannot be read".into(),
            },
        })
    }
}

fn format_db(centibels: i64) -> String {
    let sign = if centibels < 0 { "−" } else { "" };
    let magnitude = centibels.unsigned_abs();
    format!("{sign}{}.{:02}", magnitude / 100, magnitude % 100)
}

/// Immutable snapshot published on every state change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerStatus {
    pub state: PlaybackState,
    pub track: Option<NowPlaying>,
    /// Why the state is [`PlaybackState::Error`].
    pub error: Option<PlayerError>,
}

impl PlayerStatus {
    pub(crate) const STOPPED: Self = Self {
        state: PlaybackState::Stopped,
        track: None,
        error: None,
    };
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use format_pcm::{Codec, SampleEncoding, StreamInfo};

    use super::{BitPerfect, NowPlaying};
    use crate::sink::{HardwareVolume, OutputFormat};

    fn playing(volume: HardwareVolume) -> NowPlaying {
        let source = StreamInfo {
            codec: Codec::Flac,
            sample_rate: 96_000,
            channels: 2,
            bits_per_sample: 24,
            total_frames: None,
        };
        let output = OutputFormat {
            encoding: SampleEncoding::S24In3Le,
            sample_rate: 96_000,
            channels: 2,
            hardware_volume: volume,
        };
        NowPlaying {
            path: PathBuf::new(),
            device: String::new(),
            source,
            output: Some(output),
        }
    }

    #[test]
    fn verdict_follows_the_dac_volume() {
        assert_eq!(
            playing(HardwareVolume::Unity).bit_perfect(),
            Some(BitPerfect::Yes)
        );
        assert_eq!(
            playing(HardwareVolume::None).bit_perfect(),
            Some(BitPerfect::Yes)
        );
        assert_eq!(
            playing(HardwareVolume::Attenuated { centibels: -601 }).bit_perfect(),
            Some(BitPerfect::No {
                reason: "DAC volume is −6.01 dB, not 0 dB".into()
            })
        );
        assert!(matches!(
            playing(HardwareVolume::Unknown).bit_perfect(),
            Some(BitPerfect::Unverified { .. })
        ));
    }
}
