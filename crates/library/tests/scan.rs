//! Scanning folders: formats, artwork, rescans, unavailable folders, playlists.

mod common;

use id3::{Tag, TagLike, Version};
use library::{Library, LibraryError, ScanEvent, Scanner, TaggedLoudness, read_tags, scan};
use std::fs;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::Duration;

use common::{
    JPEG, fixture, flac_with_comments, id3_tag, library_with_playlist, music, playlist_ids,
    run_scan, tagged_dsf, temp_dir,
};

#[test]
fn scans_tags_formats_and_artwork() {
    let dir = temp_dir("scans_tags_formats_and_artwork");
    let root = dir.join("music");
    music(&root);
    let art = dir.join("art");
    let mut library = Library::open(&dir.join("library.db")).unwrap();
    library.add_folder(&root).unwrap();

    let summary = run_scan(&mut library, &art);
    assert_eq!((summary.added, summary.failed), (4, 1), "{summary:?}");
    assert!(summary.failures[0].0.ends_with("broken.flac"));

    let albums = library.albums().unwrap();
    let titles: Vec<_> = albums
        .iter()
        .map(|album| (album.artist.as_str(), album.title.as_str(), album.tracks))
        .collect();
    assert_eq!(
        titles,
        [
            ("David Bowie", "Aladdin Sane", 2),
            ("Unknown Artist", "Test Tones", 2)
        ]
    );
    let bowie = &albums[0];
    assert_eq!((bowie.year, bowie.best_codec.as_str()), (Some(1973), "dsf"));
    assert_eq!(bowie.genre.as_deref(), Some("Rock"));
    assert!(bowie.duration_ms.is_some_and(|ms| ms > 0));
    let tones = &albums[1];
    assert_eq!(
        (tones.best_rate, tones.best_codec.as_str()),
        (96_000, "wav")
    );
    // Embedded front cover for the DSD album, Cover.JPG for the untagged one.
    for album in &albums {
        let name = album.art.as_deref().unwrap();
        assert_eq!(fs::read(art.join(name)).unwrap(), JPEG, "{}", album.title);
    }

    let tracks = library.album_tracks(bowie.id).unwrap();
    let listed: Vec<_> = tracks
        .iter()
        .map(|track| (track.track, track.title.as_str(), track.codec.as_str()))
        .collect();
    assert_eq!(
        listed,
        [
            (Some(1), "Watch That Man", "dsf"),
            (Some(2), "Aladdin Sane", "dff")
        ]
    );
    assert_eq!(
        (
            tracks[0].sample_rate,
            tracks[0].bits_per_sample,
            tracks[0].channels
        ),
        (2_822_400, 1, 2)
    );
    let flac = library.album_tracks(tones.id).unwrap();
    assert_eq!((flac[0].title.as_str(), flac[0].sample_rate), ("a", 44_100));
    assert!(flac[0].duration_ms.is_some());
}

#[test]
fn rescans_only_what_changed() {
    let dir = temp_dir("rescans_only_what_changed");
    let root = dir.join("music");
    music(&root);
    let art = dir.join("art");
    let mut library = Library::open(&dir.join("library.db")).unwrap();
    library.add_folder(&root).unwrap();
    run_scan(&mut library, &art);

    let again = run_scan(&mut library, &art);
    assert_eq!(
        (again.added, again.updated, again.removed),
        (0, 0, 0),
        "{again:?}"
    );

    // Retag one file, delete another, add one.
    let bowie = root.join("Bowie");
    fs::write(
        bowie.join("01.dsf"),
        tagged_dsf(&id3_tag("Watch That Man (2013)", 1, true)),
    )
    .unwrap();
    fs::remove_file(bowie.join("02.dff")).unwrap();
    fs::write(
        root.join("Test Tones/c.flac"),
        fixture("format-pcm", "s24_96000.flac"),
    )
    .unwrap();
    let changed = run_scan(&mut library, &art);
    assert_eq!(
        (changed.added, changed.updated, changed.removed),
        (1, 1, 1),
        "{changed:?}"
    );
    let album = library.albums().unwrap().remove(0);
    let tracks = library.album_tracks(album.id).unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].title, "Watch That Man (2013)");
}

