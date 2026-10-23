//! Volume levelling through the DAC's own volume: the samples are never
//! touched. The user's slider sets a listening level; while a track plays,
//! the DAC is set to that level plus the track's (or album's) offset from a
//! reference loudness, never above the device's cap or 0 dB.
//!
//! The volume watcher drives it (it sees the DAC's mixer events, so it can
//! tell Lyra's own changes from the volume keys); the volume commands read
//! and move the listening level through it.

use std::sync::Mutex;

use library::{Library, Loudness};
use unda::{PlayerStatus, QueueItem};

use crate::commands::locked;
use crate::settings::Levelling;

/// Loudness the offsets aim at (as `REPLAYGAIN_*` tags): tracks louder than this
/// are turned down, quieter ones up.
pub const REFERENCE_LUFS: f64 = -18.0;
/// Most a quiet track is raised: tags or measurements can be wrong, and
/// the listening level was set by ear.
pub const MAX_BOOST_DB: f64 = 6.0;
/// A DAC reading this close to what Lyra set is Lyra's own change: volume
/// controls move in steps (often 0.5 dB or 1/256 dB).
const OWN_CHANGE_DB: f64 = 0.6;

/// Which loudness an offset comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Basis {
    Album,
    Track,
}

/// The offset in effect for the track playing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Offset {
    pub decibels: f64,
    pub basis: Basis,
    /// The loudness it levels, in LUFS.
    pub loudness: f64,
}

/// Where the playing track sits in the queue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub shuffle: bool,
    /// The track before or after it in the queue is from the same album.
    pub album_neighbour: bool,
}

/// The loudness of the track playing and where it sits in `queue`; `None`
/// for a file outside the library.
pub fn input(
    library: &Library,
    queue: &[QueueItem],
    status: &PlayerStatus,
) -> Option<(Loudness, Context)> {
    let track = status.track.as_ref()?;
    let id = track.id?;
    let neighbour = |index: Option<usize>| {
        index
            .and_then(|index| queue.get(index)?.id)
            .is_some_and(|other| library.same_album(id, other).unwrap_or(false))
    };
    let context = Context {
        shuffle: status.shuffle,
        album_neighbour: neighbour(track.index.checked_sub(1))
            || neighbour(track.index.checked_add(1)),
    };
    let loudness = library.loudness(&[id]).ok()?.remove(&id)?;
    Some((loudness, context))
}

/// The offset for a track; `None` when levelling is off or its loudness is
/// unknown (not measured yet, silent).
pub fn offset(mode: Levelling, loudness: Loudness, context: Context) -> Option<Offset> {
    let album = match mode {
        Levelling::Off => return None,
        Levelling::Album => true,
        Levelling::Track => false,
        // An album played in order keeps its own dynamics.
        Levelling::Auto => !context.shuffle && context.album_neighbour,
    };
    let (basis, lufs) = match (album, loudness.album, loudness.track) {
        (true, Some(lufs), _) | (false, Some(lufs), None) => (Basis::Album, lufs),
        (_, _, Some(lufs)) => (Basis::Track, lufs),
        (_, None, None) => return None,
    };
    Some(Offset {
        decibels: (REFERENCE_LUFS - lufs).min(MAX_BOOST_DB),
        basis,
        loudness: lufs,
    })
}

/// What the DAC is set to: the listening level moved by the offset, at
/// most `cap` (≤ 0 dB). The flag says the cap or 0 dB held it back.
pub fn target(listening: f64, offset: Option<Offset>, cap: f64) -> (f64, bool) {
    let wanted = listening + offset.map_or(0.0, |offset| offset.decibels);
    let ceiling = cap.min(0.0);
    (wanted.min(ceiling), wanted > ceiling + 0.05)
}

/// Levelling on one device at a time: the one playing.
#[derive(Debug, Default)]
pub struct Leveller {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// The device levelled, while a track plays on it with levelling on.
    device: Option<String>,
    /// The user's level, in dB, without the offset.
    listening: f64,
    offset: Option<Offset>,
    limited: bool,
    /// The DAC volume Lyra last set, to recognise its own mixer events.
    written: Option<f64>,
}

/// What the volume control shows while levelling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    pub listening: f64,
    pub offset: Option<Offset>,
    pub limited: bool,
}

impl Leveller {
    /// Starts levelling `device`, whose DAC reads `current` dB. A device
    /// already levelled keeps its listening level.
    pub fn begin(&self, device: &str, current: f64) {
        let mut state = locked(&self.state);
        if state.device.as_deref() != Some(device) {
            *state = State {
                device: Some(device.to_owned()),
                listening: current,
                ..State::default()
            };
        }
    }

    /// Stops levelling; the DAC volume to restore, if it was levelling.
    pub fn end(&self) -> Option<(String, f64)> {
        let mut state = locked(&self.state);
        let device = state.device.take()?;
        let restore = state.listening;
        *state = State::default();
        Some((device, restore))
    }

    pub fn device(&self) -> Option<String> {
        locked(&self.state).device.clone()
    }

    /// Applies `offset` on `device`; the DAC volume to set.
    pub fn apply(&self, device: &str, offset: Option<Offset>, cap: f64) -> Option<f64> {
        let mut state = locked(&self.state);
        (state.device.as_deref() == Some(device)).then(|| {
            state.offset = offset;
            state.write(cap)
        })
    }

    /// The user moved the slider to `listening` dB on `device`; the DAC
    /// volume to set, or `None` when `device` is not being levelled.
    pub fn set_listening(&self, device: &str, listening: f64, cap: f64) -> Option<f64> {
        let mut state = locked(&self.state);
        (state.device.as_deref() == Some(device)).then(|| {
            state.listening = listening.min(cap.min(0.0));
            state.write(cap)
        })
    }

