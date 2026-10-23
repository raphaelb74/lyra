//! Scanner and library behaviour on real files in a temporary folder.
#![expect(clippy::unwrap_used, reason = "test helpers fail loudly")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use id3::{Tag, TagLike, Version};
use std::sync::mpsc;
use std::time::Duration;

use library::{
    Edit, EditChange, Field, Library, LibraryError, Origin, SavedEdit, ScanEvent, ScanSummary,
    Scanner, Target, scan,
};

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
    // ID3v1 genre 17, written the old numeric way.
    tag.set_genre("(17)");
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

/// A library of `music(root)`, scanned, with every track in one playlist.
fn library_with_playlist(dir: &Path) -> (Library, PathBuf, PathBuf, i64) {
    let root = dir.join("music");
    music(&root);
    let art = dir.join("art");
    let mut library = Library::open(&dir.join("library.db")).unwrap();
    library.add_folder(&root).unwrap();
    run_scan(&mut library, &art);
    let ids: Vec<i64> = library.tracks().unwrap().iter().map(|t| t.id).collect();
    let playlist = library.create_playlist("Keep").unwrap();
    library.add_to_playlist(playlist, &ids).unwrap();
    (library, root, art, playlist)
}

fn playlist_ids(library: &Library, playlist: i64) -> Vec<i64> {
    library
        .playlist_entries(playlist)
        .unwrap()
        .into_iter()
        .map(|entry| entry.track.id)
        .collect()
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

/// Tags for [`dsf`]: title, artist, album, and whatever else a test needs.
struct Tags<'a> {
    title: &'a str,
    artist: &'a str,
    album: &'a str,
    album_artist: Option<&'a str>,
    release_id: Option<&'a str>,
    compilation: bool,
    cover: bool,
}

impl<'a> Tags<'a> {
    fn new(title: &'a str, artist: &'a str, album: &'a str) -> Self {
        Tags {
            title,
            artist,
            album,
            album_artist: None,
            release_id: None,
            compilation: false,
            cover: false,
        }
    }
}

/// Writes a DSF file tagged with `tags`, creating its folder.
fn dsf(path: &Path, tags: &Tags<'_>) {
    let mut tag = Tag::new();
    tag.set_title(tags.title);
    tag.set_artist(tags.artist);
    tag.set_album(tags.album);
    if let Some(album_artist) = tags.album_artist {
        tag.set_album_artist(album_artist);
    }
    if let Some(id) = tags.release_id {
        tag.add_frame(id3::frame::ExtendedText {
            description: "MusicBrainz Album Id".into(),
            value: id.into(),
        });
    }
    if tags.compilation {
        tag.set_text("TCMP", "1");
    }
    if tags.cover {
        tag.add_frame(id3::frame::Picture {
            mime_type: "image/jpeg".into(),
            picture_type: id3::frame::PictureType::CoverFront,
            description: String::new(),
            data: JPEG.to_vec(),
        });
    }
    let mut bytes = Vec::new();
    tag.write_to(&mut bytes, Version::Id3v24).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, tagged_dsf(&bytes)).unwrap();
}

/// A library over `root`, scanned once.
fn scanned_library(dir: &Path, root: &Path) -> Library {
    let mut library = Library::open(&dir.join("library.db")).unwrap();
    library.add_folder(root).unwrap();
    run_scan(&mut library, &dir.join("art"));
    library
}

/// (title, album artist, tracks) of every album, sorted.
fn album_list(library: &Library) -> Vec<(String, String, u32)> {
    let mut albums: Vec<_> = library
        .albums()
        .unwrap()
        .into_iter()
        .map(|album| (album.title, album.artist, album.tracks))
        .collect();
    albums.sort();
    albums
}

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

/// (id, file name) of every track, by path.
fn track_ids(library: &Library) -> Vec<(i64, String)> {
    let mut tracks: Vec<_> = library
        .tracks()
        .unwrap()
        .into_iter()
        .map(|track| {
            let name = track
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (track.id, name)
        })
        .collect();
    tracks.sort_by(|a, b| a.1.cmp(&b.1));
    tracks
}

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

fn change(target: Target, field: Field, edit: Edit) -> EditChange {
    EditChange {
        target,
        field,
        edit,
    }
}

fn set(target: Target, field: Field, value: &str) -> EditChange {
    change(target, field, Edit::Set(value.to_owned()))
}

/// One album of two tracks, its track IDs, and the album's.
fn edited_library(dir: &Path, root: &Path) -> (Library, [i64; 2], i64) {
    for (file, title) in [("1.dsf", "Mysterons"), ("2.dsf", "Sour Times")] {
        dsf(
            &root.join("Dummy").join(file),
            &Tags::new(title, "Portishead", "Dummy"),
        );
    }
    let library = scanned_library(dir, root);
    let tracks = track_ids(&library);
    let album = library.albums().unwrap()[0].id;
    (library, [tracks[0].0, tracks[1].0], album)
}

fn title_of(library: &Library, id: i64) -> String {
    library.track(id).unwrap().unwrap().title
}

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
