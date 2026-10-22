//! Music library: the folders the user chose, their tracks, albums and
//! artists in SQLite, plus an artwork cache.
//!
//! Independent of Tauri and of the audio engine. The scanner reads tags only;
//! playback opens files itself.

mod art;
mod db;
mod playlists;
mod scan;
mod scanner;
mod tags;

pub use art::{MAX_CACHE_BYTES, THUMB_DIR, delete_art, is_art_name};
pub use db::{Album, Artist, Folder, Library, LibraryError, Track, UNKNOWN_ARTIST};
pub use playlists::{Entry, Playlist};
pub use scan::{ScanProgress, ScanSummary, is_audio_file, scan};
pub use scanner::{ScanEvent, Scanner};
pub use tags::{Picture, Scanned, read as read_tags};
