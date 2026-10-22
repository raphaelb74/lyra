use std::fmt;

use format_dsd::DsdLayout;
use format_pcm::SampleEncoding;

/// What the source produces. The device must take it exactly: no rate,
/// channel or sample conversion. The one exception is opt-in: DSD converted
/// to PCM on a device without DSD support (`convert`), always reported as
/// [`Transport::Converted`].
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
        /// With [`DsdTransport::Auto`] only: when neither native DSD nor DoP
        /// can be used, convert to PCM.
        convert: bool,
    },
}

/// How DSD may reach the DAC. Native and DoP are detected separately; PCM
/// is never used unless [`DsdFallback::Convert`] allows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DsdTransport {
    /// Native DSD when the device exposes it, otherwise DoP.
    #[default]
    Auto,
    Native,
    Dop,
}

/// The user's DSD settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DsdPolicy {
    pub transport: DsdTransport,
    pub fallback: DsdFallback,
}

/// What to do when a device takes neither native DSD nor DoP (built-in
/// audio, most non-USB outputs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DsdFallback {
    /// Refuse the track and say why. Nothing is converted silently.
    #[default]
    Refuse,
    /// Convert to 24-bit PCM, never bit-perfect, with this gain in dB.
    /// Applies with [`DsdTransport::Auto`] only: an explicit Native or DoP
    /// choice is never overridden.
    Convert { gain_db: i8 },
}

impl DsdPolicy {
    /// The gain conversion applies, in dB; 0 when conversion is off.
    pub fn conversion_gain_db(self) -> i8 {
        match self.fallback {
            DsdFallback::Convert { gain_db } => gain_db,
            DsdFallback::Refuse => 0,
        }
    }
}

/// How the audio reaches the DAC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Pcm,
    /// DSD in 24-bit PCM frames with alternating markers.
    Dop,
    /// DSD in an ALSA DSD format.
    Native,
    /// DSD converted to PCM: not bit-perfect.
    Converted,
}

/// The byte layout written to the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Pcm(SampleEncoding),
    /// DoP: 24 significant bits in a PCM layout.
    Dop(SampleEncoding),
    Dsd(DsdLayout),
    /// DSD converted to 24-bit PCM, in a PCM layout.
    Converted(SampleEncoding),
}

impl Encoding {
    pub fn transport(self) -> Transport {
        match self {
            Self::Pcm(_) => Transport::Pcm,
            Self::Dop(_) => Transport::Dop,
            Self::Dsd(_) => Transport::Native,
            Self::Converted(_) => Transport::Converted,
        }
    }

    /// Appends `frames` frames of silence in this layout: zeros for PCM,
    /// the DSD idle pattern for native DSD, and the idle pattern with DoP
    /// markers (starting at 0x05, as a track does) for DoP. Keep `frames`
    /// even for DoP so the marker phase is unchanged after it.
    pub fn silence(self, channels: usize, frames: usize, out: &mut Vec<u8>) {
        match self {
            Self::Pcm(layout) | Self::Converted(layout) => {
                out.resize(out.len() + frames * channels * layout.bytes(), 0);
            }
            Self::Dsd(layout) => {
                out.resize(
                    out.len() + frames * channels * layout.bytes(),
                    format_dsd::SILENCE,
                );
            }
            Self::Dop(layout) => {
                let idle = format_dsd::SILENCE;
                let mut samples = Vec::with_capacity(frames * channels);
                for frame in 0..frames {
                    let marker = if frame % 2 == 0 { 0x05 } else { 0xFA };
                    // Left-justified 24-bit: marker, then two DSD bytes.
                    let sample = i32::from_be_bytes([marker, idle, idle, 0]);
                    samples.extend(std::iter::repeat_n(sample, channels));
                }
                layout.pack(&samples, out);
            }
        }
    }

