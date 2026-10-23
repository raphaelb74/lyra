//! Finding the release for an album, pairing tracks, and suggested edits.
#![allow(clippy::unwrap_used, reason = "helpers fail loudly")]

mod common;

use common::{RELEASE, Server, json_reply, release_lookup, reply, search_page};
use library::{Edit, Field, FieldState, Target};
use musicbrainz::Release;
use musicbrainz::matching::{
    Alignment, LocalAlbum, LocalTrack, PairedBy, VARIOUS_ARTISTS_ID, align, align_by_hand,
    candidates, rank, search_query, suggest,
};
use serde_json::json;

fn states(fields: &[(Field, &str)]) -> Vec<FieldState> {
    fields
        .iter()
        .map(|&(field, value)| FieldState {
            field,
            values: value.split('\0').map(str::to_owned).collect(),
            edited: false,
            review: false,
        })
        .collect()
}

fn track(id: i64, disc: Option<u32>, number: Option<u32>, title: &str, ms: u64) -> LocalTrack {
    let disc = disc.map(|disc| disc.to_string());
    let number = number.map(|number| number.to_string());
    let mut fields = vec![(Field::Title, title)];
    if let Some(disc) = &disc {
        fields.push((Field::Disc, disc));
    }
    if let Some(number) = &number {
        fields.push((Field::Track, number));
    }
    LocalTrack {
        id,
        duration_ms: Some(ms),
        fields: states(&fields),
    }
}

fn album(fields: &[(Field, &str)], tracks: Vec<LocalTrack>) -> LocalAlbum {
    LocalAlbum {
        id: 7,
        fields: states(fields),
        tracks,
    }
}

fn release(value: &serde_json::Value) -> Release {
    serde_json::from_value(value.clone()).unwrap()
}

/// Two discs of two tracks, 200 s each, ids ending in disc and track.
fn two_discs() -> Release {
    let medium = |disc: u32| {
        let tracks: Vec<_> = (1..=2)
            .map(|n| {
                json!({
                    "id": format!("00000000-0000-4000-8000-0000000000{disc}{n}"),
                    "number": n.to_string(), "position": n,
                    "title": format!("Song {disc}.{n}"), "length": 200_000,
                    "recording": { "id": format!("10000000-0000-4000-8000-0000000000{disc}{n}") }
                })
            })
            .collect();
        json!({ "position": disc, "format": "CD", "track-count": 2, "tracks": tracks })
    };
    release(&json!({
        "id": RELEASE, "title": "Two Discs",
        "artist-credit": [{ "name": "Band", "joinphrase": "", "artist": { "id": "20000000-0000-4000-8000-000000000001", "name": "Band", "sort-name": "Band" } }],
        "media": [medium(1), medium(2)]
    }))
}

#[test]
fn queries_escape_and_lower_case() {
    assert_eq!(
        search_query("AC/DC: Live (1992) AND More!", Some("AC/DC")).as_deref(),
        Some(r"release:(ac\/dc\: live \(1992\) and more\!) AND artist:(ac\/dc)")
    );
    assert_eq!(
        search_query("Blue", None).as_deref(),
        Some("release:(blue)")
    );
    assert_eq!(search_query("  ", Some("X")), None);
}

#[test]
fn ranking_puts_what_agrees_first() {
    let local = album(
        &[
            (Field::Album, "kind of blue"),
            (Field::AlbumArtist, "Miles Davis"),
            (Field::Date, "1959"),
        ],
        (1..=5)
            .map(|n| track(n, None, u32::try_from(n).ok(), "x", 1))
            .collect(),
    );
    let page: Vec<Release> = serde_json::from_value(search_page()["releases"].clone()).unwrap();
    // The first result has five tracks, the second none listed.
    let ranked = rank(&local, page.into_iter().rev().collect());
    assert_eq!(ranked[0].release.id, RELEASE);
    let evidence = ranked[0].evidence;
    assert!(evidence.track_count && evidence.title && evidence.artist && evidence.year);
    assert!(!evidence.release_id);
    assert!(!ranked[1].evidence.track_count);
}

#[test]
fn a_tagged_release_ranks_first() {
    let local = album(
        &[
            (Field::Album, "Other"),
            (Field::ReleaseId, "0bbbcd4b-4e7a-4b5a-8d1b-1b0f2d1e3c4a"),
        ],
        Vec::new(),
    );
    let page: Vec<Release> = serde_json::from_value(search_page()["releases"].clone()).unwrap();
    let ranked = rank(&local, page);
    assert_eq!(ranked[0].release.id, "0bbbcd4b-4e7a-4b5a-8d1b-1b0f2d1e3c4a");
    assert!(ranked[0].evidence.release_id);
}

