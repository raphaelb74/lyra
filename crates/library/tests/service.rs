//! The edit service: previews, commits bound to what a preview saw,
//! conflict-checked undo, reset, source preferences, and release edits
//! following their tracks.
#![expect(clippy::unwrap_used, reason = "test helpers fail loudly")]

mod common;

use library::{Edit, Field, FieldPreview, Library, LibraryError, Origin, Target};

use common::{
    Tags, change, dsf, edited_library, run_scan, scanned_library, set, temp_dir, title_of,
};

fn values(library: &Library, target: Target) -> Vec<(Field, Option<String>, String)> {
    library
        .edits(target)
        .unwrap()
        .into_iter()
        .map(|edit| (edit.field, edit.value, edit.origin))
        .collect()
}

#[test]
fn a_preview_shows_what_a_commit_then_saves() {
    let dir = temp_dir("a_preview_shows_what_a_commit_then_saves");
    let root = dir.join("music");
    let (mut library, [first, _], album) = edited_library(&dir, &root);
    let changes = [
        set(Target::Track(first), Field::Title, "Mysterons (Live)"),
        set(Target::Release(album), Field::Date, "1994"),
    ];
    let preview = library.preview_edits(&changes, &Origin::Manual).unwrap();
    assert_eq!(
        preview.fields,
        [
            FieldPreview {
                target: Target::Track(first),
                field: Field::Title,
                before: vec!["Mysterons".to_owned()],
                after: vec!["Mysterons (Live)".to_owned()],
            },
            FieldPreview {
                target: Target::Release(album),
                field: Field::Date,
                before: vec![],
                after: vec!["1994".to_owned()],
            },
        ]
    );
    assert_eq!(
        preview.affected,
        [Target::Track(first), Target::Release(album)]
    );
    assert_eq!(
        title_of(&library, first),
        "Mysterons",
        "a preview saves nothing"
    );

    library
        .commit_edits(&changes, &Origin::Manual, &preview.revisions)
        .unwrap();
    assert_eq!(title_of(&library, first), "Mysterons (Live)");
    assert_eq!(library.albums().unwrap()[0].year, Some(1994));
    let again = library.preview_edits(&changes, &Origin::Manual).unwrap();
    assert_eq!(again.fields, [], "saved: nothing left to change");
    assert!(library.undo_edits().unwrap());
    assert_eq!(title_of(&library, first), "Mysterons");
}

#[test]
fn a_stale_preview_saves_nothing() {
    let dir = temp_dir("a_stale_preview_saves_nothing");
    let root = dir.join("music");
    let (mut library, [first, second], _) = edited_library(&dir, &root);
    let changes = [set(Target::Track(first), Field::Title, "Mysterons (Live)")];

    // An edit elsewhere leaves the preview good.
    let preview = library.preview_edits(&changes, &Origin::Manual).unwrap();
    library
        .save_edits(
            &[set(Target::Track(second), Field::Title, "Sour Times!")],
            &Origin::Manual,
        )
        .unwrap();
    library
        .commit_edits(&changes, &Origin::Manual, &preview.revisions)
        .unwrap();
    assert!(library.undo_edits().unwrap());

    // An edit of the same track does not.
    let preview = library.preview_edits(&changes, &Origin::Manual).unwrap();
    library
        .save_edits(
            &[set(Target::Track(first), Field::Artist, "Beth Gibbons")],
            &Origin::Manual,
        )
        .unwrap();
    let result = library.commit_edits(&changes, &Origin::Manual, &preview.revisions);
    assert!(matches!(result, Err(LibraryError::Stale)), "{result:?}");
    assert_eq!(title_of(&library, first), "Mysterons");

    // Nor does a new read of its file.
    let preview = library.preview_edits(&changes, &Origin::Manual).unwrap();
    dsf(
        &root.join("Dummy/1.dsf"),
        &Tags::new("Mysterons?", "Portishead", "Dummy"),
    );
    run_scan(&mut library, &dir.join("art"));
    let result = library.commit_edits(&changes, &Origin::Manual, &preview.revisions);
    assert!(matches!(result, Err(LibraryError::Stale)), "{result:?}");
    assert_eq!(title_of(&library, first), "Mysterons?");

    // Revisions of other changes do not cover these.
    let other = library
        .preview_edits(
            &[set(Target::Track(second), Field::Title, "x")],
            &Origin::Manual,
        )
        .unwrap();
    let result = library.commit_edits(&changes, &Origin::Manual, &other.revisions);
    assert!(matches!(result, Err(LibraryError::Stale)), "{result:?}");
}

