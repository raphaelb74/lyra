//! Scanner and library behaviour on real files in a temporary folder.
#![expect(clippy::unwrap_used, reason = "test helpers fail loudly")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use id3::{Tag, TagLike, Version};
use std::sync::mpsc;
use std::time::Duration;

use library::{Library, LibraryError, ScanEvent, ScanSummary, Scanner, scan};

/// The smallest JPEG-looking payload: the scanner stores images as found.
const JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0xFF, 0xD9,
];

fn fixture(crate_name: &str, name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(crate_name)
        .join("tests/fixtures")
        .join(name);
    fs::read(path).unwrap()
}

/// A fresh, empty directory for one test.
fn temp_dir(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(test);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn id3_tag(title: &str, track: u32, with_cover: bool) -> Vec<u8> {
    let mut tag = Tag::new();
    tag.set_title(title);
    tag.set_artist("David Bowie");
    tag.set_album("Aladdin Sane");
    tag.set_track(track);
    tag.set_year(1973);
    if with_cover {
        tag.add_frame(id3::frame::Picture {
            mime_type: "image/jpeg".into(),
            picture_type: id3::frame::PictureType::CoverFront,
            description: String::new(),
            data: JPEG.to_vec(),
        });
    }
    let mut bytes = Vec::new();
    tag.write_to(&mut bytes, Version::Id3v24).unwrap();
    bytes
}

/// The DSD64 fixture with an ID3v2 tag at the end, where the DSF metadata
/// pointer says.
fn tagged_dsf(tag: &[u8]) -> Vec<u8> {
    let mut file = fixture("format-dsd", "dsd64_stereo.dsf");
    let pointer = file.len() as u64;
    file[20..28].copy_from_slice(&pointer.to_le_bytes());
    file.extend_from_slice(tag);
    let total = file.len() as u64;
    file[12..20].copy_from_slice(&total.to_le_bytes());
    file
}

/// The DFF fixture with an `ID3 ` chunk after the sound data.
fn tagged_dff(tag: &[u8]) -> Vec<u8> {
    let mut file = fixture("format-dsd", "dsd64_stereo.dff");
    file.extend_from_slice(b"ID3 ");
    file.extend_from_slice(&(tag.len() as u64).to_be_bytes());
    file.extend_from_slice(tag);
    if tag.len() % 2 == 1 {
        file.push(0);
    }
    let form = file.len() as u64 - 12;
    file[4..12].copy_from_slice(&form.to_be_bytes());
    file
}

/// A small library: a tagged DSD album, an untagged PCM folder with a
/// cover image, a broken file, and files that must be ignored.
fn music(root: &Path) {
    let bowie = root.join("Bowie");
    fs::create_dir_all(&bowie).unwrap();
    fs::write(
        bowie.join("01.dsf"),
        tagged_dsf(&id3_tag("Watch That Man", 1, true)),
    )
    .unwrap();
    fs::write(
        bowie.join("02.dff"),
        tagged_dff(&id3_tag("Aladdin Sane", 2, false)),
    )
    .unwrap();
    fs::write(bowie.join("._01.dsf"), b"macOS resource fork").unwrap();

    let tones = root.join("Test Tones");
    fs::create_dir_all(&tones).unwrap();
    fs::write(
        tones.join("a.flac"),
        fixture("format-pcm", "s16_44100.flac"),
    )
    .unwrap();
    fs::write(tones.join("b.wav"), fixture("format-pcm", "s24_96000.wav")).unwrap();
    fs::write(tones.join("Cover.JPG"), JPEG).unwrap();
    fs::write(tones.join("broken.flac"), b"not audio").unwrap();
    fs::write(tones.join("notes.txt"), b"ignored").unwrap();
}

fn run_scan(library: &mut Library, art: &Path) -> ScanSummary {
    scan(library, art, &AtomicBool::new(false), |_| {}).unwrap()
}

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