#[test]
fn folders_never_overlap_and_removal_cleans_up() {
    let dir = temp_dir("folders_never_overlap_and_removal_cleans_up");
    let root = dir.join("music");
    music(&root);
    let art = dir.join("art");
    let mut library = Library::open(&dir.join("library.db")).unwrap();

    library.add_folder(&root.join("Bowie")).unwrap();
    // The parent replaces the folder inside it.
    let outer = library.add_folder(&root).unwrap();
    assert_eq!(library.folders().unwrap(), std::slice::from_ref(&outer));
    assert!(matches!(
        library.add_folder(&root.join("Test Tones")),
        Err(LibraryError::AlreadyCovered { .. })
    ));
    assert!(matches!(
        library.add_folder(&root.join("Test Tones/a.flac")),
        Err(LibraryError::NotADirectory(_))
    ));

    run_scan(&mut library, &art);
    assert_eq!(library.albums().unwrap().len(), 2);
    let unused = library.remove_folder(outer.id).unwrap();
    assert_eq!(library.albums().unwrap(), []);
    assert_eq!(unused.len(), 2, "both albums had art");
    library::delete_art(&art, &unused);
    assert_eq!(fs::read_dir(&art).unwrap().count(), 0);
    // The music itself is untouched.
    assert!(root.join("Bowie/01.dsf").exists());
}

#[test]
fn a_cancelled_scan_keeps_what_it_did_not_reach() {
    let dir = temp_dir("a_cancelled_scan_keeps_what_it_did_not_reach");
    let root = dir.join("music");
    music(&root);
    let art = dir.join("art");
    let mut library = Library::open(&dir.join("library.db")).unwrap();
    library.add_folder(&root).unwrap();
    run_scan(&mut library, &art);

    fs::remove_dir_all(root.join("Bowie")).unwrap();
    let summary = scan(&mut library, &art, &AtomicBool::new(true), |_| {}).unwrap();
    assert!(summary.cancelled);
    assert_eq!(summary.removed, 0);
    assert_eq!(library.albums().unwrap().len(), 2);

    assert_eq!(run_scan(&mut library, &art).removed, 2);
    assert_eq!(library.albums().unwrap().len(), 1);
}

#[test]
fn reopening_keeps_the_library() {
    let dir = temp_dir("reopening_keeps_the_library");
    let root = dir.join("music");
    music(&root);
    let db = dir.join("library.db");
    {
        let mut library = Library::open(&db).unwrap();
        library.add_folder(&root).unwrap();
        run_scan(&mut library, &dir.join("art"));
    }
    let library = Library::open(&db).unwrap();
    assert_eq!(library.folders().unwrap().len(), 1);
    let album = library.albums().unwrap().remove(0);
    let first = library.album_tracks(album.id).unwrap().remove(0);
    assert_eq!(library.track(first.id).unwrap(), Some(first));
    assert_eq!(library.album(album.id).unwrap(), Some(album));
    assert_eq!(library.album(-1).unwrap(), None);
}

#[test]
fn the_scanner_runs_in_the_background_and_reports() {
    let dir = temp_dir("the_scanner_runs_in_the_background_and_reports");
    let root = dir.join("music");
    music(&root);
    let db = dir.join("library.db");
    Library::open(&db).unwrap().add_folder(&root).unwrap();

    let (tx, events) = mpsc::channel();
    let scanner = Scanner::new(db.clone(), dir.join("art"), move |event| {
        let _ = tx.send(event);
    });
    scanner.request();
    let summary = loop {
        match events.recv_timeout(Duration::from_secs(10)).unwrap() {
            ScanEvent::Progress(progress) => assert!(progress.done <= progress.total),
            ScanEvent::Finished(result) => break result.unwrap(),
        }
    };
    assert_eq!(summary.added, 4);
    // The UI's own connection sees the result.
    assert_eq!(Library::open(&db).unwrap().albums().unwrap().len(), 2);
}

#[test]
fn artists_and_all_tracks() {
    let dir = temp_dir("artists_and_all_tracks");
    let root = dir.join("music");
    music(&root);
    let mut library = Library::open(&dir.join("library.db")).unwrap();
    library.add_folder(&root).unwrap();
    run_scan(&mut library, &dir.join("art"));

    let artists = library.artists().unwrap();
    let names: Vec<_> = artists
        .iter()
        .map(|artist| (artist.name.as_str(), artist.albums, artist.tracks))
        .collect();
    assert_eq!(names, [("David Bowie", 1, 2), ("Unknown Artist", 1, 2)]);
    let albums = library.artist_albums(artists[0].id).unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].title, "Aladdin Sane");
    assert_eq!(
        artists[0].art, albums[0].art,
        "the artist shows its album's cover"
    );

    let tracks = library.tracks().unwrap();
    assert_eq!(tracks.len(), 4);
    assert_eq!(
        (tracks[0].title.as_str(), tracks[0].album.as_str()),
        ("Watch That Man", "Aladdin Sane")
    );
}

