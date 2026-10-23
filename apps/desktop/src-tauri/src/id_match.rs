//! ID Match commands: adapters from IPC to `musicbrainz::matching` and the
//! library. Every command refuses while online lookups are off. Values saved
//! as MusicBrainz edits are computed here from MusicBrainz's answer, never
//! taken from the webview: it only says which suggestions it accepts.

// Tauri injects `State` and arguments by value.
#![expect(clippy::needless_pass_by_value, reason = "Tauri command signature")]

use std::collections::HashSet;

use library::{EditChange, Field, Origin, Revisions, Target, image_type};
use musicbrainz::matching::{
    self, Alignment, Candidate, LocalAlbum, PROVIDER, PairedBy, Suggestion, align, align_by_hand,
};
use musicbrainz::{COVER_ART_ARCHIVE, Client, Release};
use tauri::{AppHandle, Emitter, State};

use crate::commands::{AppState, LIBRARY_CHANGED_EVENT, locked, query};
use crate::ipc::{
    CandidateInfo, FieldRef, IpcError, LocalTrackInfo, MatchInfo, MediumInfo, PairIn, PairInfo,
    ReleaseTrackInfo, RevisionInfo, SlotRef, SuggestionInfo, TargetRef, ms_to_seconds,
};

const RELEASE_PAGE: &str = "https://musicbrainz.org/release/";
/// Longest title or artist a search takes.
const MAX_SEARCH_BYTES: usize = 500;

