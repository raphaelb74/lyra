use std::fmt;
use std::path::PathBuf;

use format_dsd::DsdError;
use format_pcm::DecodeError;

use crate::dop;
use crate::order::{Repeat, following};
use crate::sink::{DsdPolicy, Encoding, HardwareVolume, OutputFormat, SinkError, SinkErrorKind};
use crate::source::SourceInfo;

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

impl From<DsdError> for PlayerError {
    fn from(error: DsdError) -> Self {
        let kind = match error {
            DsdError::Unsupported(_) => ErrorKind::Unsupported,
            DsdError::Io(_)
            | DsdError::NotDsd
            | DsdError::Malformed(_)
            | DsdError::SeekOutOfRange => ErrorKind::InvalidFile,
        };
        Self {
            kind,
            message: error.to_string(),
        }
    }
}

/// The track being played and how it reaches the DAC: its signal path is
/// `source`, then [`Self::processing`], then `output` (with the device's own
/// volume), then the [`Self::bit_perfect`] verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NowPlaying {
    pub path: PathBuf,
    /// The caller's id for this queue entry (e.g. a library track id).
    pub id: Option<i64>,
    /// Position in the queue.
    pub index: usize,
    pub device: String,
    pub source: SourceInfo,
    /// Set once the device accepted the stream.
    pub output: Option<OutputFormat>,
    /// The DSD settings this track plays under (conversion gain).
    pub dsd: DsdPolicy,
}

