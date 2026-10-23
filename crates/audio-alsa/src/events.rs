//! Waiting for a card's mixer to change, instead of reading it on a timer.
//!
//! The kernel signals every control change (the desktop's volume keys through
//! PipeWire, `alsamixer`, Lyra's own 0 dB correction) on the mixer's
//! descriptors, so a watcher can sleep until something happens.

use std::os::fd::{AsRawFd, BorrowedFd};
use std::time::Duration;

use alsa::Mixer;
use alsa::poll::{Flags, pollfd};

use crate::error::AlsaError;
use crate::sink::card_control;

/// Change notifications of the card behind one device.
pub struct MixerEvents {
    mixer: Mixer,
}

impl std::fmt::Debug for MixerEvents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MixerEvents").finish_non_exhaustive()
    }
}

/// Why [`MixerEvents::wait`] returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Woken {
    /// A control of the card changed.
    Mixer,
    /// `other` became readable.
    Other,
    Timeout,
    /// The card went away (unplugged): wait on something else.
    Gone,
}

impl MixerEvents {
    /// Opens the mixer of the card behind `device` (`hw:CARD=ONE,DEV=0`).
    pub fn open(device: &str) -> Result<Self, AlsaError> {
        let mixer = Mixer::new(&card_control(device), false).map_err(|source| AlsaError::Alsa {
            device: device.to_owned(),
            source,
        })?;
        Ok(Self { mixer })
    }

    /// Sleeps until a control changes, `other` is readable, or `timeout`
    /// passes (never, with `None`). `other` is checked first, so a pending
    /// wake is never starved by a busy mixer.
    pub fn wait(&self, other: BorrowedFd<'_>, timeout: Option<Duration>) -> Woken {
        let wake = pollfd {
            fd: other.as_raw_fd(),
            events: Flags::IN.bits(),
            revents: 0,
        };
        let Ok(ready) = alsa::poll::poll_all(&[&wake, &self.mixer], millis(timeout)) else {
            // Interrupted: the caller looks again, as after a wake.
            return Woken::Other;
        };
        let mut woken = Woken::Timeout;
        for (source, flags) in ready {
            if std::ptr::addr_eq(source, &raw const wake) {
                return Woken::Other;
            }
            if flags.intersects(Flags::ERR | Flags::HUP | Flags::NVAL) {
                return Woken::Gone;
            }
            // Clears the event so the next wait sleeps again.
            woken = match self.mixer.handle_events() {
                Ok(_) => Woken::Mixer,
                Err(_) => Woken::Gone,
            };
        }
        woken
    }
}

/// Milliseconds for poll(2): -1 waits forever.
fn millis(timeout: Option<Duration>) -> i32 {
    timeout.map_or(-1, |timeout| {
        i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX)
    })
}
