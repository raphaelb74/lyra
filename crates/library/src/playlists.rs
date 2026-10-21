//! Playlists: ordered lists of library tracks. Entries have their own id
//! (a track may appear twice); positions only order them and may have gaps
//! where tracks left the library.

use rusqlite::{OptionalExtension, Transaction, params};

use crate::db::{
    Library, LibraryError, TRACK_FIELDS, Track, now, track_columns, track_row, track_tables,
};

/// Longest playlist name kept, in characters.
const MAX_NAME_CHARS: usize = 200;

/// One playlist entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: i64,
    pub track: Track,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub tracks: u32,
    pub duration_ms: u64,
}

impl Library {
    /// Playlists by name.
    pub fn playlists(&self) -> Result<Vec<Playlist>, LibraryError> {
        let mut statement = self.conn.prepare(
            "SELECT playlists.id, playlists.name, COUNT(tracks.id),
                    COALESCE(SUM(tracks.duration_ms), 0)
             FROM playlists
             LEFT JOIN playlist_items AS items ON items.playlist_id = playlists.id
             LEFT JOIN tracks ON tracks.id = items.track_id
             GROUP BY playlists.id
             ORDER BY playlists.name COLLATE NOCASE, playlists.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                tracks: row.get(2)?,
                duration_ms: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn create_playlist(&self, name: &str) -> Result<i64, LibraryError> {
        let name = clean_name(name)?;
        self.conn.execute(
            "INSERT INTO playlists (name, created_at) VALUES (?1, ?2)",
            params![name, now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn rename_playlist(&self, id: i64, name: &str) -> Result<(), LibraryError> {
        let name = clean_name(name)?;
        let changed = self.conn.execute(
            "UPDATE playlists SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        found(changed)
    }

    /// Deletes a playlist; its tracks stay in the library.
    pub fn delete_playlist(&self, id: i64) -> Result<(), LibraryError> {
        found(
            self.conn
                .execute("DELETE FROM playlists WHERE id = ?1", [id])?,
        )
    }

    /// The playlist's entries in order.
    pub fn playlist_entries(&self, id: i64) -> Result<Vec<Entry>, LibraryError> {
        let mut statement = self.conn.prepare(concat!(
            "SELECT ",
            track_columns!(),
            ", items.id FROM ",
            track_tables!(),
            " JOIN playlist_items AS items ON items.track_id = tracks.id
             WHERE items.playlist_id = ?1 ORDER BY items.position, items.id"
        ))?;
        let rows = statement.query_map([id], |row| {
            Ok(Entry {
                track: track_row(row)?,
                id: row.get(TRACK_FIELDS)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Appends tracks in the given order.
    pub fn add_to_playlist(&mut self, id: i64, track_ids: &[i64]) -> Result<(), LibraryError> {
        let transaction = self.conn.transaction()?;
        exists(&transaction, id)?;
        let first: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_items WHERE playlist_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        for (position, &track) in (first..).zip(track_ids) {
            let known: Option<i64> = transaction
                .query_row("SELECT id FROM tracks WHERE id = ?1", [track], |row| {
                    row.get(0)
                })
                .optional()?;
            if known.is_none() {
                return Err(LibraryError::Invalid(
                    "A track is no longer in the library. Rescan.".into(),
                ));
            }
            transaction.execute(
                "INSERT INTO playlist_items (playlist_id, position, track_id)
                 VALUES (?1, ?2, ?3)",
                params![id, position, track],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Removes one entry.
    pub fn remove_from_playlist(&self, entry: i64) -> Result<(), LibraryError> {
        found(
            self.conn
                .execute("DELETE FROM playlist_items WHERE id = ?1", [entry])?,
        )
    }

    /// Moves `entry` to index `to` of its playlist (clamped to the end).
    pub fn move_in_playlist(&mut self, entry: i64, to: usize) -> Result<(), LibraryError> {
        let transaction = self.conn.transaction()?;
        let playlist: i64 = transaction
            .query_row(
                "SELECT playlist_id FROM playlist_items WHERE id = ?1",
                [entry],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| LibraryError::Invalid("That playlist entry no longer exists.".into()))?;
        let mut order: Vec<i64> = {
            let mut statement = transaction.prepare(
                "SELECT id FROM playlist_items WHERE playlist_id = ?1 ORDER BY position, id",
            )?;
            statement
                .query_map([playlist], |row| row.get(0))?
                .collect::<Result<_, _>>()?
        };
        order.retain(|&id| id != entry);
        order.insert(to.min(order.len()), entry);
        {
            let mut renumber =
                transaction.prepare("UPDATE playlist_items SET position = ?2 WHERE id = ?1")?;
            for (position, id) in order.iter().enumerate() {
                renumber.execute(params![id, i64::try_from(position).unwrap_or(i64::MAX)])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }
}

fn clean_name(name: &str) -> Result<String, LibraryError> {
    let name: String = name.trim().chars().take(MAX_NAME_CHARS).collect();
    if name.is_empty() {
        return Err(LibraryError::Invalid("A playlist needs a name.".into()));
    }
    Ok(name)
}

fn exists(transaction: &Transaction<'_>, id: i64) -> Result<(), LibraryError> {
    let known: Option<i64> = transaction
        .query_row("SELECT id FROM playlists WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()?;
    known
        .map(|_| ())
        .ok_or_else(|| LibraryError::Invalid("That playlist no longer exists.".into()))
}

fn found(changed: usize) -> Result<(), LibraryError> {
    if changed == 0 {
        Err(LibraryError::Invalid(
            "That playlist or entry no longer exists.".into(),
        ))
    } else {
        Ok(())
    }
}
