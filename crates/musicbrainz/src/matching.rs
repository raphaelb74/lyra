//! Matching a library album to a MusicBrainz release, and turning the release
//! into suggested edits for the listener to review. Everything here is pure,
//! apart from [`LocalAlbum::read`] and [`candidates`].

use library::{
    Edit, EditChange, Field, FieldState, Library, LibraryError, Target, UNKNOWN_ARTIST,
    matching_key,
};

use crate::{Client, Error, Release, Track};

/// The name provider edits are saved under (`Origin::Provider`).
pub const PROVIDER: &str = "MusicBrainz";
/// MusicBrainz's "Various Artists".
pub const VARIOUS_ARTISTS_ID: &str = "89ad4ac3-39f7-470e-963a-56509c546377";
/// Lengths closer than this agree: rips and pressings differ by a few seconds.
pub const LENGTH_TOLERANCE_MS: u64 = 5_000;
/// Search results asked for.
const SEARCH_LIMIT: u8 = 25;

/// An album as the library shows it, edits included.
#[derive(Clone, Debug, Default)]
pub struct LocalAlbum {
    pub id: i64,
    pub fields: Vec<FieldState>,
    /// In album order: disc, then track number.
    pub tracks: Vec<LocalTrack>,
}

#[derive(Clone, Debug, Default)]
pub struct LocalTrack {
    pub id: i64,
    pub duration_ms: Option<u64>,
    pub fields: Vec<FieldState>,
}

impl LocalAlbum {
    /// The album's shown fields and its tracks'; `None` when it is gone.
    pub fn read(library: &Library, album_id: i64) -> Result<Option<Self>, LibraryError> {
        if library.album(album_id)?.is_none() {
            return Ok(None);
        }
        let tracks = library
            .album_tracks(album_id)?
            .into_iter()
            .map(|track| {
                Ok(LocalTrack {
                    id: track.id,
                    duration_ms: track.duration_ms,
                    fields: library.fields(Target::Track(track.id))?,
                })
            })
            .collect::<Result<_, LibraryError>>()?;
        Ok(Some(Self {
            id: album_id,
            fields: library.fields(Target::Release(album_id))?,
            tracks,
        }))
    }

    fn first(&self, field: Field) -> Option<&str> {
        first(&self.fields, field)
    }

    /// The album artist, else the artist every track shares.
    fn artist(&self) -> Option<&str> {
        let known = |name: &&str| !name.trim().is_empty() && *name != UNKNOWN_ARTIST;
        self.first(Field::AlbumArtist).filter(known).or_else(|| {
            let mut artists = self
                .tracks
                .iter()
                .map(|track| first(&track.fields, Field::Artist));
            let artist = artists.next()??;
            artists
                .all(|other| other == Some(artist))
                .then_some(artist)
                .filter(known)
        })
    }
}

impl LocalTrack {
    fn number(&self, field: Field) -> Option<u32> {
        first(&self.fields, field)?.parse().ok()
    }
}

fn shown(fields: &[FieldState], field: Field) -> &[String] {
    fields
        .iter()
        .find(|state| state.field == field)
        .map_or(&[], |state| state.values.as_slice())
}

fn first(fields: &[FieldState], field: Field) -> Option<&str> {
    shown(fields, field).first().map(String::as_str)
}

/// A search for releases named `album` by `artist`; `None` without a title.
/// Words are lower-cased, so `and`, `or` and `not` in a title stay words, and
/// Lucene's special characters are escaped.
#[must_use]
pub fn search_query(album: &str, artist: Option<&str>) -> Option<String> {
    let album = terms(album)?;
    Some(match artist.and_then(terms) {
        Some(artist) => format!("release:({album}) AND artist:({artist})"),
        None => format!("release:({album})"),
    })
}

fn terms(text: &str) -> Option<String> {
    let mut escaped = String::with_capacity(text.len());
    for c in matching_key(text).chars() {
        if "+-&|!(){}[]^\"~*?:\\/".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    (!escaped.trim().is_empty()).then_some(escaped)
}

/// A release that may be the album, and why.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub release: Release,
    pub evidence: Evidence,
}

/// What agrees between the album and a release. Shown with each result; no
/// percentage is claimed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent facts, each shown to the listener"
)]
pub struct Evidence {
    /// The album is already tagged with this release.
    pub release_id: bool,
    pub track_count: bool,
    pub title: bool,
    pub artist: bool,
    pub year: bool,
}

