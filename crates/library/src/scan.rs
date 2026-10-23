//! Folder scanner. Runs on its own thread with its own connection; never
//! touches the audio path.
//!
//! Incremental: a file whose size and modification time are unchanged is
//! not read again. Tags are read outside transactions and written in small
//! batches, so the UI can read (WAL) and edit folders while a scan runs.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::{Connection, OptionalExtension, params};

use crate::art;
use crate::db::{
    Library, LibraryError, UNKNOWN_ARTIST, VARIOUS_ARTISTS, now, path_bytes, path_from, prune,
};
use crate::key::key;
use crate::tags::{self, Scanned};

/// Tag results written per transaction.
const BATCH: usize = 64;
/// Failures kept for the report (all are counted).
const MAX_REPORTED_FAILURES: usize = 50;

/// Extensions the scanner (and the player) accept; content decides later.
pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["flac", "wav", "dsf", "dff"]
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanProgress {
    /// Files checked so far.
    pub done: usize,
    /// Audio files found in all folders.
    pub total: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanSummary {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub failed: usize,
    /// The first failures: file and reason.
    pub failures: Vec<(PathBuf, String)>,
    /// Stopped early; files not reached were left as they were.
    pub cancelled: bool,
    /// Library folders that could not be read, or that look empty while the
    /// library has tracks in them (an unplugged drive): their tracks were kept.
    pub unavailable: Vec<PathBuf>,
}

/// A track already in the database, by path.
type Known = HashMap<Vec<u8>, (i64, Stamp)>;

/// Size and modification time (ns): a file is re-read when either changes.
type Stamp = (i64, i64);

/// A file read, waiting to be written.
struct Pending {
    folder_id: i64,
    file: Found,
    scanned: Result<Scanned, String>,
    existed: bool,
}

/// What a walk through one library folder found.
struct Walk {
    /// The folder itself could be listed.
    opened: bool,
    files: Vec<Found>,
    /// Folders that could not be listed, in whole or in part, and files that
    /// could not be examined: known tracks under them are kept, not removed.
    unreadable: Vec<PathBuf>,
}

impl Walk {
    fn covers(&self, path: &Path) -> bool {
        self.unreadable.iter().any(|dir| path.starts_with(dir))
    }
}

/// Releases this scan created or changed, for [`settle`].
#[derive(Default)]
struct Regroup {
    /// New release → the releases its tracks were in before (`None`: a track
    /// new to the library). Ordered, so the first created wins a tie.
    created: BTreeMap<i64, HashSet<Option<i64>>>,
    /// Releases that gained or lost a track: their album artist is decided
    /// again.
    touched: HashSet<i64>,
}

/// A file found on disk.
struct Found {
    path: PathBuf,
    size: i64,
    mtime_ns: i64,
}

/// Scans every library folder. `cancel` is checked between files.
pub fn scan(
    library: &mut Library,
    art_dir: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(ScanProgress),
) -> Result<ScanSummary, LibraryError> {
    let conn = &mut library.conn;
    conn.execute("INSERT INTO scan_runs (started_at) VALUES (?1)", [now()])?;
    let run = conn.last_insert_rowid();

    let folders: Vec<(i64, PathBuf)> = {
        let mut statement = conn.prepare("SELECT id, path FROM folders")?;
        statement
            .query_map([], |row| {
                Ok((row.get(0)?, path_from(&row.get::<_, Vec<u8>>(1)?)))
            })?
            .collect::<Result<_, _>>()?
    };
    let found: Vec<(i64, &Path, Walk)> = folders
        .iter()
        .map(|(id, root)| (*id, root.as_path(), audio_files(root)))
        .collect();
    let total = found.iter().map(|(_, _, walk)| walk.files.len()).sum();

    let mut summary = ScanSummary::default();
    let mut regroup = Regroup::default();
    let mut done = 0;
    progress(ScanProgress { done, total });
    'folders: for (folder_id, root, mut walk) in found {
        let mut known = known_tracks(conn, folder_id)?;
        // A missing drive must not empty the library: an unmounted mount
        // point is often an empty folder.
        if !walk.opened || (walk.files.is_empty() && !known.is_empty()) {
            summary.unavailable.push(root.to_owned());
            continue;
        }
        let mut pending = Vec::new();
        for file in std::mem::take(&mut walk.files) {
            if cancel.load(Ordering::Relaxed) {
                summary.cancelled = true;
                write_batch(conn, art_dir, &mut pending, &mut summary, &mut regroup)?;
                break 'folders;
            }
            let previous = known.remove(path_bytes(&file.path));
            if previous.is_none_or(|(_, stamp)| stamp != (file.size, file.mtime_ns)) {
                let scanned = tags::read(&file.path);
                pending.push(Pending {
                    folder_id,
                    file,
                    scanned,
                    existed: previous.is_some(),
                });
                if pending.len() >= BATCH {
                    write_batch(conn, art_dir, &mut pending, &mut summary, &mut regroup)?;
                }
            }
            done += 1;
            progress(ScanProgress { done, total });
        }
        write_batch(conn, art_dir, &mut pending, &mut summary, &mut regroup)?;
        // Whatever was not found any more is gone from disk, unless it sits
        // where the walk could not look.
        let transaction = conn.transaction()?;
        for (path, (id, _)) in known {
            if walk.covers(&path_from(&path)) {
                continue;
            }
            transaction.execute("DELETE FROM tracks WHERE id = ?1", [id])?;
            summary.removed += 1;
        }
        transaction.commit()?;
    }

    let replaced = settle(conn, regroup)?;
    art::delete_art(art_dir, &replaced);
    let unused = prune(conn)?;
    art::delete_art(art_dir, &unused);
    conn.execute(
        "UPDATE scan_runs SET finished_at = ?2, added = ?3, updated = ?4, removed = ?5,
         failed = ?6 WHERE id = ?1",
        params![
            run,
            now(),
            count(summary.added),
            count(summary.updated),
            count(summary.removed),
            count(summary.failed)
        ],
    )?;
    Ok(summary)
}

