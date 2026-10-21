//! Tauri commands: thin adapters from IPC to the engine. No playback policy
//! lives here.

// Tauri injects `State` and arguments by value.
#![expect(clippy::needless_pass_by_value, reason = "Tauri command signature")]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use library::{Library, LibraryError, Scanner};
use player_core::{DsdTransport, Player, QueueItem};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use crate::ipc::{
    ART_URL, AlbumInfo, AppInfo, DeviceInfo, FolderInfo, IpcError, LibraryTrack, QueueEntry,
    ScanStatus, Status, TrackDetails, file_name, frames_to_seconds,
};

/// Event telling the UI to reload folders and albums.
pub const LIBRARY_CHANGED_EVENT: &str = "library-changed";
/// Event carrying a sentence for the user about a folder they added.
pub const LIBRARY_NOTICE_EVENT: &str = "library-notice";

/// Engine, library, and the files the user dropped. Only dropped files and
/// tracks inside library folders can be played, so the webview cannot ask
/// Rust to open arbitrary paths.
pub struct AppState {
    pub player: Player,
    dropped: Mutex<HashSet<PathBuf>>,
    /// The UI's connection; the scanner has its own.
    library: Mutex<Library>,
    pub scanner: Scanner,
    /// Last scan state, kept by the scanner's listener.
    pub scan_status: Arc<Mutex<ScanStatus>>,
    pub art_dir: PathBuf,
}

impl AppState {
    pub fn new(
        player: Player,
        library: Library,
        scanner: Scanner,
        scan_status: Arc<Mutex<ScanStatus>>,
        art_dir: PathBuf,
    ) -> Self {
        Self {
            player,
            dropped: Mutex::new(HashSet::new()),
            library: Mutex::new(library),
            scanner,
            scan_status,
            art_dir,
        }
    }

    pub fn allow(&self, paths: impl IntoIterator<Item = PathBuf>) {
        self.dropped
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(paths);
    }

    fn is_allowed(&self, path: &Path) -> bool {
        self.dropped
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(path)
    }

    fn library(&self) -> std::sync::MutexGuard<'_, Library> {
        self.library.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Adds a folder the user chose (dropped or picked) and scans it.
    /// Returns a sentence for the user when it cannot be added.
    pub fn add_folder(&self, path: &Path) -> Result<(), String> {
        self.library()
            .add_folder(path)
            .map_err(|error| library_error(&error).message)?;
        self.scanner.request();
        Ok(())
    }
}

fn library_error(error: &LibraryError) -> IpcError {
    let code = match error {
        LibraryError::AlreadyCovered { .. } | LibraryError::NotADirectory(_) => "invalid_request",
        LibraryError::Db(_) | LibraryError::Io(_) => "library_failed",
    };
    IpcError {
        code,
        message: error.to_string(),
    }
}

pub use library::is_audio_file as is_playable;

#[tauri::command]
pub fn get_app_info() -> AppInfo {
    AppInfo::current()
}

/// Lists ALSA playback devices. Runs off the main thread: it talks to drivers.
#[tauri::command(async)]
pub fn list_devices() -> Result<Vec<DeviceInfo>, IpcError> {
    audio_alsa::list_playback_devices()
        .map(|devices| devices.into_iter().map(DeviceInfo::from).collect())
        .map_err(|error| IpcError {
            code: "device_list_failed",
            message: format!("Cannot list audio devices: {error}"),
        })
}

#[tauri::command]
pub fn play(path: String, device: String, state: State<'_, AppState>) -> Result<(), IpcError> {
    let path = PathBuf::from(path);
    if !state.is_allowed(&path) {
        return Err(IpcError::invalid_request(
            "Drop the file onto the Lyra window to play it.",
        ));
    }
    if !device.starts_with("hw:") {
        return Err(IpcError::invalid_request(format!(
            "{device} is not a hardware device"
        )));
    }
    // Native DSD when the DAC exposes it, else DoP (a setting from M4).
    state
        .player
        .play(path, device, DsdTransport::Auto)
        .map_err(IpcError::from)
}

