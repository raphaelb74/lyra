//! Types that cross the IPC boundary. Mirrored in `src/lib/ipc.ts`.
//!
//! Every fallible command rejects with [`IpcError`]: a stable `snake_case`
//! `code` for the UI to branch on and a complete, user-facing `message`.

use std::path::Path;

use audio_alsa::{Capabilities, Device, KernelReport, SinkReport};
use format_dsd::Container;
use format_pcm::{Codec, SampleEncoding};
use library::{
    Album, Artist, Edit, EditChange, Entry, Field, FieldState, Folder, Playlist, Proposal, Rule,
    ScanSummary, Target,
};
use musicbrainz::matching::{Candidate, credit_text};
use serde::{Deserialize, Serialize};
use unda::{
    BitPerfect, ConversionStats, Encoding, ErrorKind, HardwareVolume, NowPlaying, PlaybackState,
    PlayerError, PlayerStatus, Processing, Repeat, SourceInfo, Transport,
};

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
    pub shuffle: bool,
    pub repeat: RepeatMode,
    /// Changes whenever the queue does; the queue pane reloads on it.
    pub queue_revision: u64,
    /// Next has a track to go to (see [`PlayerStatus::has_next`]).
    pub has_next: bool,
}

/// [`Repeat`] on the wire: `off`, `all` or `one`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    Off,
    All,
    One,
}

impl From<Repeat> for RepeatMode {
    fn from(repeat: Repeat) -> Self {
        match repeat {
            Repeat::Off => Self::Off,
            Repeat::All => Self::All,
            Repeat::One => Self::One,
        }
    }
}

impl From<RepeatMode> for Repeat {
    fn from(mode: RepeatMode) -> Self {
        match mode {
            RepeatMode::Off => Self::Off,
            RepeatMode::All => Self::All,
            RepeatMode::One => Self::One,
        }
    }
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
    /// What UNDA does to the samples, in order; empty: sent as read.
    pub processing: Vec<Step>,
    pub bit_perfect: Option<Verdict>,
}

/// One processing step, as `{ "kind": "padded", "fromBits": 16, … }`.
#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Step {
    Padded { from_bits: u32, to_bits: u32 },
    DopPacked,
    Converted { gain_db: i8 },
}

impl From<Processing> for Step {
    fn from(step: Processing) -> Self {
        match step {
            Processing::Padded { from_bits, to_bits } => Self::Padded { from_bits, to_bits },
            Processing::DopPacked => Self::DopPacked,
            Processing::Converted { gain_db } => Self::Converted { gain_db },
        }
    }
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
    /// The device's own volume: `none` (no control), `unity` (0 dB),
    /// `attenuated` or `unknown` (no dB scale).
    pub hardware_volume: &'static str,
    /// Set when `attenuated`.
    pub hardware_db: Option<f64>,
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
            shuffle: status.shuffle,
            repeat: status.repeat.into(),
            queue_revision: status.queue_revision,
            has_next: status.has_next(),
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
                hardware_volume: match output.hardware_volume {
                    HardwareVolume::None => "none",
                    HardwareVolume::Unity => "unity",
                    HardwareVolume::Attenuated { .. } => "attenuated",
                    HardwareVolume::Unknown => "unknown",
                },
                hardware_db: match output.hardware_volume {
                    HardwareVolume::Attenuated { centibels } => {
                        i32::try_from(centibels).ok().map(|c| f64::from(c) / 100.0)
                    }
                    _ => None,
                },
            }),
            processing: now.processing().into_iter().map(Step::from).collect(),
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

/// Path of a thumbnail under [`ART_URL`] (see `thumbs.rs`).
pub const THUMB_PATH: &str = "thumb/";

/// The `lyra-art` URL of a cached artwork file name.
pub fn art_url(name: Option<&str>) -> Option<String> {
    name.map(|name| format!("{ART_URL}{name}"))
}

/// The URL of its thumbnail, for grids and lists.
pub fn thumb_url(name: Option<&str>) -> Option<String> {
    name.map(|name| format!("{ART_URL}{THUMB_PATH}{name}"))
}

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
    /// Its thumbnail, for the grid.
    pub thumb: Option<String>,
    pub track_count: u32,
    pub genre: Option<String>,
    pub duration_seconds: Option<f64>,
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
            art: art_url(album.art.as_deref()),
            thumb: thumb_url(album.art.as_deref()),
            track_count: album.tracks,
            genre: album.genre,
            duration_seconds: album.duration_ms.map(ms_to_seconds),
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
    /// `lyra-art://` URL of a cover standing in for a photo, or null.
    pub art: Option<String>,
}