#[test]
fn tracks_pair_by_disc_and_number() {
    let local = album(
        &[],
        vec![
            track(1, Some(1), Some(1), "a", 200_000),
            track(2, Some(1), Some(2), "b", 230_000),
            track(3, Some(2), Some(2), "d", 200_000),
            track(4, Some(3), Some(1), "extra", 100),
        ],
    );
    let alignment = align(&local, &two_discs());
    let places: Vec<_> = alignment
        .pairs
        .iter()
        .map(|pair| (pair.track, pair.medium, pair.index, pair.by))
        .collect();
    assert_eq!(
        places,
        [
            (1, 0, 0, PairedBy::Number),
            (2, 0, 1, PairedBy::Number),
            (3, 1, 1, PairedBy::Number)
        ]
    );
    assert_eq!(alignment.unpaired, [4]);
    assert_eq!(alignment.missing, [(1, 0)]);
    assert_eq!(alignment.pairs[0].lengths_agree(), Some(true));
    assert_eq!(alignment.pairs[1].length_difference_ms, Some(30_000));
    assert_eq!(alignment.pairs[1].lengths_agree(), Some(false));
}

#[test]
fn numbers_may_run_on_across_discs() {
    let local = album(
        &[],
        (1..=4)
            .map(|n| track(n, None, u32::try_from(n).ok(), "x", 200_000))
            .collect(),
    );
    let alignment = align(&local, &two_discs());
    let places: Vec<_> = alignment
        .pairs
        .iter()
        .map(|pair| (pair.track, pair.medium, pair.index, pair.by))
        .collect();
    assert_eq!(
        places,
        [
            (1, 0, 0, PairedBy::Number),
            (2, 0, 1, PairedBy::Number),
            (3, 1, 0, PairedBy::RunningNumber),
            (4, 1, 1, PairedBy::RunningNumber)
        ]
    );
    assert!(alignment.unpaired.is_empty() && alignment.missing.is_empty());
}

#[test]
fn ids_win_over_numbers() {
    let mut tagged = track(1, Some(1), Some(1), "a", 200_000);
    tagged.fields.push(FieldState {
        field: Field::ReleaseTrackId,
        values: vec!["00000000-0000-4000-8000-000000000022".to_owned()],
        edited: false,
        review: false,
    });
    let local = album(&[], vec![tagged, track(2, Some(2), Some(2), "b", 200_000)]);
    let alignment = align(&local, &two_discs());
    assert_eq!(
        (
            alignment.pairs[0].medium,
            alignment.pairs[0].index,
            alignment.pairs[0].by
        ),
        (1, 1, PairedBy::Id)
    );
    // Its number's slot is free, but disc 2 track 2 is taken by the ID.
    assert_eq!(alignment.unpaired, [2]);
}

#[test]
fn without_numbers_order_pairs_equal_counts_only() {
    let unnumbered = |count: i64| {
        album(
            &[],
            (1..=count)
                .map(|n| track(n, None, None, "x", 200_000))
                .collect(),
        )
    };
    let alignment = align(&unnumbered(4), &two_discs());
    assert!(
        alignment
            .pairs
            .iter()
            .all(|pair| pair.by == PairedBy::Order)
    );
    assert_eq!(alignment.pairs[3].medium, 1);
    let alignment = align(&unnumbered(3), &two_discs());
    assert_eq!(alignment.pairs.len(), 0);
    assert_eq!(alignment.unpaired, [1, 2, 3]);
}

#[test]
fn suggestions_cover_what_differs_and_nothing_else() {
    let local = album(
        &[
            (Field::Album, "Kind Of Blue"),
            (Field::AlbumArtist, "Miles Davis"),
            (Field::Date, "1997"),
            (Field::Genre, "Jazz"),
        ],
        vec![
            track(1, None, Some(1), "So What", 562_000),
            track(
                2,
                None,
                Some(2),
                "Freddie Freeloader (feat. Wynton Kelly)",
                589_000,
            ),
        ],
    );
    let release = release(&release_lookup());
    let alignment = align(&local, &release);
    let suggestions = suggest(&local, &release, &alignment);
    let find = |target: Target, field: Field| {
        suggestions
            .iter()
            .find(|s| s.target == target && s.field == field)
            .map(|s| (s.before.clone(), s.after.clone()))
    };
    let album = Target::Release(7);

    assert_eq!(
        find(album, Field::Album),
        Some((
            vec!["Kind Of Blue".to_owned()],
            vec!["Kind of Blue".to_owned()]
        ))
    );
    // Same artist and date: nothing to suggest. Genre is never suggested.
    for field in [
        Field::AlbumArtist,
        Field::Date,
        Field::Genre,
        Field::Composer,
    ] {
        assert_eq!(find(album, field), None, "{field:?}");
    }
    // The sample's sort name equals the name, and the album has none.
    assert_eq!(find(album, Field::AlbumArtistSort), None);
    assert_eq!(find(album, Field::OriginalDate).unwrap().1, ["1959-08-17"]);
    assert_eq!(find(album, Field::ReleaseId).unwrap().1, [RELEASE]);
    // Two media in the sample, the second empty: a disc total is suggested.
    assert_eq!(find(album, Field::DiscTotal).unwrap().1, ["2"]);
    assert_eq!(find(album, Field::Compilation), None);

    let second = Target::Track(2);
    assert_eq!(
        find(second, Field::Title).unwrap().1,
        ["Freddie Freeloader"]
    );
    assert_eq!(
        find(second, Field::Artist).unwrap().1,
        ["Miles Davis feat. Wynton Kelly"]
    );
    // Both credits share one artist ID in the sample; it is listed once.
    assert_eq!(find(second, Field::ArtistId).unwrap().1.len(), 1);
    assert_eq!(find(second, Field::TrackTotal).unwrap().1, ["2"]);
    assert_eq!(find(second, Field::Disc).unwrap().1, ["1"]);
    assert_eq!(find(Target::Track(1), Field::Title), None);

    let change = suggestions
        .iter()
        .find(|s| s.target == second && s.field == Field::RecordingId)
        .unwrap()
        .change();
    assert_eq!(
        change.edit,
        Edit::Set("21111111-1111-4111-8111-111111111112".to_owned())
    );
}

