//! Types that cross the IPC boundary. Mirrored in `src/lib/ipc.ts`.
//!
//! Every fallible command rejects with [`IpcError`]: a stable `snake_case`
//! `code` for the UI to branch on and a complete, user-facing `message`.

use std::path::Path;

use audio_alsa::{Capabilities, Device, KernelReport, SinkReport};
use format_dsd::Container;
use format_pcm::{Codec, SampleEncoding};
use library::{Album, Artist, Entry, Folder, Playlist, ScanSummary};
use player_core::{
    BitPerfect, ConversionStats, Encoding, ErrorKind, NowPlaying, PlaybackState, PlayerError,
    PlayerStatus, SourceInfo, Transport,
};
use serde::Serialize;

/// Response of the `get_app_info` command.
#[derive(Clone, Debug, Serialize)]
pub struct AppInfo {
    name: &'static str,
    version: &'static str,
    platform: &'static str,
}

impl AppInfo {
    pub fn current() -> Self {
        Self {
            name: "Lyra",
            version: env!("CARGO_PKG_VERSION"),
            platform: std::env::consts::OS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IpcError {
    pub code: &'static str,
    pub message: String,
}

impl IpcError {
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_request",
            message: message.into(),
        }
    }
}

impl From<PlayerError> for IpcError {
    fn from(error: PlayerError) -> Self {
        let code = match error.kind {
            ErrorKind::DeviceBusy => "device_busy",
            ErrorKind::DeviceNotFound => "device_not_found",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::InvalidFile => "invalid_file",
            ErrorKind::Output => "output_failed",
            ErrorKind::Internal => "internal",
        };
        Self {
            code,
            message: error.message,
        }
    }
}

/// A card's hardware volume, for the in-app control.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    /// 0–100 along the control's steps.
    pub percent: u8,
    pub decibels: Option<f64>,
    /// USB DAC: below 0 dB the samples usually change (not bit-perfect).
    pub usb: bool,
    /// Why the volume cannot change now (DoP playing), if it cannot.
    pub locked: Option<&'static str>,
}

/// A device's volume changed (event payload).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceVolume {
    pub device: String,
    pub volume: Option<VolumeInfo>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// ALSA name to play to, `hw:CARD=…,DEV=…`.
    pub name: String,
    /// Short name for people, e.g. `iFi USB Audio SE`, `HDMI 2`.
    pub label: String,
    pub card_name: String,
    pub pcm_name: String,
    pub usb: bool,
}

