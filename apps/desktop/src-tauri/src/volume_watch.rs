//! Follows the DAC's volume while Lyra runs, and levels it.
//!
//! The desktop's volume keys and slider change PipeWire's volume, which
//! moves the card's own controls even while Lyra holds the device. This
//! thread sleeps on the card's mixer events and passes changes on: to the UI
//! (the in-app control moves) and to the engine (the bit-perfect verdict
//! follows; DoP stops). It also wakes when the device to follow may have
//! changed: playback started or stopped, the UI shows another device, or a
//! card came or went.
//!
//! With volume levelling on, it sets the DAC for each track as its audio
//! reaches the DAC (see [`crate::levelling`]), and keeps every device under
//! its volume cap, the volume keys included.

use std::io;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use audio_alsa::{MixerEvents, Wake, Woken};
use tauri::{AppHandle, Emitter, Manager};
use unda::{PlaybackState, PlayerStatus};

use crate::commands::AppState;
use crate::ipc::{DeviceVolume, VolumeInfo};
use crate::levelling::{self, Offset};
use crate::settings::Levelling;

/// Event carrying a [`DeviceVolume`].
pub const VOLUME_EVENT: &str = "device-volume";

/// A change counts once the controls are quiet this long: setting a volume
/// moves several controls in a row (Lyra's own 0 dB correction too).
const SETTLE: Duration = Duration::from_millis(40);
/// Tracks after the one playing whose loudness is measured first.
const MEASURE_AHEAD: usize = 32;

pub fn spawn(handle: AppHandle, wake: Arc<Wake>) -> io::Result<()> {
    thread::Builder::new()
        .name("lyra-volume".into())
        .spawn(move || watch(&handle, &wake))
        .map(drop)
}

/// What the offset in effect was worked out from.
#[derive(Clone, Debug, PartialEq)]
struct Levelled {
    device: String,
    /// Queue entry: id and position.
    entry: (Option<i64>, usize),
    mode: Levelling,
    input: Option<(library::Loudness, levelling::Context)>,
}

/// An offset waiting for its track's audio to reach the DAC.
struct Due {
    at: Instant,
    device: String,
    offset: Option<Offset>,
}

fn watch(handle: &AppHandle, wake: &Wake) {
    let mut shown: Option<(String, Option<VolumeInfo>)> = None;
    let mut events: Option<(String, MixerEvents)> = None;
    let mut levelled: Option<Levelled> = None;
    let mut due: Option<Due> = None;
    let mut queue_revision = None;
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

        // Measure what is about to play first.
        if queue_revision != Some((status.queue_revision, status.track.is_some())) {
            queue_revision = Some((status.queue_revision, status.track.is_some()));
            let start = status.track.as_ref().map_or(0, |track| track.index);
            let queue = state.player.queue();
            let ids = queue.iter().skip(start).take(MEASURE_AHEAD);
            state
                .measurer
                .prioritise(ids.filter_map(|item| item.id).collect());
        }

        level(&state, &status, &mut levelled, &mut due);
        if let Some(pending) = due.take_if(|pending| pending.at <= Instant::now()) {
            let cap = state.settings().volume_cap(&pending.device);
            if let Some(target) = state.leveller.apply(&pending.device, pending.offset, cap) {
                let _ = audio_alsa::set_volume_db(&pending.device, target);
            }
        }

        if let Some(device) = &device {
            if mixer_changed {
                follow_mixer(&state, device);
            }
            show(handle, &state, device, &mut shown);
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
        let timeout = due
            .as_ref()
            .map(|pending| pending.at.saturating_duration_since(Instant::now()));
        let Some((_, mixer)) = &events else {
            wake.wait(None, timeout);
            continue;
        };
        match mixer.wait(wake, timeout) {
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

/// Starts, updates or ends levelling for what plays now. A new track's
/// offset waits until its audio reaches the DAC; a change of mode applies at
/// once, and so does a first measurement of the track playing.
fn level(
    state: &AppState,
    status: &PlayerStatus,
    levelled: &mut Option<Levelled>,
    due: &mut Option<Due>,
) {
    let mode = state.settings().levelling;
    let playing = status
        .track
        .as_ref()
        .filter(|_| mode != Levelling::Off)
        .filter(|_| matches!(status.state, PlaybackState::Playing | PlaybackState::Paused))
        .filter(|track| track.output.is_some() && status.volume_lock(&track.device).is_none());
    let started = playing.filter(|track| started_on(state, &track.device));
    let Some(track) = started else {
        *levelled = None;
        *due = None;
        state.end_levelling();
        return;
    };

    let now = Levelled {
        device: track.device.clone(),
        entry: (track.id, track.index),
        mode,
        input: state.levelling_input(status),
    };
    if levelled.as_ref() == Some(&now) {
        return;
    }
    let new_track = levelled
        .as_ref()
        .is_none_or(|before| (&before.device, before.entry) != (&now.device, now.entry));
    let same_mode = levelled
        .as_ref()
        .is_some_and(|before| before.mode == now.mode);
    // A level once set holds to the end of the track: an album measured
    // meanwhile takes over at the next one, without a jump mid-song.
    let holding = state
        .leveller
        .shown(&now.device)
        .is_some_and(|shown| shown.offset.is_some());
    if !new_track && same_mode && holding {
        *levelled = Some(now);
        return;
    }
    let offset = now
        .input
        .and_then(|(loudness, context)| levelling::offset(mode, loudness, context));
    // Not known yet (being measured): keep the level until it is.
    let offset = offset.or_else(|| {
        state
            .leveller
            .shown(&now.device)
            .and_then(|shown| shown.offset)
    });
    let wait = if new_track && levelled.is_some() {
        state.output_latency()
    } else {
        Duration::ZERO
    };
    *due = Some(Due {
        at: Instant::now() + wait,
        device: now.device.clone(),
        offset,
    });
    *levelled = Some(now);
}

/// Whether `device` is levelled, starting there when it is not yet: the
/// DAC's level is read once, and it must report dB.
fn started_on(state: &AppState, device: &str) -> bool {
    if state.leveller.device().as_deref() == Some(device) {
        return true;
    }
    state.end_levelling();
    let current = audio_alsa::volume(device)
        .ok()
        .flatten()
        .and_then(|level| level.decibels);
    if let Some(current) = current {
        state.leveller.begin(device, current);
    }
    current.is_some()
}

/// A settled change on `device`'s mixer: the volume keys move the listening
/// level, and nothing goes above the cap.
fn follow_mixer(state: &AppState, device: &str) {
    let Some(current) = audio_alsa::volume(device)
        .ok()
        .flatten()
        .and_then(|level| level.decibels)
    else {
        return;
    };
    let cap = state.settings().volume_cap(device);
    let correction = if state.leveller.device().as_deref() == Some(device) {
        state.leveller.mixer_changed(device, current, cap)
    } else {
        (current > cap + 0.05).then_some(cap)
    };
    if let Some(target) = correction
        && state.player.status().volume_lock(device).is_none()
    {
        let _ = audio_alsa::set_volume_db(device, target);
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
fn show(
    handle: &AppHandle,
    state: &AppState,
    device: &str,
    shown: &mut Option<(String, Option<VolumeInfo>)>,
) {
    let Ok(level) = audio_alsa::volume(device) else {
        return;
    };
    let now = (
        device.to_owned(),
        level.map(|level| state.volume_info(device, level)),
    );
    if shown.as_ref() != Some(&now) {
        let _ = handle.emit(
            VOLUME_EVENT,
            DeviceVolume {
                device: now.0.clone(),
                volume: now.1.clone(),
            },
        );
        *shown = Some(now);
    }
}