    /// The DAC on `device` now reads `current` dB. Lyra's own change is
    /// ignored; another (the volume keys) moves the listening level. Returns
    /// the DAC volume to set when the change went above the cap.
    pub fn mixer_changed(&self, device: &str, current: f64, cap: f64) -> Option<f64> {
        let mut state = locked(&self.state);
        if state.device.as_deref() != Some(device)
            || state
                .written
                .is_some_and(|written| (written - current).abs() < OWN_CHANGE_DB)
        {
            return None;
        }
        let offset = state.offset.map_or(0.0, |offset| offset.decibels);
        state.listening = (current - offset).min(cap.min(0.0));
        if current > cap + 0.05 {
            return Some(state.write(cap));
        }
        state.written = Some(current);
        state.limited = target(state.listening, state.offset, cap).1;
        None
    }

    /// The listening level and offset of `device`, when it is levelled.
    pub fn shown(&self, device: &str) -> Option<Shown> {
        let state = locked(&self.state);
        (state.device.as_deref() == Some(device)).then_some(Shown {
            listening: state.listening,
            offset: state.offset,
            limited: state.limited,
        })
    }
}

impl State {
    fn write(&mut self, cap: f64) -> f64 {
        let (decibels, limited) = target(self.listening, self.offset, cap);
        self.limited = limited;
        self.written = Some(decibels);
        decibels
    }
}

#[cfg(test)]
mod tests {
    use library::Loudness;

    use super::{Basis, Context, Leveller, Offset, offset, target};
    use crate::settings::Levelling;

    const BOTH: Loudness = Loudness {
        track: Some(-10.0),
        album: Some(-12.0),
    };
    const IN_ORDER: Context = Context {
        shuffle: false,
        album_neighbour: true,
    };

    fn offset_db(mode: Levelling, loudness: Loudness, context: Context) -> Option<(f64, Basis)> {
        offset(mode, loudness, context).map(|offset| (offset.decibels, offset.basis))
    }

    #[test]
    fn auto_follows_how_the_album_is_played() {
        assert_eq!(
            offset_db(Levelling::Auto, BOTH, IN_ORDER),
            Some((-6.0, Basis::Album))
        );
        let shuffled = Context {
            shuffle: true,
            ..IN_ORDER
        };
        assert_eq!(
            offset_db(Levelling::Auto, BOTH, shuffled),
            Some((-8.0, Basis::Track))
        );
        let alone = Context::default();
        assert_eq!(
            offset_db(Levelling::Auto, BOTH, alone),
            Some((-8.0, Basis::Track))
        );
        assert_eq!(
            offset_db(Levelling::Album, BOTH, alone),
            Some((-6.0, Basis::Album))
        );
        assert_eq!(
            offset_db(Levelling::Track, BOTH, IN_ORDER),
            Some((-8.0, Basis::Track))
        );
        assert_eq!(offset_db(Levelling::Off, BOTH, IN_ORDER), None);
    }

    #[test]
    fn a_missing_loudness_falls_back_or_leaves_the_level() {
        let track_only = Loudness {
            track: Some(-20.0),
            album: None,
        };
        assert_eq!(
            offset_db(Levelling::Album, track_only, IN_ORDER),
            Some((2.0, Basis::Track))
        );
        let album_only = Loudness {
            track: None,
            album: Some(-30.0),
        };
        // Quiet: raised, but at most 6 dB.
        assert_eq!(
            offset_db(Levelling::Track, album_only, IN_ORDER),
            Some((6.0, Basis::Album))
        );
        assert_eq!(
            offset_db(Levelling::Auto, Loudness::default(), IN_ORDER),
            None
        );
    }

    #[test]
    fn the_dac_never_goes_above_the_cap_or_0_db() {
        let up = Some(Offset {
            decibels: 6.0,
            basis: Basis::Track,
            loudness: -24.0,
        });
        assert_eq!(target(-20.0, up, 0.0), (-14.0, false));
        assert_eq!(target(-3.0, up, 0.0), (0.0, true));
        assert_eq!(target(-20.0, up, -16.0), (-16.0, true));
        assert_eq!(target(-20.0, None, 0.0), (-20.0, false));
    }

    #[test]
    fn own_changes_are_ignored_and_the_volume_keys_move_the_level() {
        let leveller = Leveller::default();
        let device = "hw:CARD=SE,DEV=0";
        leveller.begin(device, -20.0);
        let down = offset(Levelling::Track, BOTH, IN_ORDER);
        assert_eq!(leveller.apply(device, down, 0.0), Some(-28.0));
        // Its own write, read back at the control's step.
        assert_eq!(leveller.mixer_changed(device, -28.25, 0.0), None);
        assert_eq!(leveller.shown(device).unwrap().listening, -20.0);
        // The volume keys: 3 dB up.
        assert_eq!(leveller.mixer_changed(device, -25.0, 0.0), None);
        assert_eq!(leveller.shown(device).unwrap().listening, -17.0);
        // The slider, then a cap that holds it.
        assert_eq!(leveller.set_listening(device, -10.0, -20.0), Some(-28.0));
        // Volume keys above the cap are pulled back.
        assert_eq!(leveller.mixer_changed(device, -5.0, -20.0), Some(-28.0));
        // Other devices are not levelled; ending gives the level back.
        assert_eq!(leveller.set_listening("hw:CARD=PCH,DEV=0", -5.0, 0.0), None);
        assert_eq!(leveller.end(), Some((device.to_owned(), -20.0)));
        assert_eq!(leveller.shown(device), None);
    }
}
