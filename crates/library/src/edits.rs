//! The user's edits: values set or cleared in the library, on top of what
//! the files say, with a history for undo. Music files are never touched.
//!
//! An edit keeps the file value it was made over (`base`). When a rescan
//! finds the file saying something else, the edit stays and is marked for
//! review.
//!
//! Saving is in two parts: [`Library::preview_edits`] says what would
//! change, and [`Library::commit_edits`] saves it only if nothing it looked
//! at changed since. Of two origins, the stronger keeps a field: manual
//! over provider over rule.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::db::{Library, LibraryError, now, prune_artists};
use crate::shown::{self, Edited};
use crate::values::{self, Field, FileValue, Problem, Scope};

/// What an edit applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

    fn rank(&self) -> u8 {
        match self {
            Origin::Rule(_) => 0,
            Origin::Provider(_) => 1,
            Origin::Manual => 2,
        }
    }
}

/// The rank of a stored origin; unknown ones rank as manual, so nothing
/// overwrites what it cannot place.
fn stored_rank(stored: &str) -> u8 {
    if stored.starts_with("rule:") {
        0
    } else if stored.starts_with("provider:") {
        1
    } else {
        2
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditChange {
    pub target: Target,
    pub field: Field,
    pub edit: Edit,
}

/// What a preview saw: the revision of every track and release it looked
/// at. [`Library::commit_edits`] saves nothing once one has moved on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Revisions(BTreeMap<Target, i64>);

/// A field a commit would change, as the library shows it before and after.
/// Several values are listed in order; none means nothing is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldPreview {
    pub target: Target,
    pub field: Field,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

/// What [`Library::commit_edits`] would do with some changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    pub fields: Vec<FieldPreview>,
    /// Changes that would not be saved: a stronger origin's edit is in
    /// force, named here.
    pub held: Vec<(EditChange, String)>,
    /// Tracks and releases whose shown values would be rewritten.
    pub affected: Vec<Target>,
    pub revisions: Revisions,
}

impl Revisions {
    pub fn iter(&self) -> impl Iterator<Item = (Target, i64)> + '_ {
        self.0.iter().map(|(&target, &revision)| (target, revision))
    }
}

impl FromIterator<(Target, i64)> for Revisions {
    fn from_iter<I: IntoIterator<Item = (Target, i64)>>(iter: I) -> Self {
        Revisions(iter.into_iter().collect())
    }
}

