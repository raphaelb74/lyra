//! Loudness for volume levelling: what gain tags say, and what Lyra
//! measured where an album's tags are incomplete.
//!
//! A track's loudness is its measurement, else its tag. An album's is the
//! measurement of all its tracks gated together, else the track's album tag.
//! Measurements are kept for the audio they were made from (`audio_id`), so
//! a changed file is measured again and a moved one keeps its measurement.

use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::{OptionalExtension, params};

use crate::db::{Library, LibraryError, path_from};
use crate::meter::Histogram;

/// Loudness in LUFS.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Loudness {
    pub track: Option<f64>,
    pub album: Option<f64>,
}

/// A track waiting to be measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unmeasured {
    pub track_id: i64,
    pub path: PathBuf,
    pub audio_id: String,
}

/// Tracks with a measurement of their current audio.
const MEASURED: &str = "loudness.track_id = tracks.id AND loudness.audio_id = tracks.audio_id";

/// Tracks of albums whose tags leave a track or album loudness out.
const UNTAGGED_ALBUM: &str = "EXISTS (SELECT 1 FROM tracks AS mate
    WHERE mate.album_id = tracks.album_id
    AND (mate.tagged_loudness IS NULL OR mate.tagged_album_loudness IS NULL))";

impl Library {
    /// Loudness of the tracks `ids`; unknown ids are left out.
    pub fn loudness(&self, ids: &[i64]) -> Result<HashMap<i64, Loudness>, LibraryError> {
        let mut track_query = self.conn.prepare_cached(&format!(
            "SELECT album_id, tagged_loudness, tagged_album_loudness,
                 (SELECT blocks FROM loudness WHERE {MEASURED})
             FROM tracks WHERE id = ?1"
        ))?;
        let mut albums: HashMap<i64, Option<f64>> = HashMap::new();
        let mut found = HashMap::with_capacity(ids.len());
        for &id in ids {
            let row = track_query
                .query_row([id], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<i32>>(1)?,
                        row.get::<_, Option<i32>>(2)?,
                        row.get::<_, Option<Vec<u8>>>(3)?,
                    ))
                })
                .optional()?;
            let Some((album_id, tagged, tagged_album, blocks)) = row else {
                continue;
            };
            let measured = blocks
                .as_deref()
                .and_then(Histogram::from_bytes)
                .and_then(|histogram| histogram.loudness());
            let album = if let Some(album) = albums.get(&album_id) {
                *album
            } else {
                let album = self.measured_album(album_id)?;
                albums.insert(album_id, album);
                album
            };
            found.insert(
                id,
                Loudness {
                    track: measured.or(tagged.map(from_hundredths)),
                    album: album.or(tagged_album.map(from_hundredths)),
                },
            );
        }
        Ok(found)
    }

    /// Whether tracks `a` and `b` are on the same album.
    pub fn same_album(&self, a: i64, b: i64) -> Result<bool, LibraryError> {
        let same: Option<bool> = self
            .conn
            .prepare_cached(
                "SELECT (SELECT album_id FROM tracks WHERE id = ?1)
                     = (SELECT album_id FROM tracks WHERE id = ?2)",
            )?
            .query_row([a, b], |row| row.get(0))?;
        Ok(same == Some(true))
    }

    /// The album measured as one programme, once every track is measured.
    fn measured_album(&self, album_id: i64) -> Result<Option<f64>, LibraryError> {
        let mut query = self.conn.prepare_cached(&format!(
            "SELECT (SELECT blocks FROM loudness WHERE {MEASURED})
             FROM tracks WHERE album_id = ?1"
        ))?;
        let mut album = Histogram::default();
        let mut rows = query.query([album_id])?;
        while let Some(row) = rows.next()? {
            let blocks: Option<Vec<u8>> = row.get(0)?;
            let Some(histogram) = blocks.as_deref().and_then(Histogram::from_bytes) else {
                return Ok(None);
            };
            album.merge(&histogram);
        }
        Ok(album.loudness())
    }

    /// The next track to measure: one of `first` (in order), each before
    /// the rest of its album. Only albums whose tags leave something out,
    /// and only audio not measured yet (or that failed to measure). Nothing
    /// outside `first`'s albums: the library is not measured ahead.
    pub fn next_unmeasured(&self, first: &[i64]) -> Result<Option<Unmeasured>, LibraryError> {
        let mut query = self.conn.prepare_cached(&format!(
            "SELECT id, path, audio_id FROM tracks
             WHERE album_id = (SELECT album_id FROM tracks WHERE id = ?1)
                 AND audio_id IS NOT NULL AND {UNTAGGED_ALBUM}
                 AND NOT EXISTS (SELECT 1 FROM loudness WHERE {MEASURED})
             ORDER BY id = ?1 DESC, disc, track, id LIMIT 1"
        ))?;
        for &id in first {
            let found = query
                .query_row([id], |row| {
                    Ok(Unmeasured {
                        track_id: row.get(0)?,
                        path: path_from(&row.get::<_, Vec<u8>>(1)?),
                        audio_id: row.get(2)?,
                    })
                })
                .optional()?;
            if found.is_some() {
                return Ok(found);
            }
        }
        Ok(None)
    }

    /// Keeps a measurement of `audio_id`; `None` records that the file could
    /// not be measured, so it is not tried again until it changes.
    pub(crate) fn store_measurement(
        &self,
        track_id: i64,
        audio_id: &str,
        blocks: Option<&Histogram>,
    ) -> Result<(), LibraryError> {
        // The track may have left the library while it was measured.
        self.conn.execute(
            "INSERT INTO loudness (track_id, audio_id, blocks, failed)
             SELECT id, ?2, ?3, ?4 FROM tracks WHERE id = ?1
             ON CONFLICT (track_id) DO UPDATE SET
                 audio_id = excluded.audio_id, blocks = excluded.blocks, failed = excluded.failed",
            params![
                track_id,
                audio_id,
                blocks.map(Histogram::to_bytes),
                blocks.is_none()
            ],
        )?;
        Ok(())
    }
}

fn from_hundredths(hundredths: i32) -> f64 {
    f64::from(hundredths) / 100.0
}
