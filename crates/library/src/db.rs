//! SQLite schema, migrations and queries.
//!
//! Paths are stored as raw bytes (BLOB): Linux file names need not be UTF-8.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

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
];

/// Shown when a file has no artist tag.
pub const UNKNOWN_ARTIST: &str = "Unknown Artist";

#[derive(Debug)]
pub enum LibraryError {
    Db(rusqlite::Error),
    Io(std::io::Error),
    /// The folder is already scanned as part of another one.
    AlreadyCovered {
        folder: PathBuf,
        by: PathBuf,
    },
    NotADirectory(PathBuf),
    /// A request the user can fix (empty name, unknown playlist, …).
    Invalid(String),
    /// Upgrading the database to `version` failed; nothing was changed.
    Upgrade {
        version: usize,
        reason: String,
    },
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Db(error) => write!(f, "library database: {error}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::AlreadyCovered { folder, by } => write!(
                f,
                "{} is already in the library, inside {}",
                folder.display(),
                by.display()
            ),
            Self::NotADirectory(path) => write!(f, "{} is not a folder", path.display()),
            Self::Invalid(reason) => f.write_str(reason),
            Self::Upgrade { version, reason } => {
                write!(f, "library database upgrade to version {version}: {reason}")
            }
        }
    }
}

impl std::error::Error for LibraryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Db(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for LibraryError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Db(error)
    }
}

impl From<std::io::Error> for LibraryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub id: i64,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Album {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    /// File name in the artwork directory.
    pub art: Option<String>,
    pub tracks: u32,
    /// Highest source format among the tracks, for a badge: the sample rate
    /// (DSD bit rate for DSD) and its codec.
    pub best_rate: u32,
    pub best_codec: String,
    /// The genre most of its tracks carry.
    pub genre: Option<String>,
    /// Sum of the track lengths that are known.
    pub duration_ms: Option<u64>,
}

/// An album artist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artist {
    pub id: i64,
    pub name: String,
    pub albums: u32,
    pub tracks: u32,
    /// Artwork file name of the artist's latest album with a cover: the
    /// library has no artist photos.
    pub art: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub id: i64,
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album_id: i64,
    pub album: String,
    pub disc: Option<u32>,
    pub track: Option<u32>,
    pub duration_ms: Option<u64>,
    pub codec: String,
    pub sample_rate: u32,
    pub bits_per_sample: u32,
    pub channels: u32,
}

/// One connection to the library database. Each thread opens its own; WAL
/// lets the scanner write while the UI reads.
#[derive(Debug)]
pub struct Library {
    pub(crate) conn: Connection,
}

pub(crate) fn path_bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_bytes()
}

pub(crate) fn path_from(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsStr::from_bytes(bytes))
}

pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}