impl From<Device> for DeviceInfo {
    fn from(device: Device) -> Self {
        let usb = device.is_usb();
        Self {
            name: device.name,
            label: device.label,
            card_name: device.card_name,
            pcm_name: device.pcm_name,
            usb,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: &'static str,
    pub track: Option<Track>,
    pub error: Option<IpcError>,
    pub queue_length: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    /// Library track id; `None` for a dropped file.
    pub library_id: Option<i64>,
    pub queue_index: usize,
    pub file_name: String,
    pub device: String,
    /// `flac`, `wav`, `dsf` or `dff`.
    pub codec: &'static str,
    /// PCM sample rate, or DSD bit rate per channel.
    pub sample_rate: u32,
    /// 1 for DSD.
    pub bits_per_sample: u32,
    /// 64 for DSD64, …; `None` for PCM.
    pub dsd_multiple: Option<u32>,
    pub channels: usize,
    pub duration_seconds: Option<f64>,
    pub output: Option<Output>,
    pub bit_perfect: Option<Verdict>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    /// `pcm`, `dop`, `native` or `converted` (DSD converted to PCM).
    pub transport: &'static str,
    /// ALSA format name, e.g. `S24_3LE` or `DSD_U32_BE`.
    pub format: &'static str,
    pub sample_rate: u32,
    pub channels: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Verdict {
    /// `yes`, `no` or `unverified`.
    pub status: &'static str,
    pub reason: Option<String>,
}

impl From<&PlayerStatus> for Status {
    fn from(status: &PlayerStatus) -> Self {
        Self {
            state: state_name(status.state),
            track: status.track.as_ref().map(Track::from),
            error: status.error.clone().map(IpcError::from),
            queue_length: status.queue_length,
        }
    }
}

impl From<&NowPlaying> for Track {
    fn from(now: &NowPlaying) -> Self {
        let source = now.source;
        let (codec, sample_rate, bits_per_sample, dsd_multiple) = match source {
            SourceInfo::Pcm(info) => (
                match info.codec {
                    Codec::Flac => "flac",
                    Codec::Wav => "wav",
                },
                info.sample_rate,
                info.bits_per_sample,
                None,
            ),
            SourceInfo::Dsd(info) => (
                match info.container {
                    Container::Dsf => "dsf",
                    Container::Dff => "dff",
                },
                info.dsd_rate,
                1,
                Some(info.multiple()),
            ),
        };
        Self {
            library_id: now.id,
            queue_index: now.index,
            file_name: file_name(&now.path),
            device: now.device.clone(),
            codec,
            sample_rate,
            bits_per_sample,
            dsd_multiple,
            channels: source.channels(),
            duration_seconds: source
                .total_frames()
                .map(|frames| frames_to_seconds(frames, source.frame_rate())),
            output: now.output.map(|output| Output {
                transport: match output.encoding.transport() {
                    Transport::Pcm => "pcm",
                    Transport::Dop => "dop",
                    Transport::Native => "native",
                    Transport::Converted => "converted",
                },
                format: alsa_format_name(output.encoding),
                sample_rate: output.sample_rate,
                channels: output.channels,
            }),
            bit_perfect: now.bit_perfect().map(|verdict| match verdict {
                BitPerfect::Yes => Verdict {
                    status: "yes",
                    reason: None,
                },
                BitPerfect::No { reason } => Verdict {
                    status: "no",
                    reason: Some(reason),
                },
                BitPerfect::Unverified { reason } => Verdict {
                    status: "unverified",
                    reason: Some(reason),
                },
            }),
        }
    }
}

fn state_name(state: PlaybackState) -> &'static str {
    match state {
        PlaybackState::Stopped => "stopped",
        PlaybackState::Loading => "loading",
        PlaybackState::Negotiating => "negotiating",
        PlaybackState::Buffering => "buffering",
        PlaybackState::Playing => "playing",
        PlaybackState::Paused => "paused",
        PlaybackState::Error => "error",
    }
}

fn alsa_format_name(encoding: Encoding) -> &'static str {
    let layout = match encoding {
        Encoding::Pcm(layout) | Encoding::Dop(layout) | Encoding::Converted(layout) => layout,
        Encoding::Dsd(layout) => return layout.name(),
    };
    match layout {
        SampleEncoding::S16Le => "S16_LE",
        SampleEncoding::S24In3Le => "S24_3LE",
        SampleEncoding::S24In4Le => "S24_LE",
        SampleEncoding::S32Le => "S32_LE",
    }
}

/// Response of `get_diagnostics`: what the kernel and Lyra report about one
/// device, read now.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsInfo {
    pub device: String,
    pub kernel: Option<String>,
    pub alsa: Option<String>,
    pub usb_id: Option<String>,
    /// The kernel's `hw_params` while the device is open (by anyone).
    pub hw_params: Option<String>,
    /// USB stream state while playing, including the DAC's feedback rate.
    pub usb_stream: Option<String>,
    /// Present while Lyra plays on this device.
    pub playback: Option<PlaybackDiagnostics>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackDiagnostics {
    /// Since the device was opened.
    pub underruns: u64,
    pub buffer_frames: u64,
    pub period_frames: u64,
    /// Program that handed the device to Lyra, e.g. `pipewire`.
    pub taken_from: Option<String>,
    pub conversion: Option<ConversionInfo>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionInfo {
    pub clipped: u64,
    /// Highest level before clamping, dBFS; `None` while only silence played.
    pub peak_db: Option<f64>,
}

impl DiagnosticsInfo {
    pub fn new(
        device: String,
        kernel: KernelReport,
        playback: Option<SinkReport>,
        conversion: Option<ConversionStats>,
    ) -> Self {
        let playback = playback.map(|report| PlaybackDiagnostics {
            underruns: report.underruns,
            buffer_frames: report.buffer.buffer_frames,
            period_frames: report.buffer.period_frames,
            taken_from: report.taken_from,
            conversion: conversion.map(|stats| ConversionInfo {
                clipped: stats.clipped,
                peak_db: (stats.peak > 0.0).then(|| 20.0 * stats.peak.log10()),
            }),
        });
        Self {
            device,
            kernel: kernel.kernel,
            alsa: kernel.alsa,
            usb_id: kernel.usb_id,
            hw_params: kernel.hw_params,
            usb_stream: kernel.usb_stream,
            playback,
        }
    }
}

/// Response of `probe_device`: what the driver says the device accepts.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitiesInfo {
    pub channels_min: u32,
    pub channels_max: u32,
    pub formats: Vec<FormatSupport>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FormatSupport {
    /// ALSA name, e.g. `S24_3LE` or `DSD_U32_BE`.
    pub format: &'static str,
    pub rates: Vec<u32>,
}

impl From<Capabilities> for CapabilitiesInfo {
    fn from(capabilities: Capabilities) -> Self {
        let pcm = capabilities.pcm.into_iter().map(|support| FormatSupport {
            format: alsa_format_name(Encoding::Pcm(support.encoding)),
            rates: support.rates,
        });
        let dsd = capabilities
            .dsd
            .into_iter()
            .map(|(layout, rates)| FormatSupport {
                format: layout.name(),
                rates,
            });
        Self {
            channels_min: capabilities.channels_min,
            channels_max: capabilities.channels_max,
            formats: pcm.chain(dsd).collect(),
        }
    }
}

/// URL of a cached artwork file, served by the `lyra-art` protocol.
pub const ART_URL: &str = "lyra-art://localhost/";

#[derive(Clone, Debug, Serialize)]
pub struct FolderInfo {
    pub id: i64,
    pub path: String,
}

impl From<Folder> for FolderInfo {
    fn from(folder: Folder) -> Self {
        Self {
            id: folder.id,
            path: folder.path.display().to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumInfo {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    /// Artwork URL, or `None`.
    pub art: Option<String>,
    pub track_count: u32,
    /// Best source format among the tracks.
    pub codec: String,
    pub sample_rate: u32,
    pub dsd_multiple: Option<u32>,
}

impl From<Album> for AlbumInfo {
    fn from(album: Album) -> Self {
        Self {
            id: album.id,
            title: album.title,
            artist: album.artist,
            year: album.year,
            art: album.art.map(|name| format!("{ART_URL}{name}")),
            track_count: album.tracks,
            dsd_multiple: dsd_multiple(&album.best_codec, album.best_rate),
            codec: album.best_codec,
            sample_rate: album.best_rate,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryTrack {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_id: i64,
    pub disc: Option<u32>,
    pub track: Option<u32>,
    pub duration_seconds: Option<f64>,
    pub codec: String,
    pub sample_rate: u32,
    pub bits_per_sample: u32,
    pub channels: u32,
    pub dsd_multiple: Option<u32>,
}

impl From<library::Track> for LibraryTrack {
    fn from(track: library::Track) -> Self {
        Self {
            id: track.id,
            dsd_multiple: dsd_multiple(&track.codec, track.sample_rate),
            title: track.title,
            artist: track.artist,
            album: track.album,
            album_id: track.album_id,
            disc: track.disc,
            track: track.track,
            duration_seconds: track.duration_ms.map(ms_to_seconds),
            codec: track.codec,
            sample_rate: track.sample_rate,
            bits_per_sample: track.bits_per_sample,
            channels: track.channels,
        }
    }
}

fn dsd_multiple(codec: &str, rate: u32) -> Option<u32> {
    matches!(codec, "dsf" | "dff").then(|| format_dsd::multiple(rate))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistInfo {
    pub id: i64,
    pub name: String,
    pub album_count: u32,
    pub track_count: u32,
}

impl From<Artist> for ArtistInfo {
    fn from(artist: Artist) -> Self {
        Self {
            id: artist.id,
            name: artist.name,
            album_count: artist.albums,
            track_count: artist.tracks,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistInfo {
    pub id: i64,
    pub name: String,
    pub track_count: u32,
    pub duration_seconds: f64,
}

impl From<Playlist> for PlaylistInfo {
    fn from(playlist: Playlist) -> Self {
        Self {
            id: playlist.id,
            name: playlist.name,
            track_count: playlist.tracks,
            duration_seconds: ms_to_seconds(playlist.duration_ms),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    /// Identifies this entry (a track may appear twice).
    pub entry_id: i64,
    pub track: LibraryTrack,
}

impl From<Entry> for PlaylistEntry {
    fn from(entry: Entry) -> Self {
        Self {
            entry_id: entry.id,
            track: LibraryTrack::from(entry.track),
        }
    }
}

/// One queue entry, described for the Queue view.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueEntry {
    pub track_id: Option<i64>,
    pub title: String,
    pub artist: Option<String>,
    pub duration_seconds: Option<f64>,
}

/// Library details of the playing track, for Now Playing.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackDetails {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub album_id: i64,
    pub year: Option<i32>,
    pub art: Option<String>,
}

/// Pushed on the `scan-status` event and returned by `get_scan_status`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ScanStatus {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    /// Result of the last finished scan.
    pub last: Option<ScanReport>,
    /// Why the last scan failed.
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScanReport {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub failed: usize,
}

impl From<&ScanSummary> for ScanReport {
    fn from(summary: &ScanSummary) -> Self {
        Self {
            added: summary.added,
            updated: summary.updated,
            removed: summary.removed,
            failed: summary.failed,
        }
    }
}

pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Library durations are milliseconds; the UI shows seconds.
#[expect(
    clippy::cast_precision_loss,
    reason = "durations are far below 2^52 ms"
)]
pub fn ms_to_seconds(ms: u64) -> f64 {
    ms as f64 / 1000.0
}

/// Display only: exact positions stay in frames inside the engine.
#[expect(
    clippy::cast_precision_loss,
    reason = "2^52 frames is far beyond any track"
)]
pub fn frames_to_seconds(frames: u64, sample_rate: u32) -> f64 {
    frames as f64 / f64::from(sample_rate)
}

#[cfg(test)]
mod tests {
    use player_core::{ErrorKind, PlayerError};

    use super::{DiagnosticsInfo, IpcError};

    #[test]
    fn player_errors_keep_their_message_and_get_a_stable_code() {
        let error = IpcError::from(PlayerError {
            kind: ErrorKind::DeviceBusy,
            message: "hw:CARD=ONE,DEV=0 is in use by pipewire (pid 1283)".into(),
        });
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({
                "code": "device_busy",
                "message": "hw:CARD=ONE,DEV=0 is in use by pipewire (pid 1283)",
            })
        );
    }

    #[test]
    fn diagnostics_report_conversion_peak_in_dbfs() {
        let buffer = audio_alsa::BufferConfig {
            buffer_frames: 22_050,
            period_frames: 5_513,
        };
        let playback = audio_alsa::SinkReport {
            device: "hw:CARD=PCH,DEV=0".into(),
            underruns: 2,
            buffer,
            taken_from: Some("pipewire".into()),
        };
        let peak = |peak| player_core::ConversionStats { clipped: 3, peak };
        let json = |peak| {
            serde_json::to_value(DiagnosticsInfo::new(
                "hw:CARD=PCH,DEV=0".into(),
                audio_alsa::KernelReport::default(),
                Some(playback.clone()),
                Some(peak),
            ))
            .unwrap()
        };
        let half = json(peak(0.5));
        let db = half["playback"]["conversion"]["peakDb"].as_f64().unwrap();
        assert!((db + 6.0206).abs() < 1e-3, "{db}");
        assert_eq!(half["playback"]["underruns"], 2);
        assert_eq!(half["playback"]["takenFrom"], "pipewire");
        assert!(json(peak(0.0))["playback"]["conversion"]["peakDb"].is_null());
    }
}
