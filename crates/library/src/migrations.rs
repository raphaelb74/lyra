//! Schema versions and the upgrade between them, with a backup first.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::db::{LibraryError, now};
use crate::key::key;

/// One schema change: SQL, or Rust where SQL cannot do it.
enum Migration {
    Sql(&'static str),
    Code(fn(&Transaction<'_>) -> Result<(), LibraryError>),
}

/// Copies of the database kept from before upgrades.
const KEPT_BACKUPS: usize = 3;

/// Schema versions, applied in order; `PRAGMA user_version` records the last.
/// Never edit a released migration: add a new one.
const MIGRATIONS: &[Migration] = &[
    // 1: library basics.
    Migration::Sql(
        "
    CREATE TABLE folders (
        id INTEGER PRIMARY KEY,
        path BLOB NOT NULL UNIQUE,
        added_at INTEGER NOT NULL
    );
    CREATE TABLE artists (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE COLLATE NOCASE
    );
    CREATE TABLE albums (
        id INTEGER PRIMARY KEY,
        title TEXT NOT NULL COLLATE NOCASE,
        artist_id INTEGER NOT NULL REFERENCES artists(id),
        year INTEGER,
        art TEXT,
        art_bytes INTEGER NOT NULL DEFAULT 0,
        UNIQUE (title, artist_id)
    );
    CREATE TABLE scan_runs (
        id INTEGER PRIMARY KEY,
        started_at INTEGER NOT NULL,
        finished_at INTEGER,
        added INTEGER NOT NULL DEFAULT 0,
        updated INTEGER NOT NULL DEFAULT 0,
        removed INTEGER NOT NULL DEFAULT 0,
        failed INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE tracks (
        id INTEGER PRIMARY KEY,
        folder_id INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
        path BLOB NOT NULL UNIQUE,
        size INTEGER NOT NULL,
        mtime_ns INTEGER NOT NULL,
        album_id INTEGER NOT NULL REFERENCES albums(id),
        artist_id INTEGER NOT NULL REFERENCES artists(id),
        title TEXT NOT NULL,
        disc INTEGER,
        track INTEGER,
        duration_ms INTEGER,
        codec TEXT NOT NULL,
        sample_rate INTEGER NOT NULL,
        bits_per_sample INTEGER NOT NULL,
        channels INTEGER NOT NULL
    );
    CREATE INDEX tracks_album ON tracks (album_id);
    CREATE INDEX tracks_folder ON tracks (folder_id);
    ",
    ),
    // 2: playlists. Items follow their track: a track that leaves the
    // library leaves its playlists too.
    Migration::Sql(
        "
    CREATE TABLE playlists (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        created_at INTEGER NOT NULL
    );
    CREATE TABLE playlist_items (
        id INTEGER PRIMARY KEY,
        playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
        position INTEGER NOT NULL,
        track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE
    );
    CREATE INDEX playlist_items_order ON playlist_items (playlist_id, position);
    CREATE INDEX playlist_items_track ON playlist_items (track_id);
    ",
    ),
    // 3: genre. Tracks already scanned are read again by the next scan,
    // which fills it in.
    Migration::Sql(
        "
    ALTER TABLE tracks ADD COLUMN genre TEXT;
    UPDATE tracks SET mtime_ns = -1;
    ",
    ),
    // 4: artists are one per matching key, not per ASCII-caseless name.
    Migration::Code(artist_keys),
    // 5: an album is a release, found by `grouping` (see `scan::grouping`),
    // not by its title and artist. Existing albums get a placeholder; the
    // next scan reads every track again and regroups them. A release whose
    // tracks all move to a new grouping keeps its ID and artwork.
    Migration::Sql(
        "
    CREATE TABLE albums_new (
        id INTEGER PRIMARY KEY,
        title TEXT NOT NULL COLLATE NOCASE,
        artist_id INTEGER NOT NULL REFERENCES artists(id),
        year INTEGER,
        art TEXT,
        art_bytes INTEGER NOT NULL DEFAULT 0,
        grouping BLOB NOT NULL UNIQUE
    );
    INSERT INTO albums_new (id, title, artist_id, year, art, art_bytes, grouping)
        SELECT id, title, artist_id, year, art, art_bytes, CAST('old:' || id AS BLOB)
        FROM albums;
    DROP TABLE albums;
    ALTER TABLE albums_new RENAME TO albums;
    ALTER TABLE tracks ADD COLUMN album_artist_id INTEGER REFERENCES artists(id);
    ALTER TABLE tracks ADD COLUMN compilation INTEGER NOT NULL DEFAULT 0;
    UPDATE tracks SET mtime_ns = -1;
    ",
    ),
    // 6: what identifies a track's audio whatever its path (see
    // `scan::audio_id`), so a moved file keeps its track. Every track is
    // read again to fill it in.
    Migration::Sql(
        "
    ALTER TABLE tracks ADD COLUMN audio_id TEXT;
    UPDATE tracks SET mtime_ns = -1;
    ",
    ),
    // 7: what files say, exactly (`file_values`), and the user's edits on
    // top, with a history for undo. The `tracks` and `albums` columns hold
    // what is shown (see `shown`). Every track is read again to fill it in.
    Migration::Sql(
        "
    CREATE TABLE file_values (
        track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
        field TEXT NOT NULL,
        position INTEGER NOT NULL,
        value TEXT NOT NULL,
        cut INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (track_id, field, position)
    ) WITHOUT ROWID;
    CREATE TABLE edits (
        id INTEGER PRIMARY KEY,
        track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE,
        album_id INTEGER REFERENCES albums(id) ON DELETE CASCADE,
        field TEXT NOT NULL,
        value TEXT,
        origin TEXT NOT NULL,
        base TEXT,
        review INTEGER NOT NULL DEFAULT 0,
        edited_at INTEGER NOT NULL,
        CHECK ((track_id IS NULL) <> (album_id IS NULL))
    );
    CREATE UNIQUE INDEX edits_track ON edits (track_id, field) WHERE track_id IS NOT NULL;
    CREATE UNIQUE INDEX edits_album ON edits (album_id, field) WHERE album_id IS NOT NULL;
    CREATE TABLE edit_changes (
        id INTEGER PRIMARY KEY,
        made_at INTEGER NOT NULL
    );
    CREATE TABLE edit_history (
        id INTEGER PRIMARY KEY,
        change_id INTEGER NOT NULL REFERENCES edit_changes(id) ON DELETE CASCADE,
        track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE,
        album_id INTEGER REFERENCES albums(id) ON DELETE CASCADE,
        field TEXT NOT NULL,
        had_before INTEGER NOT NULL,
        before_value TEXT,
        before_origin TEXT,
        before_base TEXT,
        before_review INTEGER,
        had_after INTEGER NOT NULL,
        after_value TEXT
    );
    CREATE INDEX edit_history_change ON edit_history (change_id);
    UPDATE tracks SET mtime_ns = -1;
    ",
    ),
    // 8: FLAC and WAV tags were never read (the scanner skipped the latest
    // metadata revision, which is a file's only one). Read them again.
    Migration::Sql("UPDATE tracks SET mtime_ns = -1 WHERE codec IN ('flac', 'wav');"),
    // 9: revisions. One counter rises on every change; a track or release
    // keeps the value of its last change, so a preview can tell whether
    // what it saw is still so. A track changes with its edits or its file
    // (size, mtime: every read sets them), a release with its edits or its
    // tracks. Triggers, so that no writer can forget.
    Migration::Sql(
        "
    CREATE TABLE revision (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        last INTEGER NOT NULL
    );
    INSERT INTO revision (id, last) VALUES (1, 0);
    ALTER TABLE tracks ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE albums ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;

    CREATE TRIGGER edits_inserted AFTER INSERT ON edits BEGIN
        UPDATE revision SET last = last + 1;
        UPDATE tracks SET revision = (SELECT last FROM revision) WHERE id = NEW.track_id;
        UPDATE albums SET revision = (SELECT last FROM revision) WHERE id = NEW.album_id;
    END;
    CREATE TRIGGER edits_updated AFTER UPDATE ON edits BEGIN
        UPDATE revision SET last = last + 1;
        UPDATE tracks SET revision = (SELECT last FROM revision) WHERE id = NEW.track_id;
        UPDATE albums SET revision = (SELECT last FROM revision) WHERE id = NEW.album_id;
    END;
    CREATE TRIGGER edits_deleted AFTER DELETE ON edits BEGIN
        UPDATE revision SET last = last + 1;
        UPDATE tracks SET revision = (SELECT last FROM revision) WHERE id = OLD.track_id;
        UPDATE albums SET revision = (SELECT last FROM revision) WHERE id = OLD.album_id;
    END;
    CREATE TRIGGER tracks_inserted AFTER INSERT ON tracks BEGIN
        UPDATE revision SET last = last + 1;
        UPDATE tracks SET revision = (SELECT last FROM revision) WHERE id = NEW.id;
        UPDATE albums SET revision = (SELECT last FROM revision) WHERE id = NEW.album_id;
    END;
    CREATE TRIGGER tracks_updated AFTER UPDATE OF album_id, size, mtime_ns ON tracks BEGIN
        UPDATE revision SET last = last + 1;
        UPDATE tracks SET revision = (SELECT last FROM revision) WHERE id = NEW.id;
        UPDATE albums SET revision = (SELECT last FROM revision)
            WHERE id IN (OLD.album_id, NEW.album_id);
    END;
    CREATE TRIGGER tracks_deleted AFTER DELETE ON tracks BEGIN
        UPDATE revision SET last = last + 1;
        UPDATE albums SET revision = (SELECT last FROM revision) WHERE id = OLD.album_id;
    END;
    ",
    ),
    // 10: artwork the user chose, kept whole in the database: the cache
    // file is only its copy, and backups include it.
    Migration::Sql(
        "
    CREATE TABLE chosen_art (
        album_id INTEGER PRIMARY KEY REFERENCES albums(id) ON DELETE CASCADE,
        mime TEXT NOT NULL,
        data BLOB NOT NULL
    );
    ",
    ),
    // 11: where chosen artwork came from: null for a file the user picked,
    // else the online source's name.
    Migration::Sql("ALTER TABLE chosen_art ADD COLUMN source TEXT;"),
    // 12: loudness, for volume levelling. What the gain tags say (hundredths
    // of a LUFS), and what Lyra measured: a histogram of the audio named by
    // `audio_id` (another audio_id means the file changed). Every file is
    // read again for its tags.
    Migration::Sql(
        "
    ALTER TABLE tracks ADD COLUMN tagged_loudness INTEGER;
    ALTER TABLE tracks ADD COLUMN tagged_album_loudness INTEGER;
    CREATE TABLE loudness (
        track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
        audio_id TEXT NOT NULL,
        blocks BLOB,
        failed INTEGER NOT NULL DEFAULT 0
    );
    UPDATE tracks SET mtime_ns = -1;
    ",
    ),
];

/// Brings the schema up to date, after a backup of an existing database
/// at `path`. Foreign keys are off meanwhile, as SQLite requires to
/// rebuild a table, and checked before each step commits.
pub(crate) fn migrate(conn: &mut Connection, path: Option<&Path>) -> Result<(), LibraryError> {
    let version: usize = conn
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?
        .try_into()
        .unwrap_or(0);
    if version >= MIGRATIONS.len() {
        return Ok(());
    }
    if let (Some(path), true) = (path, version > 0) {
        backup(conn, path, version)?;
    }
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    for (index, migration) in MIGRATIONS.iter().enumerate().skip(version) {
        let target = index + 1;
        let failed = |reason: String| LibraryError::Upgrade {
            version: target,
            reason,
        };
        let transaction = conn.transaction()?;
        match migration {
            Migration::Sql(sql) => transaction.execute_batch(sql)?,
            Migration::Code(step) => step(&transaction)?,
        }
        let broken: i64 =
            transaction.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })?;
        if broken > 0 {
            return Err(failed(format!("{broken} broken references")));
        }
        transaction.pragma_update(
            None,
            "user_version",
            i64::try_from(target).unwrap_or(i64::MAX),
        )?;
        transaction.commit()?;
    }
    Ok(())
}

/// Writes a consistent copy of the database at `path`, at schema `version`,
/// beside it (`library-v3-1792695553.db`), and keeps the newest
/// [`KEPT_BACKUPS`].
fn backup(conn: &Connection, path: &Path, version: usize) -> Result<(), LibraryError> {
    let not_saved = |reason: &str| LibraryError::Upgrade {
        version: version + 1,
        reason: format!("no backup made: {reason}"),
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    let stem = path
        .file_stem()
        .and_then(OsStr::to_str)
        .ok_or_else(|| not_saved("the database name is not plain text"))?;
    let prefix = format!("{stem}-v");
    let copy = dir.join(format!("{prefix}{version}-{}.db", now()));
    let target = copy
        .to_str()
        .ok_or_else(|| not_saved("the folder name is not plain text"))?;
    // `VACUUM INTO` never overwrites: a copy from this very second is kept.
    if !copy.exists() {
        conn.execute("VACUUM INTO ?1", [target])?;
    }

    let mut kept: Vec<(SystemTime, PathBuf)> = fs::read_dir(dir)?
        .flatten()
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.starts_with(&prefix) && Path::new(name).extension() == Some(OsStr::new("db"))
            })
        })
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    kept.sort();
    let surplus = kept.len().saturating_sub(KEPT_BACKUPS);
    for (_, old) in kept.into_iter().take(surplus) {
        // A copy that cannot be removed only costs disk space.
        let _ = fs::remove_file(old);
    }
    Ok(())
}

