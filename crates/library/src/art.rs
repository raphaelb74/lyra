//! Album artwork cache: one file per album in a directory Lyra owns, with a
//! total size cap. Images are stored as found; the UI scales them.
//!
//! Artwork the user chose is also kept in the database, so a scan never
//! replaces it and a cleared cache gets it back.

use std::fs;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::LibraryError;
use crate::db::{Library, now, path_from};
use crate::tags::{self, MAX_PICTURE_BYTES, Picture, image_mime};

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

impl Library {
    /// Makes the image at `image` the album's artwork, in place of what the
    /// files have, until [`Library::reset_artwork`].
    pub fn choose_artwork(
        &mut self,
        art_dir: &Path,
        album: i64,
        image: &Path,
    ) -> Result<(), LibraryError> {
        let invalid = |reason: &str| LibraryError::Invalid(reason.to_owned());
        if fs::metadata(image)?.len() > MAX_PICTURE_BYTES as u64 {
            return Err(invalid("That image is too large (16 MB at most)."));
        }
        let data = fs::read(image)?;
        let mime = sniff(&data).ok_or_else(|| invalid("That is not a JPEG, PNG or WebP image."))?;
        let picture = Picture { mime, data };
        let transaction = self.conn.transaction()?;
        let old = current_art(&transaction, album)?
            .ok_or_else(|| invalid("That album is no longer in the library."))?;
        transaction.execute(
            "INSERT INTO chosen_art (album_id, mime, data) VALUES (?1, ?2, ?3)
             ON CONFLICT (album_id) DO UPDATE SET mime = ?2, data = ?3",
            params![album, picture.mime, picture.data],
        )?;
        if !store(&transaction, art_dir, album, &picture)? {
            return Err(invalid("The artwork cache is full."));
        }
        transaction.commit()?;
        delete_art(art_dir, &old);
        Ok(())
    }

    /// Shows the files' artwork again: the first embedded picture of the
    /// album's tracks, else a cover image in the first track's folder.
    pub fn reset_artwork(&mut self, art_dir: &Path, album: i64) -> Result<(), LibraryError> {
        let paths: Vec<Vec<u8>> = self
            .conn
            .prepare("SELECT path FROM tracks WHERE album_id = ?1 ORDER BY disc, track, path")?
            .query_map([album], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        let paths: Vec<_> = paths.iter().map(|path| path_from(path)).collect();
        let picture = paths
            .iter()
            .find_map(|path| tags::read(path).ok()?.picture)
            .or_else(|| folder_image(paths.first()?.parent()?));

        let transaction = self.conn.transaction()?;
        let Some(old) = current_art(&transaction, album)? else {
            return Ok(());
        };
        transaction.execute("DELETE FROM chosen_art WHERE album_id = ?1", [album])?;
        transaction.execute(
            "UPDATE albums SET art = NULL, art_bytes = 0 WHERE id = ?1",
            [album],
        )?;
        if let Some(picture) = picture {
            store(&transaction, art_dir, album, &picture)?;
        }
        transaction.commit()?;
        delete_art(art_dir, &old);
        Ok(())
    }
}

/// An album's artwork files (none or one); `None` when there is no such
/// album.
fn current_art(conn: &Connection, album: i64) -> Result<Option<Vec<String>>, LibraryError> {
    let art: Option<Option<String>> = conn
        .query_row("SELECT art FROM albums WHERE id = ?1", [album], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(art.map(|art| art.into_iter().collect()))
}

/// Copies chosen artwork missing from the cache back into it.
pub(crate) fn restore_chosen(conn: &Connection, art_dir: &Path) -> Result<(), LibraryError> {
    let chosen: Vec<(i64, Option<String>, String, Vec<u8>)> = conn
        .prepare(
            "SELECT albums.id, albums.art, chosen_art.mime, chosen_art.data
             FROM chosen_art JOIN albums ON albums.id = chosen_art.album_id",
        )?
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<_, _>>()?;
    for (album, art, mime, data) in chosen {
        if art.is_some_and(|name| art_dir.join(name).is_file()) {
            continue;
        }
        if let Some(mime) = image_mime(&mime) {
            store(conn, art_dir, album, &Picture { mime, data })?;
        }
    }
    Ok(())
}

/// The image type, from the first bytes.
fn sniff(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
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
