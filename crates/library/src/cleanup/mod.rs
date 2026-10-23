//! Cleanup rules: from messy tags to a well-structured library
//! ([decision](../../../../docs/METADATA_DECISIONS.md) 10).
//!
//! Rules only propose. Each is a pure function over a snapshot of the
//! library's current values; a proposal becomes an edit only when the user
//! accepts it ([`Library::save_edits`] with [`Origin::Rule`]). Rules run in
//! a fixed order, each on what the ones before left, so one field gets one
//! proposal that names every rule that changed it.

mod numbers;
mod text;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::params;

use crate::db::{Library, LibraryError, VARIOUS_ARTISTS};
use crate::edits::{Edit, EditChange, Origin, Target};
use crate::key::key;
use crate::values::{self, Field, Kind, Scope};

/// A cleanup rule, in the order they run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    /// Spaces, invisible characters, Unicode form.
    TextHygiene,
    /// UTF-8 that was read as Latin-1 or Windows-1252.
    EncodingRepair,
    /// `3/12`, `03`, `Album (Disc 2)`.
    Numbers,
    /// Dates as ISO 8601.
    Dates,
    /// ID3 genre numbers, `Rock; Pop`, one spelling per genre.
    Genres,
    /// ALL CAPS or all lower case as title case.
    Capitals,
    /// `Song (feat. X)` → the guest in the artist.
    FeaturedArtists,
    /// One spelling per artist across the library.
    DuplicateArtists,
    /// A missing album artist.
    AlbumArtist,
    /// `The Beatles` sorts as `Beatles, The`.
    SortNames,
}

impl Rule {
    pub fn name(self) -> &'static str {
        match self {
            Rule::TextHygiene => "text_hygiene",
            Rule::EncodingRepair => "encoding_repair",
            Rule::Numbers => "numbers",
            Rule::Dates => "dates",
            Rule::Genres => "genres",
            Rule::Capitals => "capitals",
            Rule::FeaturedArtists => "featured_artists",
            Rule::DuplicateArtists => "duplicate_artists",
            Rule::AlbumArtist => "album_artist",
            Rule::SortNames => "sort_names",
        }
    }
}

/// A proposed value for one field of one track or release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub target: Target,
    pub field: Field,
    /// The current values; empty when the field has none.
    pub before: Vec<String>,
    pub after: Vec<String>,
    /// Every rule that changed it, in order.
    pub rules: Vec<Rule>,
}

impl Proposal {
    /// The edit that accepts this proposal.
    pub fn change(&self) -> EditChange {
        EditChange {
            target: self.target,
            field: self.field,
            edit: Edit::Set(self.after.join("\0")),
        }
    }

    /// Where an accepted proposal comes from: `rule:capitals+text_hygiene`.
    pub fn origin(&self) -> Origin {
        let names: Vec<&str> = self.rules.iter().map(|rule| rule.name()).collect();
        Origin::Rule(names.join("+"))
    }
}

/// Each field's values, in order.
pub(crate) type Values = BTreeMap<Field, Vec<String>>;

/// What the rules look at: every track's track fields and every release's
/// release fields, as currently shown (edits applied).
#[derive(Debug, Default)]
pub(crate) struct Snapshot {
    /// (track, its release, values), in release, disc and track order.
    pub tracks: Vec<(i64, i64, Values)>,
    pub releases: Vec<(i64, Values)>,
}

/// The rules' working copy: current values, and what changed.
struct Work {
    values: HashMap<Target, Values>,
    /// Original values of the fields a rule changed.
    before: BTreeMap<(Target, Field), Vec<String>>,
    rules: HashMap<(Target, Field), BTreeSet<Rule>>,
}

impl Work {
    fn get(&self, target: Target, field: Field) -> &[String] {
        self.values
            .get(&target)
            .and_then(|values| values.get(&field))
            .map_or(&[], Vec::as_slice)
    }

    fn first(&self, target: Target, field: Field) -> Option<&str> {
        self.get(target, field).first().map(String::as_str)
    }

    fn set(&mut self, target: Target, field: Field, after: Vec<String>, rule: Rule) {
        let current = self.get(target, field);
        if current == after.as_slice() {
            return;
        }
        let current = current.to_vec();
        self.before.entry((target, field)).or_insert(current);
        self.rules.entry((target, field)).or_default().insert(rule);
        self.values.entry(target).or_default().insert(field, after);
    }

