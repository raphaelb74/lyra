//! Tracks keep their ID when their files move or are renamed.

mod common;

use library::Library;
use std::fs;

use common::{Tags, dsf, fixture, playlist_ids, run_scan, scanned_library, temp_dir, track_ids};

#[test]
fn a_renamed_folder_keeps_its_tracks_release_and_playlists() {
    let dir = temp_dir("a_renamed_folder_keeps_its_tracks_release_and_playlists");
    let root = dir.join("music");
    // Titles of different lengths: the files' sizes tell them apart.
    for (file, title) in [("1.dsf", "Mysterons"), ("2.dsf", "Sour Times")] {
        let mut tags = Tags::new(title, "Portishead", "Dummy");
        tags.cover = true;
        dsf(&root.join("Dummy").join(file), &tags);
    }
    let mut library = scanned_library(&dir, &root);
    let tracks = track_ids(&library);
    let album = library.albums().unwrap()[0].clone();
    let playlist = library.create_playlist("Trip hop").unwrap();
    let ids: Vec<i64> = tracks.iter().map(|(id, _)| *id).collect();
    library.add_to_playlist(playlist, &ids).unwrap();

    fs::create_dir_all(root.join("Portishead")).unwrap();
    fs::rename(
        root.join("Dummy"),
        root.join("Portishead").join("Dummy (1994)"),
    )
    .unwrap();
    fs::rename(
        root.join("Portishead/Dummy (1994)/2.dsf"),
        root.join("Portishead/Dummy (1994)/02 Sour Times.dsf"),
    )
    .unwrap();
    let summary = run_scan(&mut library, &dir.join("art"));

    assert_eq!(
        (summary.moved, summary.added, summary.removed),
        (2, 0, 0),
        "{summary:?}"
    );
    let after = track_ids(&library);
    assert_eq!(
        after,
        [
            (tracks[1].0, "02 Sour Times.dsf".to_owned()),
            (tracks[0].0, "1.dsf".to_owned())
        ]
    );
    assert!(
        library.tracks().unwrap()[0]
            .path
            .starts_with(root.join("Portishead"))
    );
    assert_eq!(playlist_ids(&library, playlist), ids);
    let albums = library.albums().unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(
        (albums[0].id, &albums[0].art),
        (album.id, &album.art),
        "same release"
    );
}

#[test]
fn a_flac_moved_to_another_library_folder_keeps_its_track() {
    let dir = temp_dir("a_flac_moved_to_another_library_folder_keeps_its_track");
    let (first, second) = (dir.join("first"), dir.join("second"));
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    fs::write(
        first.join("a.flac"),
        fixture("format-pcm", "s16_44100.flac"),
    )
    .unwrap();
    fs::write(first.join("b.wav"), fixture("format-pcm", "s24_96000.wav")).unwrap();
    let mut library = Library::open(&dir.join("library.db")).unwrap();
    library.add_folder(&first).unwrap();
    library.add_folder(&second).unwrap();
    run_scan(&mut library, &dir.join("art"));
    let before = track_ids(&library);

    // The FLAC moves; the WAV stays, so the first folder is not empty.
    fs::rename(first.join("a.flac"), second.join("renamed.flac")).unwrap();
    let summary = run_scan(&mut library, &dir.join("art"));
    assert_eq!((summary.moved, summary.removed), (1, 0), "{summary:?}");
    let after = track_ids(&library);
    assert_eq!(
        after,
        [
            (before[1].0, "b.wav".to_owned()),
            (before[0].0, "renamed.flac".to_owned())
        ]
    );
}

#[test]
fn identical_copies_are_not_guessed() {
    let dir = temp_dir("identical_copies_are_not_guessed");
    let root = dir.join("music");
    for folder in ["One", "Two"] {
        fs::create_dir_all(root.join(folder)).unwrap();
        fs::write(
            root.join(folder).join("a.flac"),
            fixture("format-pcm", "s16_44100.flac"),
        )
        .unwrap();
    }
    let mut library = scanned_library(&dir, &root);
    let before = track_ids(&library);

    fs::create_dir_all(root.join("All")).unwrap();
    fs::rename(root.join("One/a.flac"), root.join("All/one.flac")).unwrap();
    fs::rename(root.join("Two/a.flac"), root.join("All/two.flac")).unwrap();
    let summary = run_scan(&mut library, &dir.join("art"));
    assert_eq!(
        (summary.moved, summary.added, summary.removed),
        (0, 2, 2),
        "{summary:?}"
    );
    let after = track_ids(&library);
    assert!(
        after
            .iter()
            .all(|(id, _)| before.iter().all(|(old, _)| old != id))
    );
}

#[test]
fn an_unavailable_folder_is_never_a_move_source() {
    let dir = temp_dir("an_unavailable_folder_is_never_a_move_source");
    let (drive, home) = (dir.join("drive"), dir.join("home"));
    fs::create_dir_all(&drive).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::write(
        drive.join("a.flac"),
        fixture("format-pcm", "s16_44100.flac"),
    )
    .unwrap();
    let mut library = Library::open(&dir.join("library.db")).unwrap();
    library.add_folder(&drive).unwrap();
    library.add_folder(&home).unwrap();
    run_scan(&mut library, &dir.join("art"));
    let before = track_ids(&library);

    // The drive now looks unplugged; a copy of its file sits at home.
    fs::rename(drive.join("a.flac"), home.join("a.flac")).unwrap();
    let summary = run_scan(&mut library, &dir.join("art"));
    assert_eq!((summary.moved, summary.added), (0, 1), "{summary:?}");
    assert_eq!(summary.unavailable, [drive.canonicalize().unwrap()]);
    let after = track_ids(&library);
    assert_eq!(after.len(), 2);
    assert!(after.contains(&before[0]), "the drive's track is kept");
}
