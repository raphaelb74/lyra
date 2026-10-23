//! How tracks group into releases, and releases keep their IDs.

mod common;

use library::Library;
use std::fs;

use common::{
    Tags, album_list, dsf, fixture, music, playlist_ids, run_scan, scanned_library, temp_dir,
};

#[test]
fn editions_in_two_folders_stay_apart() {
    let dir = temp_dir("editions_in_two_folders_stay_apart");
    let root = dir.join("music");
    for folder in ["Homogenic (1997)", "Homogenic (2024 Remaster)"] {
        for file in ["1.dsf", "2.dsf"] {
            dsf(
                &root.join(folder).join(file),
                &Tags::new(file, "Bj\u{f6}rk", "Homogenic"),
            );
        }
    }
    let library = scanned_library(&dir, &root);
    let homogenic = ("Homogenic".to_owned(), "Bj\u{f6}rk".to_owned(), 2);
    assert_eq!(album_list(&library), [homogenic.clone(), homogenic]);
}

#[test]
fn disc_folders_are_one_release() {
    let dir = temp_dir("disc_folders_are_one_release");
    let root = dir.join("music");
    let album = root.join("Mellon Collie");
    for (folder, title) in [("CD1", "Tonight"), ("CD2", "Where Boys Fear")] {
        dsf(
            &album.join(folder).join("01.dsf"),
            &Tags::new(title, "The Smashing Pumpkins", "Mellon Collie"),
        );
    }
    // Untagged files are named after the release folder, not the disc.
    fs::create_dir_all(root.join("Tones").join("Disc 1")).unwrap();
    fs::write(
        root.join("Tones").join("Disc 1").join("a.flac"),
        fixture("format-pcm", "s16_44100.flac"),
    )
    .unwrap();
    let library = scanned_library(&dir, &root);

    let albums = library.albums().unwrap();
    assert_eq!(
        album_list(&library),
        [
            (
                "Mellon Collie".to_owned(),
                "The Smashing Pumpkins".to_owned(),
                2
            ),
            ("Tones".to_owned(), "Unknown Artist".to_owned(), 1),
        ]
    );
    let mellon = albums.iter().find(|a| a.title == "Mellon Collie").unwrap();
    let discs: Vec<_> = library
        .album_tracks(mellon.id)
        .unwrap()
        .into_iter()
        .map(|track| (track.disc, track.title))
        .collect();
    assert_eq!(
        discs,
        [
            (Some(1), "Tonight".to_owned()),
            (Some(2), "Where Boys Fear".to_owned())
        ],
        "the folder gives the disc number when the tag does not"
    );
}

#[test]
fn compilations_are_one_release() {
    let dir = temp_dir("compilations_are_one_release");
    let root = dir.join("music");
    // No album-artist tag, different artists: Various Artists.
    for (file, artist) in [("1.dsf", "Air"), ("2.dsf", "Moby"), ("3.dsf", "Air")] {
        dsf(
            &root.join("Chill Out").join(file),
            &Tags::new(file, artist, "Chill Out"),
        );
    }
    // No album-artist tag, one artist: that artist.
    for file in ["1.dsf", "2.dsf"] {
        dsf(
            &root.join("Moon Safari").join(file),
            &Tags::new(file, "Air", "Moon Safari"),
        );
    }
    // The compilation flag wins over agreeing artists.
    for file in ["1.dsf", "2.dsf"] {
        let mut tags = Tags::new(file, "Moby", "Best Of Chill");
        tags.compilation = true;
        dsf(&root.join("Best Of Chill").join(file), &tags);
    }
    // A tagged album artist wins over everything.
    for (file, artist) in [("1.dsf", "Massive Attack"), ("2.dsf", "Tricky")] {
        let mut tags = Tags::new(file, artist, "Blue Lines");
        tags.album_artist = Some("Massive Attack");
        dsf(&root.join("Blue Lines").join(file), &tags);
    }
    let library = scanned_library(&dir, &root);
    let various = "Various Artists".to_owned();
    assert_eq!(
        album_list(&library),
        [
            ("Best Of Chill".to_owned(), various.clone(), 2),
            ("Blue Lines".to_owned(), "Massive Attack".to_owned(), 2),
            ("Chill Out".to_owned(), various, 3),
            ("Moon Safari".to_owned(), "Air".to_owned(), 2),
        ]
    );
    // Track artists are kept as tagged.
    let artists: Vec<_> = library
        .tracks()
        .unwrap()
        .into_iter()
        .filter(|track| track.album == "Chill Out")
        .map(|track| track.artist)
        .collect();
    assert_eq!(artists.len(), 3);
    assert!(artists.contains(&"Moby".to_owned()));
}