    /// Applies `rule` to every value of `field` on `targets`.
    fn each_value(
        &mut self,
        targets: &[Target],
        field: Field,
        rule: Rule,
        fix: impl Fn(&str) -> Option<String>,
    ) {
        for &target in targets {
            let values = self.get(target, field);
            if values.iter().all(|value| fix(value).is_none()) {
                continue;
            }
            let fixed = values
                .iter()
                .map(|value| fix(value).unwrap_or_else(|| value.clone()))
                .collect();
            self.set(target, field, fixed, rule);
        }
    }
}

/// Every proposal the rules make for `snapshot`.
pub(crate) fn propose(snapshot: &Snapshot) -> Vec<Proposal> {
    let mut work = Work {
        values: HashMap::new(),
        before: BTreeMap::new(),
        rules: HashMap::new(),
    };
    for (id, _, values) in &snapshot.tracks {
        work.values.insert(Target::Track(*id), values.clone());
    }
    for (id, values) in &snapshot.releases {
        work.values.insert(Target::Release(*id), values.clone());
    }
    let tracks: Vec<Target> = snapshot
        .tracks
        .iter()
        .map(|(id, ..)| Target::Track(*id))
        .collect();
    let releases: Vec<Target> = snapshot
        .releases
        .iter()
        .map(|(id, _)| Target::Release(*id))
        .collect();
    let mut release_tracks: HashMap<i64, Vec<Target>> = HashMap::new();
    for (id, release, _) in &snapshot.tracks {
        release_tracks
            .entry(*release)
            .or_default()
            .push(Target::Track(*id));
    }

    for field in Field::ALL.into_iter().filter(|f| f.kind() == Kind::Text) {
        let targets = scoped(field, &tracks, &releases);
        work.each_value(targets, field, Rule::TextHygiene, text::hygiene);
        if !is_id(field) {
            work.each_value(targets, field, Rule::EncodingRepair, text::repair_encoding);
        }
    }

    numbers_rule(&mut work, &tracks, snapshot, &release_tracks);
    for field in [Field::Date, Field::OriginalDate] {
        work.each_value(&releases, field, Rule::Dates, numbers::iso_date);
    }
    genres_rule(&mut work, &tracks);

    for (field, lower_too) in [
        (Field::Title, true),
        (Field::Artist, false),
        (Field::Composer, false),
        (Field::Album, true),
        (Field::AlbumArtist, false),
    ] {
        let targets = scoped(field, &tracks, &releases);
        work.each_value(targets, field, Rule::Capitals, |value| {
            text::capitals(value, lower_too)
        });
    }

    featured_rule(&mut work, &tracks);
    duplicates_rule(&mut work, &tracks, &releases);
    album_artist_rule(&mut work, snapshot, &release_tracks);
    for (field, sort, targets) in [
        (Field::Artist, Field::ArtistSort, &tracks),
        (Field::AlbumArtist, Field::AlbumArtistSort, &releases),
    ] {
        for &target in targets {
            if work.get(target, sort).is_empty()
                && let Some(sorted) = work.first(target, field).and_then(text::sort_name)
            {
                work.set(target, sort, vec![sorted], Rule::SortNames);
            }
        }
    }

    let mut proposals: Vec<Proposal> = work
        .before
        .into_iter()
        .filter_map(|((target, field), before)| {
            let after = work.values.get(&target)?.get(&field)?.clone();
            let rules = work.rules.remove(&(target, field))?.into_iter().collect();
            (after != before).then_some(Proposal {
                target,
                field,
                before,
                after,
                rules,
            })
        })
        .collect();
    proposals.sort_by_key(|proposal| (proposal.target, proposal.field));
    proposals
}

fn scoped<'a>(field: Field, tracks: &'a [Target], releases: &'a [Target]) -> &'a [Target] {
    match field.scope() {
        Scope::Track => tracks,
        Scope::Release => releases,
    }
}

fn is_id(field: Field) -> bool {
    matches!(
        field,
        Field::ReleaseId
            | Field::ReleaseGroupId
            | Field::RecordingId
            | Field::ReleaseTrackId
            | Field::ArtistId
            | Field::AlbumArtistId
    )
}

