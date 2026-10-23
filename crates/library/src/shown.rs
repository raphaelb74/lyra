//! What the library shows: the files' values, with the user's edits on top.
//!
//! The shown values live in the `tracks` and `albums` columns that browsing
//! reads. They are computed here only, from the file values, the path and
//! the edits, so clearing an edit shows exactly what a scan would.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::{LibraryError, UNKNOWN_ARTIST, VARIOUS_ARTISTS, path_from};
use crate::edits::{self, Target};
use crate::key::key;
use crate::values::{self, Field, FileValue};

/// Edited fields of one track or release: a value, or `None` for cleared.
pub(crate) type Edited = HashMap<Field, Option<String>>;

/// The shown columns of a track.
pub(crate) struct TrackShown {
    pub title: String,
    pub artist: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub genre: Option<String>,
}

/// What an edit's value shows: several values are NUL-separated, and rows
/// show the first.
fn edit_text(value: &str) -> Option<String> {
    value.split('\0').next().and_then(values::text)
}

/// A field's shown text: the edit if there is one, else the file's.
fn pick(values: &[FileValue], edited: &Edited, field: Field) -> Option<String> {
    match edited.get(&field) {
        Some(edit) => edit.as_deref().and_then(edit_text),
        None => values::file_shown(values, field),
    }
}

pub(crate) fn track_shown(path: &Path, values: &[FileValue], edited: &Edited) -> TrackShown {
    let number = |field| pick(values, edited, field).and_then(|n| values::number(&n));
    let folder_disc = path
        .parent()
        .and_then(Path::file_name)
        .and_then(disc_folder);
    TrackShown {
        title: pick(values, edited, Field::Title).unwrap_or_else(|| {
            path.file_stem()
                .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned())
        }),
        artist: pick(values, edited, Field::Artist).unwrap_or_else(|| UNKNOWN_ARTIST.to_owned()),
        track: number(Field::Track),
        // A disc folder numbers what its tags do not, unless the user cleared it.
        disc: number(Field::Disc).or(folder_disc.filter(|_| !edited.contains_key(&Field::Disc))),
        genre: pick(values, edited, Field::Genre),
    }
}

/// The folder that holds a release: the track's folder, or its parent when
/// that is a disc folder (`Album/CD1`). Also the disc folder's number.
pub(crate) fn release_dir(path: &Path) -> (&Path, Option<u32>) {
    let dir = path.parent().unwrap_or(Path::new("/"));
    match dir.file_name().and_then(disc_folder) {
        Some(disc) => (dir.parent().unwrap_or(dir), Some(disc)),
        None => (dir, None),
    }
}

/// The disc number of a disc folder (`CD1`, `CD 2`, `disc_3`, `Disk 2 -
/// Live`), matched without case; `None` for any other folder name.
pub(crate) fn disc_folder(name: &OsStr) -> Option<u32> {
    let name = name.to_str()?.to_lowercase();
    let rest = ["disc", "disk", "cd"]
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))?
        .trim_start_matches([' ', '_', '-', '.']);
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (number, after) = rest.split_at(digits);
    if after.chars().next().is_some_and(char::is_alphanumeric) {
        return None;
    }
    number.parse().ok()
}

/// Folder name as an album title for untagged files.
pub(crate) fn file_label(dir: &Path) -> String {
    dir.file_name().map_or_else(
        || "Unknown Album".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The artist named `name` (by matching key), created if new.
pub(crate) fn artist_id(conn: &Connection, name: &str) -> Result<i64, LibraryError> {
    let key = key(name);
    conn.prepare_cached(
        "INSERT INTO artists (name, key) VALUES (?1, ?2) ON CONFLICT (key) DO NOTHING",
    )?
    .execute([name, &key])?;
    let id = conn
        .prepare_cached("SELECT id FROM artists WHERE key = ?1")?
        .query_row([&key], |row| row.get(0))
        .optional()?;
    id.ok_or(LibraryError::Db(rusqlite::Error::QueryReturnedNoRows))
}

/// A track's file values, each field's in file order.
pub(crate) fn file_values(conn: &Connection, track: i64) -> Result<Vec<FileValue>, LibraryError> {
    let mut statement = conn.prepare_cached(
        "SELECT field, value, cut FROM file_values WHERE track_id = ?1 ORDER BY field, position",
    )?;
    let rows = statement.query_map([track], |row| {
        Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?))
    })?;
    let mut values = Vec::new();
    for row in rows {
        let (field, value, cut) = row?;
        if let Some(field) = Field::from_name(&field) {
            values.push(FileValue { field, value, cut });
        }
    }
    Ok(values)
}

/// Replaces a track's file values with what a read found.
pub(crate) fn store_file_values(
    conn: &Connection,
    track: i64,
    values: &[FileValue],
) -> Result<(), LibraryError> {
    conn.prepare_cached("DELETE FROM file_values WHERE track_id = ?1")?
        .execute([track])?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO file_values (track_id, field, position, value, cut)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut positions: HashMap<Field, u32> = HashMap::new();
    for value in values {
        let position = positions.entry(value.field).or_default();
        insert.execute(params![
            track,
            value.field.name(),
            *position,
            value.value,
            value.cut
        ])?;
        *position += 1;
    }
    Ok(())
}

