//! The window's size, kept in `window.json` in the app's config folder and
//! restored at launch. Position is left to the window manager.
//!
//! Like the settings, a missing or odd file means the default size.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::{LogicalSize, Runtime, Window};

use crate::settings::temporary_path;

/// Larger than any screen; a size beyond it is a corrupt file.
const MAX_SIDE: f64 = 16_384.0;

/// Logical pixels, so the size survives a change of display scale.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct WindowState {
    width: f64,
    height: f64,
    maximized: bool,
}

impl WindowState {
    fn is_valid(&self) -> bool {
        [self.width, self.height]
            .iter()
            .all(|side| side.is_finite() && (1.0..=MAX_SIDE).contains(side))
    }
}

fn load(path: &Path) -> Option<WindowState> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice::<WindowState>(&bytes)
        .ok()
        .filter(WindowState::is_valid)
}

fn store(state: WindowState, path: &Path) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let json = serde_json::to_vec(&state).map_err(io::Error::other)?;
    let temporary = temporary_path(path);
    fs::write(&temporary, json)?;
    fs::rename(&temporary, path)
}

/// Gives `window` the size it had when Lyra last closed. The window's
/// minimum size still applies.
pub fn restore<R: Runtime>(window: &Window<R>, path: &Path) -> tauri::Result<()> {
    let Some(state) = load(path) else {
        return Ok(());
    };
    window.set_size(LogicalSize::new(state.width, state.height))?;
    if state.maximized {
        window.maximize()?;
    }
    Ok(())
}

/// Remembers `window`'s size. A maximized window keeps the size it had
/// before, so un-maximizing after the next launch still has somewhere to go.
pub fn save<R: Runtime>(window: &Window<R>, path: &Path) -> tauri::Result<()> {
    let maximized = window.is_maximized()?;
    let previous = load(path).filter(|_| maximized);
    let state = if let Some(previous) = previous {
        WindowState {
            maximized,
            ..previous
        }
    } else {
        let size = window
            .inner_size()?
            .to_logical::<f64>(window.scale_factor()?);
        WindowState {
            width: size.width,
            height: size.height,
            maximized,
        }
    };
    store(state, path).map_err(tauri::Error::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(test: &str) -> std::path::PathBuf {
        let folder =
            std::env::temp_dir().join(format!("lyra-window-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn stores_and_loads() {
        let path = folder("roundtrip").join("window.json");
        let state = WindowState {
            width: 1440.0,
            height: 900.0,
            maximized: true,
        };
        store(state, &path).unwrap();
        assert_eq!(load(&path), Some(state));
    }

    #[test]
    fn an_odd_file_means_the_default_size() {
        let folder = folder("odd");
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("window.json");
        for content in [
            "not json",
            r#"{"width": 0, "height": 800, "maximized": false}"#,
            r#"{"width": 1e9, "height": 800, "maximized": false}"#,
        ] {
            fs::write(&path, content).unwrap();
            assert_eq!(load(&path), None, "{content}");
        }
        assert_eq!(load(&folder.join("missing.json")), None);
    }
}