/// Numbers in their plain form, totals out of `3/12`, discs out of album
/// titles.
fn numbers_rule(
    work: &mut Work,
    tracks: &[Target],
    snapshot: &Snapshot,
    release_tracks: &HashMap<i64, Vec<Target>>,
) {
    let release_of: HashMap<Target, Target> = snapshot
        .tracks
        .iter()
        .map(|(id, release, _)| (Target::Track(*id), Target::Release(*release)))
        .collect();
    for &track in tracks {
        for (number, total) in [
            (Field::Track, Field::TrackTotal),
            (Field::Disc, Field::DiscTotal),
        ] {
            let Some((n, of)) = work.first(track, number).and_then(numbers::number_parts) else {
                continue;
            };
            work.set(track, number, vec![n.to_string()], Rule::Numbers);
            // The disc total is the release's.
            let total_target = match total.scope() {
                Scope::Release => match release_of.get(&track) {
                    Some(&release) => release,
                    None => continue,
                },
                Scope::Track => track,
            };
            if let Some(of) = of
                && work.get(total_target, total).is_empty()
            {
                work.set(total_target, total, vec![of.to_string()], Rule::Numbers);
            }
        }
        if let Some((n, _)) = work
            .first(track, Field::TrackTotal)
            .and_then(numbers::number_parts)
        {
            work.set(track, Field::TrackTotal, vec![n.to_string()], Rule::Numbers);
        }
    }
    for (id, _) in &snapshot.releases {
        let release = Target::Release(*id);
        if let Some((n, _)) = work
            .first(release, Field::DiscTotal)
            .and_then(numbers::number_parts)
        {
            work.set(
                release,
                Field::DiscTotal,
                vec![n.to_string()],
                Rule::Numbers,
            );
        }
        let Some((album, disc)) = work
            .first(release, Field::Album)
            .and_then(numbers::disc_in_album)
        else {
            continue;
        };
        work.set(release, Field::Album, vec![album], Rule::Numbers);
        for &track in release_tracks.get(id).into_iter().flatten() {
            if work.get(track, Field::Disc).is_empty() {
                work.set(track, Field::Disc, vec![disc.to_string()], Rule::Numbers);
            }
        }
    }
}

/// ID3 genre numbers as names, `Rock; Pop` as two genres, then one spelling
/// per genre across the library (the most used).
fn genres_rule(work: &mut Work, tracks: &[Target]) {
    for &track in tracks {
        let genres = work.get(track, Field::Genre);
        let mut split: Vec<String> = Vec::new();
        for value in genres {
            for part in value.split(';') {
                let Some(genre) = values::genre(part) else {
                    continue;
                };
                if !split.iter().any(|known| key(known) == key(&genre)) {
                    split.push(genre);
                }
            }
        }
        if !split.is_empty() {
            work.set(track, Field::Genre, split, Rule::Genres);
        }
    }
    let spelling = most_used(
        tracks
            .iter()
            .flat_map(|&track| work.get(track, Field::Genre)),
        key,
    );
    for &track in tracks {
        let genres = work.get(track, Field::Genre);
        let fixed: Vec<String> = genres
            .iter()
            .map(|genre| {
                spelling
                    .get(&key(genre))
                    .cloned()
                    .unwrap_or_else(|| genre.clone())
            })
            .collect();
        work.set(track, Field::Genre, fixed, Rule::Genres);
    }
}

/// For each group of names that `group` puts together, its most used
/// spelling. Ties go to the spelling with more letters beyond ASCII (accents
/// kept), then to the one that sorts first.
fn most_used<'a>(
    names: impl Iterator<Item = &'a String>,
    group: impl Fn(&str) -> String,
) -> HashMap<String, String> {
    let mut counts: HashMap<String, HashMap<&'a str, usize>> = HashMap::new();
    for name in names {
        *counts
            .entry(group(name))
            .or_default()
            .entry(name.as_str())
            .or_default() += 1;
    }
    counts
        .into_iter()
        .filter_map(|(group, spellings)| {
            let best = spellings.into_iter().max_by(|(a, count_a), (b, count_b)| {
                let accents = |name: &str| name.chars().filter(|c| !c.is_ascii()).count();
                count_a
                    .cmp(count_b)
                    .then_with(|| accents(a).cmp(&accents(b)))
                    .then_with(|| b.cmp(a))
            })?;
            Some((group, best.0.to_owned()))
        })
        .collect()
}

