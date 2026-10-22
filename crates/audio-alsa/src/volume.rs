//! The DAC's own playback volume.
//!
//! USB Audio class devices expose a feature-unit volume. Many DACs apply it
//! digitally in firmware, so anything but 0 dB alters the samples. PipeWire
//! sets it (often to −0.01 dB, a rounding artefact) and devices restore it on
//! replug, so it is checked on every open.
//!
//! Policy:
//! - only USB Audio cards are touched; built-in codecs never are;
//! - a control within [`AUTO_UNITY_CENTIBELS`] of 0 dB is set to exactly
//!   0 dB while Lyra holds the device, and restored on close;
//! - a lower setting is a deliberate volume (possibly a headphone level), so it
//!   is only reported: forcing 0 dB there could be dangerously loud.

use alsa::mixer::{MilliBel, Selem, SelemChannelId, SelemId};
use alsa::{Ctl, Mixer, Round};
use unda::HardwareVolume;

use crate::error::AlsaError;
use crate::sink::card_control;

/// Largest attenuation corrected automatically: 0.5 dB.
const AUTO_UNITY_CENTIBELS: i64 = -50;

/// Values Lyra replaced, to put back on close.
#[derive(Debug, Default)]
pub(crate) struct SavedVolume {
    mixer: String,
    controls: Vec<SavedControl>,
}

#[derive(Debug)]
struct SavedControl {
    name: String,
    index: u32,
    /// (channel, original raw value, raw value Lyra set)
    channels: Vec<(SelemChannelId, i64, i64)>,
}

/// Checks the card's playback volumes and corrects near-0 dB ones.
/// `card` is a card control name (`hw:CARD=ONE` or `hw:2`). Never fails
/// playback: on any mixer error the state is [`HardwareVolume::Unknown`].
pub(crate) fn prepare(card: &str) -> (HardwareVolume, SavedVolume) {
    inspect(card, true)
}

/// The volume state of the card behind `device`, as playback judges it,
/// read only. Watch it while playing: the desktop's volume keys move a
/// USB DAC's controls through PipeWire even while Lyra holds the device.
pub fn hardware_volume(device: &str) -> HardwareVolume {
    inspect(&card_control(device), false).0
}

/// With `correct`, near-0 dB controls are set to 0 dB (and saved to put back).
fn inspect(card: &str, correct: bool) -> (HardwareVolume, SavedVolume) {
    let mixer_name = card.to_owned();
    let mut saved = SavedVolume {
        mixer: mixer_name.clone(),
        controls: Vec::new(),
    };
    if !is_usb_audio(&mixer_name) {
        return (HardwareVolume::Unknown, saved);
    }
    let Ok(mixer) = Mixer::new(&mixer_name, false) else {
        return (HardwareVolume::Unknown, saved);
    };

    let mut state = HardwareVolume::None;
    for selem in mixer.iter().filter_map(Selem::new) {
        if !selem.has_playback_volume() {
            continue;
        }
        let Some(control) = adjust(&selem, correct) else {
            state = combine(state, HardwareVolume::Unknown);
            continue;
        };
        state = combine(state, control.state);
        if let Some(control) = control.saved {
            saved.controls.push(control);
        }
    }
    (state, saved)
}

impl Drop for SavedVolume {
    /// Puts back what [`prepare`] changed, unless someone changed it since.
    /// Runs when the stream closes, fails, or the device is unplugged.
    fn drop(&mut self) {
        let controls = std::mem::take(&mut self.controls);
        if controls.is_empty() {
            return;
        }
        let Ok(mixer) = Mixer::new(&self.mixer, false) else {
            return;
        };
        for control in controls {
            let Some(selem) = mixer.find_selem(&SelemId::new(&control.name, control.index)) else {
                continue;
            };
            for (channel, original, set) in control.channels {
                if selem.get_playback_volume(channel).ok() == Some(set) {
                    let _ = selem.set_playback_volume(channel, original);
                }
            }
        }
    }
}

/// The card's playback volume, as the in-app control shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeLevel {
    /// 0–100 along the controls' own steps (the lowest, when several).
    pub percent: u8,
    /// Total attenuation, when every control reports dB.
    pub decibels: Option<f64>,
    /// USB DACs often apply their volume digitally: below 0 dB the samples
    /// change. Built-in cards are not checked.
    pub usb: bool,
}

/// Preferred controls on cards other than USB DACs, most global first;
/// otherwise the first one with a playback volume.
const MAIN_CONTROLS: [&str; 2] = ["Master", "PCM"];

/// The playback volume of the card behind the `hw:` device `device`;
/// `None` when it has none.
pub fn volume(device: &str) -> Result<Option<VolumeLevel>, AlsaError> {
    let card = card_control(device);
    let mixer = open_mixer(device, &card)?;
    let usb = is_usb_audio(&card);
    Ok(level(&controls(&mixer, usb), usb))
}

/// Sets the card's playback volume to `percent` (clamped to 100). The
/// hardware applies it; Lyra never scales samples.
pub fn set_volume(device: &str, percent: u8) -> Result<Option<VolumeLevel>, AlsaError> {
    let card = card_control(device);
    let mixer = open_mixer(device, &card)?;
    let usb = is_usb_audio(&card);
    let controls = controls(&mixer, usb);
    for selem in &controls {
        let (min, max) = selem.get_playback_volume_range();
        let raw = min + ((max - min) * i64::from(percent.min(100)) + 50) / 100;
        selem
            .set_playback_volume_all(raw)
            .map_err(|source| AlsaError::Alsa {
                device: device.to_owned(),
                source,
            })?;
    }
    Ok(level(&controls, usb))
}

fn open_mixer(device: &str, card: &str) -> Result<Mixer, AlsaError> {
    Mixer::new(card, false).map_err(|source| AlsaError::Alsa {
        device: device.to_owned(),
        source,
    })
}