/// Releases for the album, best first: the release it is tagged with when
/// MusicBrainz still has it, else a search. Empty without a title to search.
pub fn candidates(client: &Client, album: &LocalAlbum) -> Result<Vec<Candidate>, Error> {
    if let Some(id) = album.first(Field::ReleaseId) {
        match client.release(id) {
            Ok(release) => return Ok(rank(album, vec![release])),
            // A merged or removed release, or a malformed tag: search instead.
            Err(Error::NotFound | Error::BadId) => {}
            Err(error) => return Err(error),
        }
    }
    let Some(query) = album
        .first(Field::Album)
        .and_then(|title| search_query(title, album.artist()))
    else {
        return Ok(Vec::new());
    };
    Ok(rank(album, client.search_releases(&query, SEARCH_LIMIT)?))
}

/// Orders releases by what agrees with the album, then by MusicBrainz's score.
#[must_use]
pub fn rank(album: &LocalAlbum, releases: Vec<Release>) -> Vec<Candidate> {
    let title = album.first(Field::Album).map(matching_key);
    let artist = album.artist().map(matching_key);
    let year = album.first(Field::Date).and_then(year);
    let release_id = album.first(Field::ReleaseId);
    let mut candidates: Vec<Candidate> = releases
        .into_iter()
        .map(|release| {
            let evidence = Evidence {
                release_id: release_id == Some(release.id.as_str()),
                track_count: track_total(&release) == Some(album.tracks.len()),
                title: title.as_deref() == Some(matching_key(&release.title).as_str()),
                artist: artist.as_deref()
                    == Some(matching_key(&credit_text(&release.artist_credit)).as_str()),
                year: year.is_some() && year == release.date.as_deref().and_then(self::year),
            };
            Candidate { release, evidence }
        })
        .collect();
    // Stable: equal evidence keeps MusicBrainz's order.
    candidates.sort_by_key(|candidate| {
        let e = candidate.evidence;
        std::cmp::Reverse((
            e.release_id,
            e.track_count,
            e.title,
            e.artist,
            e.year,
            candidate.release.score.unwrap_or(0),
        ))
    });
    candidates
}

fn year(date: &str) -> Option<&str> {
    date.get(..4)
        .filter(|year| year.bytes().all(|b| b.is_ascii_digit()))
}

/// Tracks on the release: the search's count, else the media's.
fn track_total(release: &Release) -> Option<usize> {
    release
        .track_count
        .or_else(|| {
            (!release.media.is_empty())
                .then(|| release.media.iter().map(|medium| medium.track_count).sum())
        })
        .and_then(|count| usize::try_from(count).ok())
}

/// A credit as printed: names with their join phrases.
fn credit_text(credits: &[crate::ArtistCredit]) -> String {
    credits.iter().fold(String::new(), |mut text, credit| {
        text.push_str(&credit.name);
        text.push_str(&credit.joinphrase);
        text
    })
}

/// A credit for sorting: each artist's sort name with the join phrases.
fn credit_sort(credits: &[crate::ArtistCredit]) -> String {
    credits.iter().fold(String::new(), |mut text, credit| {
        if credit.artist.sort_name.is_empty() {
            text.push_str(&credit.name);
        } else {
            text.push_str(&credit.artist.sort_name);
        }
        text.push_str(&credit.joinphrase);
        text
    })
}

fn credit_ids(credits: &[crate::ArtistCredit]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for credit in credits {
        if !credit.artist.id.is_empty() && !ids.contains(&credit.artist.id) {
            ids.push(credit.artist.id.clone());
        }
    }
    ids
}

/// Which release track a library track is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub track: i64,
    /// Index into the release's media, then into that medium's tracks.
    pub medium: usize,
    pub index: usize,
    /// How the pair was made.
    pub by: PairedBy,
    /// Library length minus release length, in milliseconds, when both are known.
    pub length_difference_ms: Option<i64>,
}

