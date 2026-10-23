//! Notices DACs being plugged in or out while Lyra runs, so the device
//! list follows without pressing Refresh.
//!
//! ALSA has no hot-plug notification of its own; the kernel's card list is
//! a few hundred bytes, read twice a second.

use std::io;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::wake::Wake;

/// Event with no payload: list the devices again.
pub const DEVICES_CHANGED_EVENT: &str = "devices-changed";

const INTERVAL: Duration = Duration::from_millis(500);

/// `volume` is woken too: the card it follows may have come or gone.
pub fn spawn(handle: AppHandle, volume: Arc<Wake>) -> io::Result<()> {
    thread::Builder::new()
        .name("lyra-devices".into())
        .spawn(move || watch(&handle, &volume))
        .map(drop)
}

fn watch(handle: &AppHandle, volume: &Wake) {
    let mut announced = audio_alsa::card_list().ok();
    let mut last = announced.clone();
    loop {
        thread::sleep(INTERVAL);
        let cards = audio_alsa::card_list().ok();
        // Announced once the same for two readings: a card that just
        // appeared may not have its devices ready yet.
        if cards == last && cards != announced {
            let _ = handle.emit(DEVICES_CHANGED_EVENT, ());
            volume.notify();
            announced.clone_from(&cards);
        }
        last = cards;
    }
}
