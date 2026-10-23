//! The user's settings, kept in `settings.json` in the app's config folder.
//!
//! A missing or unreadable file means the defaults; nothing here can stop
//! Lyra from starting. Saving writes a temporary file and renames it, so a
//! crash never leaves half a file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use unda::{DsdFallback, DsdPolicy, DsdTransport};

/// Gains offered for converted DSD, in dB. DSD is mastered with its peaks
/// at most 3 dB above the SACD reference level, which converts to about
/// −3 dBFS: 0 dB never clips; +3 and +6 come closer to the loudness of PCM
/// releases, and the loudest masters may clip (clamped and counted).
pub const CONVERSION_GAINS_DB: [i8; 3] = [0, 3, 6];

/// Pauses offered before playback when the format changes, in ms.
pub const RESYNC_DELAYS_MS: [u16; 5] = [0, 250, 500, 1000, 2000];

/// Most devices remembered as decoding DoP; far more than anyone plugs in.
const MAX_DOP_DEVICES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub dsd_transport: Transport,
    /// DSD on devices without DSD support.
    pub dsd_fallback: Fallback,
    /// One of [`CONVERSION_GAINS_DB`].
    pub conversion_gain_db: i8,
    /// Silence before playback when the format changes, so the DAC can
    /// switch without noise; one of [`RESYNC_DELAYS_MS`].
    pub resync_delay_ms: u16,
    /// ALSA devices the user says decode DoP (`hw:CARD=…,DEV=…`). Others get
    /// DoP only when they also expose native DSD.
    pub dop_devices: Vec<String>,
    /// ID Match may ask MusicBrainz and the Cover Art Archive. Off until the
    /// user turns it on; nothing goes online at startup or during scans.
    pub online_lookups: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dsd_transport: Transport::default(),
            dsd_fallback: Fallback::default(),
            conversion_gain_db: 0,
            // Enough for the DACs tested so far to switch between PCM and DSD.
            resync_delay_ms: 500,
            dop_devices: Vec::new(),
            online_lookups: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// Native DSD when the DAC exposes it, otherwise DoP.
    #[default]
    Auto,
    Native,
    Dop,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fallback {
    #[default]
    Refuse,
    Convert,
}

impl Settings {
    /// Reads `path`; the defaults when it is missing or not valid.
    pub fn load(path: &Path) -> Self {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(Settings::is_valid)
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let temporary = temporary_path(path);
        fs::write(&temporary, json)?;
        fs::rename(&temporary, path)
    }

    pub fn is_valid(&self) -> bool {
        CONVERSION_GAINS_DB.contains(&self.conversion_gain_db)
            && RESYNC_DELAYS_MS.contains(&self.resync_delay_ms)
            && self.dop_devices.len() <= MAX_DOP_DEVICES
            && self
                .dop_devices
                .iter()
                .all(|device| device.starts_with("hw:") && device.len() <= 128)
    }

    pub fn resync_delay(&self) -> Duration {
        Duration::from_millis(u64::from(self.resync_delay_ms))
    }

    /// What the engine applies to DSD files played on `device`.
    pub fn dsd_policy(&self, device: &str) -> DsdPolicy {
        DsdPolicy {
            transport: match self.dsd_transport {
                Transport::Auto => DsdTransport::Auto,
                Transport::Native => DsdTransport::Native,
                Transport::Dop => DsdTransport::Dop,
            },
            fallback: match self.dsd_fallback {
                Fallback::Refuse => DsdFallback::Refuse,
                Fallback::Convert => DsdFallback::Convert {
                    gain_db: self.conversion_gain_db,
                },
            },
            dop_confirmed: self.dop_devices.iter().any(|known| known == device),
        }
    }
}

pub fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".tmp");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use unda::{DsdFallback, DsdTransport};

    use super::{Fallback, Settings, Transport};

    /// A fresh folder per test: tests run in parallel.
    fn folder(test: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lyra-settings-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn saves_and_loads() {
        let path = folder("save").join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default());

        let settings = Settings {
            dsd_transport: Transport::Dop,
            dsd_fallback: Fallback::Convert,
            conversion_gain_db: 3,
            resync_delay_ms: 1000,
            dop_devices: vec!["hw:CARD=E30,DEV=0".to_owned()],
            online_lookups: true,
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        let policy = settings.dsd_policy("hw:CARD=E30,DEV=0");
        assert_eq!(policy.transport, DsdTransport::Dop);
        assert_eq!(policy.fallback, DsdFallback::Convert { gain_db: 3 });
        assert!(policy.dop_confirmed);
        assert!(!settings.dsd_policy("hw:CARD=PCH,DEV=0").dop_confirmed);
    }

    #[test]
    fn a_bad_file_means_the_defaults() {
        let path = folder("bad").join("bad.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for content in [
            "{",
            r#"{"conversionGainDb": 5}"#,
            r#"{"dsdTransport": "pcm"}"#,
            r#"{"resyncDelayMs": 300}"#,
            r#"{"dopDevices": ["/etc/passwd"]}"#,
        ] {
            std::fs::write(&path, content).unwrap();
            assert_eq!(Settings::load(&path), Settings::default(), "{content}");
        }
        // Online lookups stay off unless the file turns them on.
        assert!(!Settings::default().online_lookups);
        // Unknown and missing fields are fine: older and newer files load.
        std::fs::write(&path, r#"{"dsdFallback": "convert", "theme": "dark"}"#).unwrap();
        assert_eq!(Settings::load(&path).dsd_fallback, Fallback::Convert);
        assert_eq!(
            Settings::default().dsd_policy("hw:CARD=SE,DEV=0").fallback,
            DsdFallback::Refuse
        );
    }
}
