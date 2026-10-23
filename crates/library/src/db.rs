//! The library database: opening it, its errors, and the browsing queries.
//!
//! Paths are stored as raw bytes (BLOB): Linux file names need not be UTF-8.

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use crate::edits::Target;
use crate::migrations;
use crate::values::{Field, Problem};

/// Shown when a file has no artist tag.
pub const UNKNOWN_ARTIST: &str = "Unknown Artist";
/// Album artist of a compilation, or of an album whose tracks have no
/// album-artist tag and different artists.
pub const VARIOUS_ARTISTS: &str = "Various Artists";

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
    /// An edit that cannot be saved; nothing was saved.
    InvalidEdit {
        target: Target,
        field: Field,
        problem: Problem,
    },
    /// What a preview showed has changed since; nothing was saved.
    Stale,
    /// The edits a step saved have changed since; nothing was undone.
    UndoConflict,
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
            Self::InvalidEdit { field, problem, .. } => {
                write!(f, "{} {}", field.name(), problem.message())
            }
            Self::Stale => f.write_str("The library changed in the meantime; nothing was saved."),
            Self::UndoConflict => {
                f.write_str("Those edits have changed since; nothing was undone.")
            }
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
        migrations::migrate(&mut library.conn, path)?;
        library.conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(library)
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
    prune_artists(conn)?;
    Ok(unused)
}

/// Deletes artists nothing refers to.
pub(crate) fn prune_artists(conn: &Connection) -> Result<(), LibraryError> {
    conn.execute(
        "DELETE FROM artists WHERE id NOT IN (SELECT artist_id FROM tracks)
         AND id NOT IN (SELECT artist_id FROM albums)
         AND id NOT IN (SELECT album_artist_id FROM tracks WHERE album_artist_id IS NOT NULL)",
        [],
    )?;
    Ok(())
}