/// Migration 4: artists become one per [`key`]. Spellings that now share a
/// key merge into the one with the most tracks and albums, whose name is
/// shown; their albums merge where the title then repeats.
fn artist_keys(transaction: &Transaction<'_>) -> Result<(), LibraryError> {
    transaction.execute_batch(
        "CREATE TABLE artists_new (
             id INTEGER PRIMARY KEY,
             name TEXT NOT NULL COLLATE NOCASE,
             key TEXT NOT NULL UNIQUE
         );",
    )?;
    let artists: Vec<(i64, String)> = transaction
        .prepare(
            "SELECT id, name FROM artists
             ORDER BY (SELECT COUNT(*) FROM tracks WHERE artist_id = artists.id)
                    + (SELECT COUNT(*) FROM albums WHERE artist_id = artists.id) DESC, id",
        )?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut kept: HashMap<String, i64> = HashMap::new();
    for (id, name) in artists {
        let key = key(&name);
        if let Some(&into) = kept.get(&key) {
            merge_artist(transaction, id, into)?;
        } else {
            transaction.execute(
                "INSERT INTO artists_new (id, name, key) VALUES (?1, ?2, ?3)",
                params![id, name, key],
            )?;
            kept.insert(key, id);
        }
    }
    transaction.execute_batch(
        "DROP TABLE artists;
         ALTER TABLE artists_new RENAME TO artists;",
    )?;
    Ok(())
}

