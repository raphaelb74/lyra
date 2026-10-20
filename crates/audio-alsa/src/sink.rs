use std::ffi::CString;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use alsa::pcm::{Access, HwParams, State};
use alsa::{PCM, ValueOr};
use player_core::{AudioSink, OutputFormat, PcmStream, SinkError, SinkErrorKind};

use crate::device::{alsa_format, open};
use crate::error::{AlsaError, errno};
use crate::volume::{self, SavedVolume};

/// Device buffer. Long enough to ride out scheduling hiccups on a busy
/// desktop; seek and stop drop it, so it adds no control latency.
const BUFFER_US: u32 = 250_000;
const PERIOD_US: u32 = BUFFER_US / 4;
/// Longest single wait for buffer space before re-checking the device.
const WAIT_MS: u32 = 100;

/// Plays to one ALSA `hw:` device, unchanged.
///
/// The device is opened with the stream's exact rate, channel count and a
/// lossless sample layout. Opening fails instead of converting anything.
#[derive(Debug, Default)]
pub struct AlsaSink {
    stream: Option<Stream>,
    underruns: Arc<AtomicU64>,
}

/// Buffer geometry the driver granted, for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferConfig {
    pub buffer_frames: u64,
    pub period_frames: u64,
}

#[derive(Debug)]
struct Stream {
    pcm: PCM,
    /// Declared after `pcm`, so the device closes before the volume is restored.
    _volume: SavedVolume,
    device: String,
    frame_bytes: usize,
    can_pause: bool,
    buffer: BufferConfig,
}

impl AlsaSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Underruns since creation (the device ran out of audio). Shared, so it
    /// can be read while the sink is owned by the writer thread.
    pub fn underruns(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.underruns)
    }

    pub fn buffer_config(&self) -> Option<BufferConfig> {
        self.stream.as_ref().map(|stream| stream.buffer)
    }

    fn stream(&self) -> Result<&Stream, SinkError> {
        self.stream
            .as_ref()
            .ok_or_else(|| SinkError::new(SinkErrorKind::Failed, "no output device is open"))
    }
}

impl AudioSink for AlsaSink {
    fn open(&mut self, device: &str, stream: &PcmStream) -> Result<OutputFormat, SinkError> {
        self.close();
        let fail = |error| {
            sink_error(&AlsaError::from_alsa(
                device,
                card_index(device),
                device_number(device),
                error,
            ))
        };
        let unsupported = |reason: String| {
            sink_error(&AlsaError::Unsupported {
                device: device.to_owned(),
                reason,
            })
        };

        let pcm = open(device).map_err(fail)?;
        let channels = u32::try_from(stream.channels)
            .map_err(|_| unsupported(format!("{} channels", stream.channels)))?;
        let output = {
            let hw = HwParams::any(&pcm).map_err(fail)?;
            hw.set_access(Access::RWInterleaved).map_err(fail)?;
            hw.set_channels(channels)
                .map_err(|_| unsupported(format!("{channels} channels")))?;
            // Exact rate only: a refused rate is an error, never a resample.
            hw.set_rate(stream.sample_rate, ValueOr::Nearest)
                .map_err(|_| unsupported(format!("{} Hz", stream.sample_rate)))?;
            let output = OutputFormat::negotiate(stream, |encoding| {
                hw.test_format(alsa_format(encoding)).is_ok()
            })
            .ok_or_else(|| {
                unsupported(format!(
                    "{}-bit samples at {} Hz",
                    stream.bits_per_sample, stream.sample_rate
                ))
            })?;
            hw.set_format(alsa_format(output.encoding)).map_err(fail)?;
            hw.set_period_time_near(PERIOD_US, ValueOr::Nearest)
                .map_err(fail)?;
            hw.set_buffer_time_near(BUFFER_US, ValueOr::Nearest)
                .map_err(fail)?;
            pcm.hw_params(&hw).map_err(fail)?;
            output
        };

        let (can_pause, buffer) = {
            let current = pcm.hw_params_current().map_err(fail)?;
            // The driver must report exactly what was asked for.
            let granted = (
                current.get_rate().map_err(fail)?,
                current.get_channels().map_err(fail)?,
                current.get_format().map_err(fail)?,
            );
            if granted != (stream.sample_rate, channels, alsa_format(output.encoding)) {
                return Err(unsupported(format!(
                    "driver changed the format to {granted:?}"
                )));
            }
            let buffer_frames = current.get_buffer_size().map_err(fail)?;
            let period_frames = current.get_period_size().map_err(fail)?;
            let sw = pcm.sw_params_current().map_err(fail)?;
            // Start once the buffer is nearly full, so playback begins with margin.
            sw.set_start_threshold(buffer_frames - period_frames)
                .map_err(fail)?;
            sw.set_avail_min(period_frames).map_err(fail)?;
            pcm.sw_params(&sw).map_err(fail)?;
            let to_u64 = |frames| u64::try_from(frames).unwrap_or(0);
            let buffer = BufferConfig {
                buffer_frames: to_u64(buffer_frames),
                period_frames: to_u64(period_frames),
            };
            (current.can_pause(), buffer)
        };

        let mut output = output;
        let (hardware_volume, saved_volume) = volume::prepare(&card_control(device));
        output.hardware_volume = hardware_volume;

        self.stream = Some(Stream {
            pcm,
            _volume: saved_volume,
            device: device.to_owned(),
            frame_bytes: output.encoding.bytes() * output.channels,
            can_pause,
            buffer,
        });
        Ok(output)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), SinkError> {
        let stream = self.stream()?;
        let io = stream.pcm.io_bytes();
        let mut offset = 0;
        while offset < bytes.len() {
            match io.writei(&bytes[offset..]) {
                Ok(frames) => offset += frames * stream.frame_bytes,
                Err(error) => match errno::of(&error) {
                    errno::EAGAIN => {
                        stream
                            .pcm
                            .wait(Some(WAIT_MS))
                            .map_err(|error| stream.failed(error))?;
                    }
                    errno::EPIPE => {
                        // Underrun: the device played silence. Continue from
                        // the next sample; nothing is skipped.
                        self.underruns.fetch_add(1, Ordering::Relaxed);
                        stream.pcm.prepare().map_err(|error| stream.failed(error))?;
                    }
                    errno::ESTRPIPE => {
                        stream
                            .pcm
                            .try_recover(error, true)
                            .map_err(|error| stream.failed(error))?;
                    }
                    _ => return Err(stream.failed(error)),
                },
            }
        }
        Ok(())
    }