/// Plays library tracks as the queue, from `start` (e.g. an album from the
/// clicked track).
#[tauri::command(async)]
pub fn play_tracks(
    track_ids: Vec<i64>,
    start: usize,
    device: String,
    state: State<'_, AppState>,
) -> Result<(), IpcError> {
    if !device.starts_with("hw:") {
        return Err(IpcError::invalid_request(format!(
            "{device} is not a hardware device"
        )));
    }
    let items = {
        let library = state.library();
        let mut items = Vec::with_capacity(track_ids.len());
        for id in track_ids {
            let track = library
                .track(id)
                .map_err(|error| library_error(&error))?
                .ok_or_else(|| {
                    IpcError::invalid_request("A track is no longer in the library. Rescan.")
                })?;
            items.push(QueueItem {
                path: track.path,
                id: Some(id),
            });
        }
        items
    };
    state
        .player
        .play_queue(items, start, device, DsdTransport::Auto)
        .map_err(IpcError::from)
}

#[tauri::command]
pub fn next(state: State<'_, AppState>) -> Result<(), IpcError> {
    state.player.next().map_err(IpcError::from)
}

#[tauri::command]
pub fn previous(state: State<'_, AppState>) -> Result<(), IpcError> {
    state.player.previous().map_err(IpcError::from)
}

#[tauri::command]
pub fn skip_to(index: usize, state: State<'_, AppState>) -> Result<(), IpcError> {
    state.player.skip_to(index).map_err(IpcError::from)
}

/// The queue, described from the library where possible.
#[tauri::command(async)]
pub fn get_queue(state: State<'_, AppState>) -> Result<Vec<QueueEntry>, IpcError> {
    let queue = state.player.queue();
    let library = state.library();
    let mut entries = Vec::with_capacity(queue.len());
    for item in queue.iter() {
        let track = match item.id {
            Some(id) => library.track(id).map_err(|error| library_error(&error))?,
            None => None,
        };
        entries.push(match track {
            Some(track) => QueueEntry {
                track_id: Some(track.id),
                duration_seconds: LibraryTrack::from(track.clone()).duration_seconds,
                title: track.title,
                artist: Some(track.artist),
            },
            None => QueueEntry {
                track_id: None,
                title: file_name(&item.path),
                artist: None,
                duration_seconds: None,
            },
        });
    }
    Ok(entries)
}

/// Title, album and artwork of a library track, for Now Playing.
#[tauri::command(async)]
pub fn track_details(
    track_id: i64,
    state: State<'_, AppState>,
) -> Result<Option<TrackDetails>, IpcError> {
    let library = state.library();
    let Some(track) = library
        .track(track_id)
        .map_err(|error| library_error(&error))?
    else {
        return Ok(None);
    };
    let album = library
        .album(track.album_id)
        .map_err(|error| library_error(&error))?;
    Ok(Some(TrackDetails {
        title: track.title,
        artist: track.artist,
        album_id: track.album_id,
        year: album.as_ref().and_then(|album| album.year),
        art: album
            .as_ref()
            .and_then(|album| album.art.as_ref())
            .map(|name| format!("{ART_URL}{name}")),
        album: album.map(|album| album.title),
    }))
}

#[tauri::command(async)]
pub fn list_folders(state: State<'_, AppState>) -> Result<Vec<FolderInfo>, IpcError> {
    let folders = state
        .library()
        .folders()
        .map_err(|error| library_error(&error))?;
    Ok(folders.into_iter().map(FolderInfo::from).collect())
}

/// Opens the system folder picker. The choice arrives as `library-changed`
/// or `library-notice`; the webview never sends a path.
#[tauri::command]
pub fn pick_folder(app: AppHandle) {
    let handle = app.clone();
    app.dialog()
        .file()
        .set_title("Add a music folder")
        .pick_folder(move |picked| {
            let Some(path) = picked.and_then(|picked| picked.into_path().ok()) else {
                return; // Cancelled.
            };
            let state = tauri::Manager::state::<AppState>(&handle);
            let _ = match state.add_folder(&path) {
                Ok(()) => handle.emit(LIBRARY_CHANGED_EVENT, ()),
                Err(message) => handle.emit(LIBRARY_NOTICE_EVENT, message),
            };
        });
}

