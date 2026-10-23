//! Metadata fields, the values files give them, and how a value is read for
//! display.
//!
//! File values are kept exactly as the file has them; only what the library
//! shows is trimmed and capped.

use id3::TagLike;

/// A metadata field Lyra knows. Stored by [`Field::name`]: rename nothing,
/// only add.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Field {
    Title,
    Artist,
    Album,
    AlbumArtist,
    Track,
    TrackTotal,
    Disc,
    DiscTotal,
    /// Release date: `1973`, `1973-04` or `1973-04-13`.
    Date,
    OriginalDate,
    Genre,
    Composer,
    /// `1` for a compilation, `0` for not.
    Compilation,
    TitleSort,
    ArtistSort,
    AlbumSort,
    AlbumArtistSort,
    /// MusicBrainz release (one edition of an album).
    ReleaseId,
    ReleaseGroupId,
    RecordingId,
    ReleaseTrackId,
    ArtistId,
    AlbumArtistId,
}

/// What a field describes: one track, or the release all its tracks share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Track,
    Release,
}

/// How a field's value is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Text,
    /// A positive whole number.
    Number,
    Date,
    Flag,
}

impl Field {
    pub const ALL: [Field; 23] = [
        Field::Title,
        Field::Artist,
        Field::Album,
        Field::AlbumArtist,
        Field::Track,
        Field::TrackTotal,
        Field::Disc,
        Field::DiscTotal,
        Field::Date,
        Field::OriginalDate,
        Field::Genre,
        Field::Composer,
        Field::Compilation,
        Field::TitleSort,
        Field::ArtistSort,
        Field::AlbumSort,
        Field::AlbumArtistSort,
        Field::ReleaseId,
        Field::ReleaseGroupId,
        Field::RecordingId,
        Field::ReleaseTrackId,
        Field::ArtistId,
        Field::AlbumArtistId,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Field::Title => "title",
            Field::Artist => "artist",
            Field::Album => "album",
            Field::AlbumArtist => "album_artist",
            Field::Track => "track",
            Field::TrackTotal => "track_total",
            Field::Disc => "disc",
            Field::DiscTotal => "disc_total",
            Field::Date => "date",
            Field::OriginalDate => "original_date",
            Field::Genre => "genre",
            Field::Composer => "composer",
            Field::Compilation => "compilation",
            Field::TitleSort => "title_sort",
            Field::ArtistSort => "artist_sort",
            Field::AlbumSort => "album_sort",
            Field::AlbumArtistSort => "album_artist_sort",
            Field::ReleaseId => "musicbrainz_release_id",
            Field::ReleaseGroupId => "musicbrainz_release_group_id",
            Field::RecordingId => "musicbrainz_recording_id",
            Field::ReleaseTrackId => "musicbrainz_release_track_id",
            Field::ArtistId => "musicbrainz_artist_id",
            Field::AlbumArtistId => "musicbrainz_album_artist_id",
        }
    }

    pub fn from_name(name: &str) -> Option<Field> {
        Field::ALL.into_iter().find(|field| field.name() == name)
    }

    pub fn scope(self) -> Scope {
        match self {
            Field::Album
            | Field::AlbumArtist
            | Field::DiscTotal
            | Field::Date
            | Field::OriginalDate
            | Field::Compilation
            | Field::AlbumSort
            | Field::AlbumArtistSort
            | Field::ReleaseId
            | Field::ReleaseGroupId
            | Field::AlbumArtistId => Scope::Release,
            _ => Scope::Track,
        }
    }

    pub(crate) fn kind(self) -> Kind {
        match self {
            Field::Track | Field::TrackTotal | Field::Disc | Field::DiscTotal => Kind::Number,
            Field::Date | Field::OriginalDate => Kind::Date,
            Field::Compilation => Kind::Flag,
            _ => Kind::Text,
        }
    }
}

/// Longest file value stored. Longer ones keep their start and are marked
/// `cut`, so nothing can write the shortened value back as if it were whole.
pub(crate) const MAX_VALUE_BYTES: usize = 64 * 1024;
/// Longest text the library shows, in bytes.
const MAX_SHOWN_BYTES: usize = 1024;

/// One value a file gives a field, exactly as the file has it. A field may
/// have several, in file order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileValue {
    pub field: Field,
    pub value: String,
    /// Longer than the library stores: only the start is kept.
    pub cut: bool,
}

impl FileValue {
    pub(crate) fn new(field: Field, value: &str) -> Self {
        let cut = value.len() > MAX_VALUE_BYTES;
        FileValue {
            field,
            value: prefix(value, MAX_VALUE_BYTES).to_owned(),
            cut,
        }
    }
}

/// The first value a file gives `field`.
pub(crate) fn first(values: &[FileValue], field: Field) -> Option<&str> {
    values
        .iter()
        .find(|value| value.field == field)
        .map(|value| value.value.as_str())
}

/// What the library shows for `field` from the file values alone, or `None`
/// when the file has nothing usable.
pub(crate) fn file_shown(values: &[FileValue], field: Field) -> Option<String> {
    let value = first(values, field);
    match field {
        Field::Track | Field::Disc => value.and_then(number).map(|n| n.to_string()),
        Field::TrackTotal | Field::DiscTotal => {
            let part = if field == Field::TrackTotal {
                Field::Track
            } else {
                Field::Disc
            };
            value
                .and_then(number)
                .or_else(|| first(values, part).and_then(total))
                .map(|n| n.to_string())
        }
        Field::Genre => value.and_then(genre),
        Field::Compilation => value
            .and_then(flag)
            .map(|on| if on { "1" } else { "0" }.to_owned()),
        _ => value.and_then(text),
    }
}