/// The controls the in-app volume moves. A USB DAC applies every one of
/// its volume controls in series (often a master and per-channel ones), so
/// all move together and 100% means all at 0 dB. On other cards, only the
/// main control: the rest route to speakers, headphones, etc.
fn controls(mixer: &Mixer, usb: bool) -> Vec<Selem<'_>> {
    let with_volume = || {
        mixer
            .iter()
            .filter_map(Selem::new)
            .filter(Selem::has_playback_volume)
    };
    if usb {
        return with_volume().collect();
    }
    MAIN_CONTROLS
        .iter()
        .find_map(|name| with_volume().find(|selem| selem.get_id().get_name().ok() == Some(*name)))
        .or_else(|| with_volume().next())
        .into_iter()
        .collect()
}

/// The lowest position among `controls`, and their attenuation added up
/// (they apply in series). `None` without controls.
fn level(controls: &[Selem<'_>], usb: bool) -> Option<VolumeLevel> {
    let mut percent: Option<i64> = None;
    let mut decibels = Some(0.0);
    for selem in controls {
        let Some(channel) = SelemChannelId::all()
            .iter()
            .copied()
            .find(|&channel| selem.has_playback_channel(channel))
        else {
            continue;
        };
        let (min, max) = selem.get_playback_volume_range();
        let Ok(raw) = selem.get_playback_volume(channel) else {
            continue;
        };
        let position = if max > min {
            (((raw - min) * 100 + (max - min) / 2) / (max - min)).clamp(0, 100)
        } else {
            100
        };
        percent = Some(percent.map_or(position, |lowest| lowest.min(position)));
        let (min_db, max_db) = selem.get_playback_db_range();
        let db = (min_db != max_db)
            .then(|| selem.get_playback_vol_db(channel).ok())
            .flatten();
        decibels = match (decibels, db) {
            (Some(total), Some(MilliBel(centibels))) => {
                Some(total + f64::from(i32::try_from(centibels).unwrap_or(i32::MIN)) / 100.0)
            }
            _ => None,
        };
    }
    Some(VolumeLevel {
        percent: u8::try_from(percent?).unwrap_or(100),
        decibels,
        usb,
    })
}

struct Adjusted {
    state: HardwareVolume,
    saved: Option<SavedControl>,
}

/// `None` when the control has no usable dB information.
fn adjust(selem: &Selem<'_>, correct: bool) -> Option<Adjusted> {
    let (min_db, max_db) = selem.get_playback_db_range();
    if min_db == max_db {
        return None;
    }
    let channels: Vec<SelemChannelId> = SelemChannelId::all()
        .iter()
        .copied()
        .filter(|&channel| channel != SelemChannelId::Unknown && channel != SelemChannelId::Last)
        .filter(|&channel| selem.has_playback_channel(channel))
        .collect();
    let mut lowest = 0;
    let mut changed = Vec::new();
    let unity = selem.ask_playback_db_vol(MilliBel(0), Round::Floor).ok()?;
    let unity_is_exact = selem.ask_playback_vol_db(unity).ok()? == MilliBel(0);

    for channel in channels {
        let raw = selem.get_playback_volume(channel).ok()?;
        let MilliBel(db) = selem.get_playback_vol_db(channel).ok()?;
        if db == 0 {
            continue;
        }
        if correct && unity_is_exact && (AUTO_UNITY_CENTIBELS..0).contains(&db) {
            selem.set_playback_volume(channel, unity).ok()?;
            changed.push((channel, raw, unity));
        } else {
            lowest = lowest.min(db);
        }
    }

    let state = if lowest < 0 {
        HardwareVolume::Attenuated { centibels: lowest }
    } else {
        HardwareVolume::Unity
    };
    let saved = (!changed.is_empty()).then(|| {
        let id = selem.get_id();
        SavedControl {
            name: id.get_name().unwrap_or_default().to_owned(),
            index: id.get_index(),
            channels: changed,
        }
    });
    Some(Adjusted { state, saved })
}

/// The overall state of controls applied in series: attenuations add up;
/// a known attenuation outranks an unknown control.
fn combine(total: HardwareVolume, control: HardwareVolume) -> HardwareVolume {
    use HardwareVolume::{Attenuated, None, Unity, Unknown};
    match (total, control) {
        (Attenuated { centibels: a }, Attenuated { centibels: b }) => {
            Attenuated { centibels: a + b }
        }
        (attenuated @ Attenuated { .. }, _) | (_, attenuated @ Attenuated { .. }) => attenuated,
        (Unknown, _) | (_, Unknown) => Unknown,
        (None | Unity, Unity) | (Unity, None) => Unity,
        (None, None) => None,
    }
}

fn is_usb_audio(ctl_name: &str) -> bool {
    Ctl::new(ctl_name, false)
        .and_then(|ctl| {
            ctl.card_info()
                .map(|info| info.get_driver().ok() == Some("USB-Audio"))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{HardwareVolume, combine};

    #[test]
    fn attenuations_add_up() {
        let attenuated = HardwareVolume::Attenuated { centibels: -600 };
        assert_eq!(
            combine(HardwareVolume::None, HardwareVolume::Unity),
            HardwareVolume::Unity
        );
        assert_eq!(combine(HardwareVolume::Unity, attenuated), attenuated);
        assert_eq!(
            combine(attenuated, HardwareVolume::Attenuated { centibels: -100 }),
            HardwareVolume::Attenuated { centibels: -700 }
        );
        assert_eq!(combine(HardwareVolume::Unknown, attenuated), attenuated);
        assert_eq!(
            combine(HardwareVolume::Unity, HardwareVolume::Unknown),
            HardwareVolume::Unknown
        );
    }
}