/// Guests out of titles into artists; `ft.` and `featuring` as `feat.`.
fn featured_rule(work: &mut Work, tracks: &[Target]) {
    for &track in tracks {
        if let Some((song, guest)) = work
            .first(track, Field::Title)
            .and_then(text::guest_in_title)
        {
            let mut artists = work.get(track, Field::Artist).to_vec();
            match artists.first_mut() {
                Some(main) => *main = text::with_guest(main, &guest),
                None => artists.push(guest),
            }
            work.set(track, Field::Title, vec![song], Rule::FeaturedArtists);
            work.set(track, Field::Artist, artists, Rule::FeaturedArtists);
        }
    }
    work.each_value(
        tracks,
        Field::Artist,
        Rule::FeaturedArtists,
        text::guest_marker,
    );
}

/// Names that match loosely (`The Beatles`, `Beatles`, `Beyonce`,
/// `Beyoncé`) take their most used spelling, in track artists and album
/// artists alike.
fn duplicates_rule(work: &mut Work, tracks: &[Target], releases: &[Target]) {
    let names = tracks
        .iter()
        .flat_map(|&track| work.get(track, Field::Artist))
        .chain(
            releases
                .iter()
                .flat_map(|&release| work.get(release, Field::AlbumArtist)),
        );
    let spelling = most_used(names, text::loose_key);
    for (field, targets) in [(Field::Artist, tracks), (Field::AlbumArtist, releases)] {
        work.each_value(targets, field, Rule::DuplicateArtists, |name| {
            spelling
                .get(&text::loose_key(name))
                .filter(|best| key(best) != key(name))
                .cloned()
        });
    }
}

/// A release without an album artist gets its tracks' main artist when they
/// all agree, else Various Artists (always for a compilation).
fn album_artist_rule(
    work: &mut Work,
    snapshot: &Snapshot,
    release_tracks: &HashMap<i64, Vec<Target>>,
) {
    for (id, _) in &snapshot.releases {
        let release = Target::Release(*id);
        if !work.get(release, Field::AlbumArtist).is_empty() {
            continue;
        }
        let compilation = work.first(release, Field::Compilation) == Some("1");
        // Guests do not make an album a compilation.
        let artists: Vec<String> = release_tracks
            .get(id)
            .into_iter()
            .flatten()
            .filter_map(|&track| work.first(track, Field::Artist))
            .map(|artist| text::main_artist(artist).to_owned())
            .collect();
        let Some(first) = artists.first() else {
            continue;
        };
        let agree = artists.iter().all(|artist| key(artist) == key(first));
        let album_artist = if agree && !compilation {
            first.clone()
        } else {
            VARIOUS_ARTISTS.to_owned()
        };
        work.set(
            release,
            Field::AlbumArtist,
            vec![album_artist],
            Rule::AlbumArtist,
        );
    }
}

impl Library {
    /// What the cleanup rules propose for the whole library, as it is now
    /// shown. Changes nothing. Fields whose file value was too long to keep
    /// whole are left out.
    pub fn propose_cleanup(&self) -> Result<Vec<Proposal>, LibraryError> {
        Ok(propose(&self.snapshot()?))
    }