/// Audio files under `root`, sorted. Hidden entries (`.name`, including
/// macOS `._` files) are skipped, and symlinked folders are not followed
/// (no cycles). What cannot be read is reported, not taken as absent.
fn audio_files(root: &Path) -> Walk {
    let mut files = Vec::new();
    let mut unreadable = Vec::new();
    let mut opened = false;
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            unreadable.push(folder);
            continue;
        };
        opened |= folder == root;
        for entry in entries {
            let Ok(entry) = entry else {
                unreadable.push(folder.clone());
                continue;
            };
            if entry.file_name().as_encoded_bytes().starts_with(b".") {
                continue;
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                unreadable.push(path);
                continue;
            };
            if kind.is_dir() {
                folders.push(path);
                continue;
            }
            if !is_audio_file(&path) {
                continue;
            }
            // Follows a symlinked file; skips one that points at a folder.
            let Ok(metadata) = fs::metadata(&path) else {
                unreadable.push(path);
                continue;
            };
            if metadata.is_file() {
                files.push(Found {
                    path,
                    size: i64::try_from(metadata.len()).unwrap_or(i64::MAX),
                    mtime_ns: metadata
                        .mtime()
                        .saturating_mul(1_000_000_000)
                        .saturating_add(metadata.mtime_nsec()),
                });
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Walk {
        opened,
        files,
        unreadable,
    }
}

/// Tracks already in the database for a folder: path → (id, size, mtime).
fn known_tracks(conn: &Connection, folder_id: i64) -> Result<Known, LibraryError> {
    let mut statement =
        conn.prepare("SELECT path, id, size, mtime_ns FROM tracks WHERE folder_id = ?1")?;
    let rows = statement.query_map([folder_id], |row| {
        Ok((row.get(0)?, (row.get(1)?, (row.get(2)?, row.get(3)?))))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn write_batch(
    conn: &mut Connection,
    art_dir: &Path,
    pending: &mut Vec<Pending>,
    summary: &mut ScanSummary,
    regroup: &mut Regroup,
) -> Result<(), LibraryError> {
    if pending.is_empty() {
        return Ok(());
    }
    let transaction = conn.transaction()?;
    // Folders already checked for a cover image in this batch.
    let mut looked: HashSet<PathBuf> = HashSet::new();
    for Pending {
        folder_id,
        file,
        scanned,
        existed,
    } in pending.drain(..)
    {
        match scanned {
            Ok(scanned) => {
                write_track(
                    &transaction,
                    art_dir,
                    folder_id,
                    &file,
                    scanned,
                    &mut looked,
                    regroup,
                )?;
                if existed {
                    summary.updated += 1;
                } else {
                    summary.added += 1;
                }
            }
            Err(reason) => {
                // A known track keeps its entry and playlists: the failure may
                // be passing (a slow drive). Its old stamp makes the next scan
                // read it again.
                summary.failed += 1;
                if summary.failures.len() < MAX_REPORTED_FAILURES {
                    summary.failures.push((file.path, reason));
                }
            }
        }
    }
    transaction.commit()?;
    Ok(())
}

fn write_track(
    conn: &Connection,
    art_dir: &Path,
    folder_id: i64,
    file: &Found,
    scanned: Scanned,
    looked: &mut HashSet<PathBuf>,
    regroup: &mut Regroup,
) -> Result<(), LibraryError> {
    let dir = file.path.parent().unwrap_or(Path::new("/"));
    // `Album/CD1` and `Album/CD2` are one release, in `Album`.
    let folder_disc = dir.file_name().and_then(disc_folder);
    let release_dir = match folder_disc {
        Some(_) => dir.parent().unwrap_or(dir),
        None => dir,
    };
    let artist = scanned.artist.as_deref().unwrap_or(UNKNOWN_ARTIST);
    let album_title = scanned
        .album
        .clone()
        .unwrap_or_else(|| file_label(release_dir));
    let title = scanned.title.clone().unwrap_or_else(|| {
        file.path
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned())
    });

    let artist_id = artist_id(conn, artist)?;
    let album_artist_id = match scanned.album_artist.as_deref() {
        Some(name) => Some(artist_id_or(conn, name, artist, artist_id)?),
        None => None,
    };
    let grouping = grouping(
        scanned.release_id.as_deref(),
        release_dir,
        &album_title,
        scanned.album_artist.as_deref(),
    );
    let (album_id, has_art) = release(
        conn,
        &grouping,
        &album_title,
        album_artist_id.unwrap_or(artist_id),
        scanned.year,
        regroup,
    )?;
    let previous: Option<i64> = conn
        .prepare_cached("SELECT album_id FROM tracks WHERE path = ?1")?
        .query_row([path_bytes(&file.path)], |row| row.get(0))
        .optional()?;
    if let Some(sources) = regroup.created.get_mut(&album_id) {
        sources.insert(previous);
    }
    regroup.touched.insert(album_id);
    regroup.touched.extend(previous);

    conn.prepare_cached(
        "INSERT INTO tracks (folder_id, path, size, mtime_ns, album_id, artist_id, title,
             disc, track, duration_ms, codec, sample_rate, bits_per_sample, channels, genre,
             album_artist_id, compilation)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT (path) DO UPDATE SET folder_id = ?1, size = ?3, mtime_ns = ?4,
             album_id = ?5, artist_id = ?6, title = ?7, disc = ?8, track = ?9,
             duration_ms = ?10, codec = ?11, sample_rate = ?12, bits_per_sample = ?13,
             channels = ?14, genre = ?15, album_artist_id = ?16, compilation = ?17",
    )?
    .execute(params![
        folder_id,
        path_bytes(&file.path),
        file.size,
        file.mtime_ns,
        album_id,
        artist_id,
        title,
        scanned.disc.or(folder_disc),
        scanned.track,
        scanned.duration_ms.and_then(|ms| i64::try_from(ms).ok()),
        scanned.codec,
        scanned.sample_rate,
        scanned.bits_per_sample,
        scanned.channels,
        scanned.genre,
        album_artist_id,
        scanned.compilation,
    ])?;

    if !has_art {
        let picture = match scanned.picture {
            Some(picture) => Some(picture),
            None if looked.insert(dir.to_owned()) => art::folder_image(dir),
            None => None,
        };
        if let Some(picture) = picture {
            art::store(conn, art_dir, album_id, &picture)?;
        }
    }
    Ok(())
}

/// The release with `grouping`, created if new, and whether it has
/// artwork. A new release's `artist_id` is provisional: [`settle`] decides it
/// once every track is in.
fn release(
    conn: &Connection,
    grouping: &[u8],
    title: &str,
    artist_id: i64,
    year: Option<i32>,
    regroup: &mut Regroup,
) -> Result<(i64, bool), LibraryError> {
    let found: Option<(i64, bool)> = conn
        .prepare_cached("SELECT id, art IS NOT NULL FROM albums WHERE grouping = ?1")?
        .query_row([grouping], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?;
    if let Some((id, has_art)) = found {
        conn.prepare_cached("UPDATE albums SET year = COALESCE(year, ?2) WHERE id = ?1")?
            .execute(params![id, year])?;
        return Ok((id, has_art));
    }
    conn.prepare_cached(
        "INSERT INTO albums (title, artist_id, year, grouping) VALUES (?1, ?2, ?3, ?4)",
    )?
    .execute(params![title, artist_id, year, grouping])?;
    let id = conn.last_insert_rowid();
    regroup.created.insert(id, HashSet::new());
    Ok((id, false))
}

/// Why tracks belong to one release, first rule that applies:
/// - their MusicBrainz release ID;
/// - their release folder, album and album artist (left out when untagged,
///   so a compilation in one folder stays one release).
///
/// Parts are compared by matching key and joined by NUL, which neither a
/// path nor a key contains.
fn grouping(
    release_id: Option<&str>,
    release_dir: &Path,
    album: &str,
    album_artist: Option<&str>,
) -> Vec<u8> {
    if let Some(id) = release_id.map(key).filter(|id| !id.is_empty()) {
        return [b"mb:".as_slice(), id.as_bytes()].concat();
    }
    let mut grouping = b"dir:".to_vec();
    grouping.extend_from_slice(path_bytes(release_dir));
    grouping.push(0);
    grouping.extend_from_slice(key(album).as_bytes());
    grouping.push(0);
    if let Some(album_artist) = album_artist {
        grouping.extend_from_slice(key(album_artist).as_bytes());
    }
    grouping
}

/// The disc number of a disc folder (`CD1`, `CD 2`, `disc_3`, `Disk 2 -
/// Live`), matched without case; `None` for any other folder name.
fn disc_folder(name: &OsStr) -> Option<u32> {
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

/// Ends a scan. A release created in this scan whose tracks all came from
/// one release, now empty, takes that release's place: the ID and artwork
/// stay, so a retagged or regrouped album is still the same album. Then
/// every touched release gets its album artist. Returns artwork files no
/// longer used.
///
/// A scan cancelled halfway through moving a release leaves it split; the
/// moved part then keeps its new ID.
fn settle(conn: &mut Connection, regroup: Regroup) -> Result<Vec<String>, LibraryError> {
    let transaction = conn.transaction()?;
    let mut unused = Vec::new();
    for (new, sources) in regroup.created {
        let mut sources = sources.into_iter();
        let (Some(Some(old)), None) = (sources.next(), sources.next()) else {
            continue;
        };
        let Some(old_art) = transaction
            .query_row(
                "SELECT art FROM albums WHERE id = ?1
                 AND NOT EXISTS (SELECT 1 FROM tracks WHERE album_id = ?1)",
                [old],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
        else {
            continue;
        };
        let (grouping, title, year, art, art_bytes): (
            Vec<u8>,
            String,
            Option<i32>,
            Option<String>,
            i64,
        ) = transaction.query_row(
            "SELECT grouping, title, year, art, art_bytes FROM albums WHERE id = ?1",
            [new],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
        transaction.execute(
            "UPDATE tracks SET album_id = ?1 WHERE album_id = ?2",
            [old, new],
        )?;
        // The grouping is unique: the new row goes before the old one takes it.
        transaction.execute("DELETE FROM albums WHERE id = ?1", [new])?;
        transaction.execute(
            "UPDATE albums SET grouping = ?2, title = ?3, year = COALESCE(?4, year) WHERE id = ?1",
            params![old, grouping, title, year],
        )?;
        if old_art.is_none() {
            transaction.execute(
                "UPDATE albums SET art = ?2, art_bytes = ?3 WHERE id = ?1",
                params![old, art, art_bytes],
            )?;
        } else {
            unused.extend(art);
        }
    }

    let mut various = None;
    for album in regroup.touched {
        let tracks: Vec<(Option<i64>, i64, bool)> = transaction
            .prepare_cached(
                "SELECT album_artist_id, artist_id, compilation FROM tracks WHERE album_id = ?1",
            )?
            .query_map([album], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<_, _>>()?;
        if tracks.is_empty() {
            continue;
        }
        let artist = match album_artist(&tracks) {
            Some(id) => id,
            None => match various {
                Some(id) => id,
                None => *various.insert(artist_id(&transaction, VARIOUS_ARTISTS)?),
            },
        };
        transaction
            .prepare_cached("UPDATE albums SET artist_id = ?2 WHERE id = ?1 AND artist_id != ?2")?
            .execute([album, artist])?;
    }
    transaction.commit()?;
    Ok(unused)
}

/// A release's album artist from its tracks' (album artist tag, artist,
/// compilation flag): the album artist most tagged; else, unless it is a
/// compilation, the artist when every track agrees. `None` is Various
/// Artists.
fn album_artist(tracks: &[(Option<i64>, i64, bool)]) -> Option<i64> {
    let mut tagged: HashMap<i64, usize> = HashMap::new();
    for &(album_artist, _, _) in tracks {
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
    if tracks.iter().any(|&(_, _, compilation)| compilation) {
        return None;
    }
    let (_, first, _) = *tracks.first()?;
    tracks
        .iter()
        .all(|&(_, artist, _)| artist == first)
        .then_some(first)
}

fn artist_id(conn: &Connection, name: &str) -> Result<i64, LibraryError> {
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

/// `name`'s id, reusing `known_id` when it is the same artist.
fn artist_id_or(
    conn: &Connection,
    name: &str,
    known: &str,
    known_id: i64,
) -> Result<i64, LibraryError> {
    if key(name) == key(known) {
        Ok(known_id)
    } else {
        artist_id(conn, name)
    }
}

/// SQLite integers are signed 64-bit.
fn count(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Folder name as an album title for untagged files.
fn file_label(dir: &Path) -> String {
    dir.file_name().map_or_else(
        || "Unknown Album".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
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