/// A field as the library shows it, for an editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldState {
    pub field: Field,
    /// In order; none when nothing is shown.
    pub values: Vec<String>,
    /// An edit is in force: what the files say is not what is shown.
    pub edited: bool,
    /// The file changed under the edit.
    pub review: bool,
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
    /// What saving `changes` with `origin` would change, without saving.
    /// Checks every change as a commit does.
    pub fn preview_edits(
        &self,
        changes: &[EditChange],
        origin: &Origin,
    ) -> Result<Preview, LibraryError> {
        // One read transaction: the revisions match what was read.
        let transaction = self.conn.unchecked_transaction()?;
        let plan = plan(&transaction, changes, origin)?;
        let mut fields = Vec::with_capacity(plan.steps.len());
        for step in &plan.steps {
            fields.push(FieldPreview {
                target: step.target,
                field: step.field,
                before: shown_values(&transaction, step.target, step.field, step.before.as_ref())?,
                after: shown_values(&transaction, step.target, step.field, step.after.as_ref())?,
            });
        }
        let stepped: HashSet<Target> = plan.steps.iter().map(|step| step.target).collect();
        let mut revisions = BTreeMap::new();
        for target in with_releases(&transaction, changes.iter().map(|change| change.target))? {
            if let Some(revision) = revision(&transaction, target)? {
                revisions.insert(target, revision);
            }
        }
        Ok(Preview {
            fields,
            held: plan.held,
            affected: with_releases(&transaction, stepped)?.into_iter().collect(),
            revisions: Revisions(revisions),
        })
    }

    /// Saves `changes` as [`Library::preview_edits`] showed them, as one step
    /// that [`Library::undo_edits`] reverses. [`LibraryError::Stale`], saving
    /// nothing, when a track or release the preview looked at changed since.
    pub fn commit_edits(
        &mut self,
        changes: &[EditChange],
        origin: &Origin,
        seen: &Revisions,
    ) -> Result<(), LibraryError> {
        let transaction = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let plan = plan(&transaction, changes, origin)?;
        for target in with_releases(&transaction, changes.iter().map(|change| change.target))? {
            if revision(&transaction, target)? != seen.0.get(&target).copied() {
                return Err(LibraryError::Stale);
            }
        }
        apply(&transaction, &plan)?;
        transaction.commit()?;
        Ok(())
    }

    /// Saves `changes` over whatever is there now, as one undoable step:
    /// [`Library::commit_edits`] without a preview.
    pub fn save_edits(
        &mut self,
        changes: &[EditChange],
        origin: &Origin,
    ) -> Result<(), LibraryError> {
        let transaction = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let plan = plan(&transaction, changes, origin)?;
        apply(&transaction, &plan)?;
        transaction.commit()?;
        Ok(())
    }

    /// Saves changes of several origins (fixes of different rules, say) as
    /// one undoable step, over whatever is there now.
    pub fn save_edits_each(
        &mut self,
        changes: &[(EditChange, Origin)],
    ) -> Result<(), LibraryError> {
        let transaction = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changes: Vec<_> = changes
            .iter()
            .map(|(change, origin)| (change, origin))
            .collect();
        let plan = plan_each(&transaction, &changes)?;
        apply(&transaction, &plan)?;
        transaction.commit()?;
        Ok(())
    }

    /// Drops every edit of `targets`, showing the file values again, as one
    /// undoable step.
    pub fn reset_edits(&mut self, targets: &[Target]) -> Result<(), LibraryError> {
        let mut changes = Vec::new();
        for &target in targets {
            for (field, _) in edits_of(&self.conn, target)? {
                changes.push(EditChange {
                    target,
                    field,
                    edit: Edit::Reset,
                });
            }
        }
        self.save_edits(&changes, &Origin::Manual)
    }

    /// Reverses the last saved step. `false` when there is nothing to undo.
    /// [`LibraryError::UndoConflict`], undoing nothing, when an edit the
    /// step saved is no longer what it saved.
    pub fn undo_edits(&mut self) -> Result<bool, LibraryError> {
        let transaction = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(step): Option<i64> =
            transaction.query_row("SELECT MAX(id) FROM edit_changes", [], |row| row.get(0))?
        else {
            return Ok(false);
        };
        // Newest first: the first row of a field holds what the step left.
        let rows: Vec<Undo> = transaction
            .prepare(
                "SELECT track_id, album_id, field, had_before, before_value, before_origin,
                        before_base, before_review, had_after, after_value
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
                Ok(Undo {
                    target,
                    field: Field::from_name(&row.get::<_, String>(2)?),
                    before,
                    had_after: row.get(8)?,
                    after_value: row.get(9)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        let mut checked = HashSet::new();
        let mut touched = HashSet::new();
        for undo in rows {
            let (Some(target), Some(field)) = (undo.target, undo.field) else {
                continue;
            };
            if checked.insert((target, field)) {
                let unchanged = match edit_of(&transaction, target, field)? {
                    Some(row) => undo.had_after && row.value == undo.after_value,
                    None => !undo.had_after,
                };
                if !unchanged {
                    return Err(LibraryError::UndoConflict);
                }
            }
            put(&transaction, target, field, undo.before.as_ref())?;
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

    /// Every field of a track or release, as shown, in [`Field::ALL`] order.
    pub fn fields(&self, target: Target) -> Result<Vec<FieldState>, LibraryError> {
        let scope = match target {
            Target::Track(_) => Scope::Track,
            Target::Release(_) => Scope::Release,
        };
        let edits: HashMap<Field, Row> = edits_of(&self.conn, target)?.into_iter().collect();
        Field::ALL
            .into_iter()
            .filter(|field| field.scope() == scope)
            .map(|field| {
                let row = edits.get(&field);
                Ok(FieldState {
                    field,
                    values: shown_values(&self.conn, target, field, row)?,
                    edited: row.is_some(),
                    review: row.is_some_and(|row| row.review),
                })
            })
            .collect()
    }

    /// The revisions of `targets` now, for [`Library::commit_edits`] after
    /// an editor showed them.
    pub fn revisions(&self, targets: &[Target]) -> Result<Revisions, LibraryError> {
        let mut revisions = BTreeMap::new();
        for &target in targets {
            if let Some(revision) = revision(&self.conn, target)? {
                revisions.insert(target, revision);
            }
        }
        Ok(Revisions(revisions))
    }

    /// What a track's file says, as last read.
    pub fn file_values(&self, track: i64) -> Result<Vec<FileValue>, LibraryError> {
        shown::file_values(&self.conn, track)
    }
}

/// A row of a step's history, read for undo: the edit before the step,
/// and whether there was one after it, of which value (`None`: cleared).
struct Undo {
    target: Option<Target>,
    field: Option<Field>,
    before: Option<Row>,
    had_after: bool,
    after_value: Option<String>,
}

/// One field a save changes: the edit before and after.
struct Step {
    target: Target,
    field: Field,
    before: Option<Row>,
    after: Option<Row>,
}

struct Plan {
    steps: Vec<Step>,
    held: Vec<(EditChange, String)>,
}

/// Checks `changes` and works out what saving them changes. A change
/// equal to what is in force changes nothing; one over a stronger origin's
/// edit is held.
fn plan(conn: &Connection, changes: &[EditChange], origin: &Origin) -> Result<Plan, LibraryError> {
    let changes: Vec<_> = changes.iter().map(|change| (change, origin)).collect();
    plan_each(conn, &changes)
}

/// [`plan`], each change with its own origin.
fn plan_each(conn: &Connection, changes: &[(&EditChange, &Origin)]) -> Result<Plan, LibraryError> {
    for (change, _) in changes {
        check(conn, change)?;
    }
    // What the changes so far leave, for a field changed twice.
    let mut current: HashMap<(Target, Field), Option<Row>> = HashMap::new();
    let mut plan = Plan {
        steps: Vec::new(),
        held: Vec::new(),
    };
    for &(change, origin) in changes {
        let key = (change.target, change.field);
        let before = match current.get(&key) {
            Some(row) => row.clone(),
            None => edit_of(conn, change.target, change.field)?,
        };
        if let Some(row) = &before
            && stored_rank(&row.origin) > origin.rank()
        {
            plan.held.push((change.clone(), row.origin.clone()));
            continue;
        }
        let after = match &change.edit {
            Edit::Reset => None,
            Edit::Set(_) | Edit::Clear => Some(Row {
                value: match &change.edit {
                    Edit::Set(value) => Some(value.clone()),
                    _ => None,
                },
                origin: origin.stored(),
                base: file_value(conn, change.target, change.field)?,
                review: false,
            }),
        };
        let same = match (&before, &after) {
            (Some(a), Some(b)) => a.value == b.value,
            (None, None) => true,
            _ => false,
        };
        if same {
            continue;
        }
        current.insert(key, after.clone());
        plan.steps.push(Step {
            target: change.target,
            field: change.field,
            before,
            after,
        });
    }
    Ok(plan)
}

fn check(conn: &Connection, change: &EditChange) -> Result<(), LibraryError> {
    let invalid = |problem| LibraryError::InvalidEdit {
        target: change.target,
        field: change.field,
        problem,
    };
    let scope = match change.target {
        Target::Track(_) => Scope::Track,
        Target::Release(_) => Scope::Release,
    };
    if change.field.scope() != scope {
        return Err(invalid(Problem::WrongScope));
    }
    if let Edit::Set(value) = &change.edit {
        values::check(change.field, value).map_err(invalid)?;
    }
    if !exists(conn, change.target)? {
        return Err(invalid(Problem::NoSuchTarget));
    }
    Ok(())
}

/// Saves a plan as one step, with its history, and rewrites what it shows.
fn apply(transaction: &Transaction<'_>, plan: &Plan) -> Result<(), LibraryError> {
    if plan.steps.is_empty() {
        return Ok(());
    }
    transaction.execute("INSERT INTO edit_changes (made_at) VALUES (?1)", [now()])?;
    let id = transaction.last_insert_rowid();
    let mut touched = HashSet::new();
    for step in &plan.steps {
        put(transaction, step.target, step.field, step.after.as_ref())?;
        record(transaction, id, step)?;
        touched.insert(step.target);
    }
    refresh(transaction, &touched)
}

/// `targets` and the releases of those that are tracks.
fn with_releases(
    conn: &Connection,
    targets: impl IntoIterator<Item = Target>,
) -> Result<BTreeSet<Target>, LibraryError> {
    let mut all = BTreeSet::new();
    for target in targets {
        all.insert(target);
        if let Target::Track(id) = target {
            let release: Option<i64> = conn
                .prepare_cached("SELECT album_id FROM tracks WHERE id = ?1")?
                .query_row([id], |row| row.get(0))
                .optional()?;
            all.extend(release.map(Target::Release));
        }
    }
    Ok(all)
}

/// A target's revision; `None` when it is gone.
fn revision(conn: &Connection, target: Target) -> Result<Option<i64>, LibraryError> {
    let (sql, id) = match target {
        Target::Track(id) => ("SELECT revision FROM tracks WHERE id = ?1", id),
        Target::Release(id) => ("SELECT revision FROM albums WHERE id = ?1", id),
    };
    Ok(conn
        .prepare_cached(sql)?
        .query_row([id], |row| row.get(0))
        .optional()?)
}

/// A field as the library shows it with edit `row` in force, or the file's
/// value without one.
fn shown_values(
    conn: &Connection,
    target: Target,
    field: Field,
    row: Option<&Row>,
) -> Result<Vec<String>, LibraryError> {
    Ok(match row {
        Some(row) => row
            .value
            .iter()
            .flat_map(|value| value.split('\0'))
            .map(str::to_owned)
            .collect(),
        None => file_value(conn, target, field)?.into_iter().collect(),
    })
}

/// Fields of which an edit stronger than `origin` is in force.
pub(crate) fn outranking(
    conn: &Connection,
    origin: &Origin,
) -> Result<HashSet<(Target, Field)>, LibraryError> {
    let mut statement = conn.prepare("SELECT track_id, album_id, field, origin FROM edits")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, Option<i64>>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut held = HashSet::new();
    for row in rows {
        let (track, album, field, stored) = row?;
        let target = match (track, album) {
            (Some(id), _) => Target::Track(id),
            (None, Some(id)) => Target::Release(id),
            (None, None) => continue,
        };
        if let Some(field) = Field::from_name(&field)
            && stored_rank(&stored) > origin.rank()
        {
            held.insert((target, field));
        }
    }
    Ok(held)
}

/// Hands the edits of release `old`, emptied by a scan, to the releases its
/// tracks went to, most tracks first. Each takes the fields it has no edit
/// of; the first also takes their history, so the steps stay undoable.
pub(crate) fn follow(conn: &Connection, old: i64, to: &[i64]) -> Result<(), LibraryError> {
    for (field, row) in edits_of(conn, Target::Release(old))? {
        for (index, &new) in to.iter().enumerate() {
            if edit_of(conn, Target::Release(new), field)?.is_some() {
                continue;
            }
            put(conn, Target::Release(new), field, Some(&row))?;
            if index == 0 {
                conn.prepare_cached(
                    "UPDATE edit_history SET album_id = ?2 WHERE album_id = ?1 AND field = ?3",
                )?
                .execute(params![old, new, field.name()])?;
            }
        }
    }
    Ok(())
}

fn record(transaction: &Transaction<'_>, id: i64, step: &Step) -> Result<(), LibraryError> {
    let (track, album) = columns(step.target);
    let (before, after) = (step.before.as_ref(), step.after.as_ref());
    transaction
        .prepare_cached(
            "INSERT INTO edit_history (change_id, track_id, album_id, field, had_before,
                 before_value, before_origin, before_base, before_review, had_after, after_value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?
        .execute(params![
            id,
            track,
            album,
            step.field.name(),
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
