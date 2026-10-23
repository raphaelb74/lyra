//! Artwork the user chooses: kept through rescans and a cleared cache,
//! reset to what the files have.
#![expect(clippy::unwrap_used, reason = "test helpers fail loudly")]

mod common;

use std::fs;
use std::path::Path;

use library::{ArtworkSource, Library, LibraryError};

use common::{JPEG, Tags, dsf, run_scan, scanned_library, temp_dir};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really a picture, but the right start";

fn art(library: &Library, art_dir: &Path) -> Vec<u8> {
    let name = library.albums().unwrap()[0].art.clone().unwrap();
    fs::read(art_dir.join(name)).unwrap()
}

#[test]
fn chosen_artwork_stays_until_reset() {
    let dir = temp_dir("chosen_artwork_stays_until_reset");
    let root = dir.join("music");
    let art_dir = dir.join("art");
    let mut tags = Tags::new("Mysterons", "Portishead", "Dummy");
    tags.cover = true;
    dsf(&root.join("Dummy/1.dsf"), &tags);
    let mut library = scanned_library(&dir, &root);
    let album = library.albums().unwrap()[0].id;
    assert_eq!(art(&library, &art_dir), JPEG, "the file's cover");

    let image = dir.join("picked.png");
    fs::write(&image, PNG).unwrap();
    library.choose_artwork(&art_dir, album, &image).unwrap();
    assert_eq!(art(&library, &art_dir), PNG);
    assert_eq!(
        fs::read_dir(&art_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().is_file())
            .count(),
        1,
        "the old cover's file is gone"
    );

    // A rescan, a full re-read, and a cleared cache keep it.
    run_scan(&mut library, &art_dir);
    assert_eq!(art(&library, &art_dir), PNG);
    let conn = rusqlite::Connection::open(dir.join("library.db")).unwrap();
    conn.execute("UPDATE tracks SET mtime_ns = -1", []).unwrap();
    drop(conn);
    fs::remove_dir_all(&art_dir).unwrap();
    run_scan(&mut library, &art_dir);
    assert_eq!(art(&library, &art_dir), PNG);

    library.reset_artwork(&art_dir, album).unwrap();
    assert_eq!(art(&library, &art_dir), JPEG);
}

#[test]
fn only_images_can_be_chosen() {
    let dir = temp_dir("only_images_can_be_chosen");
    let root = dir.join("music");
    let art_dir = dir.join("art");
    dsf(
        &root.join("Dummy/1.dsf"),
        &Tags::new("Mysterons", "Portishead", "Dummy"),
    );
    let mut library = scanned_library(&dir, &root);
    let album = library.albums().unwrap()[0].id;

    let text = dir.join("notes.png");
    fs::write(&text, "not an image").unwrap();
    let result = library.choose_artwork(&art_dir, album, &text);
    assert!(
        matches!(result, Err(LibraryError::Invalid(_))),
        "{result:?}"
    );
    let image = dir.join("picked.png");
    fs::write(&image, PNG).unwrap();
    let result = library.choose_artwork(&art_dir, album + 99, &image);
    assert!(
        matches!(result, Err(LibraryError::Invalid(_))),
        "{result:?}"
    );
    assert_eq!(library.albums().unwrap()[0].art, None);

    // No cover in the files: a reset shows none.
    library.choose_artwork(&art_dir, album, &image).unwrap();
    library.reset_artwork(&art_dir, album).unwrap();
    assert_eq!(library.albums().unwrap()[0].art, None);
}

#[test]
fn downloaded_artwork_keeps_its_source() {
    let dir = temp_dir("downloaded_artwork_keeps_its_source");
    let root = dir.join("music");
    let art_dir = dir.join("art");
    let mut tags = Tags::new("Mysterons", "Portishead", "Dummy");
    tags.cover = true;
    dsf(&root.join("Dummy/1.dsf"), &tags);
    let mut library = scanned_library(&dir, &root);
    let album = library.albums().unwrap()[0].id;
    assert_eq!(library.artwork_source(album).unwrap(), ArtworkSource::Files);

    library
        .set_artwork(&art_dir, album, PNG.to_vec(), Some("Cover Art Archive"))
        .unwrap();
    assert_eq!(art(&library, &art_dir), PNG);
    assert_eq!(
        library.artwork_source(album).unwrap(),
        ArtworkSource::Online("Cover Art Archive".to_owned())
    );
    run_scan(&mut library, &art_dir);
    assert_eq!(art(&library, &art_dir), PNG, "a rescan keeps it");

    // Not an image, or too large: refused, and the cover stays.
    let mut huge = PNG.to_vec();
    huge.resize(16 * 1024 * 1024 + 1, 0);
    for data in [b"<html>".to_vec(), huge] {
        let result = library.set_artwork(&art_dir, album, data, Some("Cover Art Archive"));
        assert!(
            matches!(result, Err(LibraryError::Invalid(_))),
            "{result:?}"
        );
    }
    assert_eq!(art(&library, &art_dir), PNG);

    // A file the user picks replaces it, and says so.
    let image = dir.join("picked.jpg");
    fs::write(&image, JPEG).unwrap();
    library.choose_artwork(&art_dir, album, &image).unwrap();
    assert_eq!(
        library.artwork_source(album).unwrap(),
        ArtworkSource::Picked
    );

    library.reset_artwork(&art_dir, album).unwrap();
    assert_eq!(library.artwork_source(album).unwrap(), ArtworkSource::Files);
    assert_eq!(art(&library, &art_dir), JPEG, "the file's cover again");
}
