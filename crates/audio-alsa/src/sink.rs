use std::ffi::CString;
use std::sync::Arc;
use std::time::{Duration, Instant};

use alsa::pcm::{Access, HwParams, State};
use alsa::{PCM, ValueOr};
use player_core::{AudioSink, Encoding, OutputFormat, SinkError, SinkErrorKind, StreamFormat};

use crate::device::{alsa_format, open};
use crate::diagnostics::SinkStats;
use crate::error::{AlsaError, errno, find_holder};
use crate::reserve::Reservation;
use crate::volume::{self, SavedVolume};

/// Periods in the device buffer.
const PERIODS: u32 = 4;
/// Longest period, as a fraction of a second: with [`PERIODS`] the buffer is
/// 125–250 ms, long enough to ride out scheduling hiccups on a busy
/// desktop. Seek and stop drop it, so it adds no control latency.
const PERIODS_PER_SECOND: u32 = 16;
/// Native DSD warm-up (see [`AlsaSink::warm_up`]): idle pattern played, then
/// a pause before the real open.
const WARM_UP_SILENCE: Duration = Duration::from_millis(100);
const WARM_UP_PAUSE: Duration = Duration::from_millis(150);
/// Longest single wait for buffer space before re-checking the device.
const WAIT_MS: u32 = 100;
/// Longest wait for the sound server to close a card it handed over.
const HANDOVER_WAIT: Duration = Duration::from_secs(2);

/// Plays to one ALSA `hw:` device, unchanged.
///
/// The device is opened with the stream's exact rate, channel count and a
/// lossless sample layout (native DSD or DoP for DSD files). Opening fails
/// instead of converting anything.
///
/// A card the sound server holds is requested from it (see [`Reservation`])
/// and kept until [`AudioSink::close`], so PipeWire does not take it back
/// between tracks.
#[derive(Debug, Default)]
pub struct AlsaSink {
    /// Declared before `reservation`: the device closes before the card is
    /// handed back.
    stream: Option<Stream>,
    reservation: Option<Reservation>,
    stats: Arc<SinkStats>,
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

    /// Underruns, buffer geometry and hand-over of the open device. Shared,
    /// so it can be read while the writer thread owns the sink.
    pub fn stats(&self) -> Arc<SinkStats> {
        Arc::clone(&self.stats)
    }

    pub fn buffer_config(&self) -> Option<BufferConfig> {
        self.stream.as_ref().map(|stream| stream.buffer)
    }

    fn stream(&self) -> Result<&Stream, SinkError> {
        self.stream
            .as_ref()
            .ok_or_else(|| SinkError::new(SinkErrorKind::Failed, "no output device is open"))
    }

    fn open_stream(
        &mut self,
        device: &str,
        stream: &StreamFormat,
    ) -> Result<OutputFormat, SinkError> {
        self.stream = None;
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

        let pcm = self.open_pcm(device).map_err(fail)?;
        let channels = u32::try_from(stream.channels())
            .map_err(|_| unsupported(format!("{} channels", stream.channels())))?;
        // Parameters with access and channels fixed; each candidate starts
        // from a fresh set, since formats differ in their rates.
        let base = || -> Result<HwParams<'_>, alsa::Error> {
            let hw = HwParams::any(&pcm)?;
            hw.set_access(Access::RWInterleaved)?;
            hw.set_channels(channels)?;
            Ok(hw)
        };
        if base().is_err() {
            return Err(unsupported(format!("{channels} channels")));
        }
        // Before negotiating: whether DoP would arrive intact decides
        // between DoP and conversion when conversion is allowed.
        let (hardware_volume, saved_volume) = volume::prepare(&card_control(device));
        let output = OutputFormat::negotiate(stream, hardware_volume, |encoding, rate| {
            base().is_ok_and(|hw| {
                hw.set_format(alsa_format(encoding)).is_ok() && hw.test_rate(rate).is_ok()
            })
        })
        .ok_or_else(|| unsupported(stream.to_string()))?;
        {
            let hw = base().map_err(fail)?;
            hw.set_format(alsa_format(output.encoding)).map_err(fail)?;
            // Exact rate only: a refused rate is an error, never a resample.
            hw.set_rate(output.sample_rate, ValueOr::Nearest)
                .map_err(fail)?;
            let period = period_frames(output.sample_rate);
            hw.set_period_size_near(period, ValueOr::Nearest)
                .map_err(fail)?;
            hw.set_buffer_size_near(period * alsa::pcm::Frames::from(PERIODS))
                .map_err(fail)?;
            pcm.hw_params(&hw).map_err(fail)?;
        }

