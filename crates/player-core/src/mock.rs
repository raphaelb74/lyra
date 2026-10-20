//! A sink that records what it receives, for tests and DAC-less runs.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use format_pcm::SampleEncoding;

use crate::sink::{AudioSink, HardwareVolume, OutputFormat, PcmStream, SinkError, SinkErrorKind};

/// Calls the sink received, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SinkCall {
    Open(OutputFormat),
    Write { bytes: usize },
    Paused(bool),
    Discard,
    Drain,
    Close,
}

#[derive(Debug, Default)]
pub struct Recording {
    pub calls: Vec<SinkCall>,
    /// Bytes written since the last open or discard.
    pub bytes: Vec<u8>,
}

/// Shared view of a [`MockSink`]'s recording.
#[derive(Clone, Debug, Default)]
pub struct MockRecorder(Arc<Mutex<Recording>>);

impl MockRecorder {
    pub fn with<T>(&self, read: impl FnOnce(&Recording) -> T) -> T {
        read(&self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn record(&self, call: SinkCall, bytes: Option<&[u8]>) {
        let mut recording = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(call, SinkCall::Open(_) | SinkCall::Discard) {
            recording.bytes.clear();
        }
        if let Some(bytes) = bytes {
            recording.bytes.extend_from_slice(bytes);
        }
        recording.calls.push(call);
    }
}

#[derive(Debug)]
pub struct MockSink {
    /// Layouts accepted, at any rate in `rates`.
    pub encodings: Vec<SampleEncoding>,
    pub rates: Vec<u32>,
    /// Returned by `open` when set (e.g. a busy device).
    pub open_error: Option<SinkError>,
    /// Sleep per write, to make playback take time.
    pub write_delay: Duration,
    /// Reported in the opened [`OutputFormat`].
    pub hardware_volume: HardwareVolume,
    pub recorder: MockRecorder,
}

impl MockSink {
    /// A sink that accepts every layout at the usual rates, instantly.
    pub fn new() -> Self {
        Self {
            encodings: SampleEncoding::lossless_for(0).collect(),
            rates: vec![44_100, 48_000, 88_200, 96_000, 176_400, 192_000],
            open_error: None,
            write_delay: Duration::ZERO,
            hardware_volume: HardwareVolume::None,
            recorder: MockRecorder::default(),
        }
    }
}

impl Default for MockSink {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSink for MockSink {
    fn open(&mut self, device: &str, stream: &PcmStream) -> Result<OutputFormat, SinkError> {
        if let Some(error) = &self.open_error {
            return Err(error.clone());
        }
        let unsupported = || {
            SinkError::new(
                SinkErrorKind::Unsupported,
                format!(
                    "{device} cannot play {} Hz / {} bit",
                    stream.sample_rate, stream.bits_per_sample
                ),
            )
        };
        if !self.rates.contains(&stream.sample_rate) {
            return Err(unsupported());
        }
        let mut format =
            OutputFormat::negotiate(stream, |encoding| self.encodings.contains(&encoding))
                .ok_or_else(unsupported)?;
        format.hardware_volume = self.hardware_volume;
        self.recorder.record(SinkCall::Open(format), None);
        Ok(format)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), SinkError> {
        if !self.write_delay.is_zero() {
            std::thread::sleep(self.write_delay);
        }
        self.recorder
            .record(SinkCall::Write { bytes: bytes.len() }, Some(bytes));
        Ok(())
    }

    fn set_paused(&mut self, paused: bool) -> Result<(), SinkError> {
        self.recorder.record(SinkCall::Paused(paused), None);
        Ok(())
    }

    fn discard(&mut self) -> Result<(), SinkError> {
        self.recorder.record(SinkCall::Discard, None);
        Ok(())
    }

    fn drain(&mut self) -> Result<(), SinkError> {
        self.recorder.record(SinkCall::Drain, None);
        Ok(())
    }

    fn close(&mut self) {
        self.recorder.record(SinkCall::Close, None);
    }
}