#[test]
fn playlists_keep_order_through_edits_and_rescans() {
    let dir = temp_dir("playlists_keep_order_through_edits_and_rescans");
    let root = dir.join("music");
    music(&root);
    let art = dir.join("art");
    let mut library = Library::open(&dir.join("library.db")).unwrap();
    library.add_folder(&root).unwrap();
    run_scan(&mut library, &art);
    let ids: Vec<i64> = library.tracks().unwrap().iter().map(|t| t.id).collect();
    let titles = |library: &Library, playlist| -> Vec<String> {
        library
            .playlist_entries(playlist)
            .unwrap()
            .into_iter()
            .map(|entry| entry.track.title)
            .collect()
    };

    let mix = library.create_playlist("  Late night  ").unwrap();
    library
        .add_to_playlist(mix, &[ids[0], ids[1], ids[2]])
        .unwrap();
    library.add_to_playlist(mix, &[ids[0]]).unwrap(); // the same track twice
    assert_eq!(
        titles(&library, mix),
        ["Watch That Man", "Aladdin Sane", "a", "Watch That Man"]
    );

    let entries = library.playlist_entries(mix).unwrap();
    library.move_in_playlist(entries[2].id, 0).unwrap();
    library.remove_from_playlist(entries[3].id).unwrap();
    assert_eq!(
        titles(&library, mix),
        ["a", "Watch That Man", "Aladdin Sane"]
    );
    library.move_in_playlist(entries[2].id, 99).unwrap(); // clamped to the end
    assert_eq!(
        titles(&library, mix),
        ["Watch That Man", "Aladdin Sane", "a"]
    );

    library.rename_playlist(mix, "Night").unwrap();
    let listed = library.playlists().unwrap();
    assert_eq!((listed[0].name.as_str(), listed[0].tracks), ("Night", 3));

    // A deleted file leaves the playlist; the order of the rest holds.
    fs::remove_file(root.join("Bowie/02.dff")).unwrap();
    run_scan(&mut library, &art);
    assert_eq!(titles(&library, mix), ["Watch That Man", "a"]);
    library.add_to_playlist(mix, &[ids[3]]).unwrap();
    assert_eq!(titles(&library, mix), ["Watch That Man", "a", "b"]);

    // User mistakes are explained, not panicked on.
    assert!(matches!(
        library.create_playlist("   "),
        Err(LibraryError::Invalid(_))
    ));
    assert!(matches!(
        library.add_to_playlist(mix, &[-5]),
        Err(LibraryError::Invalid(_))
    ));
    assert!(matches!(
        library.rename_playlist(-1, "x"),
        Err(LibraryError::Invalid(_))
    ));

    library.delete_playlist(mix).unwrap();
    assert_eq!(library.playlists().unwrap(), []);
    assert_eq!(library.tracks().unwrap().len(), 3, "tracks stay");
}

#[test]
fn an_unplugged_drive_keeps_its_tracks_and_playlists() {
    let dir = temp_dir("an_unplugged_drive_keeps_its_tracks_and_playlists");
    let (mut library, root, art, playlist) = library_with_playlist(&dir);
    let before = playlist_ids(&library, playlist);
    assert_eq!(before.len(), 4);

    // Unplugged: the folder is gone.
    let away = dir.join("away");
    fs::rename(&root, &away).unwrap();
    let missing = run_scan(&mut library, &art);
    assert_eq!(missing.removed, 0, "{missing:?}");
    assert_eq!(missing.unavailable, std::slice::from_ref(&root));
    assert_eq!(playlist_ids(&library, playlist), before);

    // Unmounted: an empty mount point is left behind.
    fs::create_dir(&root).unwrap();
    let empty = run_scan(&mut library, &art);
    assert_eq!(
        (empty.removed, empty.unavailable.len()),
        (0, 1),
        "{empty:?}"
    );
    assert_eq!(playlist_ids(&library, playlist), before);

    // Back again: nothing to do, same tracks.
    fs::remove_dir(&root).unwrap();
    fs::rename(&away, &root).unwrap();
    let back = run_scan(&mut library, &art);
    assert_eq!(
        (
            back.added,
            back.updated,
            back.removed,
            back.unavailable.len()
        ),
        (0, 0, 0, 0),
        "{back:?}"
    );
    assert_eq!(playlist_ids(&library, playlist), before);
}

#[test]
fn an_unreadable_folder_keeps_its_tracks() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir("an_unreadable_folder_keeps_its_tracks");
    let (mut library, root, art, playlist) = library_with_playlist(&dir);
    let before = playlist_ids(&library, playlist);
    let bowie = root.join("Bowie");
    fs::set_permissions(&bowie, fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::read_dir(&bowie).is_ok(); // root ignores permissions
    let summary = run_scan(&mut library, &art);
    fs::set_permissions(&bowie, fs::Permissions::from_mode(0o755)).unwrap();
    if readable {
        return;
    }
    assert_eq!(summary.removed, 0, "{summary:?}");
    assert_eq!(playlist_ids(&library, playlist), before);
}