    fn set_paused(&mut self, paused: bool) -> Result<(), SinkError> {
        let stream = self.stream()?;
        // Without hardware pause the buffer plays out and the device
        // underruns; resuming continues at the next sample, losing nothing.
        if !stream.can_pause {
            return Ok(());
        }
        let state = stream.pcm.state();
        if (paused && state == State::Running) || (!paused && state == State::Paused) {
            stream
                .pcm
                .pause(paused)
                .map_err(|error| stream.failed(error))?;
        }
        Ok(())
    }

    fn discard(&mut self) -> Result<(), SinkError> {
        let stream = self.stream()?;
        stream.pcm.drop().map_err(|error| stream.failed(error))?;
        stream.pcm.prepare().map_err(|error| stream.failed(error))
    }

    fn drain(&mut self) -> Result<(), SinkError> {
        let stream = self.stream()?;
        if stream.pcm.state() == State::Paused {
            stream
                .pcm
                .pause(false)
                .map_err(|error| stream.failed(error))?;
        }
        match stream.pcm.drain() {
            Ok(()) => Ok(()),
            // Non-blocking handle: the drain runs on; wait for it to finish.
            Err(error) if errno::of(&error) == errno::EAGAIN => {
                while stream.pcm.state() == State::Draining {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(())
            }
            Err(error) => Err(stream.failed(error)),
        }
    }

    fn close(&mut self) {
        // Dropping the handle closes the device; queued audio is discarded.
        self.stream = None;
    }
}

impl Stream {
    fn failed(&self, error: alsa::Error) -> SinkError {
        if errno::of(&error) == errno::ENODEV || self.pcm.state() == State::Disconnected {
            return SinkError::new(
                SinkErrorKind::NotFound,
                format!("{} was disconnected", self.device),
            );
        }
        sink_error(&AlsaError::Alsa {
            device: self.device.clone(),
            source: error,
        })
    }
}

fn sink_error(error: &AlsaError) -> SinkError {
    let kind = match *error {
        AlsaError::Busy { .. } => SinkErrorKind::Busy,
        AlsaError::NotFound { .. } => SinkErrorKind::NotFound,
        AlsaError::Unsupported { .. } => SinkErrorKind::Unsupported,
        AlsaError::Alsa { .. } => SinkErrorKind::Failed,
    };
    SinkError::new(kind, error.to_string())
}

/// Card index for `hw:CARD=<id>,…` or `hw:<n>,…`, to find a busy device's holder.
fn card_index(device: &str) -> Option<i32> {
    let card = hw_part(device, "CARD", 0)?;
    if let Ok(index) = card.parse() {
        return Some(index);
    }
    let id = CString::new(card).ok()?;
    alsa::Card::from_str(&id).ok().map(|card| card.get_index())
}

/// Control name for the card of a `hw:` PCM name (`hw:CARD=ONE` / `hw:2`).
fn card_control(device: &str) -> String {
    match hw_part(device, "CARD", 0) {
        Some(card) if card.parse::<u32>().is_ok() => format!("hw:{card}"),
        Some(card) => format!("hw:CARD={card}"),
        None => device.to_owned(),
    }
}

fn device_number(device: &str) -> u32 {
    hw_part(device, "DEV", 1)
        .and_then(|dev| dev.parse().ok())
        .unwrap_or(0)
}

/// A field of an ALSA `hw:` name, by key (`CARD=ONE`) or position (`hw:1,0`).
fn hw_part<'a>(device: &'a str, key: &str, position: usize) -> Option<&'a str> {
    let fields = device.strip_prefix("hw:")?;
    let mut parts = fields.split(',');
    let by_key = fields.split(',').find_map(|part| {
        part.split_once('=')
            .and_then(|(name, value)| (name == key).then_some(value))
    });
    by_key.or_else(|| parts.nth(position).filter(|part| !part.contains('=')))
}

#[cfg(test)]
mod tests {
    use super::{card_control, device_number, hw_part};

    #[test]
    fn parses_hw_names() {
        assert_eq!(hw_part("hw:CARD=ONE,DEV=0", "CARD", 0), Some("ONE"));
        assert_eq!(hw_part("hw:2,1", "CARD", 0), Some("2"));
        assert_eq!(device_number("hw:CARD=PCH,DEV=3"), 3);
        assert_eq!(device_number("hw:2,1"), 1);
        assert_eq!(hw_part("plughw:ONE", "CARD", 0), None);
        assert_eq!(card_control("hw:CARD=ONE,DEV=0"), "hw:CARD=ONE");
        assert_eq!(card_control("hw:2,0"), "hw:2");
    }
}
