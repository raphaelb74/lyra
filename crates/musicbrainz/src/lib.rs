//! Read-only client for MusicBrainz and the Cover Art Archive.
//!
//! Blocking, meant for a worker thread. Every MusicBrainz request goes through
//! one limiter shared by all callers, names Lyra in its User-Agent and asks for
//! JSON. No account, token or cookie is ever sent. [`matching`] turns a
//! release into suggested library edits; nothing here writes the library,
//! touches music files or the audio path.

mod client;
mod error;
pub mod matching;
mod types;

pub use client::{CONTACT, Client, Service};
pub use error::Error;
pub use types::{
    Artist, ArtistCredit, Label, LabelInfo, Medium, Recording, Release, ReleaseGroup, Track,
};

/// Whether `id` is a MusicBrainz ID: a lowercase UUID, as the API writes them.
/// Checked before an ID goes into a URL.
#[must_use]
pub fn is_mbid(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, &byte)| match i {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
        })
}

#[cfg(test)]
mod tests {
    use super::is_mbid;

    #[test]
    fn mbids_are_lowercase_uuids() {
        assert!(is_mbid("b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"));
        assert!(!is_mbid("B10BBBFC-CF9E-42E0-BE17-E2C3E1D2600D"));
        assert!(!is_mbid("b10bbbfc-cf9e-42e0-be17-e2c3e1d2600"));
        assert!(!is_mbid("b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d/x"));
        assert!(!is_mbid("../../release/b10bbbfc-cf9e-42e0-be1"));
        assert!(!is_mbid(""));
    }
}