/// What UNDA does to the samples between the file and the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Processing {
    /// PCM samples sent in a wider sample format, the extra low bits zero:
    /// lossless.
    Padded { from_bits: u32, to_bits: u32 },
    /// DSD packed into PCM frames with DoP markers: lossless for a DAC that
    /// decodes DoP.
    DopPacked,
    /// DSD converted to PCM (low-pass filter and decimation), with this gain
    /// in dB: not bit-perfect.
    Converted { gain_db: i8 },
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
    /// What happens to the samples on their way to the device; empty when
    /// they go as read, or while the device opens.
    pub fn processing(&self) -> Vec<Processing> {
        let Some(output) = self.output else {
            return Vec::new();
        };
        let step = match (self.source, output.encoding) {
            (SourceInfo::Pcm(info), Encoding::Pcm(layout))
                if layout.bits() > info.bits_per_sample =>
            {
                Some(Processing::Padded {
                    from_bits: info.bits_per_sample,
                    to_bits: layout.bits(),
                })
            }
            (_, Encoding::Dop(_)) => Some(Processing::DopPacked),
            (_, Encoding::Converted(_)) => Some(Processing::Converted {
                gain_db: self.dsd.conversion_gain_db(),
            }),
            _ => None,
        };
        step.into_iter().collect()
    }

    /// Verdict for the current output. The format path is exact by
    /// construction (same rate and channels, lossless layout, or the device
    /// is refused), so the remaining question is the device's own volume.
    pub fn bit_perfect(&self) -> Option<BitPerfect> {
        let output = self.output?;
        if let Encoding::Converted(_) = output.encoding {
            return Some(BitPerfect::No {
                reason: format!(
                    "DSD is converted to PCM at {} Hz (conversion is on in Settings)",
                    output.sample_rate
                ),
            });
        }
        let exact = output.channels == self.source.channels()
            && match (self.source, output.encoding) {
                (SourceInfo::Pcm(info), Encoding::Pcm(layout)) => {
                    output.sample_rate == info.sample_rate && layout.bits() >= info.bits_per_sample
                }
                (SourceInfo::Dsd(info), Encoding::Dop(layout)) => {
                    output.sample_rate == info.dsd_rate / 16 && layout.bits() >= 24
                }
                (SourceInfo::Dsd(info), Encoding::Dsd(layout)) => {
                    output.sample_rate == layout.frame_rate(info.dsd_rate)
                }
                _ => false,
            };
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

/// `−6.01` for −601 centibels.
pub(crate) fn format_db(centibels: i64) -> String {
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
    /// Entries in the queue ([`crate::Player::queue`] lists them).
    pub queue_length: usize,
    /// The queue plays in shuffled order.
    pub shuffle: bool,
    pub repeat: Repeat,
    /// Changes whenever the queue does (order, entries), so a view of it
    /// knows to reload.
    pub queue_revision: u64,
}

impl PlayerStatus {
    /// Why `device`'s volume must not change now, if it must not: DoP is
    /// playing on it (see [`crate::dop`]).
    pub fn volume_lock(&self, device: &str) -> Option<&'static str> {
        let track = self.track.as_ref().filter(|track| track.device == device)?;
        let active = !matches!(self.state, PlaybackState::Stopped | PlaybackState::Error);
        (active && dop::is_dop(&track.output?)).then_some(dop::VOLUME_LOCKED)
    }

    /// Next has somewhere to go: a later entry, or the start again when
    /// repeating.
    pub fn has_next(&self) -> bool {
        self.track.as_ref().is_some_and(|track| {
            following(track.index, self.queue_length, self.repeat, true).is_some()
        })
    }

    pub(crate) const STOPPED: Self = Self {
        state: PlaybackState::Stopped,
        track: None,
        error: None,
        queue_length: 0,
        shuffle: false,
        repeat: Repeat::Off,
        queue_revision: 0,
    };
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use format_pcm::{Codec, SampleEncoding, StreamInfo};

    use super::{BitPerfect, NowPlaying, PlaybackState, PlayerStatus, Processing, Repeat};
    use crate::sink::{DsdFallback, DsdPolicy, Encoding, HardwareVolume, OutputFormat};
    use crate::source::SourceInfo;

    fn playing(volume: HardwareVolume) -> NowPlaying {
        let source = StreamInfo {
            codec: Codec::Flac,
            sample_rate: 96_000,
            channels: 2,
            bits_per_sample: 24,
            total_frames: None,
        };
        let output = OutputFormat {
            encoding: Encoding::Pcm(SampleEncoding::S24In3Le),
            sample_rate: 96_000,
            channels: 2,
            hardware_volume: volume,
        };
        NowPlaying {
            path: PathBuf::new(),
            id: None,
            index: 0,
            device: String::new(),
            source: SourceInfo::Pcm(source),
            output: Some(output),
            dsd: DsdPolicy::default(),
        }
    }

    #[test]
    fn processing_names_what_changes_on_the_way() {
        let with = |bits, encoding| {
            let mut track = playing(HardwareVolume::Unity);
            if let SourceInfo::Pcm(info) = &mut track.source {
                info.bits_per_sample = bits;
            }
            if let Some(output) = &mut track.output {
                output.encoding = encoding;
            }
            track.processing()
        };
        assert_eq!(with(24, Encoding::Pcm(SampleEncoding::S24In3Le)), []);
        assert_eq!(
            with(16, Encoding::Pcm(SampleEncoding::S32Le)),
            [Processing::Padded {
                from_bits: 16,
                to_bits: 32
            }]
        );
        assert_eq!(
            with(1, Encoding::Dop(SampleEncoding::S24In3Le)),
            [Processing::DopPacked]
        );

        let mut converted = playing(HardwareVolume::None);
        converted.dsd.fallback = DsdFallback::Convert { gain_db: 6 };
        if let Some(output) = &mut converted.output {
            output.encoding = Encoding::Converted(SampleEncoding::S32Le);
        }
        assert_eq!(
            converted.processing(),
            [Processing::Converted { gain_db: 6 }]
        );

        converted.output = None;
        assert_eq!(converted.processing(), [], "nothing until the device opens");
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

    #[test]
    fn volume_is_locked_only_while_dop_plays() {
        let status = |state, encoding| {
            let mut track = playing(HardwareVolume::Unity);
            if let Some(output) = &mut track.output {
                output.encoding = encoding;
            }
            PlayerStatus {
                state,
                track: Some(track),
                error: None,
                queue_length: 1,
                shuffle: false,
                repeat: Repeat::Off,
                queue_revision: 0,
            }
        };
        let dop = Encoding::Dop(SampleEncoding::S24In3Le);
        let pcm = Encoding::Pcm(SampleEncoding::S24In3Le);
        let here = "";
        assert!(
            status(PlaybackState::Playing, dop)
                .volume_lock(here)
                .is_some()
        );
        assert!(
            status(PlaybackState::Paused, dop)
                .volume_lock(here)
                .is_some()
        );
        assert_eq!(status(PlaybackState::Stopped, dop).volume_lock(here), None);
        assert_eq!(status(PlaybackState::Playing, pcm).volume_lock(here), None);
        // Another device is not locked by DoP playing here.
        assert_eq!(
            status(PlaybackState::Playing, dop).volume_lock("hw:2"),
            None
        );
    }
}