/// Editions that may be the album, best first. With a `title`, those found
/// for what the listener typed; without, the release the album is tagged
/// with, else a search by its title and artist.
#[tauri::command(async)]
pub fn match_candidates(
    album_id: i64,
    title: Option<String>,
    artist: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<CandidateInfo>, IpcError> {
    let client = online(&state)?;
    let (album, _) = read_album(&state, album_id)?;
    let typed = |text: Option<String>| text.filter(|text| !text.trim().is_empty());
    let candidates = match typed(title) {
        Some(title) => {
            let artist = typed(artist);
            if title.len() > MAX_SEARCH_BYTES
                || artist.as_ref().is_some_and(|a| a.len() > MAX_SEARCH_BYTES)
            {
                return Err(IpcError::invalid_request("That search is too long."));
            }
            matching::search(client, &album, &title, artist.as_deref())
        }
        None => matching::candidates(client, &album),
    }
    .map_err(online_error)?;
    Ok(candidates.into_iter().map(CandidateInfo::from).collect())
}

/// One edition compared with the album: tracks paired automatically, or as
/// the listener chose (`pairs`), and the fields it would change.
#[tauri::command(async)]
pub fn match_release(
    album_id: i64,
    release_id: String,
    pairs: Option<Vec<PairIn>>,
    state: State<'_, AppState>,
) -> Result<MatchInfo, IpcError> {
    let client = online(&state)?;
    // Revisions are read with the album, before anything goes online, so a
    // save is refused if the album changed while this was on screen.
    let (album, revisions) = read_album(&state, album_id)?;
    let release = client.release(&release_id).map_err(online_error)?;
    let alignment = pairing(&album, &release, pairs.as_deref())?;
    let suggestions = matching::suggest(&album, &release, &alignment);
    let changes: Vec<EditChange> = suggestions.iter().map(Suggestion::change).collect();
    let held = query(&state, |library| {
        Ok(library.preview_edits(&changes, &provider())?.held)
    })?;
    let kept: HashSet<(Target, Field)> = held
        .into_iter()
        .map(|(change, _)| (change.target, change.field))
        .collect();
    Ok(match_info(
        album,
        release,
        &alignment,
        suggestions,
        &kept,
        revisions,
    ))
}

/// The edition's front cover as a `data:` URL for the comparison, or `None`
/// when it has none. Kept in memory, so saving it does not fetch it again.
#[tauri::command(async)]
pub fn match_cover(
    release_id: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, IpcError> {
    let bytes = cover(&state, &release_id)?;
    Ok(bytes.and_then(|bytes| Some(data_url(image_type(&bytes)?, &bytes))))
}

/// Saves the accepted suggestions as MusicBrainz edits, one undoable step,
/// and the edition's cover when `cover` is set. Refused, saving nothing,
/// when the album changed since `revisions` were read.
#[tauri::command(async)]
#[expect(clippy::too_many_arguments, reason = "Tauri command signature")]
pub fn apply_match(
    album_id: i64,
    release_id: String,
    pairs: Option<Vec<PairIn>>,
    accepted: Vec<FieldRef>,
    revisions: Vec<RevisionInfo>,
    cover: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), IpcError> {
    let client = online(&state)?;
    let accepted = accepted
        .into_iter()
        .map(|item| {
            let field = Field::from_name(&item.field)
                .ok_or_else(|| IpcError::invalid_request("Unknown field."))?;
            Ok((Target::from(item.target), field))
        })
        .collect::<Result<HashSet<_>, IpcError>>()?;
    let (album, _) = read_album(&state, album_id)?;
    // Answered from the client's memory: the comparison asked for it.
    let release = client.release(&release_id).map_err(online_error)?;
    let alignment = pairing(&album, &release, pairs.as_deref())?;
    let changes: Vec<EditChange> = matching::suggest(&album, &release, &alignment)
        .iter()
        .filter(|suggestion| accepted.contains(&(suggestion.target, suggestion.field)))
        .map(Suggestion::change)
        .collect();
    if changes.len() != accepted.len() {
        // A ticked field the release no longer changes: the album moved on.
        return Err(changed_meanwhile());
    }
    let picture = if cover {
        Some(self::cover(&state, &release_id)?.ok_or_else(|| {
            IpcError::invalid_request("This edition has no cover in the Cover Art Archive.")
        })?)
    } else {
        None
    };
    let seen: Revisions = revisions
        .into_iter()
        .map(|info| (Target::from(info.target), info.revision))
        .collect();
    query(&state, |library| {
        if !changes.is_empty() {
            library.commit_edits(&changes, &provider(), &seen)?;
        }
        if let Some(picture) = picture {
            library.set_artwork(&state.art_dir, album_id, picture, Some(COVER_ART_ARCHIVE))?;
        }
        Ok(())
    })?;
    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());
    Ok(())
}

fn online(state: &AppState) -> Result<&Client, IpcError> {
    if state.settings().online_lookups {
        Ok(&state.musicbrainz)
    } else {
        Err(IpcError {
            code: "online_off",
            message: "Online lookups are off. Turn them on in Settings to use ID Match.".to_owned(),
        })
    }
}

fn online_error(error: musicbrainz::Error) -> IpcError {
    IpcError {
        code: match error {
            musicbrainz::Error::NotFound => "not_found",
            musicbrainz::Error::BadId => "invalid_request",
            _ => "online_failed",
        },
        message: error.to_string(),
    }
}

fn changed_meanwhile() -> IpcError {
    IpcError {
        code: "changed_meanwhile",
        message: "The album changed in the meantime; nothing was saved.".to_owned(),
    }
}

fn provider() -> Origin {
    Origin::Provider(PROVIDER.to_owned())
}

/// The album as shown, and the revisions of it and its tracks.
fn read_album(state: &AppState, album_id: i64) -> Result<(LocalAlbum, Revisions), IpcError> {
    query(state, |library| {
        let Some(album) = LocalAlbum::read(library, album_id)? else {
            return Ok(None);
        };
        let targets: Vec<Target> = std::iter::once(Target::Release(album_id))
            .chain(album.tracks.iter().map(|track| Target::Track(track.id)))
            .collect();
        let revisions = library.revisions(&targets)?;
        Ok(Some((album, revisions)))
    })?
    .ok_or_else(|| IpcError::invalid_request("That album is no longer in the library."))
}

fn pairing(
    album: &LocalAlbum,
    release: &Release,
    pairs: Option<&[PairIn]>,
) -> Result<Alignment, IpcError> {
    match pairs {
        None => Ok(align(album, release)),
        Some(pairs) => {
            let chosen: Vec<_> = pairs
                .iter()
                .map(|pair| (pair.track, pair.slot.medium, pair.slot.index))
                .collect();
            align_by_hand(album, release, &chosen).ok_or_else(|| {
                IpcError::invalid_request("Those track pairs do not fit this edition.")
            })
        }
    }
}

/// The edition's front cover: the one already fetched, else from the
/// archive. Only the last one is kept.
fn cover(state: &AppState, release_id: &str) -> Result<Option<Vec<u8>>, IpcError> {
    let client = online(state)?;
    if let Some((id, bytes)) = locked(&state.match_cover).as_ref()
        && id == release_id
    {
        return Ok(Some(bytes.clone()));
    }
    let bytes = client.front_cover(release_id).map_err(online_error)?;
    if let Some(bytes) = &bytes {
        *locked(&state.match_cover) = Some((release_id.to_owned(), bytes.clone()));
    }
    Ok(bytes)
}

fn match_info(
    album: LocalAlbum,
    release: Release,
    alignment: &Alignment,
    suggestions: Vec<Suggestion>,
    kept: &HashSet<(Target, Field)>,
    revisions: Revisions,
) -> MatchInfo {
    let shown = |fields: &[library::FieldState], field: Field| {
        fields
            .iter()
            .find(|state| state.field == field)
            .and_then(|state| state.values.first().cloned())
    };
    let seconds = |ms: Option<u64>| ms.map(ms_to_seconds);
    let media = release
        .media
        .iter()
        .map(|medium| MediumInfo {
            position: medium.position,
            format: medium.format.clone(),
            title: medium.title.clone().filter(|title| !title.is_empty()),
            tracks: medium
                .tracks
                .iter()
                .map(|track| ReleaseTrackInfo {
                    number: track.number.clone(),
                    title: track.title.clone(),
                    seconds: seconds(track.length.or(track.recording.length)),
                })
                .collect(),
        })
        .collect();
    let tracks = album
        .tracks
        .iter()
        .map(|track| LocalTrackInfo {
            id: track.id,
            disc: shown(&track.fields, Field::Disc),
            number: shown(&track.fields, Field::Track),
            title: shown(&track.fields, Field::Title),
            seconds: seconds(track.duration_ms),
        })
        .collect();
    let pairs = alignment
        .pairs
        .iter()
        .map(|pair| PairInfo {
            track: pair.track,
            slot: SlotRef {
                medium: pair.medium,
                index: pair.index,
            },
            by: match pair.by {
                PairedBy::Id => "id",
                PairedBy::Number => "number",
                PairedBy::RunningNumber => "runningNumber",
                PairedBy::Order => "order",
                PairedBy::Hand => "hand",
            },
            length_difference_seconds: pair.length_difference_ms.map(|ms| {
                let sign = if ms < 0 { -1.0 } else { 1.0 };
                sign * ms_to_seconds(ms.unsigned_abs())
            }),
            lengths_agree: pair.lengths_agree(),
        })
        .collect();
    let suggestions = suggestions
        .into_iter()
        .map(|suggestion| SuggestionInfo {
            kept: kept.contains(&(suggestion.target, suggestion.field)),
            target: TargetRef::from(suggestion.target),
            field: suggestion.field.name(),
            field_label: suggestion.field.label(),
            before: suggestion.before,
            after: suggestion.after,
        })
        .collect();
    let url = format!("{RELEASE_PAGE}{}", release.id);
    let release = CandidateInfo::from(Candidate {
        evidence: matching::evidence(&album, &release),
        release,
    });
    MatchInfo {
        release,
        url,
        media,
        tracks,
        pairs,
        unpaired: alignment.unpaired.clone(),
        missing: alignment
            .missing
            .iter()
            .map(|&(medium, index)| SlotRef { medium, index })
            .collect(),
        suggestions,
        revisions: revisions
            .iter()
            .map(|(target, revision)| RevisionInfo {
                target: target.into(),
                revision,
            })
            .collect(),
    }
}

/// A `data:` URL: the cover preview crosses IPC as text, and the webview
/// never fetches anything itself.
fn data_url(mime: &str, bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut url = String::with_capacity(bytes.len() * 4 / 3 + mime.len() + 16);
    url.push_str("data:");
    url.push_str(mime);
    url.push_str(";base64,");
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                url.push(char::from(ALPHABET[(n >> shift) as usize & 63]));
            } else {
                url.push('=');
            }
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::data_url;

    #[test]
    fn data_urls_are_base64() {
        for (bytes, encoded) in [
            (&b""[..], ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"\xff\xd8\xff", "/9j/"),
        ] {
            assert_eq!(
                data_url("image/jpeg", bytes),
                format!("data:image/jpeg;base64,{encoded}")
            );
        }
    }
}
