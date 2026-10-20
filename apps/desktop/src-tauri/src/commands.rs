//! Tauri commands: thin adapters from IPC to the engine. No playback policy
//! lives here.

// Tauri injects `State` and arguments by value.
#![expect(clippy::needless_pass_by_value, reason = "Tauri command signature")]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use player_core::Player;
use tauri::State;

use crate::ipc::{AppInfo, DeviceInfo, IpcError, Status, frames_to_seconds};

/// Engine plus the files the user dropped. Only dropped files can be played,
/// so the webview cannot ask Rust to open arbitrary paths.
pub struct AppState {
    pub player: Player,
    dropped: Mutex<HashSet<PathBuf>>,
}

impl AppState {
    pub fn new(player: Player) -> Self {
        Self {
            player,
            dropped: Mutex::new(HashSet::new()),
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
}

/// FLAC and WAV, by extension; content is checked when the file is opened.
pub fn is_playable(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("flac") || extension.eq_ignore_ascii_case("wav")
        })
}

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
    state.player.play(path, device).map_err(IpcError::from)
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
        .map(|track| track.source.sample_rate)
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
        .map_or(0, |track| track.source.sample_rate);
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
        assert!(!is_playable(Path::new("/music/c.mp3")));
        assert!(!is_playable(Path::new("/music/noext")));
    }

    #[test]
    fn seek_rounds_to_the_nearest_frame() {
        assert_eq!(seconds_to_frames(1.5, 44_100), 66_150);
        assert_eq!(seconds_to_frames(0.0, 96_000), 0);
    }
}