impl Library {
    /// Opens (creating if needed) and migrates the database at `path`.
    pub fn open(path: &Path) -> Result<Self, LibraryError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Self::with_connection(Connection::open(path)?, Some(path))
    }

    /// A private in-memory library, for tests.
    pub fn in_memory() -> Result<Self, LibraryError> {
        Self::with_connection(Connection::open_in_memory()?, None)
    }

    fn with_connection(conn: Connection, path: Option<&Path>) -> Result<Self, LibraryError> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let mut library = Self { conn };
        library.migrate(path)?;
        library.conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(library)
    }

    /// Brings the schema up to date, after a backup of an existing database
    /// at `path`. Foreign keys are off meanwhile, as SQLite requires to
    /// rebuild a table, and checked before each step commits.
    fn migrate(&mut self, path: Option<&Path>) -> Result<(), LibraryError> {
        let version: usize = self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?
            .try_into()
            .unwrap_or(0);
        if version >= MIGRATIONS.len() {
            return Ok(());
        }
        if let (Some(path), true) = (path, version > 0) {
            backup(&self.conn, path, version)?;
        }
        self.conn.pragma_update(None, "foreign_keys", "OFF")?;
        for (index, migration) in MIGRATIONS.iter().enumerate().skip(version) {
            let target = index + 1;
            let failed = |reason: String| LibraryError::Upgrade {
                version: target,
                reason,
            };
            let transaction = self.conn.transaction()?;
            match migration {
                Migration::Sql(sql) => transaction.execute_batch(sql)?,
                Migration::Code(step) => step(&transaction)?,
            }
            let broken: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM pragma_foreign_key_check",
                [],
                |row| row.get(0),
            )?;
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

    pub fn folders(&self) -> Result<Vec<Folder>, LibraryError> {
        let mut statement = self
            .conn
            .prepare("SELECT id, path FROM folders ORDER BY path")?;
        let rows = statement.query_map([], |row| {
            Ok(Folder {
                id: row.get(0)?,
                path: path_from(&row.get::<_, Vec<u8>>(1)?),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Adds a folder to scan. Folders inside it are merged into it, their
    /// tracks (and so playlists) kept; a folder already inside a library
    /// folder is refused.
    pub fn add_folder(&mut self, path: &Path) -> Result<Folder, LibraryError> {
        if !path.is_dir() {
            return Err(LibraryError::NotADirectory(path.to_owned()));
        }
        let path = path.canonicalize()?;
        let existing = self.folders()?;
        if let Some(outer) = existing
            .iter()
            .find(|folder| path.starts_with(&folder.path))
        {
            return Err(LibraryError::AlreadyCovered {
                folder: path,
                by: outer.path.clone(),
            });
        }
        let transaction = self.conn.transaction()?;
        transaction.execute(
            "INSERT INTO folders (path, added_at) VALUES (?1, ?2)",
            params![path_bytes(&path), now()],
        )?;
        let id = transaction.last_insert_rowid();
        for inner in existing
            .iter()
            .filter(|folder| folder.path.starts_with(&path))
        {
            transaction.execute(
                "UPDATE tracks SET folder_id = ?1 WHERE folder_id = ?2",
                [id, inner.id],
            )?;
            transaction.execute("DELETE FROM folders WHERE id = ?1", [inner.id])?;
        }
        transaction.commit()?;
        Ok(Folder { id, path })
    }

    /// Removes a folder and its tracks from the library (files are untouched).
    /// Returns the artwork files no longer used, for the caller to delete.
    pub fn remove_folder(&mut self, id: i64) -> Result<Vec<String>, LibraryError> {
        let transaction = self.conn.transaction()?;
        transaction.execute("DELETE FROM folders WHERE id = ?1", [id])?;
        let unused = prune(&transaction)?;
        transaction.commit()?;
        Ok(unused)
    }

    /// Albums sorted by artist, then year and title.
    pub fn albums(&self) -> Result<Vec<Album>, LibraryError> {
        let mut statement = self.conn.prepare(&format!(
            "{ALBUM_COLUMNS} GROUP BY albums.id ORDER BY artists.name, albums.year, albums.title"
        ))?;
        let rows = statement.query_map([], album_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn album(&self, id: i64) -> Result<Option<Album>, LibraryError> {
        Ok(self
            .conn
            .query_row(
                &format!("{ALBUM_COLUMNS} WHERE albums.id = ?1 GROUP BY albums.id"),
                [id],
                album_row,
            )
            .optional()?)
    }

    /// Tracks of an album in disc and track order.
    pub fn album_tracks(&self, album_id: i64) -> Result<Vec<Track>, LibraryError> {
        let mut statement = self.conn.prepare(&format!(
            "{TRACK_COLUMNS} WHERE tracks.album_id = ?1
             ORDER BY tracks.disc, tracks.track, tracks.title"
        ))?;
        let rows = statement.query_map([album_id], track_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Cached statement: queues look tracks up one by one, hundreds at a time.
    pub fn track(&self, id: i64) -> Result<Option<Track>, LibraryError> {
        Ok(self
            .conn
            .prepare_cached(&format!("{TRACK_COLUMNS} WHERE tracks.id = ?1"))?
            .query_row([id], track_row)
            .optional()?)
    }

    /// Album artists with their album and track counts and a cover, by name.
    pub fn artists(&self) -> Result<Vec<Artist>, LibraryError> {
        let mut statement = self.conn.prepare(
            "SELECT artists.id, artists.name, COUNT(DISTINCT albums.id), COUNT(tracks.id),
                    (SELECT latest.art FROM albums AS latest
                     WHERE latest.artist_id = artists.id AND latest.art IS NOT NULL
                     ORDER BY latest.year DESC, latest.title LIMIT 1)
             FROM artists
             JOIN albums ON albums.artist_id = artists.id
             JOIN tracks ON tracks.album_id = albums.id
             GROUP BY artists.id
             ORDER BY artists.name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Artist {
                id: row.get(0)?,
                name: row.get(1)?,
                albums: row.get(2)?,
                tracks: row.get(3)?,
                art: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Albums whose album artist is `artist_id`, by year.
    pub fn artist_albums(&self, artist_id: i64) -> Result<Vec<Album>, LibraryError> {
        let mut statement = self.conn.prepare(&format!(
            "{ALBUM_COLUMNS} WHERE albums.artist_id = ?1 GROUP BY albums.id
             ORDER BY albums.year, albums.title"
        ))?;
        let rows = statement.query_map([artist_id], album_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Every track, by artist, album and position.
    pub fn tracks(&self) -> Result<Vec<Track>, LibraryError> {
        let mut statement = self.conn.prepare(&format!(
            "{TRACK_COLUMNS} ORDER BY artists.name, albums.title, tracks.disc, tracks.track,
             tracks.title"
        ))?;
        let rows = statement.query_map([], track_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Artwork file name of an album, if it has one.
    pub fn album_art(&self, album_id: i64) -> Result<Option<String>, LibraryError> {
        Ok(self
            .conn
            .query_row("SELECT art FROM albums WHERE id = ?1", [album_id], |row| {
                row.get(0)
            })
            .optional()?
            .flatten())
    }
}

const ALBUM_COLUMNS: &str = "SELECT albums.id, albums.title, artists.name, albums.year,
        albums.art, COUNT(tracks.id), MAX(tracks.sample_rate),
        (SELECT codec FROM tracks AS best WHERE best.album_id = albums.id
         ORDER BY best.sample_rate DESC LIMIT 1),
        (SELECT genre FROM tracks AS tagged
         WHERE tagged.album_id = albums.id AND tagged.genre IS NOT NULL
             -- Placeholders some rippers write; they say nothing.
             AND lower(tagged.genre) NOT IN ('other', 'music', 'unknown', 'genre')
         GROUP BY tagged.genre ORDER BY COUNT(*) DESC LIMIT 1),
        SUM(tracks.duration_ms)
    FROM albums
    JOIN artists ON artists.id = albums.artist_id
    JOIN tracks ON tracks.album_id = albums.id";

fn album_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Album> {
    Ok(Album {
        id: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        year: row.get(3)?,
        art: row.get(4)?,
        tracks: row.get(5)?,
        best_rate: row.get(6)?,
        best_codec: row.get(7)?,
        genre: row.get(8)?,
        duration_ms: row
            .get::<_, Option<i64>>(9)?
            .and_then(|ms| u64::try_from(ms).ok()),
    })
}

/// The columns [`track_row`] reads, in order.
macro_rules! track_columns {
    () => {
        "tracks.id, tracks.path, tracks.title, artists.name, tracks.album_id, albums.title,
         tracks.disc, tracks.track, tracks.duration_ms, tracks.codec, tracks.sample_rate,
         tracks.bits_per_sample, tracks.channels"
    };
}

/// The tables those columns come from.
macro_rules! track_tables {
    () => {
        "tracks
         JOIN artists ON artists.id = tracks.artist_id
         JOIN albums ON albums.id = tracks.album_id"
    };
}

pub(crate) use {track_columns, track_tables};

/// Columns read by [`track_row`]; extra columns of a query start here.
pub(crate) const TRACK_FIELDS: usize = 13;

const TRACK_COLUMNS: &str = concat!("SELECT ", track_columns!(), " FROM ", track_tables!());

pub(crate) fn track_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        id: row.get(0)?,
        path: path_from(&row.get::<_, Vec<u8>>(1)?),
        title: row.get(2)?,
        artist: row.get(3)?,
        album_id: row.get(4)?,
        album: row.get(5)?,
        disc: row.get(6)?,
        track: row.get(7)?,
        duration_ms: row
            .get::<_, Option<i64>>(8)?
            .and_then(|ms| u64::try_from(ms).ok()),
        codec: row.get(9)?,
        sample_rate: row.get(10)?,
        bits_per_sample: row.get(11)?,
        channels: row.get(12)?,
    })
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

/// Deletes albums and artists without tracks. Returns the artwork files of
/// deleted albums.
pub(crate) fn prune(conn: &Connection) -> Result<Vec<String>, LibraryError> {
    let unused = {
        let mut statement = conn.prepare(
            "SELECT art FROM albums WHERE art IS NOT NULL
             AND id NOT IN (SELECT album_id FROM tracks)",
        )?;
        statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?
    };
    conn.execute(
        "DELETE FROM albums WHERE id NOT IN (SELECT album_id FROM tracks)",
        [],
    )?;
    conn.execute(
        "DELETE FROM artists WHERE id NOT IN (SELECT artist_id FROM tracks)
         AND id NOT IN (SELECT artist_id FROM albums)",
        [],
    )?;
    Ok(unused)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use rusqlite::Connection;

    use super::{KEPT_BACKUPS, Library, MIGRATIONS, Migration, backup};

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
