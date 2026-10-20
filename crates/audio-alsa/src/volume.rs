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
use player_core::HardwareVolume;

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
        let Some(control) = adjust(&selem) else {
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

struct Adjusted {
    state: HardwareVolume,
    saved: Option<SavedControl>,
}

/// `None` when the control has no usable dB information.
fn adjust(selem: &Selem<'_>) -> Option<Adjusted> {
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
        if unity_is_exact && (AUTO_UNITY_CENTIBELS..0).contains(&db) {
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

/// The overall state is the worst control's; a known attenuation outranks
/// an unknown control.
fn combine(total: HardwareVolume, control: HardwareVolume) -> HardwareVolume {
    use HardwareVolume::{Attenuated, None, Unity, Unknown};
    match (total, control) {
        (Attenuated { centibels: a }, Attenuated { centibels: b }) => Attenuated {
            centibels: a.min(b),
        },
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
    fn worst_control_wins() {
        let attenuated = HardwareVolume::Attenuated { centibels: -600 };
        assert_eq!(
            combine(HardwareVolume::None, HardwareVolume::Unity),
            HardwareVolume::Unity
        );
        assert_eq!(combine(HardwareVolume::Unity, attenuated), attenuated);
        assert_eq!(
            combine(attenuated, HardwareVolume::Attenuated { centibels: -100 }),
            attenuated
        );
        assert_eq!(combine(HardwareVolume::Unknown, attenuated), attenuated);
        assert_eq!(
            combine(HardwareVolume::Unity, HardwareVolume::Unknown),
            HardwareVolume::Unknown
        );
    }
}