#[test]
fn undo_refuses_edits_changed_since() {
    let dir = temp_dir("undo_refuses_edits_changed_since");
    let root = dir.join("music");
    let (mut library, [first, _], _) = edited_library(&dir, &root);
    library
        .save_edits(
            &[set(Target::Track(first), Field::Title, "Mysterons (Live)")],
            &Origin::Manual,
        )
        .unwrap();

    // Another writer changes the edit the step saved.
    let other = rusqlite::Connection::open(dir.join("library.db")).unwrap();
    other
        .execute("UPDATE edits SET value = 'Elsewhere'", [])
        .unwrap();
    let result = library.undo_edits();
    assert!(
        matches!(result, Err(LibraryError::UndoConflict)),
        "{result:?}"
    );
    assert_eq!(
        values(&library, Target::Track(first)),
        [(
            Field::Title,
            Some("Elsewhere".to_owned()),
            "manual".to_owned()
        )],
        "nothing was undone"
    );

    // Once it is what the step saved again, the step undoes.
    other
        .execute("UPDATE edits SET value = 'Mysterons (Live)'", [])
        .unwrap();
    assert!(library.undo_edits().unwrap());
    assert_eq!(values(&library, Target::Track(first)), []);
    assert_eq!(title_of(&library, first), "Mysterons");
}

#[test]
fn reset_shows_the_files_again_in_one_step() {
    let dir = temp_dir("reset_shows_the_files_again_in_one_step");
    let root = dir.join("music");
    let (mut library, [first, second], album) = edited_library(&dir, &root);
    library
        .save_edits(
            &[
                set(Target::Track(first), Field::Title, "One"),
                change(Target::Track(first), Field::Artist, Edit::Clear),
                set(Target::Track(second), Field::Title, "Two"),
                set(Target::Release(album), Field::Album, "Dummy (Deluxe)"),
            ],
            &Origin::Manual,
        )
        .unwrap();
    library
        .reset_edits(&[Target::Track(first), Target::Release(album)])
        .unwrap();
    assert_eq!(values(&library, Target::Track(first)), []);
    assert_eq!(values(&library, Target::Release(album)), []);
    assert_eq!(title_of(&library, first), "Mysterons");
    assert_eq!(title_of(&library, second), "Two", "not asked to reset");
    assert_eq!(library.albums().unwrap()[0].title, "Dummy");

    assert!(library.undo_edits().unwrap());
    assert_eq!(title_of(&library, first), "One");
    assert_eq!(library.albums().unwrap()[0].title, "Dummy (Deluxe)");
}

#[test]
fn the_stronger_origin_keeps_a_field() {
    let dir = temp_dir("the_stronger_origin_keeps_a_field");
    let root = dir.join("music");
    let (mut library, [first, second], _) = edited_library(&dir, &root);
    let rule = Origin::Rule("capitals".to_owned());
    let provider = Origin::Provider("musicbrainz".to_owned());
    library
        .save_edits(
            &[set(Target::Track(first), Field::Title, "MYSTERONS LIVE")],
            &Origin::Manual,
        )
        .unwrap();

    // A rule does not overwrite the user, and the preview says why.
    let over_manual = [set(Target::Track(first), Field::Title, "Mysterons Live")];
    let preview = library.preview_edits(&over_manual, &rule).unwrap();
    assert_eq!(preview.fields, []);
    assert_eq!(
        preview.held,
        [(over_manual[0].clone(), "manual".to_owned())]
    );
    library.save_edits(&over_manual, &rule).unwrap();
    assert_eq!(title_of(&library, first), "MYSTERONS LIVE");
    let proposals = library.propose_cleanup().unwrap();
    assert!(
        !proposals
            .iter()
            .any(|proposal| proposal.target == Target::Track(first)
                && proposal.field == Field::Title),
        "no proposal over a manual edit: {proposals:?}"
    );

    // A provider overwrites a rule, not the other way round.
    let field = |title| [set(Target::Track(second), Field::Title, title)];
    library
        .save_edits(&field("Sour Times (rule)"), &rule)
        .unwrap();
    library
        .save_edits(&field("Sour Times (provider)"), &provider)
        .unwrap();
    library
        .save_edits(&field("Sour Times (rule again)"), &rule)
        .unwrap();
    assert_eq!(title_of(&library, second), "Sour Times (provider)");
    // The user overwrites anything.
    library
        .save_edits(&field("Sour Times (mine)"), &Origin::Manual)
        .unwrap();
    assert_eq!(title_of(&library, second), "Sour Times (mine)");
}

