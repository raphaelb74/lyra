//! The user's edits: values set or cleared in the library, on top of what
//! the files say, with a history for undo. Music files are never touched.
//!
//! An edit keeps the file value it was made over (`base`). When a rescan
//! finds the file saying something else, the edit stays and is marked for
//! review.

use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::db::{Library, LibraryError, now, prune_artists};
use crate::shown::{self, Edited};
use crate::values::{self, Field, FileValue, Scope};

/// What an edit applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Track(i64),
    /// An album, as a release.
    Release(i64),
}

/// A change to one field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Show this value. Several values are separated by NUL.
    Set(String),
    /// Show nothing, whatever the file says.
    Clear,
    /// Drop the edit: show the file's value again.
    Reset,
}

/// Where an edit came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Manual,
    /// A cleanup rule, by name.
    Rule(String),
    /// An online source, by name.
    Provider(String),
}

impl Origin {
    fn stored(&self) -> String {
        match self {
            Origin::Manual => "manual".to_owned(),
            Origin::Rule(name) => format!("rule:{name}"),
            Origin::Provider(name) => format!("provider:{name}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditChange {
    pub target: Target,
    pub field: Field,
    pub edit: Edit,
}

/// An edit in force.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedEdit {
    pub field: Field,
    /// `None`: cleared.
    pub value: Option<String>,
    /// `manual`, `rule:<name>` or `provider:<name>`.
    pub origin: String,
    /// The file now says something else than when the edit was made.
    pub review: bool,
}

/// An edit row as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    value: Option<String>,
    origin: String,
    base: Option<String>,
    review: bool,
}

/// `(track_id, album_id)` columns of a target.
fn columns(target: Target) -> (Option<i64>, Option<i64>) {
    match target {
        Target::Track(id) => (Some(id), None),
        Target::Release(id) => (None, Some(id)),
    }
}

const TARGET: &str = "track_id IS ?1 AND album_id IS ?2";

fn edits_of(conn: &Connection, target: Target) -> Result<Vec<(Field, Row)>, LibraryError> {
    let (track, album) = columns(target);
    let mut statement = conn.prepare_cached(&format!(
        "SELECT field, value, origin, base, review FROM edits WHERE {TARGET} ORDER BY field"
    ))?;
    let rows = statement.query_map(params![track, album], |row| {
        Ok((
            row.get::<_, String>(0)?,
            Row {
                value: row.get(1)?,
                origin: row.get(2)?,
                base: row.get(3)?,
                review: row.get(4)?,
            },
        ))
    })?;
    let mut edits = Vec::new();
    for row in rows {
        let (field, row) = row?;
        if let Some(field) = Field::from_name(&field) {
            edits.push((field, row));
        }
    }
    Ok(edits)
}

fn edit_of(conn: &Connection, target: Target, field: Field) -> Result<Option<Row>, LibraryError> {
    let (track, album) = columns(target);
    Ok(conn
        .prepare_cached(&format!(
            "SELECT value, origin, base, review FROM edits WHERE {TARGET} AND field = ?3"
        ))?
        .query_row(params![track, album, field.name()], |row| {
            Ok(Row {
                value: row.get(0)?,
                origin: row.get(1)?,
                base: row.get(2)?,
                review: row.get(3)?,
            })
        })
        .optional()?)
}

fn put(
    conn: &Connection,
    target: Target,
    field: Field,
    row: Option<&Row>,
) -> Result<(), LibraryError> {
    let (track, album) = columns(target);
    conn.prepare_cached(&format!("DELETE FROM edits WHERE {TARGET} AND field = ?3"))?
        .execute(params![track, album, field.name()])?;
    if let Some(row) = row {
        conn.prepare_cached(
            "INSERT INTO edits (track_id, album_id, field, value, origin, base, review, edited_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?
        .execute(params![
            track,
            album,
            field.name(),
            row.value,
            row.origin,
            row.base,
            row.review,
            now()
        ])?;
    }
    Ok(())
}

/// A target's edits, marking for review those whose file value is no longer
/// what it was when they were made.
pub(crate) fn review(
    conn: &Connection,
    target: Target,
    file_value: impl Fn(Field) -> Option<String>,
) -> Result<Edited, LibraryError> {
    let (track, album) = columns(target);
    let mut edited = Edited::new();
    for (field, row) in edits_of(conn, target)? {
        if !row.review && file_value(field) != row.base {
            conn.prepare_cached(&format!(
                "UPDATE edits SET review = 1 WHERE {TARGET} AND field = ?3"
            ))?
            .execute(params![track, album, field.name()])?;
        }
        edited.insert(field, row.value);
    }
    Ok(edited)
}

/// What the files say for a target's field now.
fn file_value(
    conn: &Connection,
    target: Target,
    field: Field,
) -> Result<Option<String>, LibraryError> {
    match target {
        Target::Track(id) => Ok(values::file_shown(&shown::file_values(conn, id)?, field)),
        Target::Release(id) => shown::release_file_value(conn, id, field),
    }
}

fn exists(conn: &Connection, target: Target) -> Result<bool, LibraryError> {
    let sql = match target {
        Target::Track(_) => "SELECT EXISTS (SELECT 1 FROM tracks WHERE id = ?1)",
        Target::Release(_) => "SELECT EXISTS (SELECT 1 FROM albums WHERE id = ?1)",
    };
    let (Target::Track(id) | Target::Release(id)) = target;
    Ok(conn.query_row(sql, [id], |row| row.get(0))?)
}

/// Rewrites the shown values of what `targets` touch: tracks, then every
/// release involved (a track's artist can decide its album artist).
fn refresh(conn: &Connection, targets: &HashSet<Target>) -> Result<(), LibraryError> {
    let mut releases = HashSet::new();
    for &target in targets {
        match target {
            Target::Track(id) => releases.extend(shown::refresh_track(conn, id)?),
            Target::Release(id) => {
                releases.insert(id);
            }
        }
    }
    for release in releases {
        shown::refresh_release(conn, release)?;
    }
    prune_artists(conn)
}

impl Library {
    /// Saves `changes` as one step that [`Library::undo_edits`] reverses.
    /// Every change is checked first: an unknown target, a field of the
    /// wrong scope or a malformed value saves nothing.
    pub fn save_edits(
        &mut self,
        changes: &[EditChange],
        origin: &Origin,
    ) -> Result<(), LibraryError> {
        for change in changes {
            let scope = match change.target {
                Target::Track(_) => Scope::Track,
                Target::Release(_) => Scope::Release,
            };
            if change.field.scope() != scope {
                return Err(LibraryError::Invalid(format!(
                    "{} is not a {} field",
                    change.field.name(),
                    if scope == Scope::Track {
                        "track"
                    } else {
                        "release"
                    }
                )));
            }
            if let Edit::Set(value) = &change.edit
                && !values::is_valid(change.field, value)
            {
                return Err(LibraryError::Invalid(format!(
                    "not a valid {}",
                    change.field.name()
                )));
            }
            if !exists(&self.conn, change.target)? {
                return Err(LibraryError::Invalid("no such track or album".to_owned()));
            }
        }

        let transaction = self.conn.transaction()?;
        transaction.execute("INSERT INTO edit_changes (made_at) VALUES (?1)", [now()])?;
        let step = transaction.last_insert_rowid();
        let mut touched = HashSet::new();
        for change in changes {
            let before = edit_of(&transaction, change.target, change.field)?;
            let after = match &change.edit {
                Edit::Reset => None,
                Edit::Set(_) | Edit::Clear => Some(Row {
                    value: match &change.edit {
                        Edit::Set(value) => Some(value.clone()),
                        _ => None,
                    },
                    origin: origin.stored(),
                    base: file_value(&transaction, change.target, change.field)?,
                    review: false,
                }),
            };
            let same = |a: &Option<Row>, b: &Option<Row>| match (a, b) {
                (Some(a), Some(b)) => a.value == b.value,
                (None, None) => true,
                _ => false,
            };
            if same(&before, &after) {
                continue;
            }
            put(&transaction, change.target, change.field, after.as_ref())?;
            record(
                &transaction,
                step,
                change.target,
                change.field,
                before.as_ref(),
                after.as_ref(),
            )?;
            touched.insert(change.target);
        }
        if touched.is_empty() {
            transaction.execute("DELETE FROM edit_changes WHERE id = ?1", [step])?;
        }
        refresh(&transaction, &touched)?;
        transaction.commit()?;
        Ok(())
    }

    /// Reverses the last saved step. `false` when there is nothing to undo.
    pub fn undo_edits(&mut self) -> Result<bool, LibraryError> {
        let transaction = self.conn.transaction()?;
        let Some(step): Option<i64> =
            transaction.query_row("SELECT MAX(id) FROM edit_changes", [], |row| row.get(0))?
        else {
            return Ok(false);
        };
        // (target, field, the edit before the step if there was one)
        let rows: Vec<(Option<Target>, String, Option<Row>)> = transaction
            .prepare(
                "SELECT track_id, album_id, field, had_before, before_value, before_origin,
                        before_base, before_review
                 FROM edit_history WHERE change_id = ?1 ORDER BY id DESC",
            )?
            .query_map([step], |row| {
                let target = match (row.get(0)?, row.get(1)?) {
                    (Some(id), _) => Some(Target::Track(id)),
                    (None, Some(id)) => Some(Target::Release(id)),
                    (None, None) => None,
                };
                let before = row.get::<_, bool>(3)?.then_some(Row {
                    value: row.get(4)?,
                    origin: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    base: row.get(6)?,
                    review: row.get::<_, Option<bool>>(7)?.unwrap_or(false),
                });
                Ok((target, row.get(2)?, before))
            })?
            .collect::<Result<_, _>>()?;
        let mut touched = HashSet::new();
        for (target, field, before) in rows {
            let (Some(target), Some(field)) = (target, Field::from_name(&field)) else {
                continue;
            };
            put(&transaction, target, field, before.as_ref())?;
            touched.insert(target);
        }
        transaction.execute("DELETE FROM edit_changes WHERE id = ?1", [step])?;
        refresh(&transaction, &touched)?;
        transaction.commit()?;
        Ok(true)
    }

    /// The edits in force on a track or release.
    pub fn edits(&self, target: Target) -> Result<Vec<SavedEdit>, LibraryError> {
        Ok(edits_of(&self.conn, target)?
            .into_iter()
            .map(|(field, row)| SavedEdit {
                field,
                value: row.value,
                origin: row.origin,
                review: row.review,
            })
            .collect())
    }

    /// What a track's file says, as last read.
    pub fn file_values(&self, track: i64) -> Result<Vec<FileValue>, LibraryError> {
        shown::file_values(&self.conn, track)
    }
}

fn record(
    transaction: &Transaction<'_>,
    step: i64,
    target: Target,
    field: Field,
    before: Option<&Row>,
    after: Option<&Row>,
) -> Result<(), LibraryError> {
    let (track, album) = columns(target);
    transaction
        .prepare_cached(
            "INSERT INTO edit_history (change_id, track_id, album_id, field, had_before,
                 before_value, before_origin, before_base, before_review, had_after, after_value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?
        .execute(params![
            step,
            track,
            album,
            field.name(),
            before.is_some(),
            before.and_then(|row| row.value.as_deref()),
            before.map(|row| row.origin.as_str()),
            before.and_then(|row| row.base.as_deref()),
            before.map(|row| row.review),
            after.is_some(),
            after.and_then(|row| row.value.as_deref()),
        ])?;
    Ok(())
}