    pub fn frame_bytes(self, channels: usize) -> usize {
        channels
            * match self {
                Self::Pcm(layout) | Self::Dop(layout) | Self::Converted(layout) => layout.bytes(),
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
    /// digitally. Total of the controls in series (each at its lowest
    /// channel), in hundredths of a dB.
    Attenuated { centibels: i64 },
    /// Controls without dB information; cannot tell.
    Unknown,
}

impl HardwareVolume {
    /// Whether DoP arrives intact: the device's volume is known to leave the
    /// samples alone. Otherwise a device that does not decode DoP plays it
    /// as loud noise, and attenuation breaks the markers.
    pub fn passes_dop(self) -> bool {
        matches!(self, Self::None | Self::Unity)
    }
}

impl StreamFormat {
    pub fn channels(&self) -> usize {
        match *self {
            Self::Pcm { channels, .. } | Self::Dsd { channels, .. } => channels,
        }
    }

    /// Every way to send this stream, preferred first, as (layout, device
    /// rate). PCM: the lossless layouts at the source rate. DSD: native
    /// layouts, then DoP at DSD rate / 16, as the transport allows; then, if
    /// allowed, conversion to PCM, highest rate first.
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
                convert,
                ..
            } => {
                let native = DsdLayout::ALL
                    .into_iter()
                    .map(|layout| (Encoding::Dsd(layout), layout.frame_rate(dsd_rate)));
                let dop = SampleEncoding::lossless_for(24)
                    .map(|layout| (Encoding::Dop(layout), dsd_rate / 16));
                let converted = dsd_pcm::output_rates(dsd_rate)
                    .iter()
                    .filter(move |_| convert)
                    .flat_map(|&rate| {
                        SampleEncoding::lossless_for(24)
                            .map(move |layout| (Encoding::Converted(layout), rate))
                    });
                match transport {
                    DsdTransport::Auto => native.chain(dop).chain(converted).collect(),
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
                convert,
                ..
            } => {
                let dsd = format!("DSD{}", format_dsd::multiple(dsd_rate));
                let dop = dsd_rate / 16;
                match transport {
                    DsdTransport::Auto if convert => write!(
                        f,
                        "{dsd} as native DSD, as DoP at {dop} Hz, or converted to PCM at {}",
                        rates(dsd_pcm::output_rates(dsd_rate))
                    ),
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

    /// The first candidate the device accepts (`accepts(layout, rate)`) on
    /// a device whose volume is `volume`. `None` means the stream cannot be
    /// played: no rate, channel or format change is ever made beyond the
    /// opt-in DSD conversion.
    ///
    /// When conversion is allowed, DoP is skipped on a device that may not
    /// pass it intact (built-in audio accepts the DoP format but would play
    /// noise), so conversion is reached. Without conversion DoP stays, and
    /// the player refuses it with the reason.
    pub fn negotiate(
        stream: &StreamFormat,
        volume: HardwareVolume,
        mut accepts: impl FnMut(Encoding, u32) -> bool,
    ) -> Option<Self> {
        let candidates = stream.candidates();
        let converts = candidates
            .iter()
            .any(|(encoding, _)| matches!(encoding, Encoding::Converted(_)));
        candidates
            .into_iter()
            .filter(|(encoding, _)| {
                !(converts && matches!(encoding, Encoding::Dop(_)) && !volume.passes_dop())
            })
            .find(|&(encoding, rate)| accepts(encoding, rate))
            .map(|(encoding, sample_rate)| Self {
                encoding,
                sample_rate,
                channels: stream.channels(),
                hardware_volume: volume,
            })
    }
}

/// `88200 or 44100 Hz`.
fn rates(rates: &[u32]) -> String {
    let names: Vec<String> = rates.iter().map(u32::to_string).collect();
    format!("{} Hz", names.join(" or "))
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

    use super::{DsdTransport, Encoding, HardwareVolume, OutputFormat, StreamFormat};

    fn dsd64(transport: DsdTransport, convert: bool) -> StreamFormat {
        StreamFormat::Dsd {
            dsd_rate: 2_822_400,
            channels: 2,
            transport,
            convert,
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

    /// Built-in audio: 32-bit PCM at the usual rates, no DSD format.
    fn built_in(encoding: Encoding, rate: u32) -> bool {
        matches!(
            encoding,
            Encoding::Pcm(SampleEncoding::S32Le)
                | Encoding::Dop(SampleEncoding::S32Le)
                | Encoding::Converted(SampleEncoding::S32Le)
        ) && [44_100, 48_000, 88_200, 96_000, 176_400, 192_000].contains(&rate)
    }

    fn chosen(
        stream: &StreamFormat,
        volume: HardwareVolume,
        device: fn(Encoding, u32) -> bool,
    ) -> Option<(Encoding, u32)> {
        OutputFormat::negotiate(stream, volume, device)
            .map(|output| (output.encoding, output.sample_rate))
    }

    #[test]
    fn dsd_transport_policy_orders_native_before_dop() {
        let unity = HardwareVolume::Unity;
        for convert in [false, true] {
            assert_eq!(
                chosen(&dsd64(DsdTransport::Auto, convert), unity, zen_one),
                Some((Encoding::Dsd(DsdLayout::U32Be), 88_200))
            );
            assert_eq!(
                chosen(&dsd64(DsdTransport::Native, convert), unity, zen_one),
                Some((Encoding::Dsd(DsdLayout::U32Be), 88_200))
            );
            assert_eq!(
                chosen(&dsd64(DsdTransport::Dop, convert), unity, zen_one),
                Some((Encoding::Dop(SampleEncoding::S24In3Le), 176_400))
            );
        }
    }

    #[test]
    fn an_open_device_carries_only_exact_continuations() {
        let pcm = |rate, bits| StreamFormat::Pcm {
            sample_rate: rate,
            channels: 2,
            bits_per_sample: bits,
        };
        let any = |_, _| true;
        let s24 = OutputFormat::negotiate(&pcm(96_000, 24), HardwareVolume::None, any).unwrap();
        // 16-bit fits a 24-bit layout losslessly; rate and depth may not grow.
        assert!(s24.carries(&pcm(96_000, 16)));
        assert!(!s24.carries(&pcm(44_100, 24)));
        assert!(!s24.carries(&pcm(96_000, 32)));
        assert!(!s24.carries(&dsd64(DsdTransport::Auto, true)));

        let auto = dsd64(DsdTransport::Auto, false);
        let native = OutputFormat::negotiate(&auto, HardwareVolume::Unity, zen_one).unwrap();
        assert!(native.carries(&dsd64(DsdTransport::Native, false)));
        assert!(!native.carries(&dsd64(DsdTransport::Dop, false)));
    }

    #[test]
    fn dsd_never_falls_back_to_pcm() {
        let pcm_only = |encoding, _| matches!(encoding, Encoding::Pcm(_));
        let auto = dsd64(DsdTransport::Auto, false);
        assert_eq!(chosen(&auto, HardwareVolume::None, pcm_only), None);
        assert_eq!(
            auto.to_string(),
            "DSD64 as native DSD or as DoP at 176400 Hz"
        );
    }

    #[test]
    fn conversion_is_the_last_resort() {
        let convert = dsd64(DsdTransport::Auto, true);
        // A USB DAC at 0 dB keeps DoP.
        let dop_dac = |encoding, rate| matches!(encoding, Encoding::Dop(_)) && rate == 176_400;
        assert!(matches!(
            chosen(&convert, HardwareVolume::Unity, dop_dac),
            Some((Encoding::Dop(_), 176_400))
        ));
        // Built-in audio accepts the DoP format, but its volume cannot be
        // read: DoP would play noise there, so conversion is used, at the
        // highest rate it takes.
        assert_eq!(
            chosen(&convert, HardwareVolume::Unknown, built_in),
            Some((Encoding::Converted(SampleEncoding::S32Le), 88_200))
        );
        let low_rate = |encoding, rate| built_in(encoding, rate) && rate == 44_100;
        assert_eq!(
            chosen(&convert, HardwareVolume::Unknown, low_rate),
            Some((Encoding::Converted(SampleEncoding::S32Le), 44_100))
        );
        // Without the setting, DoP is chosen and the player refuses it
        // with the reason; nothing is converted.
        assert!(matches!(
            chosen(
                &dsd64(DsdTransport::Auto, false),
                HardwareVolume::Unknown,
                built_in
            ),
            Some((Encoding::Dop(_), 176_400))
        ));
        assert_eq!(
            convert.to_string(),
            "DSD64 as native DSD, as DoP at 176400 Hz, or converted to PCM at 88200 or 44100 Hz"
        );
    }

    #[test]
    fn an_explicit_transport_never_converts() {
        for transport in [DsdTransport::Native, DsdTransport::Dop] {
            let stream = dsd64(transport, true);
            assert!(
                stream
                    .candidates()
                    .iter()
                    .all(|(encoding, _)| !matches!(encoding, Encoding::Converted(_)))
            );
        }
        assert_eq!(
            chosen(
                &dsd64(DsdTransport::Native, true),
                HardwareVolume::Unknown,
                built_in
            ),
            None
        );
    }

    #[test]
    fn converted_tracks_continue_only_into_converted_tracks() {
        let convert = dsd64(DsdTransport::Auto, true);
        let output = OutputFormat::negotiate(&convert, HardwareVolume::Unknown, built_in).unwrap();
        assert!(output.carries(&convert));
        let dsd128 = StreamFormat::Dsd {
            dsd_rate: 5_644_800,
            channels: 2,
            transport: DsdTransport::Auto,
            convert: true,
        };
        assert!(output.carries(&dsd128));
        // 24-bit PCM at 88.2 kHz would fit the device, but it is not
        // converted audio: the device reopens, so the status is right.
        let pcm = StreamFormat::Pcm {
            sample_rate: 88_200,
            channels: 2,
            bits_per_sample: 24,
        };
        assert!(!output.carries(&pcm));
        assert!(!output.carries(&dsd64(DsdTransport::Auto, false)));
    }

    #[test]
    fn silence_matches_each_layout() {
        let mut out = Vec::new();
        Encoding::Pcm(SampleEncoding::S24In3Le).silence(2, 3, &mut out);
        assert_eq!(out, vec![0; 18]);

        out.clear();
        Encoding::Dsd(DsdLayout::U32Be).silence(2, 2, &mut out);
        assert_eq!(out, vec![0x69; 16]);

        // DoP, S24_3LE stereo: [low, high, marker] per channel; markers
        // alternate from 0x05, data is the idle pattern.
        out.clear();
        Encoding::Dop(SampleEncoding::S24In3Le).silence(2, 2, &mut out);
        assert_eq!(
            out,
            [
                0x69, 0x69, 0x05, 0x69, 0x69, 0x05, //
                0x69, 0x69, 0xFA, 0x69, 0x69, 0xFA,
            ]
        );
    }
}