#[test]
fn release_ids_tell_editions_apart() {
    let dir = temp_dir("release_ids_tell_editions_apart");
    let root = dir.join("music");
    // One folder, two editions; and one edition split over two folders.
    for (path, id) in [
        ("Mixed/a.dsf", "a1b2c3d4-0000-4000-8000-000000000001"),
        ("Mixed/b.dsf", "a1b2c3d4-0000-4000-8000-000000000002"),
        ("Elsewhere/c.dsf", "A1B2C3D4-0000-4000-8000-000000000002"),
    ] {
        let mut tags = Tags::new(path, "Radiohead", "OK Computer");
        tags.release_id = Some(id);
        dsf(&root.join(path), &tags);
    }
    let library = scanned_library(&dir, &root);
    let mut counts: Vec<_> = album_list(&library)
        .into_iter()
        .map(|(_, _, tracks)| tracks)
        .collect();
    counts.sort_unstable();
    assert_eq!(counts, [1, 2]);
}

#[test]
fn a_retagged_album_keeps_its_id_artwork_and_playlists() {
    let dir = temp_dir("a_retagged_album_keeps_its_id_artwork_and_playlists");
    let root = dir.join("music");
    let write = |album: &str, cover: bool| {
        for file in ["1.dsf", "2.dsf"] {
            let mut tags = Tags::new(file, "Portishead", album);
            tags.cover = cover;
            dsf(&root.join("Dummy").join(file), &tags);
        }
    };
    write("Dumy", true);
    let mut library = scanned_library(&dir, &root);
    let before = library.albums().unwrap();
    assert_eq!(before.len(), 1);
    assert!(before[0].art.is_some());
    let playlist = library.create_playlist("Trip hop").unwrap();
    let ids: Vec<i64> = library.tracks().unwrap().iter().map(|t| t.id).collect();
    library.add_to_playlist(playlist, &ids).unwrap();

    // Fixed in another tagger, without the cover this time.
    write("Dummy", false);
    run_scan(&mut library, &dir.join("art"));
    let after = library.albums().unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].title, "Dummy");
    assert_eq!(after[0].id, before[0].id, "same release");
    assert_eq!(after[0].art, before[0].art, "artwork kept");
    assert!(
        dir.join("art")
            .join(after[0].art.as_ref().unwrap())
            .exists()
    );
    assert_eq!(playlist_ids(&library, playlist), ids);
}

#[test]
fn a_split_album_keeps_its_id_for_one_part() {
    let dir = temp_dir("a_split_album_keeps_its_id_for_one_part");
    let root = dir.join("music");
    for file in ["1.dsf", "2.dsf"] {
        dsf(
            &root.join("Mixed").join(file),
            &Tags::new(file, "Low", "Things We Lost"),
        );
    }
    let mut library = scanned_library(&dir, &root);
    let before = library.albums().unwrap()[0].id;
    dsf(
        &root.join("Mixed").join("2.dsf"),
        &Tags::new("2.dsf", "Low", "Secret Name"),
    );
    run_scan(&mut library, &dir.join("art"));
    let after = library.albums().unwrap();
    assert_eq!(after.len(), 2);
    let kept = after
        .iter()
        .find(|album| album.title == "Things We Lost")
        .unwrap();
    assert_eq!(kept.id, before, "the part that stayed keeps the ID");
}

#[test]
fn the_upgrade_regroup_keeps_album_ids_and_artwork() {
    let dir = temp_dir("the_upgrade_regroup_keeps_album_ids_and_artwork");
    let root = dir.join("music");
    music(&root);
    let library = scanned_library(&dir, &root);
    let before: Vec<_> = library
        .albums()
        .unwrap()
        .into_iter()
        .map(|album| (album.id, album.title, album.art))
        .collect();
    drop(library);
    // What migration 5 leaves: placeholder groupings, every track to re-read.
    let conn = rusqlite::Connection::open(dir.join("library.db")).unwrap();
    conn.execute_batch(
        "UPDATE albums SET grouping = CAST('old:' || id AS BLOB);
         UPDATE tracks SET mtime_ns = -1;",
    )
    .unwrap();
    drop(conn);

    let mut library = Library::open(&dir.join("library.db")).unwrap();
    run_scan(&mut library, &dir.join("art"));
    let after: Vec<_> = library
        .albums()
        .unwrap()
        .into_iter()
        .map(|album| (album.id, album.title, album.art))
        .collect();
    assert_eq!(after, before);
}
