//! Tauri commands: thin adapters from IPC to the engine. No playback policy
//! lives here.

// Tauri injects `State` and arguments by value.
#![expect(clippy::needless_pass_by_value, reason = "Tauri command signature")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use audio_alsa::SinkStats;
use library::{EditChange, Library, LibraryError, Origin, Revisions, Scanner, Target};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;
use unda::{Player, QueueItem};

use crate::ipc::{
    AcceptIn, AlbumEditor, AlbumInfo, AppInfo, ArtistInfo, CapabilitiesInfo, ChangeIn, DeviceInfo,
    DiagnosticsInfo, EditorTrack, FieldInfo, FolderInfo, IpcError, LibraryTrack, PlaylistEntry,
    PlaylistInfo, ProposalInfo, QueueEntry, RepeatMode, RevisionInfo, ScanStatus, Status,
    TrackDetails, VolumeInfo, art_url, file_name, frames_to_seconds, ms_to_seconds, thumb_url,
};
use crate::settings::Settings;

/// Event telling the UI to reload folders and albums.
pub const LIBRARY_CHANGED_EVENT: &str = "library-changed";
/// Event carrying a sentence for the user about a folder they added.
pub const LIBRARY_NOTICE_EVENT: &str = "library-notice";

/// Engine and library. Only tracks inside library folders can be played, by
/// library id, so the webview cannot ask Rust to open arbitrary paths.
pub struct AppState {
    pub player: Player,
    /// The engine's sink, read for diagnostics.
    sink_stats: Arc<SinkStats>,
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
    /// MusicBrainz and the Cover Art Archive, for ID Match. Makes no request
    /// until a command asks, and those refuse while lookups are off.
    pub musicbrainz: musicbrainz::Client,
    /// The last cover ID Match showed, by release ID, so saving it does not
    /// fetch it again.
    pub match_cover: Mutex<Option<(String, Vec<u8>)>>,
}

impl AppState {
    pub fn new(
        player: Player,
        sink_stats: Arc<SinkStats>,
        library: Library,
        scanner: Scanner,
        scan_status: Arc<Mutex<ScanStatus>>,
        art_dir: PathBuf,
        settings_path: PathBuf,
    ) -> Self {
        let state = Self {
            player,
            sink_stats,
            library: Mutex::new(library),
            scanner,
            scan_status,
            art_dir,
            settings: Mutex::new(Settings::default()),
            settings_path,
            watched: Mutex::new(None),
            musicbrainz: musicbrainz::Client::new(
                musicbrainz::Service::official(),
                env!("CARGO_PKG_VERSION"),
            ),
            match_cover: Mutex::new(None),
        };
        state.apply_settings(Settings::load(&state.settings_path));
        state
    }

    pub fn watched_device(&self) -> Option<String> {
        locked(&self.watched).clone()
    }

    pub fn settings(&self) -> Settings {
        locked(&self.settings).clone()
    }

    /// Stores `settings` and applies what the engine uses.
    fn apply_settings(&self, settings: Settings) {
        self.player.set_resync_delay(settings.resync_delay());
        *locked(&self.settings) = settings;
    }