/// Moves artist `from`'s tracks and albums to artist `into`. An album whose
/// title `into` already has gives its tracks to that album and goes. Its
/// artwork file stays in the cache until the next scan regroups albums.
fn merge_artist(transaction: &Transaction<'_>, from: i64, into: i64) -> Result<(), LibraryError> {
    transaction.execute(
        "UPDATE tracks SET artist_id = ?2 WHERE artist_id = ?1",
        [from, into],
    )?;
    let albums: Vec<(i64, String)> = transaction
        .prepare("SELECT id, title FROM albums WHERE artist_id = ?1")?
        .query_map([from], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (album, title) in albums {
        let same: Option<i64> = transaction
            .query_row(
                "SELECT id FROM albums WHERE artist_id = ?1 AND title = ?2",
                params![into, title],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(same) = same {
            transaction.execute(
                "UPDATE tracks SET album_id = ?2 WHERE album_id = ?1",
                [album, same],
            )?;
            transaction.execute("DELETE FROM albums WHERE id = ?1", [album])?;
        } else {
            transaction.execute(
                "UPDATE albums SET artist_id = ?2 WHERE id = ?1",
                [album, into],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use rusqlite::Connection;

    use super::{KEPT_BACKUPS, MIGRATIONS, Migration, backup};
    use crate::db::Library;

    /// A fresh, empty directory for one test.
    fn temp_dir(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lyra-db-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A database at schema version 3, as Lyra 0.1 left it.
    fn version_3(path: &Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        for migration in &MIGRATIONS[..3] {
            let Migration::Sql(sql) = migration else {
                panic!("versions 1 to 3 are SQL");
            };
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 3).unwrap();
        conn
    }

    fn backups(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with("library-v"))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn version_3_upgrades_to_one_artist_per_spelling_after_a_backup() {
        let dir = temp_dir("upgrade");
        let path = dir.join("library.db");
        let old = version_3(&path);
        old.execute_batch(
            "INSERT INTO folders VALUES (1, x'2f6d75736963', 0);
             INSERT INTO artists VALUES (1, 'Beyonc\u{e9}'), (2, 'Beyonce\u{301}'),
                 (3, 'BEYONC\u{c9}'), (4, 'AC/DC');
             INSERT INTO albums (id, title, artist_id) VALUES (1, 'Lemonade', 1),
                 (2, 'Lemonade', 2), (3, '4', 3), (4, 'Back in Black', 4);
             INSERT INTO tracks (id, folder_id, path, size, mtime_ns, album_id, artist_id,
                     title, codec, sample_rate, bits_per_sample, channels)
                 VALUES (1, 1, x'01', 1, 1, 1, 1, 'Formation', 'flac', 44100, 16, 2),
                        (2, 1, x'02', 1, 1, 1, 1, 'Sorry', 'flac', 44100, 16, 2),
                        (3, 1, x'03', 1, 1, 2, 2, 'Hold Up', 'flac', 44100, 16, 2),
                        (4, 1, x'04', 1, 1, 3, 3, 'Love on Top', 'flac', 44100, 16, 2),
                        (5, 1, x'05', 1, 1, 4, 4, 'Hells Bells', 'flac', 44100, 16, 2);
             INSERT INTO playlists VALUES (1, 'Mix', 0);
             INSERT INTO playlist_items (playlist_id, position, track_id)
                 VALUES (1, 0, 3), (1, 1, 4), (1, 2, 5);",
        )
        .unwrap();
        drop(old);

        let library = Library::open(&path).unwrap();
        let artists: Vec<(String, u32)> = library
            .artists()
            .unwrap()
            .into_iter()
            .map(|artist| (artist.name, artist.tracks))
            .collect();
        // The spelling with the most tracks names the merged artist.
        assert_eq!(
            artists,
            [("AC/DC".to_owned(), 1), ("Beyonc\u{e9}".to_owned(), 4)]
        );
        let titles: Vec<(String, u32)> = library
            .albums()
            .unwrap()
            .into_iter()
            .map(|album| (album.title, album.tracks))
            .collect();
        assert_eq!(
            titles,
            [
                ("Back in Black".to_owned(), 1),
                ("4".to_owned(), 1),
                ("Lemonade".to_owned(), 3)
            ]
        );
        let playlist: Vec<i64> = library
            .playlist_entries(1)
            .unwrap()
            .into_iter()
            .map(|entry| entry.track.id)
            .collect();
        assert_eq!(playlist, [3, 4, 5]);
        // Migration 5: albums wait for the next scan to regroup them.
        let waiting: (i64, i64) = library
            .conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM albums WHERE grouping = CAST('old:' || id AS BLOB)),
                        (SELECT COUNT(*) FROM tracks WHERE mtime_ns = -1)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(waiting, (3, 5));
        let broken: i64 = library
            .conn
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(broken, 0);

        let made = backups(&dir);
        assert_eq!(made.len(), 1, "{made:?}");
        assert!(made[0].starts_with("library-v3-"));
        // The copy is the old database, still at version 3.
        let copy = Connection::open(dir.join(&made[0])).unwrap();
        let version: i64 = copy
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);

        // Up to date: reopening makes no further copy.
        drop(library);
        Library::open(&path).unwrap();
        assert_eq!(backups(&dir), made);
    }

    #[test]
    fn a_new_library_needs_no_backup() {
        let dir = temp_dir("new");
        Library::open(&dir.join("library.db")).unwrap();
        assert_eq!(backups(&dir), Vec::<String>::new());
    }

    #[test]
    fn only_the_newest_backups_are_kept() {
        let dir = temp_dir("kept");
        let path = dir.join("library.db");
        let conn = Connection::open(&path).unwrap();
        for old in 1..=4 {
            fs::write(dir.join(format!("library-v1-{old}.db")), b"old").unwrap();
        }
        backup(&conn, &path, 3).unwrap();
        let kept = backups(&dir);
        assert_eq!(kept.len(), KEPT_BACKUPS, "{kept:?}");
        assert!(kept.iter().any(|name| name.starts_with("library-v3-")));
        assert!(!kept.contains(&"library-v1-1.db".to_owned()));
    }
}
