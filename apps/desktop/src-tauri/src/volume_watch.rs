//! Follows the DAC's volume while Lyra runs.
//!
//! The desktop's volume keys and slider change PipeWire's volume, which
//! moves the card's own controls even while Lyra holds the device. This
//! thread reads them and passes changes on: to the UI (the in-app control
//! moves) and to the engine (the bit-perfect verdict follows; DoP stops).

use std::io;
use std::thread;
use std::time::Duration;

use player_core::{HardwareVolume, PlaybackState};
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::{AppState, volume_info, volume_lock};
use crate::ipc::DeviceVolume;

/// Event carrying a [`DeviceVolume`].
pub const VOLUME_EVENT: &str = "device-volume";

/// Short, so a volume key during DoP stops the noise within a fraction of
/// a second.
const INTERVAL: Duration = Duration::from_millis(100);

pub fn spawn(handle: AppHandle) -> io::Result<()> {
    thread::Builder::new()
        .name("lyra-volume".into())
        .spawn(move || watch(&handle))
        .map(drop)
}

fn watch(handle: &AppHandle) {
    let mut shown: Option<(String, Option<u8>, Option<i64>, bool)> = None;
    // A reading counts once seen twice in a row: one taken while Lyra sets
    // 0 dB at the start of a track must not stop it.
    let mut seen: Option<(String, HardwareVolume)> = None;
    let mut reported: Option<(String, HardwareVolume)> = None;
    loop {
        thread::sleep(INTERVAL);
        let Some(state) = handle.try_state::<AppState>() else {
            continue;
        };
        let status = state.player.status();
        let playing = status
            .track
            .as_ref()
            .filter(|_| !matches!(status.state, PlaybackState::Stopped | PlaybackState::Error))
            .map(|track| track.device.clone());
        let Some(device) = playing.clone().or_else(|| state.watched_device()) else {
            continue;
        };

        if let Ok(level) = audio_alsa::volume(&device) {
            let lock = volume_lock(&state, &device);
            // Compare in whole hundredths of a dB: readings are exact steps.
            #[expect(clippy::cast_possible_truncation, reason = "dB values are small")]
            let key = (
                device.clone(),
                level.map(|level| level.percent),
                level
                    .and_then(|level| level.decibels)
                    .map(|db| (db * 100.0).round() as i64),
                lock.is_some(),
            );
            if shown.as_ref() != Some(&key) {
                shown = Some(key);
                let _ = handle.emit(
                    VOLUME_EVENT,
                    DeviceVolume {
                        device: device.clone(),
                        volume: level.map(|level| volume_info(level, lock)),
                    },
                );
            }
        }

        if playing.as_deref() == Some(device.as_str()) {
            let reading = (device.clone(), audio_alsa::hardware_volume(&device));
            if seen.as_ref() == Some(&reading) && reported.as_ref() != Some(&reading) {
                let _ = state.player.volume_changed(reading.0.clone(), reading.1);
                reported = Some(reading.clone());
            }
            seen = Some(reading);
        } else {
            seen = None;
            reported = None;
        }
    }
}
