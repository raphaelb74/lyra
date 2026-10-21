//! Read-only facts about a device for diagnostics: what the sink saw while it
//! played, and what the kernel reports in procfs.

use std::fs;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::sink::{BufferConfig, card_index, device_number};

/// Counters the sink updates while the writer thread owns it. Shared, so the
/// app can read them at any time.
#[derive(Debug, Default)]
pub struct SinkStats {
    underruns: AtomicU64,
    buffer_frames: AtomicU64,
    period_frames: AtomicU64,
    /// Device open, and the program the card was taken from, if any.
    open: Mutex<Option<OpenDevice>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OpenDevice {
    device: String,
    taken_from: Option<String>,
}

/// The sink's view of the open device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SinkReport {
    pub device: String,
    /// Times the device ran out of audio since it was opened.
    pub underruns: u64,
    pub buffer: BufferConfig,
    /// Program that handed the card over (e.g. `pipewire`), if one did.
    pub taken_from: Option<String>,
}

impl SinkStats {
    /// `None` while no device is open.
    pub fn report(&self) -> Option<SinkReport> {
        let open = self.open.lock().ok()?.clone()?;
        Some(SinkReport {
            device: open.device,
            underruns: self.underruns.load(Ordering::Relaxed),
            buffer: BufferConfig {
                buffer_frames: self.buffer_frames.load(Ordering::Relaxed),
                period_frames: self.period_frames.load(Ordering::Relaxed),
            },
            taken_from: open.taken_from,
        })
    }

    pub(crate) fn opened(&self, device: &str, buffer: BufferConfig, taken_from: Option<String>) {
        self.underruns.store(0, Ordering::Relaxed);
        self.buffer_frames
            .store(buffer.buffer_frames, Ordering::Relaxed);
        self.period_frames
            .store(buffer.period_frames, Ordering::Relaxed);
        if let Ok(mut open) = self.open.lock() {
            *open = Some(OpenDevice {
                device: device.to_owned(),
                taken_from,
            });
        }
    }

    pub(crate) fn closed(&self) {
        if let Ok(mut open) = self.open.lock() {
            *open = None;
        }
    }

    /// Lock-free: called from the writer's streaming path.
    pub(crate) fn underrun(&self) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
    }
}

/// What the kernel reports about a `hw:` device. Each field is `None` when
/// procfs does not have it (not a USB device, nothing open, no procfs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KernelReport {
    /// Kernel release, e.g. `7.2.6-070206-generic`.
    pub kernel: Option<String>,
    /// ALSA driver version line.
    pub alsa: Option<String>,
    /// USB vendor:product, e.g. `2fc6:f082`.
    pub usb_id: Option<String>,
    /// The substream's `hw_params` while it is open: the format and rate the
    /// hardware runs at, independent of Lyra's own report.
    pub hw_params: Option<String>,
    /// USB stream state while playing: interface, altset and the rate the
    /// DAC's feedback reports (`Momentary freq`).
    pub usb_stream: Option<String>,
}

/// Reads the kernel's view of `device` (`hw:CARD=…,DEV=…`).
pub fn kernel_report(device: &str) -> KernelReport {
    let read = |path: &str| {
        fs::read_to_string(path)
            .ok()
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
    };
    let mut report = KernelReport {
        kernel: read("/proc/sys/kernel/osrelease"),
        alsa: read("/proc/asound/version"),
        ..KernelReport::default()
    };
    let Some(card) = card_index(device) else {
        return report;
    };
    let pcm = device_number(device);
    let base = format!("/proc/asound/card{card}");
    report.usb_id = read(&format!("{base}/usbid"));
    report.hw_params =
        read(&format!("{base}/pcm{pcm}p/sub0/hw_params")).filter(|params| params != "closed");
    report.usb_stream = read(&format!("{base}/stream{pcm}"))
        .as_deref()
        .and_then(playback_status);
    report
}

/// The running playback block of a USB `stream` file: the lines under
/// `Status: Running`, up to the list of altsets.
fn playback_status(stream: &str) -> Option<String> {
    let mut lines = stream
        .lines()
        .skip_while(|line| line.trim() != "Playback:")
        .skip(1);
    let status = lines.next()?.trim();
    if status != "Status: Running" {
        return None;
    }
    let block: Vec<&str> = lines
        .take_while(|line| line.starts_with("    "))
        .map(str::trim)
        .collect();
    (!block.is_empty()).then(|| block.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const STREAM: &str = "iFi iFi USB Audio SE at usb-0000:00:14.0-1, high speed : USB Audio

Playback:
  Status: Running
    Interface = 1
    Altset = 4
    Packet Size = 136
    Momentary freq = 88203 Hz (0xb.0680)
    Feedback Format = 16.16
  Interface 1
    Altset 1
    Format: S16_LE
";

    #[test]
    fn keeps_the_running_playback_block() {
        assert_eq!(
            playback_status(STREAM).as_deref(),
            Some(
                "Interface = 1\nAltset = 4\nPacket Size = 136\n\
                 Momentary freq = 88203 Hz (0xb.0680)\nFeedback Format = 16.16"
            )
        );
    }

    #[test]
    fn a_stopped_stream_has_no_block() {
        let stopped = STREAM.replace("Status: Running", "Status: Stop");
        assert_eq!(playback_status(&stopped), None);
        assert_eq!(playback_status("no playback here"), None);
    }

    #[test]
    fn reports_follow_open_and_close() {
        let stats = SinkStats::default();
        assert_eq!(stats.report(), None);
        let buffer = BufferConfig {
            buffer_frames: 22_050,
            period_frames: 5_513,
        };
        stats.underrun();
        stats.opened("hw:CARD=SE,DEV=0", buffer, Some("pipewire".into()));
        stats.underrun();
        let report = stats.report().unwrap();
        assert_eq!(report.underruns, 1, "counted from the open");
        assert_eq!(report.buffer, buffer);
        assert_eq!(report.taken_from.as_deref(), Some("pipewire"));
        stats.closed();
        assert_eq!(stats.report(), None);
    }
}
