use std::fmt;

use format_pcm::{SampleEncoding, StreamInfo};

/// The exact format the DAC was opened with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputFormat {
    pub encoding: SampleEncoding,
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

impl OutputFormat {
    /// Picks the narrowest lossless layout the device accepts at the stream's
    /// own rate and channel count. `None` means the stream cannot be played
    /// bit-perfectly: no rate or channel change is ever made.
    pub fn negotiate(
        stream: &StreamInfo,
        mut accepts: impl FnMut(SampleEncoding) -> bool,
    ) -> Option<Self> {
        SampleEncoding::lossless_for(stream.bits_per_sample)
            .find(|&encoding| accepts(encoding))
            .map(|encoding| Self {
                encoding,
                sample_rate: stream.sample_rate,
                channels: stream.channels,
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
    /// Opens `device` for exactly this stream, closing any previous stream.
    /// Fails with [`SinkErrorKind::Unsupported`] instead of adapting.
    fn open(&mut self, device: &str, stream: &StreamInfo) -> Result<OutputFormat, SinkError>;

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
