//! Album artwork cache: one file per album in a directory Lyra owns, with a
//! total size cap. Images are stored as found; the UI scales them.

use std::fs;
use std::path::Path;

use rusqlite::{Connection, params};

use crate::LibraryError;
use crate::db::now;
use crate::tags::{MAX_PICTURE_BYTES, Picture, image_mime};

/// Total artwork kept. Past it, albums go without art until space frees up.
pub const MAX_CACHE_BYTES: i64 = 512 * 1024 * 1024;

/// Image files used as album art when a folder has no embedded picture, in
/// order of preference (case-insensitive).
const FOLDER_NAMES: [&str; 4] = ["cover", "folder", "front", "album"];

/// Stores `picture` as the album's art. Returns `false` when the cache is full.
pub(crate) fn store(
    conn: &Connection,
    art_dir: &Path,
    album_id: i64,
    picture: &Picture,
) -> Result<bool, LibraryError> {
    let size = i64::try_from(picture.data.len()).unwrap_or(i64::MAX);
    let used: i64 = conn.query_row(
        "SELECT COALESCE(SUM(art_bytes), 0) FROM albums",
        [],
        |row| row.get(0),
    )?;
    if used.saturating_add(size) > MAX_CACHE_BYTES {
        return Ok(false);
    }
    let extension = match picture.mime {
        "image/png" => "png",
        "image/webp" => "webp",
        _ => "jpg",
    };
    // A new name per store, so the webview never shows a stale image when an
    // album id is reused.
    let name = format!("{album_id}-{}.{extension}", now());
    fs::create_dir_all(art_dir)?;
    let partial = art_dir.join(format!("{name}.part"));
    fs::write(&partial, &picture.data)?;
    fs::rename(&partial, art_dir.join(&name))?;
    conn.execute(
        "UPDATE albums SET art = ?2, art_bytes = ?3 WHERE id = ?1",
        params![album_id, name, size],
    )?;
    Ok(true)
}

/// `cover.jpg`, `Folder.png`, … in `dir`, if present and not too large.
pub(crate) fn folder_image(dir: &Path) -> Option<Picture> {
    let mut best: Option<(usize, std::path::PathBuf, &'static str)> = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let (Some(stem), Some(extension)) = (
            path.file_stem().and_then(|stem| stem.to_str()),
            path.extension().and_then(|extension| extension.to_str()),
        ) else {
            continue;
        };
        let Some(rank) = FOLDER_NAMES
            .iter()
            .position(|name| stem.eq_ignore_ascii_case(name))
        else {
            continue;
        };
        let mime = match extension.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            _ => continue,
        };
        if best
            .as_ref()
            .is_none_or(|(best_rank, ..)| rank < *best_rank)
        {
            best = Some((rank, path, mime));
        }
    }
    let (_, path, mime) = best?;
    let size = fs::metadata(&path).ok()?.len();
    if size > MAX_PICTURE_BYTES as u64 {
        return None;
    }
    Some(Picture {
        mime: image_mime(mime)?,
        data: fs::read(path).ok()?,
    })
}

/// Folder in the artwork directory for small copies made by the app, named
/// like their originals.
pub const THUMB_DIR: &str = "thumbs";

/// Deletes cached art files by name, and their thumbnails. Missing files
/// are fine.
pub fn delete_art(art_dir: &Path, names: &[String]) {
    for name in names {
        if is_art_name(name) {
            let _ = fs::remove_file(art_dir.join(name));
            let _ = fs::remove_file(art_dir.join(THUMB_DIR).join(name));
        }
    }
}

/// Names this cache creates: `<album>-<time>.<jpg|png|webp>`. Anything else
/// (separators, `..`) is refused, so a name never leaves the directory.
pub fn is_art_name(name: &str) -> bool {
    let Some((stem, extension)) = name.split_once('.') else {
        return false;
    };
    let Some((album, time)) = stem.split_once('-') else {
        return false;
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    digits(album) && digits(time) && ["jpg", "png", "webp"].contains(&extension)
}

#[cfg(test)]
mod tests {
    use super::is_art_name;

    #[test]
    fn only_cache_names_are_served() {
        assert!(is_art_name("12-1792532826.jpg"));
        assert!(is_art_name("3-0.webp"));
        for bad in [
            "../12-1.jpg",
            "12-1.jpg/x",
            "12.jpg",
            "a-1.jpg",
            "12-1.gif",
            "12-1.jpg.part",
        ] {
            assert!(!is_art_name(bad), "{bad}");
        }
    }
}
