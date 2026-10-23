//! Cleanup rules on a corpus of badly tagged files: what they propose, what
//! they must leave alone, and accepting their proposals as edits.
#![expect(clippy::unwrap_used, reason = "corpus helpers fail loudly")]

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use library::{Field, Library, Origin, Proposal, Rule, Target};

use common::{Tags, dsf, flac_with_comments, scanned_library, temp_dir};

fn flac(path: &Path, comments: &[&[u8]]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, flac_with_comments(comments)).unwrap();
}

/// What careless taggers leave behind.
fn messy(root: &Path) {
    let floyd = root.join("Floyd");
    flac(
        &floyd.join("01.flac"),
        &[
            b"TITLE=  SPEAK TO ME ",
            b"ARTIST=PINK FLOYD",
            b"ALBUM=THE DARK SIDE OF THE MOON",
            b"TRACKNUMBER=1/10",
            b"DATE=01.03.1973",
            b"GENRE=(17)",
        ],
    );
    flac(
        &floyd.join("02.flac"),
        &[
            b"TITLE=Breathe (feat. David Gilmour)",
            b"ARTIST=PINK FLOYD",
            b"ALBUM=THE DARK SIDE OF THE MOON",
            b"TRACKNUMBER=02",
            b"DATE=01.03.1973",
            b"GENRE=rock",
        ],
    );
    // Mojibake: UTF-8 read as Latin-1 once, then saved as UTF-8 again.
    flac(
        &root.join("Beyonce/01.flac"),
        &[
            "TITLE=Halo".as_bytes(),
            "ARTIST=Beyonc\u{c3}\u{a9}".as_bytes(),
            b"ALBUM=I Am... Sasha Fierce",
        ],
    );
    let mut tags = Tags::new(
        "Teardrop",
        "Massive Attack ft. Elizabeth Fraser",
        "Mezzanine (Disc 1)",
    );
    tags.album_artist = Some("Massive Attack");
    dsf(&root.join("Mezzanine/01.dsf"), &tags);
}

/// Names and values that look odd but are meant.
fn meant(root: &Path) {
    for (file, artist, title) in [
        ("1.dsf", "ABBA", "SOS"),
        ("2.dsf", "AC/DC", "T.N.T."),
        ("3.dsf", "deadmau5", "Strobe"),
        ("4.dsf", "k.d. lang", "Constant Craving"),
        ("5.dsf", "Simon & Garfunkel", "The Boxer"),
        ("6.dsf", "Sigur R\u{f3}s", "Hopp\u{ed}polla"),
    ] {
        let mut tags = Tags::new(title, artist, "Meant");
        tags.album_artist = Some("Various Artists");
        dsf(&root.join("Meant").join(file), &tags);
    }
    flac(
        &root.join("Ambiguous/a.flac"),
        &[
            b"TITLE=Ambiguous Date",
            b"ARTIST=Air",
            b"ALBUMARTIST=Air",
            b"ALBUM=Talkie Walkie",
            b"DATE=04/05/2004",
            b"TRACKNUMBER=1",
        ],
    );
}

fn after(proposals: &[Proposal], target: Target, field: Field) -> Option<Vec<String>> {
    proposals
        .iter()
        .find(|p| p.target == target && p.field == field)
        .map(|p| p.after.clone())
}

fn track_id(library: &Library, title_start: &str) -> i64 {
    library
        .tracks()
        .unwrap()
        .into_iter()
        .find(|track| track.title.trim_start().starts_with(title_start))
        .unwrap()
        .id
}

/// Accepts every proposal, one saved step per origin.
fn accept_all(library: &mut Library, proposals: &[Proposal]) {
    let mut by_origin: BTreeMap<String, (Origin, Vec<_>)> = BTreeMap::new();
    for proposal in proposals {
        let origin = proposal.origin();
        let Origin::Rule(name) = &origin else {
            unreachable!()
        };
        by_origin
            .entry(name.clone())
            .or_insert_with(|| (origin.clone(), Vec::new()))
            .1
            .push(proposal.change());
    }
    for (origin, changes) in by_origin.into_values() {
        library.save_edits(&changes, &origin).unwrap();
    }
}

