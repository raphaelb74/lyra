//! Helpers shared by the library's integration tests: fixtures, tagged
//! files, and a scanned library.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    reason = "each test file uses some helpers; helpers fail loudly"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use id3::{Tag, TagLike, Version};
use library::{Edit, EditChange, Field, Library, ScanSummary, Target, scan};

pub const JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0xFF, 0xD9,
];

pub fn fixture(crate_name: &str, name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(crate_name)
        .join("tests/fixtures")
        .join(name);
    fs::read(path).unwrap()
}

/// A fresh, empty directory for one test.
pub fn temp_dir(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(test);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn id3_tag(title: &str, track: u32, with_cover: bool) -> Vec<u8> {
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
pub fn tagged_dsf(tag: &[u8]) -> Vec<u8> {
    let mut file = fixture("format-dsd", "dsd64_stereo.dsf");
    let pointer = file.len() as u64;
    file[20..28].copy_from_slice(&pointer.to_le_bytes());
    file.extend_from_slice(tag);
    let total = file.len() as u64;
    file[12..20].copy_from_slice(&total.to_le_bytes());
    file
}

/// The DFF fixture with an `ID3 ` chunk after the sound data.
pub fn tagged_dff(tag: &[u8]) -> Vec<u8> {
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
pub fn music(root: &Path) {
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

pub fn run_scan(library: &mut Library, art: &Path) -> ScanSummary {
    scan(library, art, &AtomicBool::new(false), |_| {}).unwrap()
}

/// A library of `music(root)`, scanned, with every track in one playlist.
pub fn library_with_playlist(dir: &Path) -> (Library, PathBuf, PathBuf, i64) {
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

pub fn playlist_ids(library: &Library, playlist: i64) -> Vec<i64> {
    library
        .playlist_entries(playlist)
        .unwrap()
        .into_iter()
        .map(|entry| entry.track.id)
        .collect()
}

/// Tags for [`dsf`]: title, artist, album, and whatever else a test needs.
pub struct Tags<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    pub album_artist: Option<&'a str>,
    pub release_id: Option<&'a str>,
    pub compilation: bool,
    pub cover: bool,
}

impl<'a> Tags<'a> {
    pub fn new(title: &'a str, artist: &'a str, album: &'a str) -> Self {
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
pub fn dsf(path: &Path, tags: &Tags<'_>) {
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
pub fn scanned_library(dir: &Path, root: &Path) -> Library {
    let mut library = Library::open(&dir.join("library.db")).unwrap();
    library.add_folder(root).unwrap();
    run_scan(&mut library, &dir.join("art"));
    library
}

/// (title, album artist, tracks) of every album, sorted.
pub fn album_list(library: &Library) -> Vec<(String, String, u32)> {
    let mut albums: Vec<_> = library
        .albums()
        .unwrap()
        .into_iter()
        .map(|album| (album.title, album.artist, album.tracks))
        .collect();
    albums.sort();
    albums
}

/// (id, file name) of every track, by path.
pub fn track_ids(library: &Library) -> Vec<(i64, String)> {
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

pub fn change(target: Target, field: Field, edit: Edit) -> EditChange {
    EditChange {
        target,
        field,
        edit,
    }
}

pub fn set(target: Target, field: Field, value: &str) -> EditChange {
    change(target, field, Edit::Set(value.to_owned()))
}

/// One album of two tracks, its track IDs, and the album's.
pub fn edited_library(dir: &Path, root: &Path) -> (Library, [i64; 2], i64) {
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

pub fn title_of(library: &Library, id: i64) -> String {
    library.track(id).unwrap().unwrap().title
}

/// The FLAC fixture with its Vorbis comments replaced by `comments`, each
/// written as given, so a test can hold what a careless tagger leaves
/// behind (an entry without `=`, Latin-1 bytes).
pub fn flac_with_comments(comments: &[&[u8]]) -> Vec<u8> {
    let original = fixture("format-pcm", "s16_44100.flac");
    assert_eq!(&original[..4], b"fLaC");
    let mut blocks: Vec<(u8, &[u8])> = Vec::new();
    let mut at = 4;
    loop {
        let header = original[at];
        let size =
            u32::from_be_bytes([0, original[at + 1], original[at + 2], original[at + 3]]) as usize;
        // VORBIS_COMMENT (4) is replaced below.
        if header & 0x7F != 4 {
            blocks.push((header & 0x7F, &original[at + 4..at + 4 + size]));
        }
        at += 4 + size;
        if header & 0x80 != 0 {
            break;
        }
    }
    let mut comment = Vec::new();
    let vendor = b"lyra tests";
    comment.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    comment.extend_from_slice(vendor);
    comment.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for entry in comments {
        comment.extend_from_slice(&(entry.len() as u32).to_le_bytes());
        comment.extend_from_slice(entry);
    }
    // After STREAMINFO, which must come first.
    blocks.insert(1, (4, &comment));

    let mut file = b"fLaC".to_vec();
    let last = blocks.len() - 1;
    for (index, (kind, body)) in blocks.iter().enumerate() {
        let flag = if index == last { 0x80 } else { 0 };
        file.push(kind | flag);
        file.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        file.extend_from_slice(body);
    }
    file.extend_from_slice(&original[at..]);
    file
}