#[test]
fn values_the_library_refuses_are_left_out() {
    let local = album(
        &[(Field::Album, "x")],
        vec![track(1, None, Some(1), "a", 1)],
    );
    let release = release(&json!({
        "id": RELEASE, "title": "", "date": "1959-13-45",
        "release-group": { "id": "not-an-id", "secondary-types": ["Compilation"] },
        "artist-credit": [{ "name": "Various Artists", "joinphrase": "", "artist": { "id": VARIOUS_ARTISTS_ID, "name": "Various Artists", "sort-name": "Various Artists" } }],
        "media": [{ "position": 1, "track-count": 1, "tracks": [
            { "id": "bad", "number": "1", "position": 1, "title": "Line\nbreak", "recording": { "id": "" } }
        ] }]
    }));
    let suggestions = suggest(&local, &release, &align(&local, &release));
    let fields: Vec<_> = suggestions.iter().map(|s| (s.target, s.field)).collect();
    for refused in [
        (Target::Release(7), Field::Album),
        (Target::Release(7), Field::Date),
        (Target::Release(7), Field::ReleaseGroupId),
        (Target::Track(1), Field::Title),
        (Target::Track(1), Field::ReleaseTrackId),
        (Target::Track(1), Field::RecordingId),
    ] {
        assert!(!fields.contains(&refused), "{refused:?}");
    }
    assert!(fields.contains(&(Target::Release(7), Field::Compilation)));
    assert!(fields.contains(&(Target::Release(7), Field::ReleaseId)));
}

#[test]
fn sort_names_appear_where_they_differ() {
    let local = album(&[(Field::Album, "Kind of Blue")], Vec::new());
    let mut release = release(&release_lookup());
    release.artist_credit[0].artist.sort_name = "Davis, Miles".to_owned();
    let suggestions = suggest(&local, &release, &Alignment::default());
    let sort = suggestions
        .iter()
        .find(|s| s.field == Field::AlbumArtistSort)
        .unwrap();
    assert_eq!(sort.after, ["Davis, Miles"]);
}

#[test]
fn a_tagged_release_is_looked_up_and_a_stale_one_searched() {
    let tagged = album(
        &[(Field::Album, "Kind of Blue"), (Field::ReleaseId, RELEASE)],
        Vec::new(),
    );
    let server = Server::start(vec![json_reply(&release_lookup())]);
    let found = candidates(&server.client(), &tagged).unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].evidence.release_id);
    assert!(server.requests()[0].starts_with(&format!("GET /ws/2/release/{RELEASE}?")));

    let server = Server::start(vec![
        reply("404 Not Found", &[], b""),
        json_reply(&search_page()),
    ]);
    let found = candidates(&server.client(), &tagged).unwrap();
    assert_eq!(found.len(), 2);
    assert!(server.requests()[1].starts_with("GET /ws/2/release/?"));
}

#[test]
fn an_album_without_a_title_asks_nothing() {
    let server = Server::start(Vec::new());
    let found = candidates(&server.client(), &album(&[], Vec::new())).unwrap();
    assert!(found.is_empty());
    assert_eq!(server.requests().len(), 0);
}

#[test]
fn pairs_by_hand_are_checked() {
    let local = album(
        &[],
        vec![
            track(1, None, Some(1), "a", 200_000),
            track(2, None, Some(2), "b", 200_000),
        ],
    );
    let release = two_discs();
    let alignment = align_by_hand(&local, &release, &[(2, 1, 0)]).unwrap();
    assert_eq!(alignment.pairs.len(), 1);
    assert_eq!(
        (
            alignment.pairs[0].track,
            alignment.pairs[0].medium,
            alignment.pairs[0].by
        ),
        (2, 1, PairedBy::Hand)
    );
    assert_eq!(alignment.unpaired, [1]);
    assert_eq!(alignment.missing.len(), 3);

    for wrong in [
        vec![(9, 0, 0)],
        vec![(1, 5, 0)],
        vec![(1, 0, 9)],
        vec![(1, 0, 0), (2, 0, 0)],
        vec![(1, 0, 0), (1, 0, 1)],
    ] {
        assert_eq!(align_by_hand(&local, &release, &wrong), None, "{wrong:?}");
    }
}