        let (can_pause, buffer) = {
            let current = pcm.hw_params_current().map_err(fail)?;
            // The driver must report exactly what was asked for.
            let granted = (
                current.get_rate().map_err(fail)?,
                current.get_channels().map_err(fail)?,
                current.get_format().map_err(fail)?,
            );
            if granted != (output.sample_rate, channels, alsa_format(output.encoding)) {
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

        let taken_from = self
            .reservation
            .as_ref()
            .and_then(|reservation| reservation.taken_from().map(str::to_owned));
        self.stats.opened(device, buffer, taken_from);
        self.stream = Some(Stream {
            pcm,
            _volume: saved_volume,
            device: device.to_owned(),
            frame_bytes: output.encoding.frame_bytes(output.channels),
            can_pause,
            buffer,
        });
        Ok(output)
    }

    /// Opens the PCM, asking the sound server for the card if it is busy.
    fn open_pcm(&mut self, device: &str) -> Result<PCM, alsa::Error> {
        let card = card_index(device);
        if self.reservation.as_ref().map(Reservation::card) != card {
            self.reservation = None;
        }
        let error = match open(device) {
            Err(error) if errno::of(&error) == errno::EBUSY && self.reservation.is_none() => error,
            result => return result,
        };
        let holder = card
            .and_then(|card| find_holder(card, device_number(device)))
            .and_then(|holder| holder.command);
        self.reservation = card
            .and_then(Reservation::acquire)
            .map(|reservation| reservation.with_holder(holder));
        if self.reservation.is_none() {
            return Err(error);
        }
        // The server answers before its device is closed: retry briefly.
        let start = Instant::now();
        loop {
            match open(device) {
                Err(error)
                    if errno::of(&error) == errno::EBUSY && start.elapsed() < HANDOVER_WAIT =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => return result,
            }
        }
    }
}

impl AlsaSink {
    /// Plays a moment of DSD idle pattern on the open stream and closes it,
    /// keeping the card.
    ///
    /// Some USB DACs (the iFi USB Audio SE, see docs/hardware) sometimes
    /// stay in PCM mode when a native DSD stream starts after a mode change,
    /// and play the DSD as loud hiss; nothing on the host side shows it.
    /// Stopping and playing again always worked: the DAC is already in DSD
    /// at that rate. Opening twice does the same on the first play.
    fn warm_up(&mut self, output: OutputFormat) {
        let frames =
            usize::try_from(u128::from(output.sample_rate) * WARM_UP_SILENCE.as_millis() / 1000)
                .unwrap_or(0);
        let mut bytes = Vec::new();
        output.encoding.silence(output.channels, frames, &mut bytes);
        // Errors are moot: the real open follows, and reports its own.
        if self.write(&bytes).is_ok() {
            let _ = self.drain();
        }
        self.stream = None;
        std::thread::sleep(WARM_UP_PAUSE);
    }
}

impl AudioSink for AlsaSink {
    fn open(&mut self, device: &str, stream: &StreamFormat) -> Result<OutputFormat, SinkError> {
        let mut result = self.open_stream(device, stream);
        if let Ok(output) = result
            && matches!(output.encoding, Encoding::Dsd(_))
        {
            self.warm_up(output);
            result = self.open_stream(device, stream);
        }
        if result.is_err() {
            // Nothing is playing: the sound server may have the card back.
            self.stats.closed();
            self.reservation = None;
        }
        result
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
                        self.stats.underrun();
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
        self.reservation = None;
        self.stats.closed();
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
pub(crate) fn card_index(device: &str) -> Option<i32> {
    let card = hw_part(device, "CARD", 0)?;
    if let Ok(index) = card.parse() {
        return Some(index);
    }
    let id = CString::new(card).ok()?;
    alsa::Card::from_str(&id).ok().map(|card| card.get_index())
}

/// Control name for the card of a `hw:` PCM name (`hw:CARD=ONE` / `hw:2`).
pub(crate) fn card_control(device: &str) -> String {
    match hw_part(device, "CARD", 0) {
        Some(card) if card.parse::<u32>().is_ok() => format!("hw:{card}"),
        Some(card) => format!("hw:CARD={card}"),
        None => device.to_owned(),
    }
}

pub(crate) fn device_number(device: &str) -> u32 {
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

/// Frames per period at `rate`: the largest power of two up to 1/16 s.
///
/// A whole power of two keeps every period a multiple of 128 bytes for any
/// sample layout. Sized by time instead (5513 frames at 88.2 kHz), periods
/// are odd; Intel HD Audio controllers can then lose their DMA position,
/// which plays the music under loud static until the stream restarts.
fn period_frames(rate: u32) -> alsa::pcm::Frames {
    let most = (rate / PERIODS_PER_SECOND).max(64);
    let frames = 1_u32 << most.ilog2();
    alsa::pcm::Frames::from(frames)
}

#[cfg(test)]
mod tests {
    use super::{card_control, device_number, hw_part, period_frames};

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

    #[test]
    fn periods_are_powers_of_two_up_to_a_sixteenth_of_a_second() {
        // 88.2 kHz: 5512 frames by time, 4096 here (46 ms).
        assert_eq!(period_frames(88_200), 4096);
        assert_eq!(period_frames(44_100), 2048);
        assert_eq!(period_frames(48_000), 2048);
        assert_eq!(period_frames(192_000), 8192);
        // DSD512 as native DSD_U32: 705.6 kHz frames.
        assert_eq!(period_frames(705_600), 32_768);
        for rate in [32_000, 44_100, 96_000, 176_400, 352_800] {
            let frames = period_frames(rate);
            // A multiple of 128 bytes even for the smallest frame (2 bytes).
            assert_eq!((frames * 2) % 128, 0, "{rate}");
            assert!(frames * 16 <= i64::from(rate) && frames * 32 > i64::from(rate));
        }
    }
}
