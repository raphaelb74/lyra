use std::fmt;
use std::fs;

/// Process that has a PCM device open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    /// Executable name from `/proc/<pid>/comm`, e.g. `pipewire` or `wireplumber`.
    pub command: Option<String>,
}

#[derive(Debug)]
pub enum AlsaError {
    /// Another process has the device open.
    Busy {
        device: String,
        holder: Option<Holder>,
    },
    /// No such device (unplugged, or wrong name).
    NotFound { device: String },
    /// The device cannot play this exact stream; nothing was converted.
    Unsupported { device: String, reason: String },
    /// Any other ALSA failure.
    Alsa { device: String, source: alsa::Error },
}

impl AlsaError {
    pub(crate) fn from_alsa(
        device: &str,
        card: Option<i32>,
        pcm_device: u32,
        error: alsa::Error,
    ) -> Self {
        let device = device.to_owned();
        match errno::of(&error) {
            errno::EBUSY => {
                let holder = card.and_then(|card| find_holder(card, pcm_device));
                Self::Busy { device, holder }
            }
            errno::ENOENT | errno::ENODEV => Self::NotFound { device },
            _ => Self::Alsa {
                device,
                source: error,
            },
        }
    }
}

/// Linux errno values ALSA returns.
pub(crate) mod errno {
    pub const ENOENT: i32 = 2;
    pub const EAGAIN: i32 = 11;
    pub const EBUSY: i32 = 16;
    pub const ENODEV: i32 = 19;
    pub const EPIPE: i32 = 32;
    pub const ESTRPIPE: i32 = 86;

    /// The error's errno, positive. alsa-rs reports it positive or negative
    /// depending on the call.
    pub fn of(error: &alsa::Error) -> i32 {
        error.errno().abs()
    }
}

impl fmt::Display for AlsaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy {
                device,
                holder:
                    Some(Holder {
                        pid,
                        command: Some(command),
                    }),
            } => {
                write!(f, "{device} is in use by {command} (pid {pid})")
            }
            Self::Busy {
                device,
                holder: Some(Holder { pid, command: None }),
            } => {
                write!(f, "{device} is in use by process {pid}")
            }
            Self::Busy {
                device,
                holder: None,
            } => write!(f, "{device} is in use by another program"),
            Self::NotFound { device } => write!(f, "{device} not found; is the DAC connected?"),
            Self::Unsupported { device, reason } => {
                write!(f, "{device} cannot play this: {reason}")
            }
            Self::Alsa { device, source } => write!(f, "{device}: {source}"),
        }
    }
}

impl std::error::Error for AlsaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Alsa { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Reads the owner of a playback substream from procfs.
///
/// `/proc/asound/cardN/pcmDp/subS/status` lists `owner_pid` while the
/// substream is open. Best effort: `None` when procfs is unavailable.
fn find_holder(card: i32, device: u32) -> Option<Holder> {
    let base = format!("/proc/asound/card{card}/pcm{device}p");
    let entries = fs::read_dir(base).ok()?;
    entries.filter_map(Result::ok).find_map(|entry| {
        let status = fs::read_to_string(entry.path().join("status")).ok()?;
        let pid = parse_owner_pid(&status)?;
        let command = fs::read_to_string(format!("/proc/{pid}/comm"))
            .ok()
            .map(|comm| comm.trim().to_owned());
        Some(Holder { pid, command })
    })
}

fn parse_owner_pid(status: &str) -> Option<u32> {
    status.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "owner_pid")
            .then(|| value.trim().parse().ok())
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::{AlsaError, Holder, parse_owner_pid};

    #[test]
    fn owner_pid_from_running_status() {
        let status = "state: RUNNING\nowner_pid   : 2154\ntrigger_time: 1.0\n";
        assert_eq!(parse_owner_pid(status), Some(2154));
        assert_eq!(parse_owner_pid("closed\n"), None);
    }

    #[test]
    fn busy_message_names_the_holder() {
        let error = AlsaError::Busy {
            device: "hw:CARD=ONE,DEV=0".into(),
            holder: Some(Holder {
                pid: 2154,
                command: Some("wireplumber".into()),
            }),
        };
        assert_eq!(
            error.to_string(),
            "hw:CARD=ONE,DEV=0 is in use by wireplumber (pid 2154)"
        );
    }
}