    fn snapshot(&self) -> Result<Snapshot, LibraryError> {
        let mut file: HashMap<i64, Values> = HashMap::new();
        let mut cut: BTreeSet<(i64, Field)> = BTreeSet::new();
        let mut statement = self
            .conn
            .prepare("SELECT track_id, field, value, cut FROM file_values ORDER BY track_id, field, position")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
            ))
        })?;
        for row in rows {
            let (track, field, value, is_cut) = row?;
            let Some(field) = Field::from_name(&field) else {
                continue;
            };
            if is_cut {
                cut.insert((track, field));
            }
            file.entry(track)
                .or_default()
                .entry(field)
                .or_default()
                .push(value);
        }

        let mut edits: HashMap<Target, Vec<(Field, Option<String>)>> = HashMap::new();
        let mut statement = self
            .conn
            .prepare("SELECT track_id, album_id, field, value FROM edits")?;
        let rows = statement.query_map(params![], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        for row in rows {
            let (track, album, field, value) = row?;
            let target = match (track, album) {
                (Some(id), _) => Target::Track(id),
                (None, Some(id)) => Target::Release(id),
                (None, None) => continue,
            };
            if let Some(field) = Field::from_name(&field) {
                edits.entry(target).or_default().push((field, value));
            }
        }
        let overlay = |values: &mut Values, target: Target| {
            for (field, value) in edits.get(&target).into_iter().flatten() {
                match value {
                    Some(value) => {
                        values.insert(*field, value.split('\0').map(str::to_owned).collect());
                    }
                    None => {
                        values.remove(field);
                    }
                }
            }
        };

        let mut snapshot = Snapshot::default();
        let mut statement = self
            .conn
            .prepare("SELECT id, album_id FROM tracks ORDER BY album_id, disc, track, path")?;
        let order: Vec<(i64, i64)> = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let mut release: Option<(i64, Values)> = None;
        for (track, album) in order {
            let mut own = file.remove(&track).unwrap_or_default();
            own.retain(|field, _| !cut.contains(&(track, *field)));
            let (track_values, release_values): (Values, Values) = own
                .into_iter()
                .partition(|(field, _)| field.scope() == Scope::Track);
            if release.as_ref().is_none_or(|(id, _)| *id != album) {
                if let Some((id, mut values)) = release.take() {
                    overlay(&mut values, Target::Release(id));
                    snapshot.releases.push((id, values));
                }
                release = Some((album, Values::new()));
            }
            // A release field's value is its first track's that has one.
            if let Some((_, values)) = release.as_mut() {
                for (field, value) in release_values {
                    values.entry(field).or_insert(value);
                }
            }
            let mut track_values = track_values;
            overlay(&mut track_values, Target::Track(track));
            snapshot.tracks.push((track, album, track_values));
        }
        if let Some((id, mut values)) = release {
            overlay(&mut values, Target::Release(id));
            snapshot.releases.push((id, values));
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::{Proposal, Rule, Snapshot, Values, propose};
    use crate::edits::Target;
    use crate::values::Field;

    fn values(pairs: &[(Field, &str)]) -> Values {
        let mut values = Values::new();
        for (field, value) in pairs {
            values.entry(*field).or_default().push((*value).to_owned());
        }
        values
    }

    /// The snapshot with every proposal applied.
    fn apply(snapshot: &Snapshot, proposals: &[Proposal]) -> Snapshot {
        let target_values = |target: Target, values: &Values| {
            let mut values = values.clone();
            for proposal in proposals.iter().filter(|p| p.target == target) {
                values.insert(proposal.field, proposal.after.clone());
            }
            values
        };
        Snapshot {
            tracks: snapshot
                .tracks
                .iter()
                .map(|(id, release, v)| (*id, *release, target_values(Target::Track(*id), v)))
                .collect(),
            releases: snapshot
                .releases
                .iter()
                .map(|(id, v)| (*id, target_values(Target::Release(*id), v)))
                .collect(),
        }
    }

    fn messy() -> Snapshot {
        Snapshot {
            tracks: vec![
                (
                    1,
                    10,
                    values(&[
                        (Field::Title, "  SPEAK TO ME "),
                        (Field::Artist, "Pink Floyd"),
                        (Field::Track, "1/10"),
                    ]),
                ),
                (
                    2,
                    10,
                    values(&[
                        (Field::Title, "Breathe (feat. David Gilmour)"),
                        (Field::Artist, "Pink Floyd"),
                        (Field::Track, "02"),
                    ]),
                ),
                (
                    3,
                    20,
                    values(&[
                        (Field::Title, "Help!"),
                        (Field::Artist, "The Beatles"),
                        (Field::Genre, "(17)"),
                    ]),
                ),
                (
                    4,
                    20,
                    values(&[
                        (Field::Title, "Yesterday"),
                        (Field::Artist, "The Beatles"),
                        (Field::Genre, "rock"),
                    ]),
                ),
                (
                    5,
                    30,
                    values(&[
                        (Field::Title, "Something"),
                        (Field::Artist, "Beatles"),
                        (Field::Genre, "Rock; Pop"),
                    ]),
                ),
                (
                    6,
                    40,
                    values(&[(Field::Title, "La Femme d'argent"), (Field::Artist, "Air")]),
                ),
                (
                    7,
                    40,
                    values(&[(Field::Title, "Porcelain"), (Field::Artist, "Moby")]),
                ),
            ],
            releases: vec![
                (
                    10,
                    values(&[
                        (Field::Album, "THE DARK SIDE OF THE MOON"),
                        (Field::AlbumArtist, "Pink Floyd"),
                        (Field::Date, "01.03.1973"),
                    ]),
                ),
                (
                    20,
                    values(&[
                        (Field::Album, "Help! (Disc 1)"),
                        (Field::AlbumArtist, "The Beatles"),
                    ]),
                ),
                (
                    30,
                    values(&[
                        (Field::Album, "Abbey Road"),
                        (Field::AlbumArtist, "Beatles"),
                    ]),
                ),
                (40, values(&[(Field::Album, "Chill Out")])),
            ],
        }
    }

    fn find(proposals: &[Proposal], target: Target, field: Field) -> Option<&Proposal> {
        proposals
            .iter()
            .find(|p| p.target == target && p.field == field)
    }

    #[test]
    fn rules_chain_into_one_proposal_per_field() {
        let proposals = propose(&messy());
        let after = |target, field| find(&proposals, target, field).map(|p| p.after.clone());

        let title = find(&proposals, Target::Track(1), Field::Title).unwrap();
        assert_eq!(title.after, ["Speak to Me"]);
        assert_eq!(title.rules, [Rule::TextHygiene, Rule::Capitals]);
        assert_eq!(after(Target::Track(1), Field::Track).unwrap(), ["1"]);
        assert_eq!(after(Target::Track(1), Field::TrackTotal).unwrap(), ["10"]);
        assert_eq!(after(Target::Track(2), Field::Track).unwrap(), ["2"]);
        assert_eq!(after(Target::Track(2), Field::Title).unwrap(), ["Breathe"]);
        assert_eq!(
            after(Target::Track(2), Field::Artist).unwrap(),
            ["Pink Floyd feat. David Gilmour"]
        );
        assert_eq!(
            after(Target::Release(10), Field::Album).unwrap(),
            ["The Dark Side of the Moon"]
        );
        assert_eq!(
            after(Target::Release(10), Field::Date).unwrap(),
            ["1973-03-01"]
        );

        // The disc leaves the title for the tracks.
        assert_eq!(after(Target::Release(20), Field::Album).unwrap(), ["Help!"]);
        assert_eq!(after(Target::Track(3), Field::Disc).unwrap(), ["1"]);
        // One spelling per artist and per genre, the most used.
        assert_eq!(
            after(Target::Track(5), Field::Artist).unwrap(),
            ["The Beatles"]
        );
        assert_eq!(
            after(Target::Release(30), Field::AlbumArtist).unwrap(),
            ["The Beatles"]
        );
        assert_eq!(after(Target::Track(3), Field::Genre).unwrap(), ["Rock"]);
        assert_eq!(after(Target::Track(4), Field::Genre).unwrap(), ["Rock"]);
        assert_eq!(
            after(Target::Track(5), Field::Genre).unwrap(),
            ["Rock", "Pop"]
        );
        // No album artist, different artists.
        assert_eq!(
            after(Target::Release(40), Field::AlbumArtist).unwrap(),
            ["Various Artists"]
        );
        assert_eq!(
            after(Target::Track(3), Field::ArtistSort).unwrap(),
            ["Beatles, The"]
        );

        // Clean values get nothing.
        assert!(find(&proposals, Target::Track(6), Field::Title).is_none());
        assert!(find(&proposals, Target::Release(10), Field::AlbumArtist).is_none());
    }

    #[test]
    fn applied_proposals_propose_nothing_more() {
        let snapshot = messy();
        let first = propose(&snapshot);
        assert_ne!(first, Vec::<Proposal>::new());
        let again = propose(&apply(&snapshot, &first));
        assert_eq!(again, Vec::<Proposal>::new());
    }

    #[test]
    fn proposals_become_edits_named_after_their_rules() {
        let proposals = propose(&messy());
        let title = find(&proposals, Target::Track(1), Field::Title).unwrap();
        assert_eq!(
            title.change().edit,
            crate::edits::Edit::Set("Speak to Me".to_owned())
        );
        assert_eq!(
            title.origin(),
            crate::edits::Origin::Rule("text_hygiene+capitals".to_owned())
        );
        let genre = find(&proposals, Target::Track(5), Field::Genre).unwrap();
        assert_eq!(
            genre.change().edit,
            crate::edits::Edit::Set("Rock\0Pop".to_owned())
        );
    }
}