/// Writes a track's shown columns from its file values and edits. Returns
/// its release, or `None` if the track is gone.
pub(crate) fn refresh_track(conn: &Connection, track: i64) -> Result<Option<i64>, LibraryError> {
    let Some((path, album)): Option<(Vec<u8>, i64)> = conn
        .prepare_cached("SELECT path, album_id FROM tracks WHERE id = ?1")?
        .query_row([track], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?
    else {
        return Ok(None);
    };
    let values = file_values(conn, track)?;
    let edited = edits::review(conn, Target::Track(track), |field| {
        values::file_shown(&values, field)
    })?;
    let shown = track_shown(&path_from(&path), &values, &edited);
    let artist = artist_id(conn, &shown.artist)?;
    conn.prepare_cached(
        "UPDATE tracks SET title = ?2, artist_id = ?3, track = ?4, disc = ?5, genre = ?6
         WHERE id = ?1",
    )?
    .execute(params![
        track,
        shown.title,
        artist,
        shown.track,
        shown.disc,
        shown.genre
    ])?;
    Ok(Some(album))
}

/// A track as its release's shown values need it.
struct ReleaseTrack {
    id: i64,
    path: Vec<u8>,
    /// From the album-artist tag; `None` when untagged.
    album_artist: Option<i64>,
    artist: i64,
    compilation: bool,
}

/// Writes a release's title, year and album artist from its tracks' file
/// values and the release's edits. The file value of a release field is
/// that of its first track (disc, track, path order) that has one.
pub(crate) fn refresh_release(conn: &Connection, album: i64) -> Result<(), LibraryError> {
    let tracks = release_tracks(conn, album)?;
    let Some(first) = tracks.first() else {
        return Ok(());
    };
    let values = tracks
        .iter()
        .map(|track| file_values(conn, track.id))
        .collect::<Result<Vec<_>, _>>()?;
    let file_value = |field| first_shown(&values, field);
    let edited = edits::review(conn, Target::Release(album), file_value)?;
    let release = |field| match edited.get(&field) {
        Some(edit) => edit.as_deref().and_then(edit_text),
        None => file_value(field),
    };

    let title =
        release(Field::Album).unwrap_or_else(|| file_label(release_dir(&path_from(&first.path)).0));
    let year = release(Field::Date).and_then(|date| values::year(&date));
    let artist = match edited.get(&Field::AlbumArtist) {
        Some(Some(name)) => edit_text(name)
            .map(|name| artist_id(conn, &name))
            .transpose()?,
        // Cleared: decided as if no track had an album-artist tag.
        edit => {
            let compilation = match edited.get(&Field::Compilation) {
                Some(edit) => edit.as_deref() == Some("1"),
                None => tracks.iter().any(|track| track.compilation),
            };
            let tagged = edit.is_none();
            album_artist(
                tracks
                    .iter()
                    .map(|track| (track.album_artist.filter(|_| tagged), track.artist)),
                compilation,
            )
        }
    };
    let artist = match artist {
        Some(id) => id,
        None => artist_id(conn, VARIOUS_ARTISTS)?,
    };
    conn.prepare_cached("UPDATE albums SET title = ?2, year = ?3, artist_id = ?4 WHERE id = ?1")?
        .execute(params![album, title, year, artist])?;
    Ok(())
}

/// A release's tracks, in disc, track and path order.
fn release_tracks(conn: &Connection, album: i64) -> Result<Vec<ReleaseTrack>, LibraryError> {
    let tracks = conn
        .prepare_cached(
            "SELECT id, path, album_artist_id, artist_id, compilation FROM tracks
             WHERE album_id = ?1 ORDER BY disc, track, path",
        )?
        .query_map([album], |row| {
            Ok(ReleaseTrack {
                id: row.get(0)?,
                path: row.get(1)?,
                album_artist: row.get(2)?,
                artist: row.get(3)?,
                compilation: row.get(4)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(tracks)
}

/// A release field's file value: that of its first track that has one.
fn first_shown(tracks: &[Vec<FileValue>], field: Field) -> Option<String> {
    tracks
        .iter()
        .find_map(|values| values::file_shown(values, field))
}

/// What a release's files say for `field`, as [`refresh_release`] reads it.
pub(crate) fn release_file_value(
    conn: &Connection,
    album: i64,
    field: Field,
) -> Result<Option<String>, LibraryError> {
    let values = release_tracks(conn, album)?
        .iter()
        .map(|track| file_values(conn, track.id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(first_shown(&values, field))
}

/// A release's album artist from its tracks' (album-artist tag, artist):
/// the album artist most tagged; else, unless it is a compilation, the
/// artist when every track agrees. `None` is Various Artists.
fn album_artist(
    tracks: impl Iterator<Item = (Option<i64>, i64)> + Clone,
    compilation: bool,
) -> Option<i64> {
    let mut tagged: HashMap<i64, usize> = HashMap::new();
    for (album_artist, _) in tracks.clone() {
        if let Some(id) = album_artist {
            *tagged.entry(id).or_default() += 1;
        }
    }
    if let Some((id, _)) = tagged
        .into_iter()
        .max_by_key(|&(id, count)| (count, Reverse(id)))
    {
        return Some(id);
    }
    if compilation {
        return None;
    }
    let mut artists = tracks.map(|(_, artist)| artist);
    let first = artists.next()?;
    artists.all(|artist| artist == first).then_some(first)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::disc_folder;

    #[test]
    fn disc_folders_are_recognised() {
        for (name, disc) in [
            ("CD1", 1),
            ("cd 2", 2),
            ("Disc 03", 3),
            ("disk_4", 4),
            ("Disc 2 - Live", 2),
            ("CD.1 (Remastered)", 1),
        ] {
            assert_eq!(disc_folder(OsStr::new(name)), Some(disc), "{name}");
        }
        for name in ["Discography", "CDs", "CD", "cd1a", "Disco 2", "Album CD1"] {
            assert_eq!(disc_folder(OsStr::new(name)), None, "{name}");
        }
    }
}