impl From<Artist> for ArtistInfo {
    fn from(artist: Artist) -> Self {
        Self {
            id: artist.id,
            name: artist.name,
            album_count: artist.albums,
            track_count: artist.tracks,
            art: thumb_url(artist.art.as_deref()),
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
    /// Album cover thumbnail, a `lyra-art:` URL.
    pub art: Option<String>,
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
    /// Its thumbnail, for the player bar.
    pub thumb: Option<String>,
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
    /// Library folders that could not be read; their tracks were kept.
    pub unavailable: Vec<String>,
}

impl From<&ScanSummary> for ScanReport {
    fn from(summary: &ScanSummary) -> Self {
        Self {
            added: summary.added,
            updated: summary.updated,
            removed: summary.removed,
            failed: summary.failed,
            unavailable: summary
                .unavailable
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
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
    use unda::{ErrorKind, PlayerError};

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
        let peak = |peak| unda::ConversionStats { clipped: 3, peak };
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

/// A track or album, as edits name them: `{ "kind": "track", "id": 3 }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum TargetRef {
    Track(i64),
    Release(i64),
}

impl From<Target> for TargetRef {
    fn from(target: Target) -> Self {
        match target {
            Target::Track(id) => Self::Track(id),
            Target::Release(id) => Self::Release(id),
        }
    }
}

impl From<TargetRef> for Target {
    fn from(target: TargetRef) -> Self {
        match target {
            TargetRef::Track(id) => Self::Track(id),
            TargetRef::Release(id) => Self::Release(id),
        }
    }
}

/// One field in the editor.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldInfo {
    /// Stable name, sent back in a change.
    pub field: &'static str,
    pub label: &'static str,
    pub values: Vec<String>,
    /// May hold several values.
    pub multiple: bool,
    /// An edit is in force over the files.
    pub edited: bool,
    /// The file changed under the edit.
    pub review: bool,
}

impl From<FieldState> for FieldInfo {
    fn from(state: FieldState) -> Self {
        Self {
            field: state.field.name(),
            label: state.field.label(),
            values: state.values,
            multiple: state.field.is_multiple(),
            edited: state.edited,
            review: state.review,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorTrack {
    pub id: i64,
    pub file_name: String,
    pub fields: Vec<FieldInfo>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct RevisionInfo {
    pub target: TargetRef,
    pub revision: i64,
}

/// Everything the album editor shows, and the revisions it saw.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumEditor {
    pub album_id: i64,
    pub art: Option<String>,
    pub fields: Vec<FieldInfo>,
    pub tracks: Vec<EditorTrack>,
    pub revisions: Vec<RevisionInfo>,
}

/// A change from the editor.
#[derive(Clone, Debug, Deserialize)]
pub struct ChangeIn {
    pub target: TargetRef,
    pub field: String,
    pub edit: EditIn,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EditIn {
    Set { values: Vec<String> },
    Clear,
    Reset,
}

impl ChangeIn {
    pub fn into_change(self) -> Result<EditChange, IpcError> {
        let field = Field::from_name(&self.field)
            .ok_or_else(|| IpcError::invalid_request("Unknown field."))?;
        let edit = match self.edit {
            EditIn::Set { values } => Edit::Set(values.join("\0")),
            EditIn::Clear => Edit::Clear,
            EditIn::Reset => Edit::Reset,
        };
        Ok(EditChange {
            target: self.target.into(),
            field,
            edit,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RuleInfo {
    pub name: &'static str,
    pub label: &'static str,
}

/// A fix the cleanup rules propose.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalInfo {
    pub target: TargetRef,
    pub field: &'static str,
    pub field_label: &'static str,
    /// What it changes: `Album` or `Track — Album`.
    pub item: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub rules: Vec<RuleInfo>,
}

impl ProposalInfo {
    pub fn new(proposal: Proposal, item: String) -> Self {
        Self {
            target: proposal.target.into(),
            field: proposal.field.name(),
            field_label: proposal.field.label(),
            item,
            before: proposal.before,
            after: proposal.after,
            rules: proposal
                .rules
                .into_iter()
                .map(|rule| RuleInfo {
                    name: rule.name(),
                    label: rule.label(),
                })
                .collect(),
        }
    }
}

/// An accepted fix.
#[derive(Clone, Debug, Deserialize)]
pub struct AcceptIn {
    pub target: TargetRef,
    pub field: String,
    pub values: Vec<String>,
    pub rules: Vec<String>,
}

impl AcceptIn {
    /// The change, and its rules' names joined as an origin (`a+b`).
    pub fn into_change(self) -> Result<(EditChange, String), IpcError> {
        let unknown = || IpcError::invalid_request("Unknown fix rule.");
        let mut names = Vec::with_capacity(self.rules.len());
        for name in &self.rules {
            names.push(Rule::from_name(name).ok_or_else(unknown)?.name());
        }
        if names.is_empty() {
            return Err(unknown());
        }
        let change = ChangeIn {
            target: self.target,
            field: self.field,
            edit: EditIn::Set {
                values: self.values,
            },
        }
        .into_change()?;
        Ok((change, names.join("+")))
    }
}

/// One edition ID Match found, or the one being compared.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateInfo {
    pub release_id: String,
    pub title: String,
    /// The credit as printed.
    pub artist: String,
    pub disambiguation: Option<String>,
    pub date: Option<String>,
    pub country: Option<String>,
    pub status: Option<String>,
    /// `Label catalogue-number`, one per label.
    pub labels: Vec<String>,
    /// `2×CD`, `12" Vinyl`, …
    pub formats: Vec<String>,
    pub track_count: Option<u32>,
    pub evidence: EvidenceInfo,
}

/// What agrees between the album and an edition.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts, each shown to the listener"
)]
pub struct EvidenceInfo {
    pub release_id: bool,
    pub track_count: bool,
    pub title: bool,
    pub artist: bool,
    pub year: bool,
}

impl From<Candidate> for CandidateInfo {
    fn from(candidate: Candidate) -> Self {
        let Candidate { release, evidence } = candidate;
        let labels = release
            .label_info
            .iter()
            .filter_map(|info| {
                let name = info.label.as_ref().map(|label| label.name.as_str());
                let number = info.catalog_number.as_deref();
                match (name, number) {
                    (Some(name), Some(number)) => Some(format!("{name} {number}")),
                    (Some(text), None) | (None, Some(text)) => Some(text.to_owned()),
                    (None, None) => None,
                }
            })
            .collect();
        let mut formats: Vec<(String, u32)> = Vec::new();
        for medium in &release.media {
            let format = medium
                .format
                .clone()
                .unwrap_or_else(|| "Unknown".to_owned());
            match formats.last_mut() {
                Some((last, count)) if *last == format => *count += 1,
                _ => formats.push((format, 1)),
            }
        }
        let track_count = release.track_count.or_else(|| {
            (!release.media.is_empty())
                .then(|| release.media.iter().map(|medium| medium.track_count).sum())
        });
        Self {
            artist: credit_text(&release.artist_credit),
            release_id: release.id,
            title: release.title,
            disambiguation: release.disambiguation.filter(|text| !text.is_empty()),
            date: release.date.filter(|date| !date.is_empty()),
            country: release.country,
            status: release.status,
            labels,
            formats: formats
                .into_iter()
                .map(|(format, count)| {
                    if count > 1 {
                        format!("{count}×{format}")
                    } else {
                        format
                    }
                })
                .collect(),
            track_count,
            evidence: EvidenceInfo {
                release_id: evidence.release_id,
                track_count: evidence.track_count,
                title: evidence.title,
                artist: evidence.artist,
                year: evidence.year,
            },
        }
    }
}

/// An edition compared with the album: its tracks, the pairs, and what it
/// would change.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchInfo {
    pub release: CandidateInfo,
    /// The release's page on MusicBrainz, for attribution.
    pub url: String,
    pub media: Vec<MediumInfo>,
    /// The album's tracks, in album order.
    pub tracks: Vec<LocalTrackInfo>,
    pub pairs: Vec<PairInfo>,
    /// Album tracks with no release track.
    pub unpaired: Vec<i64>,
    /// Release tracks the album lacks.
    pub missing: Vec<SlotRef>,
    pub suggestions: Vec<SuggestionInfo>,
    /// What a save is checked against.
    pub revisions: Vec<RevisionInfo>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediumInfo {
    pub position: u32,
    pub format: Option<String>,
    pub title: Option<String>,
    pub tracks: Vec<ReleaseTrackInfo>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseTrackInfo {
    /// As printed: `3`, `A1`.
    pub number: String,
    pub title: String,
    pub seconds: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalTrackInfo {
    pub id: i64,
    pub disc: Option<String>,
    pub number: Option<String>,
    pub title: Option<String>,
    pub seconds: Option<f64>,
}

/// A release track: its medium and place, from 0.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotRef {
    pub medium: usize,
    pub index: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairInfo {
    pub track: i64,
    pub slot: SlotRef,
    /// `id`, `number`, `runningNumber`, `order` or `hand`.
    pub by: &'static str,
    /// Album length minus release length.
    pub length_difference_seconds: Option<f64>,
    pub lengths_agree: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionInfo {
    pub target: TargetRef,
    pub field: &'static str,
    pub field_label: &'static str,
    pub before: Vec<String>,
    pub after: Vec<String>,
    /// The listener edited this field; that edit is kept.
    pub kept: bool,
}

/// A pair the listener chose.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct PairIn {
    pub track: i64,
    pub slot: SlotRef,
}

/// A suggested field the listener accepted.
#[derive(Clone, Debug, Deserialize)]
pub struct FieldRef {
    pub target: TargetRef,
    pub field: String,
}