/// Trimmed of spaces and NULs, non-empty, capped for display on a character
/// boundary.
pub(crate) fn text(value: &str) -> Option<String> {
    let value = value.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    (!value.is_empty()).then(|| prefix(value, MAX_SHOWN_BYTES).to_owned())
}

/// `3`, `03`, ` 3 ` and `3/12` → 3. Zero is no number.
pub(crate) fn number(value: &str) -> Option<u32> {
    let value = value.trim();
    let part = value.split_once('/').map_or(value, |(part, _)| part).trim();
    part.parse().ok().filter(|&n| n > 0)
}

/// `3/12` → 12.
pub(crate) fn total(value: &str) -> Option<u32> {
    let (_, total) = value.split_once('/')?;
    total.trim().parse().ok().filter(|&n| n > 0)
}

/// `1973`, `1973-04-13` → 1973.
pub(crate) fn year(value: &str) -> Option<i32> {
    let digits = value.trim().get(..4)?;
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// `1`, `true`, `yes` and their opposites, in any case.
pub(crate) fn flag(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "y" => Some(true),
        "0" | "false" | "no" | "n" => Some(false),
        _ => None,
    }
}

/// A genre as text; ID3 version 1 genre numbers (`17`, `(17)`) become
/// their names.
pub(crate) fn genre(value: &str) -> Option<String> {
    let value = text(value)?;
    let numeric = value.starts_with('(') || value.bytes().all(|byte| byte.is_ascii_digit());
    if !numeric {
        return Some(value);
    }
    let mut tag = id3::Tag::new();
    tag.set_genre(value.as_str());
    tag.genre_parsed()
        .and_then(|name| text(&name))
        .or(Some(value))
}

/// Whether `value` is well formed for `field`: what an edit may set.
pub(crate) fn is_valid(field: Field, value: &str) -> bool {
    if value.trim().is_empty() || value.chars().any(|c| c.is_control() && c != '\0') {
        return false;
    }
    match field.kind() {
        Kind::Text => true,
        Kind::Number => value.parse::<u32>().is_ok_and(|n| n > 0),
        Kind::Flag => matches!(value, "0" | "1"),
        Kind::Date => {
            let parts: Vec<&str> = value.split('-').collect();
            let sizes_ok = matches!(parts.as_slice(), [y] | [y, _] | [y, _, _] if y.len() == 4)
                && parts[1..].iter().all(|part| part.len() == 2);
            sizes_ok
                && parts
                    .iter()
                    .all(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
        }
    }
}

/// The longest start of `value` within `bytes`, on a character boundary.
fn prefix(value: &str, bytes: usize) -> &str {
    let mut end = value.len().min(bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::{Field, FileValue, file_shown, genre, is_valid, number, text, year};

    #[test]
    fn names_are_unique_and_round_trip() {
        for field in Field::ALL {
            assert_eq!(Field::from_name(field.name()), Some(field));
        }
        let mut names: Vec<_> = Field::ALL.iter().map(|field| field.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Field::ALL.len());
    }

    #[test]
    fn file_values_are_read_for_display() {
        assert_eq!(text("  Aladdin Sane\0"), Some("Aladdin Sane".into()));
        assert_eq!(text(" \0 "), None);
        let long = "\u{e9}".repeat(600); // 1200 bytes
        assert_eq!(text(&long).map(|value| value.len()), Some(1024));
        assert_eq!(
            (number(" 03 "), number("3/12"), number("0")),
            (Some(3), Some(3), None)
        );
        assert_eq!((year("1973-04-13"), year("73")), (Some(1973), None));
        assert_eq!(genre("(17)").as_deref(), Some("Rock"));
        assert_eq!(genre("Trip Hop").as_deref(), Some("Trip Hop"));

        let values = [FileValue::new(Field::Track, "3/12")];
        assert_eq!(file_shown(&values, Field::Track).as_deref(), Some("3"));
        assert_eq!(
            file_shown(&values, Field::TrackTotal).as_deref(),
            Some("12")
        );

        let huge = FileValue::new(Field::Title, &"a".repeat(70_000));
        assert!(huge.cut);
        assert_eq!(huge.value.len(), super::MAX_VALUE_BYTES);
    }

    #[test]
    fn edits_must_fit_their_field() {
        assert!(is_valid(Field::Title, "Sour Times"));
        assert!(is_valid(Field::Artist, "Air\0Beck"), "NUL separates values");
        assert!(!is_valid(Field::Title, "  "));
        assert!(!is_valid(Field::Title, "line\nbreak"));
        assert!(is_valid(Field::Track, "7"));
        assert!(!is_valid(Field::Track, "7/12"));
        assert!(!is_valid(Field::Track, "0"));
        for date in ["1994", "1994-08", "1994-08-22"] {
            assert!(is_valid(Field::Date, date), "{date}");
        }
        for date in ["94", "1994-8", "22.08.1994", "1994-08-22-01"] {
            assert!(!is_valid(Field::Date, date), "{date}");
        }
        assert!(is_valid(Field::Compilation, "1"));
        assert!(!is_valid(Field::Compilation, "yes"));
    }
}
