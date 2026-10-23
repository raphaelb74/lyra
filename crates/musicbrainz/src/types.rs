//! The parts of MusicBrainz JSON that Lyra reads. Unknown fields are ignored;
//! missing ones are empty, so a sparse entry still parses.

use serde::{Deserialize, Deserializer};

/// One edition of an album. Search results carry `score` and `track_count`;
/// a lookup carries `media` with tracks.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Release {
    pub id: String,
    pub title: String,
    /// How well a search result matches, 0–100.
    #[serde(deserialize_with = "score")]
    pub score: Option<u8>,
    pub status: Option<String>,
    /// Disambiguation, such as "remastered" or "Japanese edition".
    pub disambiguation: Option<String>,
    pub date: Option<String>,
    pub country: Option<String>,
    pub barcode: Option<String>,
    pub artist_credit: Vec<ArtistCredit>,
    pub release_group: Option<ReleaseGroup>,
    pub label_info: Vec<LabelInfo>,
    pub track_count: Option<u32>,
    pub media: Vec<Medium>,
}

/// One name in a credit, and the text joining it to the next (" feat. ", " & ").
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ArtistCredit {
    /// The name as credited, which may differ from the artist's own.
    pub name: String,
    pub joinphrase: String,
    pub artist: Artist,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub sort_name: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ReleaseGroup {
    pub id: String,
    pub primary_type: Option<String>,
    pub secondary_types: Vec<String>,
    pub first_release_date: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct LabelInfo {
    pub catalog_number: Option<String>,
    pub label: Option<Label>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Label {
    pub name: String,
}

/// A disc, side or file set.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Medium {
    pub position: u32,
    /// "CD", "Vinyl", "Digital Media", "SACD", …
    pub format: Option<String>,
    pub title: Option<String>,
    pub track_count: u32,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Track {
    /// The release track ID: this track on this release.
    pub id: String,
    /// As printed: "3", "A1", "B2".
    pub number: String,
    /// The track's place on its medium, from 1.
    pub position: u32,
    pub title: String,
    /// Milliseconds.
    pub length: Option<u64>,
    pub artist_credit: Vec<ArtistCredit>,
    pub recording: Recording,
}

/// The recorded performance, shared by every release that carries it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Recording {
    pub id: String,
    pub title: String,
    /// Milliseconds.
    pub length: Option<u64>,
}

/// A search page; only its releases matter.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct SearchPage {
    pub releases: Vec<Release>,
}

/// Scores have been sent as numbers and as strings; accept both.
fn score<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u8>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Score {
        Number(u8),
        Text(String),
    }
    Ok(match Option::<Score>::deserialize(deserializer)? {
        Some(Score::Number(score)) => Some(score),
        Some(Score::Text(text)) => text.parse().ok(),
        None => None,
    })
}
