//! Waiting for a card's mixer to change, instead of reading it on a timer.
//!
//! The kernel signals every control change (the desktop's volume keys through
//! PipeWire, `alsamixer`, Lyra's own 0 dB correction) on the mixer's
//! descriptors, so a watcher can sleep until something happens.

use std::os::fd::AsRawFd;
use std::time::Duration;

use alsa::Mixer;
use alsa::poll::{Descriptors, Flags};

use crate::error::AlsaError;
use crate::sink::card_control;
use crate::wake::{Wake, millis, readable};

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
    /// The [`Wake`] was notified.
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

    /// Sleeps until a control changes, `wake` is notified, or `timeout`
    /// passes (never, with `None`). A pending wake comes first, so a busy
    /// mixer never starves it.
    pub fn wait(&self, wake: &Wake, timeout: Option<Duration>) -> Woken {
        let mut fds = vec![readable(wake.fd().as_raw_fd())];
        match self.mixer.get() {
            Ok(mixer) => fds.extend(mixer),
            Err(_) => return Woken::Gone,
        }
        match alsa::poll::poll(&mut fds, millis(timeout)) {
            Ok(0) => return Woken::Timeout,
            Ok(_) => {}
            // Interrupted: the caller looks again, as after a wake.
            Err(_) => return Woken::Other,
        }
        if fds[0].revents != 0 {
            return Woken::Other;
        }
        let flags = self.mixer.revents(&fds[1..]).unwrap_or(Flags::ERR);
        if flags.intersects(Flags::ERR | Flags::HUP | Flags::NVAL) {
            return Woken::Gone;
        }
        // Clears the event so the next wait sleeps again.
        match self.mixer.handle_events() {
            Ok(_) => Woken::Mixer,
            Err(_) => Woken::Gone,
        }
    }
}
