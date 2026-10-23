//! Folder scanner. Runs on its own thread with its own connection; never
//! touches the audio path.
//!
//! Incremental: a file whose size and modification time are unchanged is
//! not read again. Tags are read outside transactions and written in small
//! batches, so the UI can read (WAL) and edit folders while a scan runs.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::{Connection, OptionalExtension, params};

use crate::art;
use crate::db::{Library, LibraryError, now, path_bytes, path_from, prune};
use crate::edits::{self, Target};
use crate::key::key;
use crate::shown::{self, Edited};
use crate::tags::{self, Scanned};
use crate::values::Field;

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
    /// Includes the tracks in `moved`.
    pub updated: usize,
    /// Tracks whose file was moved or renamed: same track, new path.
    pub moved: usize,
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
    /// Releases created, in order, so the first created wins a tie.
    created: BTreeSet<i64>,
    /// Track written → the release it was in before (`None`: new to the
    /// library).
    previous: HashMap<i64, Option<i64>>,
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
    // Tracks not found in a folder walked in full.
    let mut missing = Vec::new();
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
        // Whatever was not found any more is moved or gone from disk,
        // unless it sits where the walk could not look.
        missing.extend(
            known
                .into_iter()
                .filter(|(path, _)| !walk.covers(&path_from(path)))
                .map(|(_, (id, _))| id),
        );
    }

    // A cancelled scan may not have reached where a missing track went.
    if !summary.cancelled {
        follow_moves(conn, &mut missing, &mut regroup, &mut summary)?;
        let transaction = conn.transaction()?;
        for id in missing {
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
    let (release_dir, _) = shown::release_dir(&file.path);
    let existing: Option<(i64, i64)> = conn
        .prepare_cached("SELECT id, album_id FROM tracks WHERE path = ?1")?
        .query_row([path_bytes(&file.path)], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()?;
    let previous = existing.map(|(_, album)| album);
    let edited = match existing {
        Some((id, _)) => edits::review(conn, Target::Track(id), |field| scanned.shown(field))?,
        None => Edited::new(),
    };
    let shown = shown::track_shown(&file.path, &scanned.values, &edited);
    let edited_artist = matches!(edited.get(&Field::Artist), Some(Some(_)));
    let artist_id = shown::shown_artist_id(conn, &shown.artist, edited_artist)?;

    // The release is found by what the file says; edits do not move tracks.
    let album_artist = scanned.shown(Field::AlbumArtist);
    let album_artist_id = album_artist
        .as_deref()
        .map(|name| shown::artist_id(conn, name))
        .transpose()?;
    let album_title = scanned
        .shown(Field::Album)
        .unwrap_or_else(|| shown::file_label(release_dir));
    let grouping = grouping(
        scanned.shown(Field::ReleaseId).as_deref(),
        release_dir,
        &album_title,
        album_artist.as_deref(),
    );
    let (album_id, has_art) = release(
        conn,
        &grouping,
        &album_title,
        album_artist_id.unwrap_or(artist_id),
        regroup,
    )?;
    regroup.touched.insert(album_id);
    regroup.touched.extend(previous);

    let track_id: i64 = conn
        .prepare_cached(
            "INSERT INTO tracks (folder_id, path, size, mtime_ns, album_id, artist_id, title,
             disc, track, duration_ms, codec, sample_rate, bits_per_sample, channels, genre,
             album_artist_id, compilation, audio_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
             ?18)
         ON CONFLICT (path) DO UPDATE SET folder_id = ?1, size = ?3, mtime_ns = ?4,
             album_id = ?5, artist_id = ?6, title = ?7, disc = ?8, track = ?9,
             duration_ms = ?10, codec = ?11, sample_rate = ?12, bits_per_sample = ?13,
             channels = ?14, genre = ?15, album_artist_id = ?16, compilation = ?17,
             audio_id = ?18
         RETURNING id",
        )?
        .query_row(
            params![
                folder_id,
                path_bytes(&file.path),
                file.size,
                file.mtime_ns,
                album_id,
                artist_id,
                shown.title,
                shown.disc,
                shown.track,
                scanned.duration_ms.and_then(|ms| i64::try_from(ms).ok()),
                scanned.codec,
                scanned.sample_rate,
                scanned.bits_per_sample,
                scanned.channels,
                shown.genre,
                album_artist_id,
                scanned.shown(Field::Compilation).as_deref() == Some("1"),
                audio_id(file.size, &scanned),
            ],
            |row| row.get(0),
        )?;
    regroup.previous.insert(track_id, previous);
    shown::store_file_values(conn, track_id, &scanned.values)?;

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

/// What identifies a file's audio whatever its name: the FLAC audio MD5,
/// else the codec, file size, rate, channels and length together.
fn audio_id(size: i64, scanned: &Scanned) -> Option<String> {
    if let Some(md5) = scanned.audio_md5 {
        let hex = md5.iter().fold(String::with_capacity(32), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        });
        return Some(format!("md5:{hex}"));
    }
    let frames = scanned.frames?;
    Some(format!(
        "{}:{size}:{}:{}:{frames}",
        scanned.codec, scanned.sample_rate, scanned.channels
    ))
}

/// Missing tracks that were moved or renamed: a file new to the library
/// with the same audio, the only candidate on both sides. The track takes
/// the new file's path and values and keeps its ID, playlists and release;
/// the new row goes. Moved tracks leave `missing`.
fn follow_moves(
    conn: &mut Connection,
    missing: &mut Vec<i64>,
    regroup: &mut Regroup,
    summary: &mut ScanSummary,
) -> Result<(), LibraryError> {
    let arrived: Vec<i64> = regroup
        .previous
        .iter()
        .filter(|(_, previous)| previous.is_none())
        .map(|(&id, _)| id)
        .collect();
    if missing.is_empty() || arrived.is_empty() {
        return Ok(());
    }
    let transaction = conn.transaction()?;
    let by_audio = |ids: &[i64]| -> Result<HashMap<String, Vec<i64>>, LibraryError> {
        let mut statement =
            transaction.prepare_cached("SELECT audio_id FROM tracks WHERE id = ?1")?;
        let mut found: HashMap<String, Vec<i64>> = HashMap::new();
        for &id in ids {
            let audio: Option<String> = statement.query_row([id], |row| row.get(0))?;
            if let Some(audio) = audio {
                found.entry(audio).or_default().push(id);
            }
        }
        Ok(found)
    };
    let gone = by_audio(missing)?;
    let new = by_audio(&arrived)?;
    let mut moved = HashSet::new();
    for (audio, olds) in gone {
        let ([old], Some([arrival])) = (olds.as_slice(), new.get(&audio).map(Vec::as_slice)) else {
            continue;
        };
        let album: i64 =
            transaction.query_row("SELECT album_id FROM tracks WHERE id = ?1", [old], |row| {
                row.get(0)
            })?;
        move_track(&transaction, *old, *arrival)?;
        regroup.previous.remove(arrival);
        regroup.previous.insert(*old, Some(album));
        regroup.touched.insert(album);
        moved.insert(*old);
    }
    transaction.commit()?;
    missing.retain(|id| !moved.contains(id));
    summary.moved += moved.len();
    summary.added -= moved.len();
    summary.updated += moved.len();
    Ok(())
}

/// Every `tracks` column a read of the file sets: what a moved track takes
/// from its new file. A test checks it against the schema.
const FILE_COLUMNS: &str = "folder_id, size, mtime_ns, album_id, artist_id, title, disc, track,
    duration_ms, codec, sample_rate, bits_per_sample, channels, genre, album_artist_id,
    compilation, audio_id";

/// Gives track `old` everything of track `new`, its moved file, then
/// deletes `new`.
fn move_track(conn: &Connection, old: i64, new: i64) -> Result<(), LibraryError> {
    let path: Vec<u8> = conn.query_row("SELECT path FROM tracks WHERE id = ?1", [new], |row| {
        row.get(0)
    })?;
    // Added to a playlist while the scan ran.
    conn.execute(
        "UPDATE playlist_items SET track_id = ?1 WHERE track_id = ?2",
        [old, new],
    )?;
    // Paths are unique and absolute: free this one for `old`.
    conn.execute(
        "UPDATE tracks SET path = CAST('moving:' || id AS BLOB) WHERE id = ?1",
        [new],
    )?;
    conn.execute(
        &format!(
            "UPDATE tracks SET path = ?3, ({FILE_COLUMNS}) =
                 (SELECT {FILE_COLUMNS} FROM tracks AS moved WHERE moved.id = ?2)
             WHERE id = ?1"
        ),
        params![old, new, path],
    )?;
    conn.execute("DELETE FROM file_values WHERE track_id = ?1", [old])?;
    conn.execute(
        "UPDATE file_values SET track_id = ?1 WHERE track_id = ?2",
        [old, new],
    )?;
    conn.execute("DELETE FROM tracks WHERE id = ?1", [new])?;
    // The new file's values, with the track's edits on top.
    shown::refresh_track(conn, old)?;
    Ok(())
}

/// The release with `grouping`, created if new, and whether it has
/// artwork. A new release's title and artist are provisional: [`settle`]
/// decides them once every track is in.
fn release(
    conn: &Connection,
    grouping: &[u8],
    title: &str,
    artist_id: i64,
    regroup: &mut Regroup,
) -> Result<(i64, bool), LibraryError> {
    let found: Option<(i64, bool)> = conn
        .prepare_cached("SELECT id, art IS NOT NULL FROM albums WHERE grouping = ?1")?
        .query_row([grouping], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?;
    if let Some(found) = found {
        return Ok(found);
    }
    conn.prepare_cached("INSERT INTO albums (title, artist_id, grouping) VALUES (?1, ?2, ?3)")?
        .execute(params![title, artist_id, grouping])?;
    let id = conn.last_insert_rowid();
    regroup.created.insert(id);
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

/// Ends a scan. A release created in this scan whose tracks all came from
/// one release, now empty, takes that release's place: the ID and artwork
/// stay, so a retagged or regrouped album is still the same album. Then
/// every touched release gets its shown title, year and album artist. Returns artwork files no
/// longer used.
///
/// A scan cancelled halfway through moving a release leaves it split; the
/// moved part then keeps its new ID.
fn settle(conn: &mut Connection, regroup: Regroup) -> Result<Vec<String>, LibraryError> {
    let transaction = conn.transaction()?;
    let mut unused = Vec::new();
    for &new in &regroup.created {
        let tracks: Vec<i64> = transaction
            .prepare_cached("SELECT id FROM tracks WHERE album_id = ?1")?
            .query_map([new], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        let sources: HashSet<Option<i64>> = tracks
            .iter()
            .map(|id| regroup.previous.get(id).copied().flatten())
            .collect();
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
        let (grouping, art, art_bytes): (Vec<u8>, Option<String>, i64) = transaction.query_row(
            "SELECT grouping, art, art_bytes FROM albums WHERE id = ?1",
            [new],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        transaction.execute(
            "UPDATE tracks SET album_id = ?1 WHERE album_id = ?2",
            [old, new],
        )?;
        // The grouping is unique: the new row goes before the old one takes it.
        transaction.execute("DELETE FROM albums WHERE id = ?1", [new])?;
        transaction.execute(
            "UPDATE albums SET grouping = ?2 WHERE id = ?1",
            params![old, grouping],
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

    for album in regroup.touched {
        shown::refresh_release(&transaction, album)?;
    }
    transaction.commit()?;
    Ok(unused)
}

/// SQLite integers are signed 64-bit.
fn count(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::FILE_COLUMNS;
    use crate::db::Library;

    #[test]
    fn a_move_copies_every_file_column() {
        let library = Library::in_memory().unwrap();
        let columns: BTreeSet<String> = library
            .conn
            .prepare("SELECT name FROM pragma_table_info('tracks')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let mut expected: BTreeSet<String> = FILE_COLUMNS
            .split(',')
            .map(|column| column.trim().to_owned())
            .collect();
        // Identity, and the path the move sets itself.
        expected.extend(["id".to_owned(), "path".to_owned()]);
        assert_eq!(columns, expected);
    }
}
