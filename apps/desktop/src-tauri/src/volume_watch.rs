//! Follows the DAC's volume while Lyra runs.
//!
//! The desktop's volume keys and slider change PipeWire's volume, which
//! moves the card's own controls even while Lyra holds the device. This
//! thread sleeps on the card's mixer events and passes changes on: to the UI
//! (the in-app control moves) and to the engine (the bit-perfect verdict
//! follows; DoP stops). It also wakes when the device to follow may have
//! changed: playback started or stopped, the UI shows another device, or a
//! card came or went.

use std::io;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use audio_alsa::{MixerEvents, Wake, Woken};
use tauri::{AppHandle, Emitter, Manager};
use unda::{PlaybackState, PlayerStatus};

use crate::commands::{AppState, volume_info};
use crate::ipc::DeviceVolume;

/// Event carrying a [`DeviceVolume`].
pub const VOLUME_EVENT: &str = "device-volume";

/// What the UI was last told: device, percent, hundredths of a dB, locked.
type Shown = (String, Option<u8>, Option<i64>, bool);

/// A change counts once the controls are quiet this long: setting a volume
/// moves several controls in a row (Lyra's own 0 dB correction too).
const SETTLE: Duration = Duration::from_millis(40);

pub fn spawn(handle: AppHandle, wake: Arc<Wake>) -> io::Result<()> {
    thread::Builder::new()
        .name("lyra-volume".into())
        .spawn(move || watch(&handle, &wake))
        .map(drop)
}

fn watch(handle: &AppHandle, wake: &Wake) {
    let mut shown: Option<Shown> = None;
    let mut events: Option<(String, MixerEvents)> = None;
    // Woken by the mixer, not only by news about which device to follow.
    let mut mixer_changed = false;
    loop {
        wake.drain();
        let Some(state) = handle.try_state::<AppState>() else {
            wake.wait(None, None);
            continue;
        };
        let status = state.player.status();
        let playing = status
            .track
            .as_ref()
            .filter(|_| !matches!(status.state, PlaybackState::Stopped | PlaybackState::Error))
            .map(|track| (track.device.clone(), track.output));
        let device = playing
            .as_ref()
            .map(|(device, _)| device.clone())
            .or_else(|| state.watched_device());

        if events.as_ref().map(|(watched, _)| watched) != device.as_ref() {
            events = device
                .as_ref()
                .and_then(|device| Some((device.clone(), MixerEvents::open(device).ok()?)));
        }

        if let Some(device) = &device {
            show(handle, &status, device, &mut shown);
            // Only a settled mixer change goes to the engine, and only when
            // the open stream thinks otherwise: the engine measures the
            // volume itself when it opens the device.
            if mixer_changed
                && let Some((playing, Some(output))) = &playing
                && playing == device
            {
                let volume = audio_alsa::hardware_volume(device);
                if volume != output.hardware_volume {
                    let _ = state.player.volume_changed(device.clone(), volume);
                }
            }
        }

        mixer_changed = false;
        let Some((_, mixer)) = &events else {
            wake.wait(None, None);
            continue;
        };
        match mixer.wait(wake, None) {
            Woken::Mixer => mixer_changed = settle(mixer, wake),
            Woken::Gone => {
                // Unplugged: look again once a card comes or goes.
                events = None;
                wake.wait(None, None);
            }
            Woken::Other | Woken::Timeout => {}
        }
    }
}

/// Waits for the controls to be quiet for [`SETTLE`]. False when the card
/// went away meanwhile.
fn settle(mixer: &MixerEvents, wake: &Wake) -> bool {
    loop {
        match mixer.wait(wake, Some(SETTLE)) {
            Woken::Mixer => {}
            // Other news is handled with this change.
            Woken::Timeout | Woken::Other => return true,
            Woken::Gone => return false,
        }
    }
}

/// Tells the UI the card's volume when it differs from what it shows.
fn show(handle: &AppHandle, status: &PlayerStatus, device: &str, shown: &mut Option<Shown>) {
    let Ok(level) = audio_alsa::volume(device) else {
        return;
    };
    let lock = status.volume_lock(device);
    // Compare in whole hundredths of a dB: readings are exact steps.
    #[expect(clippy::cast_possible_truncation, reason = "dB values are small")]
    let key = (
        device.to_owned(),
        level.map(|level| level.percent),
        level
            .and_then(|level| level.decibels)
            .map(|db| (db * 100.0).round() as i64),
        lock.is_some(),
    );
    if shown.as_ref() != Some(&key) {
        *shown = Some(key);
        let _ = handle.emit(
            VOLUME_EVENT,
            DeviceVolume {
                device: device.to_owned(),
                volume: level.map(|level| volume_info(level, lock)),
            },
        );
    }
}
