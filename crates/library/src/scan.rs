//! Folder scanner. Runs on its own thread with its own connection; never
//! touches the audio path.
//!
//! Incremental: a file whose size and modification time are unchanged is
//! not read again. Tags are read outside transactions and written in small
//! batches, so the UI can read (WAL) and edit folders while a scan runs.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::{Connection, OptionalExtension, params};

use crate::art;
use crate::db::{Library, LibraryError, UNKNOWN_ARTIST, now, path_bytes, path_from, prune};
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
    let found: Vec<(i64, Vec<Found>)> = folders
        .iter()
        .map(|(id, root)| (*id, audio_files(root)))
        .collect();
    let total = found.iter().map(|(_, files)| files.len()).sum();

    let mut summary = ScanSummary::default();
    let mut done = 0;
    progress(ScanProgress { done, total });
    'folders: for (folder_id, files) in found {
        let mut known = known_tracks(conn, folder_id)?;
        let mut pending = Vec::new();
        for file in files {
            if cancel.load(Ordering::Relaxed) {
                summary.cancelled = true;
                write_batch(conn, art_dir, &mut pending, &mut summary)?;
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
                    write_batch(conn, art_dir, &mut pending, &mut summary)?;
                }
            }
            done += 1;
            progress(ScanProgress { done, total });
        }
        write_batch(conn, art_dir, &mut pending, &mut summary)?;
        // Whatever was not found any more is gone from disk.
        let transaction = conn.transaction()?;
        for (id, _) in known.into_values() {
            transaction.execute("DELETE FROM tracks WHERE id = ?1", [id])?;
            summary.removed += 1;
        }
        transaction.commit()?;
    }

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
/// (no cycles). Unreadable folders are skipped.
fn audio_files(root: &Path) -> Vec<Found> {
    let mut files = Vec::new();
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_name().as_encoded_bytes().starts_with(b".") {
                continue;
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
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
    files
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
                )?;
                if existed {
                    summary.updated += 1;
                } else {
                    summary.added += 1;
                }
            }
            Err(reason) => {
                // A file that no longer reads is dropped from the library.
                transaction.execute(
                    "DELETE FROM tracks WHERE path = ?1",
                    [path_bytes(&file.path)],
                )?;
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
) -> Result<(), LibraryError> {
    let dir = file.path.parent().unwrap_or(Path::new("/"));
    let artist = scanned.artist.as_deref().unwrap_or(UNKNOWN_ARTIST);
    let album_artist = scanned.album_artist.as_deref().unwrap_or(artist);
    let album_title = scanned.album.clone().unwrap_or_else(|| file_label(dir));
    let title = scanned.title.clone().unwrap_or_else(|| {
        file.path
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned())
    });

    let artist_id = artist_id(conn, artist)?;
    let album_artist_id = artist_id_or(conn, album_artist, artist, artist_id)?;
    conn.prepare_cached(
        "INSERT INTO albums (title, artist_id, year) VALUES (?1, ?2, ?3)
         ON CONFLICT (title, artist_id) DO UPDATE SET year = COALESCE(albums.year, ?3)",
    )?
    .execute(params![album_title, album_artist_id, scanned.year])?;
    let (album_id, has_art): (i64, bool) = conn
        .prepare_cached(
            "SELECT id, art IS NOT NULL FROM albums WHERE title = ?1 AND artist_id = ?2",
        )?
        .query_row(params![album_title, album_artist_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;

    conn.prepare_cached(
        "INSERT INTO tracks (folder_id, path, size, mtime_ns, album_id, artist_id, title,
             disc, track, duration_ms, codec, sample_rate, bits_per_sample, channels, genre)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT (path) DO UPDATE SET folder_id = ?1, size = ?3, mtime_ns = ?4,
             album_id = ?5, artist_id = ?6, title = ?7, disc = ?8, track = ?9,
             duration_ms = ?10, codec = ?11, sample_rate = ?12, bits_per_sample = ?13,
             channels = ?14, genre = ?15",
    )?
    .execute(params![
        folder_id,
        path_bytes(&file.path),
        file.size,
        file.mtime_ns,
        album_id,
        artist_id,
        title,
        scanned.disc,
        scanned.track,
        scanned.duration_ms.and_then(|ms| i64::try_from(ms).ok()),
        scanned.codec,
        scanned.sample_rate,
        scanned.bits_per_sample,
        scanned.channels,
        scanned.genre,
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

fn artist_id(conn: &Connection, name: &str) -> Result<i64, LibraryError> {
    conn.prepare_cached("INSERT INTO artists (name) VALUES (?1) ON CONFLICT (name) DO NOTHING")?
        .execute([name])?;
    let id = conn
        .prepare_cached("SELECT id FROM artists WHERE name = ?1")?
        .query_row([name], |row| row.get(0))
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
    if name.eq_ignore_ascii_case(known) {
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