#[test]
fn messy_tags_get_proposals_and_accepting_them_tidies_the_library() {
    let dir = temp_dir("messy_tags_get_proposals_and_accepting_them_tidies_the_library");
    let root = dir.join("music");
    messy(&root);
    let mut library = scanned_library(&dir, &root);
    let proposals = library.propose_cleanup().unwrap();

    let speak = Target::Track(track_id(&library, "SPEAK"));
    let breathe = Target::Track(track_id(&library, "Breathe"));
    let halo = Target::Track(track_id(&library, "Halo"));
    let teardrop = Target::Track(track_id(&library, "Teardrop"));
    let album = |title: &str| {
        let id = library
            .albums()
            .unwrap()
            .into_iter()
            .find(|a| a.title == title)
            .unwrap()
            .id;
        Target::Release(id)
    };
    let floyd = album("THE DARK SIDE OF THE MOON");
    let mezzanine = album("Mezzanine (Disc 1)");

    assert_eq!(
        after(&proposals, speak, Field::Title).unwrap(),
        ["Speak to Me"]
    );
    assert_eq!(
        after(&proposals, speak, Field::Artist).unwrap(),
        ["Pink Floyd"]
    );
    assert_eq!(after(&proposals, speak, Field::TrackTotal).unwrap(), ["10"]);
    assert_eq!(after(&proposals, speak, Field::Genre).unwrap(), ["Rock"]);
    assert_eq!(
        after(&proposals, breathe, Field::Title).unwrap(),
        ["Breathe"]
    );
    assert_eq!(
        after(&proposals, breathe, Field::Artist).unwrap(),
        ["Pink Floyd feat. David Gilmour"]
    );
    assert_eq!(after(&proposals, breathe, Field::Genre).unwrap(), ["Rock"]);
    assert_eq!(
        after(&proposals, floyd, Field::Album).unwrap(),
        ["The Dark Side of the Moon"]
    );
    assert_eq!(
        after(&proposals, floyd, Field::Date).unwrap(),
        ["1973-03-01"]
    );
    assert_eq!(
        after(&proposals, floyd, Field::AlbumArtist).unwrap(),
        ["Pink Floyd"]
    );
    assert_eq!(
        after(&proposals, halo, Field::Artist).unwrap(),
        ["Beyonc\u{e9}"]
    );
    assert_eq!(
        after(&proposals, teardrop, Field::Artist).unwrap(),
        ["Massive Attack feat. Elizabeth Fraser"]
    );
    assert_eq!(
        after(&proposals, mezzanine, Field::Album).unwrap(),
        ["Mezzanine"]
    );
    assert_eq!(after(&proposals, teardrop, Field::Disc).unwrap(), ["1"]);
    let rules = &proposals
        .iter()
        .find(|p| p.target == speak && p.field == Field::Title)
        .unwrap()
        .rules;
    assert_eq!(rules, &[Rule::TextHygiene, Rule::Capitals]);

    accept_all(&mut library, &proposals);
    assert_eq!(
        library.propose_cleanup().unwrap(),
        Vec::<Proposal>::new(),
        "nothing left to tidy"
    );
    let titles: Vec<String> = library
        .albums()
        .unwrap()
        .into_iter()
        .map(|a| a.title)
        .collect();
    assert!(
        titles.contains(&"The Dark Side of the Moon".to_owned()),
        "{titles:?}"
    );
    let speak_track = library.track(track_id(&library, "Speak")).unwrap().unwrap();
    assert_eq!(
        (speak_track.title.as_str(), speak_track.artist.as_str()),
        ("Speak to Me", "Pink Floyd")
    );
    let edits = library.edits(speak).unwrap();
    assert!(
        edits
            .iter()
            .any(|e| e.field == Field::Title && e.origin == "rule:text_hygiene+capitals")
    );

    // One step per origin: undoing them all restores the files' values.
    while library.undo_edits().unwrap() {}
    assert_eq!(library.propose_cleanup().unwrap(), proposals);
}

#[test]
fn meant_names_and_ambiguous_dates_are_left_alone() {
    let dir = temp_dir("meant_names_and_ambiguous_dates_are_left_alone");
    let root = dir.join("music");
    meant(&root);
    let library = scanned_library(&dir, &root);
    assert_eq!(library.propose_cleanup().unwrap(), Vec::<Proposal>::new());
}

#[test]
fn a_leading_the_gets_a_sort_name_and_nothing_else() {
    let dir = temp_dir("a_leading_the_gets_a_sort_name_and_nothing_else");
    let root = dir.join("music");
    let mut tags = Tags::new("Intro", "The xx", "xx");
    tags.album_artist = Some("The xx");
    dsf(&root.join("xx/1.dsf"), &tags);
    let library = scanned_library(&dir, &root);
    let fields: Vec<(Field, Vec<String>)> = library
        .propose_cleanup()
        .unwrap()
        .into_iter()
        .map(|p| (p.field, p.after))
        .collect();
    assert_eq!(
        fields,
        [
            (Field::ArtistSort, vec!["xx, The".to_owned()]),
            (Field::AlbumArtistSort, vec!["xx, The".to_owned()])
        ]
    );
}