/// Removes a folder and its tracks from the library; the files stay on disk.
#[tauri::command(async)]
pub fn remove_folder(id: i64, app: AppHandle, state: State<'_, AppState>) -> Result<(), IpcError> {
    state.scanner.cancel();
    let unused = state
        .library()
        .remove_folder(id)
        .map_err(|error| library_error(&error))?;
    library::delete_art(&state.art_dir, &unused);
    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());
    Ok(())
}

#[tauri::command]
pub fn rescan(state: State<'_, AppState>) {
    state.scanner.request();
}

#[tauri::command]
pub fn get_scan_status(state: State<'_, AppState>) -> ScanStatus {
    state
        .scan_status
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

#[tauri::command(async)]
pub fn list_albums(state: State<'_, AppState>) -> Result<Vec<AlbumInfo>, IpcError> {
    let albums = state
        .library()
        .albums()
        .map_err(|error| library_error(&error))?;
    Ok(albums.into_iter().map(AlbumInfo::from).collect())
}

#[tauri::command(async)]
pub fn album_tracks(
    album_id: i64,
    state: State<'_, AppState>,
) -> Result<Vec<LibraryTrack>, IpcError> {
    let tracks = state
        .library()
        .album_tracks(album_id)
        .map_err(|error| library_error(&error))?;
    Ok(tracks.into_iter().map(LibraryTrack::from).collect())
}

#[tauri::command]
pub fn pause(state: State<'_, AppState>) -> Result<(), IpcError> {
    state.player.pause().map_err(IpcError::from)
}

#[tauri::command]
pub fn resume(state: State<'_, AppState>) -> Result<(), IpcError> {
    state.player.resume().map_err(IpcError::from)
}

#[tauri::command]
pub fn stop(state: State<'_, AppState>) -> Result<(), IpcError> {
    state.player.stop().map_err(IpcError::from)
}

#[tauri::command]
pub fn seek(seconds: f64, state: State<'_, AppState>) -> Result<(), IpcError> {
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(IpcError::invalid_request(
            "Seek position must be zero or more seconds.",
        ));
    }
    let Some(rate) = state
        .player
        .status()
        .track
        .map(|track| track.source.frame_rate())
    else {
        return Err(IpcError::invalid_request("Nothing is playing."));
    };
    state
        .player
        .seek(seconds_to_frames(seconds, rate))
        .map_err(IpcError::from)
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> Status {
    Status::from(&state.player.status())
}

/// Seconds of the current track handed to the device.
#[tauri::command]
pub fn get_position(state: State<'_, AppState>) -> f64 {
    let rate = state
        .player
        .status()
        .track
        .map_or(0, |track| track.source.frame_rate());
    if rate == 0 {
        0.0
    } else {
        frames_to_seconds(state.player.position(), rate)
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "seconds is finite and non-negative; the cast saturates past u64::MAX"
)]
fn seconds_to_frames(seconds: f64, sample_rate: u32) -> u64 {
    (seconds * f64::from(sample_rate)).round() as u64
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{is_playable, seconds_to_frames};

    #[test]
    fn plays_flac_and_wav_only() {
        assert!(is_playable(Path::new("/music/a.flac")));
        assert!(is_playable(Path::new("/music/B.WAV")));
        assert!(is_playable(Path::new("/music/c.dsf")));
        assert!(is_playable(Path::new("/music/d.DFF")));
        assert!(!is_playable(Path::new("/music/c.mp3")));
        assert!(!is_playable(Path::new("/music/noext")));
    }

    #[test]
    fn seek_rounds_to_the_nearest_frame() {
        assert_eq!(seconds_to_frames(1.5, 44_100), 66_150);
        assert_eq!(seconds_to_frames(0.0, 96_000), 0);
    }
}
