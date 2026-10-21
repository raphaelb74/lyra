use std::fmt;

use format_dsd::DsdLayout;
use format_pcm::SampleEncoding;

/// What the source produces. The device must take it exactly: no rate,
/// channel or sample conversion, no DSD→PCM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamFormat {
    Pcm {
        sample_rate: u32,
        channels: usize,
        /// Significant bits per sample; the layout must hold all of them.
        bits_per_sample: u32,
    },
    Dsd {
        /// DSD bits per second per channel.
        dsd_rate: u32,
        channels: usize,
        transport: DsdTransport,
    },
}

/// How DSD may reach the DAC. Native and DoP are detected separately; there
/// is never a fallback to PCM.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DsdTransport {
    /// Native DSD when the device exposes it, otherwise DoP.
    #[default]
    Auto,
    Native,
    Dop,
}

/// How the audio reaches the DAC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Pcm,
    /// DSD in 24-bit PCM frames with alternating markers.
    Dop,
    /// DSD in an ALSA DSD format.
    Native,
}

/// The byte layout written to the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Pcm(SampleEncoding),
    /// DoP: 24 significant bits in a PCM layout.
    Dop(SampleEncoding),
    Dsd(DsdLayout),
}

impl Encoding {
    pub fn transport(self) -> Transport {
        match self {
            Self::Pcm(_) => Transport::Pcm,
            Self::Dop(_) => Transport::Dop,
            Self::Dsd(_) => Transport::Native,
        }
    }

    pub fn frame_bytes(self, channels: usize) -> usize {
        channels
            * match self {
                Self::Pcm(layout) | Self::Dop(layout) => layout.bytes(),
                Self::Dsd(layout) => layout.bytes(),
            }
    }
}

/// The exact format the DAC was opened with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputFormat {
    pub encoding: Encoding,
    /// Device frames per second.
    pub sample_rate: u32,
    pub channels: usize,
    pub hardware_volume: HardwareVolume,
}

/// State of the device's own volume controls while Lyra plays.
///
/// USB DACs often expose a volume the firmware applies digitally: anything
/// but 0 dB changes the samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HardwareVolume {
    /// No playback volume control.
    None,
    /// Every playback volume control is at 0 dB.
    Unity,
    /// Attenuated: output is not bit-perfect if the device applies it
    /// digitally. Lowest setting, in hundredths of a dB.
    Attenuated { centibels: i64 },
    /// Controls without dB information; cannot tell.
    Unknown,
}

impl StreamFormat {
    pub fn channels(&self) -> usize {
        match *self {
            Self::Pcm { channels, .. } | Self::Dsd { channels, .. } => channels,
        }
    }

    /// Every exact way to send this stream, preferred first, as (layout,
    /// device rate). PCM: the lossless layouts at the source rate. DSD:
    /// native layouts, then DoP at DSD rate / 16, as the transport allows.
    pub fn candidates(&self) -> Vec<(Encoding, u32)> {
        match *self {
            Self::Pcm {
                sample_rate,
                bits_per_sample,
                ..
            } => SampleEncoding::lossless_for(bits_per_sample)
                .map(|layout| (Encoding::Pcm(layout), sample_rate))
                .collect(),
            Self::Dsd {
                dsd_rate,
                transport,
                ..
            } => {
                let native = DsdLayout::ALL
                    .into_iter()
                    .map(|layout| (Encoding::Dsd(layout), layout.frame_rate(dsd_rate)));
                let dop = SampleEncoding::lossless_for(24)
                    .map(|layout| (Encoding::Dop(layout), dsd_rate / 16));
                match transport {
                    DsdTransport::Auto => native.chain(dop).collect(),
                    DsdTransport::Native => native.collect(),
                    DsdTransport::Dop => dop.collect(),
                }
            }
        }
    }
}

/// For error messages: `24-bit PCM at 96000 Hz`, `DSD256 as native DSD or as
/// DoP at 705600 Hz`.
impl fmt::Display for StreamFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Pcm {
                sample_rate,
                bits_per_sample,
                ..
            } => write!(f, "{bits_per_sample}-bit PCM at {sample_rate} Hz"),
            Self::Dsd {
                dsd_rate,
                transport,
                ..
            } => {
                let dsd = format!("DSD{}", format_dsd::multiple(dsd_rate));
                let dop = dsd_rate / 16;
                match transport {
                    DsdTransport::Auto => write!(f, "{dsd} as native DSD or as DoP at {dop} Hz"),
                    DsdTransport::Native => write!(f, "{dsd} as native DSD"),
                    DsdTransport::Dop => write!(f, "{dsd} as DoP at {dop} Hz"),
                }
            }
        }
    }
}

impl OutputFormat {
    /// Whether a device opened in this format can play `stream` exactly as
    /// is: the condition for continuing into the next track without
    /// reopening (gapless).
    pub fn carries(&self, stream: &StreamFormat) -> bool {
        self.channels == stream.channels()
            && stream
                .candidates()
                .contains(&(self.encoding, self.sample_rate))
    }

