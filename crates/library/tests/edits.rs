//! Edits on top of file values: saving, resetting, review, undo.

mod common;

use id3::{Tag, TagLike, Version};
use library::{Edit, Field, Library, LibraryError, Origin, SavedEdit, Target};
use std::fs;

use common::{
    Tags, change, dsf, edited_library, flac_with_comments, run_scan, scanned_library, set,
    tagged_dsf, temp_dir, title_of,
};

#[test]
fn edits_survive_rescans_and_restarts() {
    let dir = temp_dir("edits_survive_rescans_and_restarts");
    let root = dir.join("music");
    let (mut library, [first, _], album) = edited_library(&dir, &root);
    library
        .save_edits(
            &[
                set(Target::Track(first), Field::Title, "Mysterons (Live)"),
                set(Target::Release(album), Field::Album, "Dummy (Deluxe)"),
                set(Target::Release(album), Field::AlbumArtist, "Beth Gibbons"),
            ],
            &Origin::Manual,
        )
        .unwrap();
    let check = |library: &Library| {
        assert_eq!(title_of(library, first), "Mysterons (Live)");
        let albums = library.albums().unwrap();
        assert_eq!(
            (albums[0].title.as_str(), albums[0].artist.as_str()),
            ("Dummy (Deluxe)", "Beth Gibbons")
        );
    };
    check(&library);

    // The other file changes and is read again; a full re-read too.
    dsf(
        &root.join("Dummy/2.dsf"),
        &Tags::new("Sour Times!", "Portishead", "Dummy"),
    );
    run_scan(&mut library, &dir.join("art"));
    check(&library);
    drop(library);
    let mut library = Library::open(&dir.join("library.db")).unwrap();
    check(&library);
    let conn = rusqlite::Connection::open(dir.join("library.db")).unwrap();
    conn.execute("UPDATE tracks SET mtime_ns = -1", []).unwrap();
    drop(conn);
    run_scan(&mut library, &dir.join("art"));
    check(&library);
    let edits = library.edits(Target::Track(first)).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(
        (edits[0].origin.as_str(), edits[0].review),
        ("manual", false)
    );
}

#[test]
fn resetting_or_clearing_an_edit_shows_the_file_again() {
    let dir = temp_dir("resetting_or_clearing_an_edit_shows_the_file_again");
    let root = dir.join("music");
    let (mut library, [first, second], album) = edited_library(&dir, &root);
    library
        .save_edits(
            &[
                set(Target::Track(first), Field::Title, "Edited"),
                set(Target::Release(album), Field::Album, "Edited Album"),
            ],
            &Origin::Manual,
        )
        .unwrap();
    library
        .save_edits(
            &[
                change(Target::Track(first), Field::Title, Edit::Reset),
                change(Target::Release(album), Field::Album, Edit::Reset),
                // Cleared: no title, so the file name shows.
                change(Target::Track(second), Field::Title, Edit::Clear),
            ],
            &Origin::Manual,
        )
        .unwrap();
    assert_eq!(title_of(&library, first), "Mysterons");
    assert_eq!(title_of(&library, second), "2");
    assert_eq!(library.albums().unwrap()[0].title, "Dummy");
    assert_eq!(
        library.edits(Target::Track(first)).unwrap(),
        Vec::<SavedEdit>::new()
    );
    assert_eq!(library.edits(Target::Track(second)).unwrap()[0].value, None);
}

#[test]
fn a_file_changed_under_an_edit_is_marked_for_review() {
    let dir = temp_dir("a_file_changed_under_an_edit_is_marked_for_review");
    let root = dir.join("music");
    let (mut library, [first, second], _) = edited_library(&dir, &root);
    library
        .save_edits(
            &[
                set(Target::Track(first), Field::Title, "Mine"),
                set(Target::Track(second), Field::Title, "Also mine"),
            ],
            &Origin::Rule("capitals".to_owned()),
        )
        .unwrap();
    // Another tagger changes the first title, and only the artist of the second.
    dsf(
        &root.join("Dummy/1.dsf"),
        &Tags::new("Mysterons (2024)", "Portishead", "Dummy"),
    );
    dsf(
        &root.join("Dummy/2.dsf"),
        &Tags::new("Sour Times", "PORTISHEAD", "Dummy"),
    );
    run_scan(&mut library, &dir.join("art"));

    let first_edit = &library.edits(Target::Track(first)).unwrap()[0];
    assert!(first_edit.review);
    assert_eq!(first_edit.origin, "rule:capitals");
    assert!(!library.edits(Target::Track(second)).unwrap()[0].review);
    assert_eq!(title_of(&library, first), "Mine", "the edit still shows");
}

#[test]
fn undo_steps_back_one_save_at_a_time() {
    let dir = temp_dir("undo_steps_back_one_save_at_a_time");
    let root = dir.join("music");
    let (mut library, [first, _], album) = edited_library(&dir, &root);
    library
        .save_edits(
            &[set(Target::Track(first), Field::Title, "One")],
            &Origin::Manual,
        )
        .unwrap();
    library
        .save_edits(
            &[
                set(Target::Track(first), Field::Title, "Two"),
                change(
                    Target::Release(album),
                    Field::Compilation,
                    Edit::Set("1".into()),
                ),
            ],
            &Origin::Manual,
        )
        .unwrap();
    assert_eq!(library.albums().unwrap()[0].artist, "Various Artists");

    assert!(library.undo_edits().unwrap());
    assert_eq!(title_of(&library, first), "One");
    assert_eq!(library.albums().unwrap()[0].artist, "Portishead");
    assert_eq!(
        library.edits(Target::Release(album)).unwrap(),
        Vec::<SavedEdit>::new()
    );
    assert!(library.undo_edits().unwrap());
    assert_eq!(title_of(&library, first), "Mysterons");
    assert_eq!(
        library.edits(Target::Track(first)).unwrap(),
        Vec::<SavedEdit>::new()
    );
    assert!(!library.undo_edits().unwrap());
}