    fn library(&self) -> MutexGuard<'_, Library> {
        locked(&self.library)
    }

    /// Title, album and artwork of a library track.
    pub fn track_details(&self, track_id: i64) -> Result<Option<TrackDetails>, IpcError> {
        let library = self.library();
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
            art: art_url(album.as_ref().and_then(|album| album.art.as_deref())),
            thumb: thumb_url(album.as_ref().and_then(|album| album.art.as_deref())),
            album: album.map(|album| album.title),
        }))
    }

    /// Moves playback to `seconds` into the current track.
    pub fn seek(&self, seconds: f64) -> Result<(), IpcError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(IpcError::invalid_request(
                "Seek position must be zero or more seconds.",
            ));
        }
        let Some(rate) = self
            .player
            .status()
            .track
            .map(|track| track.source.frame_rate())
        else {
            return Err(IpcError::invalid_request("Nothing is playing."));
        };
        self.player
            .seek(seconds_to_frames(seconds, rate))
            .map_err(IpcError::from)
    }

    /// Seconds of the current track handed to the device.
    pub fn position(&self) -> f64 {
        let rate = self
            .player
            .status()
            .track
            .map_or(0, |track| track.source.frame_rate());
        if rate == 0 {
            0.0
        } else {
            frames_to_seconds(self.player.position(), rate)
        }
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
        | LibraryError::Invalid(_)
        | LibraryError::InvalidEdit { .. } => "invalid_request",
        LibraryError::Stale | LibraryError::UndoConflict => "changed_meanwhile",
        LibraryError::Db(_) | LibraryError::Io(_) | LibraryError::Upgrade { .. } => {
            "library_failed"
        }
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

/// Runs one library call and converts its error for the UI.
pub fn query<T>(
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
    let items = queue_items(&state, track_ids)?;
    state
        .player
        .play_queue(
            items,
            start,
            device.clone(),
            state.settings().dsd_policy(&device),
        )
        .map_err(IpcError::from)
}

/// Adds library tracks after the playing one (`next`) or at the end of the
/// queue; plays them on `device` when nothing plays.
#[tauri::command(async)]
pub fn enqueue_tracks(
    track_ids: Vec<i64>,
    next: bool,
    device: String,
    state: State<'_, AppState>,
) -> Result<(), IpcError> {
    let device = hardware_device(device)?;
    let items = queue_items(&state, track_ids)?;
    state
        .player
        .enqueue(
            items,
            next,
            device.clone(),
            state.settings().dsd_policy(&device),
        )
        .map_err(IpcError::from)
}

#[tauri::command]
pub fn queue_move(
    index: usize,
    after: Option<usize>,
    state: State<'_, AppState>,
) -> Result<(), IpcError> {
    state
        .player
        .move_entry(index, after)
        .map_err(IpcError::from)
}

#[tauri::command]
pub fn queue_remove(index: usize, state: State<'_, AppState>) -> Result<(), IpcError> {
    state.player.remove(index).map_err(IpcError::from)
}

#[tauri::command]
pub fn clear_queue(state: State<'_, AppState>) -> Result<(), IpcError> {
    state.player.clear_queue().map_err(IpcError::from)
}

/// Queue entries for library tracks, in the given order.
fn queue_items(state: &AppState, track_ids: Vec<i64>) -> Result<Vec<QueueItem>, IpcError> {
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
    Ok(items)
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
pub fn set_shuffle(on: bool, state: State<'_, AppState>) -> Result<(), IpcError> {
    state.player.set_shuffle(on).map_err(IpcError::from)
}

#[tauri::command]
pub fn set_repeat(mode: RepeatMode, state: State<'_, AppState>) -> Result<(), IpcError> {
    state.player.set_repeat(mode.into()).map_err(IpcError::from)
}

#[tauri::command]
pub fn skip_to(index: usize, device: String, state: State<'_, AppState>) -> Result<(), IpcError> {
    let device = hardware_device(device)?;
    state
        .player
        .skip_to(index, device.clone(), state.settings().dsd_policy(&device))
        .map_err(IpcError::from)
}

/// The queue, described from the library where possible.
#[tauri::command(async)]
pub fn get_queue(state: State<'_, AppState>) -> Result<Vec<QueueEntry>, IpcError> {
    let queue = state.player.queue();
    let library = state.library();
    let mut entries = Vec::with_capacity(queue.len());
    // Covers by album: a queue is mostly whole albums.
    let mut covers: HashMap<i64, Option<String>> = HashMap::new();
    for item in queue.iter() {
        let track = match item.id {
            Some(id) => library.track(id).map_err(|error| library_error(&error))?,
            None => None,
        };
        entries.push(match track {
            Some(track) => {
                let art = if let Some(art) = covers.get(&track.album_id) {
                    art.clone()
                } else {
                    let album = library
                        .album(track.album_id)
                        .map_err(|error| library_error(&error))?;
                    let art = thumb_url(album.and_then(|album| album.art).as_deref());
                    covers.insert(track.album_id, art.clone());
                    art
                };
                QueueEntry {
                    track_id: Some(track.id),
                    duration_seconds: track.duration_ms.map(ms_to_seconds),
                    title: track.title,
                    artist: Some(track.artist),
                    art,
                }
            }
            None => QueueEntry {
                track_id: None,
                title: file_name(&item.path),
                artist: None,
                duration_seconds: None,
                art: None,
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
    state.track_details(track_id)
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
    state.seek(seconds)
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> Status {
    Status::from(&state.player.status())
}

/// Seconds of the current track handed to the device.
#[tauri::command]
pub fn get_position(state: State<'_, AppState>) -> f64 {
    state.position()
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
    state.apply_settings(settings.clone());
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

/// What the kernel and Lyra report about `device` right now. Cheap: procfs
/// and counters only, so the UI can refresh it every second.
#[tauri::command(async)]
pub fn get_diagnostics(
    device: String,
    state: State<'_, AppState>,
) -> Result<DiagnosticsInfo, IpcError> {
    let device = hardware_device(device)?;
    let playback = state
        .sink_stats
        .report()
        .filter(|report| report.device == device);
    let conversion = playback.as_ref().and_then(|_| state.player.conversion());
    let kernel = audio_alsa::kernel_report(&device);
    Ok(DiagnosticsInfo::new(device, kernel, playback, conversion))
}

/// Opens `device` briefly and asks the driver what it accepts.
#[tauri::command(async)]
pub fn probe_device(
    device: String,
    state: State<'_, AppState>,
) -> Result<CapabilitiesInfo, IpcError> {
    let device = hardware_device(device)?;
    if state
        .sink_stats
        .report()
        .is_some_and(|report| report.device == device)
    {
        return Err(IpcError::invalid_request(
            "Lyra is playing on this device. Stop playback to see every format it accepts.",
        ));
    }
    let probe_error = |message: String| IpcError {
        code: "probe_failed",
        message,
    };
    let found = audio_alsa::list_playback_devices()
        .map_err(|error| probe_error(format!("Cannot list devices: {error}")))?
        .into_iter()
        .find(|candidate| candidate.name == device)
        .ok_or_else(|| probe_error(format!("{device} not found; is the DAC connected?")))?;
    audio_alsa::probe(&found)
        .map(CapabilitiesInfo::from)
        .map_err(|error| probe_error(error.to_string()))
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

/// The album editor's fields: the album's, each track's, and the revisions
/// a save is checked against.
#[tauri::command(async)]
pub fn album_editor(album_id: i64, state: State<'_, AppState>) -> Result<AlbumEditor, IpcError> {
    query(&state, |library| {
        let album = library.album(album_id)?.ok_or_else(|| {
            LibraryError::Invalid("That album is no longer in the library.".into())
        })?;
        let fields = |target| -> Result<Vec<FieldInfo>, LibraryError> {
            Ok(library
                .fields(target)?
                .into_iter()
                .map(FieldInfo::from)
                .collect())
        };
        let mut targets = vec![Target::Release(album_id)];
        let mut tracks = Vec::new();
        for track in library.album_tracks(album_id)? {
            targets.push(Target::Track(track.id));
            tracks.push(EditorTrack {
                id: track.id,
                file_name: file_name(&track.path),
                fields: fields(Target::Track(track.id))?,
            });
        }
        let revisions = library
            .revisions(&targets)?
            .iter()
            .map(|(target, revision)| RevisionInfo {
                target: target.into(),
                revision,
            })
            .collect();
        Ok(AlbumEditor {
            album_id,
            art: art_url(album.art.as_deref()),
            fields: fields(Target::Release(album_id))?,
            tracks,
            revisions,
        })
    })
}

/// Saves the editor's changes in the library, as one undoable step, unless
/// something it showed changed since (`changed_meanwhile`).
#[tauri::command(async)]
pub fn save_album_edits(
    changes: Vec<ChangeIn>,
    revisions: Vec<RevisionInfo>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), IpcError> {
    let changes = changes
        .into_iter()
        .map(ChangeIn::into_change)
        .collect::<Result<Vec<_>, _>>()?;
    let seen: Revisions = revisions
        .into_iter()
        .map(|info| (Target::from(info.target), info.revision))
        .collect();
    query(&state, |library| {
        library.commit_edits(&changes, &Origin::Manual, &seen)
    })?;
    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());
    Ok(())
}

/// Undoes the last saved change. `false` when there is none.
#[tauri::command(async)]
pub fn undo_edit(app: AppHandle, state: State<'_, AppState>) -> Result<bool, IpcError> {
    let undone = query(&state, Library::undo_edits)?;
    if undone {
        let _ = app.emit(LIBRARY_CHANGED_EVENT, ());
    }
    Ok(undone)
}

/// Opens the system image picker for an album's artwork. The result arrives
/// as `library-changed` or `library-notice`; the webview never sends a path.
#[tauri::command]
pub fn choose_artwork(album_id: i64, app: AppHandle) {
    let handle = app.clone();
    app.dialog()
        .file()
        .set_title("Choose album artwork")
        .add_filter("Images", &["jpg", "jpeg", "png", "webp"])
        .pick_file(move |picked| {
            let Some(path) = picked.and_then(|picked| picked.into_path().ok()) else {
                return; // Cancelled.
            };
            let state = tauri::Manager::state::<AppState>(&handle);
            let result = state
                .library()
                .choose_artwork(&state.art_dir, album_id, &path);
            let _ = match result {
                Ok(()) => handle.emit(LIBRARY_CHANGED_EVENT, ()),
                Err(error) => handle.emit(LIBRARY_NOTICE_EVENT, error.to_string()),
            };
        });
}

/// Shows the files' artwork for an album again.
#[tauri::command(async)]
pub fn reset_artwork(
    album_id: i64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), IpcError> {
    query(&state, |library| {
        library.reset_artwork(&state.art_dir, album_id)
    })?;
    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());
    Ok(())
}

/// Fix tags: what the cleanup rules propose, for one album or (`None`)
/// the whole library. Changes nothing; nothing goes online.
#[tauri::command(async)]
pub fn propose_fixes(
    album_id: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<ProposalInfo>, IpcError> {
    query(&state, |library| {
        let albums: HashMap<i64, String> = library
            .albums()?
            .into_iter()
            .map(|album| (album.id, album.title))
            .collect();
        let tracks: HashMap<i64, (i64, String)> = library
            .tracks()?
            .into_iter()
            .map(|track| (track.id, (track.album_id, track.title)))
            .collect();
        let album_of = |target: Target| match target {
            Target::Release(id) => Some(id),
            Target::Track(id) => tracks.get(&id).map(|(album, _)| *album),
        };
        let mut proposals = Vec::new();
        for proposal in library.propose_cleanup()? {
            let album = album_of(proposal.target);
            if album_id.is_some() && album != album_id {
                continue;
            }
            let album_title = album
                .and_then(|id| albums.get(&id))
                .cloned()
                .unwrap_or_default();
            let item = match proposal.target {
                Target::Release(_) => album_title,
                Target::Track(id) => {
                    let title = tracks.get(&id).map_or("", |(_, title)| title.as_str());
                    format!("{title} — {album_title}")
                }
            };
            proposals.push(ProposalInfo::new(proposal, item));
        }
        Ok(proposals)
    })
}

/// Accepts proposed fixes as edits, as one undoable step; each keeps
/// the rules that made it as its origin.
#[tauri::command(async)]
pub fn apply_fixes(
    accepted: Vec<AcceptIn>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), IpcError> {
    let changes = accepted
        .into_iter()
        .map(|item| {
            let (change, rules) = item.into_change()?;
            Ok((change, Origin::Rule(rules)))
        })
        .collect::<Result<Vec<(EditChange, Origin)>, IpcError>>()?;
    query(&state, |library| library.save_edits_each(&changes))?;
    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());
    Ok(())
}