impl Pair {
    /// Whether the lengths agree within [`LENGTH_TOLERANCE_MS`]; `None` when
    /// either is unknown.
    #[must_use]
    pub fn lengths_agree(&self) -> Option<bool> {
        self.length_difference_ms
            .map(|difference| difference.unsigned_abs() <= LENGTH_TOLERANCE_MS)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairedBy {
    /// The track is tagged with this release track or recording.
    Id,
    /// Same disc and track number.
    Number,
    /// Track numbers that run on across discs (11 is disc 2's first of 10).
    RunningNumber,
    /// No numbers: the same place in the album.
    Order,
}

/// Library tracks matched to release tracks. Unpaired tracks get no
/// suggestions; release tracks the library lacks are listed, not added.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Alignment {
    /// In album order.
    pub pairs: Vec<Pair>,
    /// Library tracks with no release track.
    pub unpaired: Vec<i64>,
    /// Release tracks with no library track: (medium, index).
    pub missing: Vec<(usize, usize)>,
}

/// Pairs the album's tracks with the release's: by MusicBrainz IDs, then by
/// disc and track number, then by numbers that run on across discs, and,
/// when no track has a number and the counts agree, by order.
#[must_use]
pub fn align(album: &LocalAlbum, release: &Release) -> Alignment {
    let slots: Vec<(usize, usize, &Track)> = release
        .media
        .iter()
        .enumerate()
        .flat_map(|(m, medium)| {
            medium
                .tracks
                .iter()
                .enumerate()
                .map(move |(i, track)| (m, i, track))
        })
        .collect();
    let mut taken = vec![false; slots.len()];
    let mut found: Vec<Option<(usize, PairedBy)>> = vec![None; album.tracks.len()];

    let claim = |found: &mut Vec<Option<(usize, PairedBy)>>,
                 taken: &mut Vec<bool>,
                 local: usize,
                 slot: Option<usize>,
                 by: PairedBy| {
        if let Some(slot) = slot.filter(|&slot| !taken[slot]) {
            taken[slot] = true;
            found[local] = Some((slot, by));
        }
    };

    // IDs: the release track first, it names this edition; then the recording.
    for (local, track) in album.tracks.iter().enumerate() {
        let release_track = first(&track.fields, Field::ReleaseTrackId);
        let recording = first(&track.fields, Field::RecordingId);
        let slot = slots
            .iter()
            .position(|(_, _, slot)| release_track == Some(slot.id.as_str()))
            .or_else(|| {
                slots
                    .iter()
                    .position(|(_, _, slot)| recording == Some(slot.recording.id.as_str()))
            });
        claim(&mut found, &mut taken, local, slot, PairedBy::Id);
    }

    // Disc and track number; no disc means the first medium.
    for (local, track) in album.tracks.iter().enumerate() {
        if found[local].is_some() {
            continue;
        }
        let Some(number) = track.number(Field::Track) else {
            continue;
        };
        let disc = track.number(Field::Disc).unwrap_or(1);
        let slot = slots.iter().position(|(m, _, slot)| {
            release.media[*m].position.max(1) == disc && slot.position == number
        });
        claim(&mut found, &mut taken, local, slot, PairedBy::Number);
    }

    // Numbers that run on across discs, on tracks without a disc number.
    for (local, track) in album.tracks.iter().enumerate() {
        if found[local].is_some() || track.number(Field::Disc).is_some() {
            continue;
        }
        let slot = track
            .number(Field::Track)
            .and_then(|number| usize::try_from(number).ok())
            .and_then(|number| number.checked_sub(1))
            .filter(|&index| index < slots.len());
        claim(&mut found, &mut taken, local, slot, PairedBy::RunningNumber);
    }

    // Order, only for an album without numbers and with as many tracks.
    if slots.len() == album.tracks.len()
        && album
            .tracks
            .iter()
            .all(|track| track.number(Field::Track).is_none())
    {
        for local in 0..album.tracks.len() {
            if found[local].is_none() {
                let slot = (0..slots.len()).find(|&slot| !taken[slot]);
                claim(&mut found, &mut taken, local, slot, PairedBy::Order);
            }
        }
    }

    let mut alignment = Alignment::default();
    for (local, track) in album.tracks.iter().enumerate() {
        match found[local] {
            Some((slot, by)) => {
                let (medium, index, release_track) = slots[slot];
                alignment.pairs.push(Pair {
                    track: track.id,
                    medium,
                    index,
                    by,
                    length_difference_ms: length_difference(track, release_track),
                });
            }
            None => alignment.unpaired.push(track.id),
        }
    }
    alignment.missing = slots
        .iter()
        .zip(&taken)
        .filter(|(_, taken)| !**taken)
        .map(|((medium, index, _), _)| (*medium, *index))
        .collect();
    alignment
}

fn length_difference(local: &LocalTrack, release: &Track) -> Option<i64> {
    let theirs = release.length.or(release.recording.length)?;
    let ours = local.duration_ms?;
    Some(i64::try_from(ours).ok()? - i64::try_from(theirs).ok()?)
}

/// A field the release would change, as shown before and after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub target: Target,
    pub field: Field,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

impl Suggestion {
    /// The edit that accepts this suggestion.
    #[must_use]
    pub fn change(&self) -> EditChange {
        EditChange {
            target: self.target,
            field: self.field,
            edit: Edit::Set(self.after.join("\0")),
        }
    }
}

/// What the release would change on the album and its paired tracks. Fields
/// that already agree, and values the library would refuse, are left out.
/// Genre, composer and album sort are not MusicBrainz's to suggest here.
#[must_use]
pub fn suggest(album: &LocalAlbum, release: &Release, alignment: &Alignment) -> Vec<Suggestion> {
    let mut suggestions = Vec::new();
    let credits = &release.artist_credit;
    let several_discs = release.media.len() > 1;
    let group = release.release_group.as_ref();
    let compilation = group.is_some_and(|group| {
        group
            .secondary_types
            .iter()
            .any(|kind| kind == "Compilation")
    }) || credits
        .iter()
        .any(|credit| credit.artist.id == VARIOUS_ARTISTS_ID);

    let mut out = Suggester {
        target: Target::Release(album.id),
        fields: &album.fields,
        suggestions: &mut suggestions,
    };
    out.one(Field::Album, &release.title);
    out.credit(Field::AlbumArtist, Field::AlbumArtistSort, credits);
    out.set(Field::Date, release.date.iter().cloned().collect());
    out.set(
        Field::OriginalDate,
        group
            .and_then(|group| group.first_release_date.clone())
            .into_iter()
            .collect(),
    );
    out.one_if(
        several_discs,
        Field::DiscTotal,
        &release.media.len().to_string(),
    );
    out.one_if(
        compilation,
        Field::Compilation,
        if compilation { "1" } else { "0" },
    );
    out.one(Field::ReleaseId, &release.id);
    out.set(
        Field::ReleaseGroupId,
        group.map(|group| group.id.clone()).into_iter().collect(),
    );
    out.set(Field::AlbumArtistId, credit_ids(credits));

    for pair in &alignment.pairs {
        let local = album.tracks.iter().find(|track| track.id == pair.track);
        let medium = release.media.get(pair.medium);
        let track = medium.and_then(|medium| medium.tracks.get(pair.index));
        let (Some(local), Some(medium), Some(track)) = (local, medium, track) else {
            continue;
        };
        let mut out = Suggester {
            target: Target::Track(local.id),
            fields: &local.fields,
            suggestions: &mut suggestions,
        };
        // A track credited like its release may carry no credit of its own.
        let credits = if track.artist_credit.is_empty() {
            credits
        } else {
            &track.artist_credit
        };
        out.one(Field::Title, &track.title);
        out.credit(Field::Artist, Field::ArtistSort, credits);
        out.one(Field::Track, &track.position.to_string());
        out.one(Field::TrackTotal, &medium.track_count.to_string());
        out.one_if(
            several_discs,
            Field::Disc,
            &medium.position.max(1).to_string(),
        );
        out.one(Field::RecordingId, &track.recording.id);
        out.one(Field::ReleaseTrackId, &track.id);
        out.set(Field::ArtistId, credit_ids(credits));
    }
    suggestions
}

/// Suggestions for one track or release.
struct Suggester<'a> {
    target: Target,
    fields: &'a [FieldState],
    suggestions: &'a mut Vec<Suggestion>,
}

impl Suggester<'_> {
    fn set(&mut self, field: Field, after: Vec<String>) {
        let before = shown(self.fields, field);
        let usable = !after.is_empty() && field.check(&after.join("\0")).is_ok();
        if usable && before != after.as_slice() {
            self.suggestions.push(Suggestion {
                target: self.target,
                field,
                before: before.to_vec(),
                after,
            });
        }
    }

    fn one(&mut self, field: Field, value: &str) {
        self.set(field, vec![value.to_owned()]);
    }

    /// Only where it says something, or to correct a value already shown:
    /// disc numbers on a one-disc album, or "not a compilation", are noise.
    fn one_if(&mut self, wanted: bool, field: Field, value: &str) {
        if wanted || !shown(self.fields, field).is_empty() {
            self.one(field, value);
        }
    }

    /// The credit as printed, and its sort form where that differs.
    fn credit(&mut self, name: Field, sort: Field, credits: &[crate::ArtistCredit]) {
        let text = credit_text(credits);
        let sorted = credit_sort(credits);
        self.one(name, &text);
        self.one_if(sorted != text, sort, &sorted);
    }
}