#[test]
fn release_edits_follow_their_tracks_into_a_merged_release() {
    let dir = temp_dir("release_edits_follow_their_tracks_into_a_merged_release");
    let root = dir.join("music");
    let file = |name: &str, album| {
        dsf(
            &root.join("Mellon Collie").join(name),
            &Tags::new(name, "The Smashing Pumpkins", album),
        );
    };
    file("1.dsf", "Mellon Collie (Disc 1)");
    file("2.dsf", "Mellon Collie (Disc 2)");
    let mut library = scanned_library(&dir, &root);
    let mut albums = library.albums().unwrap();
    albums.sort_by(|a, b| a.title.cmp(&b.title));
    let (one, two) = (albums[0].id, albums[1].id);
    library
        .save_edits(
            &[set(Target::Release(one), Field::Date, "1995")],
            &Origin::Manual,
        )
        .unwrap();
    library
        .save_edits(
            &[
                set(Target::Release(two), Field::Date, "1996"),
                set(Target::Release(two), Field::AlbumSort, "Mellon Collie"),
            ],
            &Origin::Manual,
        )
        .unwrap();

    // Retagged as one album: both releases are replaced by a new one.
    file("1.dsf", "Mellon Collie");
    file("2.dsf", "Mellon Collie");
    run_scan(&mut library, &dir.join("art"));
    let albums = library.albums().unwrap();
    assert_eq!(albums.len(), 1);
    let merged = albums[0].id;
    assert!(merged != one && merged != two, "{merged} is a new release");
    assert_eq!(
        values(&library, Target::Release(merged)),
        [
            (
                Field::AlbumSort,
                Some("Mellon Collie".to_owned()),
                "manual".to_owned()
            ),
            (Field::Date, Some("1995".to_owned()), "manual".to_owned()),
        ],
        "the older release's edit wins a field both have"
    );
    assert_eq!(albums[0].year, Some(1995));

    // The steps still undo what they saved and the merged release holds.
    assert!(library.undo_edits().unwrap());
    assert_eq!(
        values(&library, Target::Release(merged)),
        [(Field::Date, Some("1995".to_owned()), "manual".to_owned())]
    );
    assert!(library.undo_edits().unwrap());
    assert_eq!(values(&library, Target::Release(merged)), []);
}

#[test]
fn fixes_of_several_rules_save_and_undo_as_one_step() {
    let dir = temp_dir("fixes_of_several_rules_save_and_undo_as_one_step");
    let root = dir.join("music");
    let (mut library, [first, second], _) = edited_library(&dir, &root);
    let rule = |name: &str| Origin::Rule(name.to_owned());
    library
        .save_edits_each(&[
            (
                set(Target::Track(first), Field::Title, "One"),
                rule("capitals"),
            ),
            (
                set(Target::Track(second), Field::Title, "Two"),
                rule("text_hygiene"),
            ),
        ])
        .unwrap();
    assert_eq!(
        values(&library, Target::Track(second)),
        [(
            Field::Title,
            Some("Two".to_owned()),
            "rule:text_hygiene".to_owned()
        )]
    );
    assert!(library.undo_edits().unwrap());
    assert_eq!(title_of(&library, first), "Mysterons");
    assert_eq!(title_of(&library, second), "Sour Times");
    assert!(!library.undo_edits().unwrap(), "it was one step");
}
