//! Tauri commands: thin adapters from IPC to the engine. No playback policy
//! lives here.

// Tauri injects `State` and arguments by value.
#![expect(clippy::needless_pass_by_value, reason = "Tauri command signature")]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use library::{Library, LibraryError, Scanner};
use player_core::{Player, QueueItem};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use crate::ipc::{
    ART_URL, AlbumInfo, AppInfo, ArtistInfo, DeviceInfo, FolderInfo, IpcError, LibraryTrack,
    PlaylistEntry, PlaylistInfo, QueueEntry, ScanStatus, Status, TrackDetails, VolumeInfo,
    file_name, frames_to_seconds, ms_to_seconds,
};
use crate::settings::Settings;

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
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    /// Device whose volume the UI shows, for the volume watcher.
    watched: Mutex<Option<String>>,
}

impl AppState {
    pub fn new(
        player: Player,
        library: Library,
        scanner: Scanner,
        scan_status: Arc<Mutex<ScanStatus>>,
        art_dir: PathBuf,
        settings_path: PathBuf,
    ) -> Self {
        let state = Self {
            player,
            dropped: Mutex::new(HashSet::new()),
            library: Mutex::new(library),
            scanner,
            scan_status,
            art_dir,
            settings: Mutex::new(Settings::default()),
            settings_path,
            watched: Mutex::new(None),
        };
        state.apply_settings(Settings::load(&state.settings_path));
        state
    }

    pub fn watched_device(&self) -> Option<String> {
        locked(&self.watched).clone()
    }

    fn settings(&self) -> Settings {
        *locked(&self.settings)
    }

    /// Stores `settings` and applies what the engine uses.
    fn apply_settings(&self, settings: Settings) {
        self.player.set_resync_delay(settings.resync_delay());
        *locked(&self.settings) = settings;
    }

    pub fn allow(&self, paths: impl IntoIterator<Item = PathBuf>) {
        locked(&self.dropped).extend(paths);
    }

    fn is_allowed(&self, path: &Path) -> bool {
        locked(&self.dropped).contains(path)
    }