    /// The first candidate the device accepts (`accepts(layout, rate)`).
    /// `None` means the stream cannot be played exactly: no rate, channel
    /// or format change is ever made.
    pub fn negotiate(
        stream: &StreamFormat,
        mut accepts: impl FnMut(Encoding, u32) -> bool,
    ) -> Option<Self> {
        stream
            .candidates()
            .into_iter()
            .find(|&(encoding, rate)| accepts(encoding, rate))
            .map(|(encoding, sample_rate)| Self {
                encoding,
                sample_rate,
                channels: stream.channels(),
                hardware_volume: HardwareVolume::None,
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SinkErrorKind {
    /// Another program holds the device.
    Busy,
    /// The device does not exist or was unplugged.
    NotFound,
    /// The device cannot take this stream without conversion.
    Unsupported,
    /// Any other output failure.
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SinkError {
    pub kind: SinkErrorKind,
    /// Complete, user-facing sentence naming the device.
    pub message: String,
}

impl SinkError {
    pub fn new(kind: SinkErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for SinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SinkError {}

/// An audio output. Implementations write bytes unchanged to the device.
///
/// All methods are called from the single writer thread.
pub trait AudioSink: Send + 'static {
    /// Opens `device` for exactly this stream (see [`OutputFormat::negotiate`]),
    /// closing any previous stream. Fails with [`SinkErrorKind::Unsupported`]
    /// instead of adapting.
    fn open(&mut self, device: &str, stream: &StreamFormat) -> Result<OutputFormat, SinkError>;

    /// Writes interleaved frames in the opened format; returns once all of
    /// `bytes` is queued on the device.
    fn write(&mut self, bytes: &[u8]) -> Result<(), SinkError>;

    /// Pauses or resumes output without losing queued audio where the device
    /// allows it.
    fn set_paused(&mut self, paused: bool) -> Result<(), SinkError>;

    /// Drops queued audio (after a seek); the device stays open.
    fn discard(&mut self) -> Result<(), SinkError>;

    /// Waits until queued audio has played.
    fn drain(&mut self) -> Result<(), SinkError>;

    /// Releases the device so other programs can use it.
    fn close(&mut self);
}

#[cfg(test)]
mod tests {
    use format_dsd::DsdLayout;
    use format_pcm::SampleEncoding;

    use super::{DsdTransport, Encoding, OutputFormat, StreamFormat};

    fn dsd64(transport: DsdTransport) -> StreamFormat {
        StreamFormat::Dsd {
            dsd_rate: 2_822_400,
            channels: 2,
            transport,
        }
    }

    /// A device exposing `DSD_U32_BE` and 24-bit PCM, like the ZEN ONE.
    fn zen_one(encoding: Encoding, rate: u32) -> bool {
        matches!(
            (encoding, rate),
            (Encoding::Dsd(DsdLayout::U32Be), 88_200)
                | (Encoding::Dop(SampleEncoding::S24In3Le), 176_400)
        )
    }

    #[test]
    fn dsd_transport_policy_orders_native_before_dop() {
        let chosen = |transport| {
            OutputFormat::negotiate(&dsd64(transport), zen_one)
                .map(|output| (output.encoding, output.sample_rate))
        };
        assert_eq!(
            chosen(DsdTransport::Auto),
            Some((Encoding::Dsd(DsdLayout::U32Be), 88_200))
        );
        assert_eq!(
            chosen(DsdTransport::Native),
            Some((Encoding::Dsd(DsdLayout::U32Be), 88_200))
        );
        assert_eq!(
            chosen(DsdTransport::Dop),
            Some((Encoding::Dop(SampleEncoding::S24In3Le), 176_400))
        );
    }

    #[test]
    fn an_open_device_carries_only_exact_continuations() {
        let pcm = |rate, bits| StreamFormat::Pcm {
            sample_rate: rate,
            channels: 2,
            bits_per_sample: bits,
        };
        let s24 = OutputFormat::negotiate(&pcm(96_000, 24), |_, _| true).unwrap();
        // 16-bit fits a 24-bit layout losslessly; rate and depth may not grow.
        assert!(s24.carries(&pcm(96_000, 16)));
        assert!(!s24.carries(&pcm(44_100, 24)));
        assert!(!s24.carries(&pcm(96_000, 32)));
        assert!(!s24.carries(&dsd64(DsdTransport::Auto)));

        let native = OutputFormat::negotiate(&dsd64(DsdTransport::Auto), zen_one).unwrap();
        assert!(native.carries(&dsd64(DsdTransport::Native)));
        assert!(!native.carries(&dsd64(DsdTransport::Dop)));
    }

    #[test]
    fn dsd_never_falls_back_to_pcm() {
        let pcm_only = |encoding, _| matches!(encoding, Encoding::Pcm(_));
        assert_eq!(
            OutputFormat::negotiate(&dsd64(DsdTransport::Auto), pcm_only),
            None
        );
        assert_eq!(
            dsd64(DsdTransport::Auto).to_string(),
            "DSD64 as native DSD or as DoP at 176400 Hz"
        );
    }
}