#[test]
fn a_file_that_fails_to_read_keeps_its_entry() {
    let dir = temp_dir("a_file_that_fails_to_read_keeps_its_entry");
    let (mut library, root, art, playlist) = library_with_playlist(&dir);
    let before = playlist_ids(&library, playlist);
    let file = root.join("Test Tones/a.flac");
    let good = fs::read(&file).unwrap();

    fs::write(&file, b"half written").unwrap();
    let broken = run_scan(&mut library, &art);
    // `broken.flac` of the fixture library fails on every scan too.
    assert_eq!((broken.failed, broken.removed), (2, 0), "{broken:?}");
    assert_eq!(playlist_ids(&library, playlist), before);

    // Readable again: read and updated in place.
    fs::write(&file, good).unwrap();
    let fixed = run_scan(&mut library, &art);
    assert_eq!((fixed.updated, fixed.failed), (1, 1), "{fixed:?}");
    assert_eq!(playlist_ids(&library, playlist), before);
}

#[test]
fn adding_a_parent_folder_keeps_tracks_and_playlists() {
    let dir = temp_dir("adding_a_parent_folder_keeps_tracks_and_playlists");
    let root = dir.join("music");
    music(&root);
    let art = dir.join("art");
    let mut library = Library::open(&dir.join("library.db")).unwrap();
    library.add_folder(&root.join("Bowie")).unwrap();
    run_scan(&mut library, &art);
    let ids: Vec<i64> = library.tracks().unwrap().iter().map(|t| t.id).collect();
    let playlist = library.create_playlist("Bowie").unwrap();
    library.add_to_playlist(playlist, &ids).unwrap();

    library.add_folder(&root).unwrap();
    assert_eq!(playlist_ids(&library, playlist), ids);
    let summary = run_scan(&mut library, &art);
    // Only the new folder's two readable files are new.
    assert_eq!((summary.added, summary.removed), (2, 0), "{summary:?}");
    assert_eq!(playlist_ids(&library, playlist), ids);
}

#[test]
fn spellings_of_one_artist_are_one_artist() {
    let dir = temp_dir("spellings_of_one_artist_are_one_artist");
    let root = dir.join("music");
    fs::create_dir_all(&root).unwrap();
    // Composed, capitals, and a decomposed ö with a stray zero-width space.
    for (file, artist) in [
        ("1.dsf", "Bj\u{f6}rk"),
        ("2.dsf", "BJ\u{d6}RK"),
        ("3.dsf", "Bjo\u{308}rk\u{200B}"),
    ] {
        let mut tag = Tag::new();
        tag.set_title(file);
        tag.set_artist(artist);
        tag.set_album("Homogenic");
        let mut bytes = Vec::new();
        tag.write_to(&mut bytes, Version::Id3v24).unwrap();
        fs::write(root.join(file), tagged_dsf(&bytes)).unwrap();
    }
    let mut library = Library::open(&dir.join("library.db")).unwrap();
    library.add_folder(&root).unwrap();
    run_scan(&mut library, &dir.join("art"));

    let artists = library.artists().unwrap();
    assert_eq!(artists.len(), 1, "{artists:?}");
    // The first spelling read names the artist.
    assert_eq!(artists[0].name, "Bj\u{f6}rk");
    assert_eq!((artists[0].albums, artists[0].tracks), (1, 3));
}

#[test]
fn reads_gain_tags_from_flac_and_dsf() {
    let dir = temp_dir("reads_gain_tags_from_flac_and_dsf");
    let flac = dir.join("gain.flac");
    fs::write(
        &flac,
        flac_with_comments(&[b"REPLAYGAIN_TRACK_GAIN=-6.52 dB", b"R128_ALBUM_GAIN=-1280"]),
    )
    .unwrap();
    assert_eq!(
        read_tags(&flac).unwrap().loudness,
        TaggedLoudness {
            track: Some(-1148),
            album: Some(-1800),
        }
    );

    let mut tag = Tag::new();
    tag.add_frame(id3::frame::ExtendedText {
        description: "REPLAYGAIN_ALBUM_GAIN".into(),
        value: "+1.50 dB".into(),
    });
    let mut bytes = Vec::new();
    tag.write_to(&mut bytes, Version::Id3v24).unwrap();
    let dsf = dir.join("gain.dsf");
    fs::write(&dsf, tagged_dsf(&bytes)).unwrap();
    assert_eq!(
        read_tags(&dsf).unwrap().loudness,
        TaggedLoudness {
            track: None,
            album: Some(-1950),
        }
    );
}