    fn library(&self) -> MutexGuard<'_, Library> {
        locked(&self.library)
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

/// Locks `mutex`. A thread that panicked while holding it leaves data that
/// is still valid for these uses (plain values and collections), so the lock
/// is taken anyway rather than failing every later command.
pub fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn library_error(error: &LibraryError) -> IpcError {
    let code = match error {
        LibraryError::AlreadyCovered { .. }
        | LibraryError::NotADirectory(_)
        | LibraryError::Invalid(_) => "invalid_request",
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
    let device = hardware_device(device)?;
    state
        .player
        .play(path, device, state.settings().dsd_policy())
        .map_err(IpcError::from)
}

/// Runs one library call and converts its error for the UI.
fn query<T>(
    state: &AppState,
    run: impl FnOnce(&mut Library) -> Result<T, LibraryError>,
) -> Result<T, IpcError> {
    run(&mut state.library()).map_err(|error| library_error(&error))
}

#[tauri::command(async)]
pub fn list_artists(state: State<'_, AppState>) -> Result<Vec<ArtistInfo>, IpcError> {
    let artists = query(&state, |library| library.artists())?;
    Ok(artists.into_iter().map(ArtistInfo::from).collect())
}

#[tauri::command(async)]
pub fn artist_albums(
    artist_id: i64,
    state: State<'_, AppState>,
) -> Result<Vec<AlbumInfo>, IpcError> {
    let albums = query(&state, |library| library.artist_albums(artist_id))?;
    Ok(albums.into_iter().map(AlbumInfo::from).collect())
}

#[tauri::command(async)]
pub fn list_tracks(state: State<'_, AppState>) -> Result<Vec<LibraryTrack>, IpcError> {
    let tracks = query(&state, |library| library.tracks())?;
    Ok(tracks.into_iter().map(LibraryTrack::from).collect())
}

#[tauri::command(async)]
pub fn list_playlists(state: State<'_, AppState>) -> Result<Vec<PlaylistInfo>, IpcError> {
    let playlists = query(&state, |library| library.playlists())?;
    Ok(playlists.into_iter().map(PlaylistInfo::from).collect())
}

/// Returns the new playlist's id.
#[tauri::command(async)]
pub fn create_playlist(name: String, state: State<'_, AppState>) -> Result<i64, IpcError> {
    query(&state, |library| library.create_playlist(&name))
}

#[tauri::command(async)]
pub fn rename_playlist(id: i64, name: String, state: State<'_, AppState>) -> Result<(), IpcError> {
    query(&state, |library| library.rename_playlist(id, &name))
}

#[tauri::command(async)]
pub fn delete_playlist(id: i64, state: State<'_, AppState>) -> Result<(), IpcError> {
    query(&state, |library| library.delete_playlist(id))
}

#[tauri::command(async)]
pub fn playlist_entries(
    id: i64,
    state: State<'_, AppState>,
) -> Result<Vec<PlaylistEntry>, IpcError> {
    let entries = query(&state, |library| library.playlist_entries(id))?;
    Ok(entries.into_iter().map(PlaylistEntry::from).collect())
}

#[tauri::command(async)]
pub fn add_to_playlist(
    id: i64,
    track_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> Result<(), IpcError> {
    query(&state, |library| library.add_to_playlist(id, &track_ids))
}

#[tauri::command(async)]
pub fn remove_from_playlist(entry_id: i64, state: State<'_, AppState>) -> Result<(), IpcError> {
    query(&state, |library| library.remove_from_playlist(entry_id))
}

#[tauri::command(async)]
pub fn move_in_playlist(
    entry_id: i64,
    to: usize,
    state: State<'_, AppState>,
) -> Result<(), IpcError> {
    query(&state, |library| library.move_in_playlist(entry_id, to))
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
    let device = hardware_device(device)?;
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
        .play_queue(items, start, device, state.settings().dsd_policy())
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
                duration_seconds: track.duration_ms.map(ms_to_seconds),
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
    locked(&state.scan_status).clone()
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

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

/// Saves new settings; they apply from the next track started.
#[tauri::command]
pub fn set_settings(settings: Settings, state: State<'_, AppState>) -> Result<Settings, IpcError> {
    if !settings.is_valid() {
        return Err(IpcError::invalid_request(
            "That setting is not one of the offered values.",
        ));
    }
    settings
        .save(&state.settings_path)
        .map_err(|error| IpcError {
            code: "settings_not_saved",
            message: format!("Cannot save the settings: {error}"),
        })?;
    state.apply_settings(settings);
    Ok(settings)
}

/// The hardware volume of `device`'s card; `None` when it has no control.
#[tauri::command]
pub fn get_volume(
    device: String,
    state: State<'_, AppState>,
) -> Result<Option<VolumeInfo>, IpcError> {
    let device = hardware_device(device)?;
    *locked(&state.watched) = Some(device.clone());
    let level = audio_alsa::volume(&device).map_err(|error| volume_error(&error))?;
    Ok(level.map(|level| volume_info(level, state.player.status().volume_lock(&device))))
}

/// Moves `device`'s hardware volume; refused while DoP plays on it.
#[tauri::command]
pub fn set_volume(
    device: String,
    percent: u8,
    state: State<'_, AppState>,
) -> Result<Option<VolumeInfo>, IpcError> {
    let device = hardware_device(device)?;
    if let Some(reason) = state.player.status().volume_lock(&device) {
        return Err(IpcError::invalid_request(reason));
    }
    let level = audio_alsa::set_volume(&device, percent).map_err(|error| volume_error(&error))?;
    Ok(level.map(|level| volume_info(level, None)))
}

fn hardware_device(device: String) -> Result<String, IpcError> {
    if device.starts_with("hw:") {
        Ok(device)
    } else {
        Err(IpcError::invalid_request(format!(
            "{device} is not a hardware device"
        )))
    }
}

pub fn volume_info(level: audio_alsa::VolumeLevel, locked: Option<&'static str>) -> VolumeInfo {
    VolumeInfo {
        percent: level.percent,
        decibels: level.decibels,
        usb: level.usb,
        locked,
    }
}

fn volume_error(error: &audio_alsa::AlsaError) -> IpcError {
    IpcError {
        code: "volume_failed",
        message: format!("Cannot use the device volume: {error}"),
    }
}