#[test]
fn an_invalid_change_saves_nothing() {
    let dir = temp_dir("an_invalid_change_saves_nothing");
    let root = dir.join("music");
    let (mut library, [first, _], album) = edited_library(&dir, &root);
    for changes in [
        vec![
            set(Target::Track(first), Field::Title, "Fine"),
            set(Target::Track(first), Field::Track, "three"),
        ],
        vec![set(
            Target::Track(first),
            Field::Album,
            "A track has no album field",
        )],
        vec![set(Target::Release(album), Field::Date, "22.08.1994")],
        vec![set(Target::Track(999), Field::Title, "No such track")],
    ] {
        let result = library.save_edits(&changes, &Origin::Manual);
        assert!(
            matches!(result, Err(LibraryError::Invalid(_))),
            "{changes:?}"
        );
    }
    assert_eq!(title_of(&library, first), "Mysterons");
    assert_eq!(
        library.edits(Target::Track(first)).unwrap(),
        Vec::<SavedEdit>::new()
    );
    assert!(!library.undo_edits().unwrap(), "no step was recorded");
}

#[test]
fn file_values_are_kept_exactly() {
    let dir = temp_dir("file_values_are_kept_exactly");
    let root = dir.join("music");
    let mut tag = Tag::new();
    tag.set_title("  Spaced\u{200B} ");
    // ID3 v2.4 multi-value: two artists.
    tag.set_artist("Air\0Beck");
    tag.set_text("TRCK", "3/12");
    tag.set_album("Beyonce\u{301}");
    let mut bytes = Vec::new();
    tag.write_to(&mut bytes, Version::Id3v24).unwrap();
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a.dsf"), tagged_dsf(&bytes)).unwrap();
    let library = scanned_library(&dir, &root);

    let track = library.tracks().unwrap().remove(0);
    let values: Vec<(Field, String)> = library
        .file_values(track.id)
        .unwrap()
        .into_iter()
        .map(|value| (value.field, value.value))
        .collect();
    for expected in [
        (Field::Title, "  Spaced\u{200B} "),
        (Field::Artist, "Air"),
        (Field::Artist, "Beck"),
        (Field::Track, "3/12"),
        (Field::Album, "Beyonce\u{301}"),
    ] {
        assert!(
            values.contains(&(expected.0, expected.1.to_owned())),
            "{expected:?} in {values:?}"
        );
    }
    // Shown: trimmed, first artist, track 3; the decomposed accent is kept.
    assert_eq!(
        (track.title.as_str(), track.artist.as_str(), track.track),
        ("Spaced\u{200B}", "Air", Some(3))
    );
    assert_eq!(track.album, "Beyonce\u{301}");
}

#[test]
fn a_moved_track_keeps_its_edits() {
    let dir = temp_dir("a_moved_track_keeps_its_edits");
    let root = dir.join("music");
    let (mut library, [first, _], _) = edited_library(&dir, &root);
    library
        .save_edits(
            &[set(Target::Track(first), Field::Title, "Mine")],
            &Origin::Manual,
        )
        .unwrap();
    fs::rename(root.join("Dummy"), root.join("Dummy (1994)")).unwrap();
    let summary = run_scan(&mut library, &dir.join("art"));
    assert_eq!(summary.moved, 2, "{summary:?}");
    assert_eq!(title_of(&library, first), "Mine");
    assert_eq!(library.edits(Target::Track(first)).unwrap().len(), 1);
}

#[test]
fn flac_comments_are_kept_once_each() {
    let dir = temp_dir("flac_comments_are_kept_once_each");
    let root = dir.join("music");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("a.flac"),
        flac_with_comments(&[
            b"TITLE=Teardrop",
            b"ARTIST=Massive Attack",
            b"ARTIST=Elizabeth Fraser",
            b"TRACKNUMBER=3/12",
            b"DISCNUMBER=1/2",
        ]),
    )
    .unwrap();
    // A number Symphonia cannot parse is still kept, and still shown.
    fs::write(
        root.join("b.flac"),
        flac_with_comments(&[b"TITLE=Angel", b"tracknumber= 7 "]),
    )
    .unwrap();
    let library = scanned_library(&dir, &root);
    let tracks = library.tracks().unwrap();
    let values = |title: &str| -> Vec<(Field, String)> {
        let track = tracks.iter().find(|track| track.title == title).unwrap();
        library
            .file_values(track.id)
            .unwrap()
            .into_iter()
            .map(|value| (value.field, value.value))
            .collect()
    };

    // Stored by field, each field's values in file order.
    assert_eq!(
        values("Teardrop"),
        [
            (Field::Artist, "Massive Attack".to_owned()),
            (Field::Artist, "Elizabeth Fraser".to_owned()),
            (Field::Disc, "1/2".to_owned()),
            (Field::Title, "Teardrop".to_owned()),
            (Field::Track, "3/12".to_owned()),
        ],
        "`3/12` once, under the track number"
    );
    assert_eq!(
        values("Angel"),
        [
            (Field::Title, "Angel".to_owned()),
            (Field::Track, " 7 ".to_owned())
        ]
    );
    let numbers: Vec<_> = tracks
        .iter()
        .map(|track| (track.title.as_str(), track.track, track.disc))
        .collect();
    assert!(
        numbers.contains(&("Teardrop", Some(3), Some(1))),
        "{numbers:?}"
    );
    assert!(numbers.contains(&("Angel", Some(7), None)), "{numbers:?}");
}
