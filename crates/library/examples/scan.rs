//! Scans a folder into a library database and prints what it found.
//!
//! `cargo run --release -p library --example scan -- <library.db> <art dir> <music folder>`
//!
//! Reads the music folder only; writes the database and artwork cache.

use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use library::{Library, LibraryError, scan};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [db, art, folder] = args.as_slice() else {
        eprintln!("usage: scan <library.db> <art dir> <music folder>");
        return ExitCode::FAILURE;
    };
    match run(Path::new(db), Path::new(art), Path::new(folder)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(db: &Path, art: &Path, folder: &Path) -> Result<(), LibraryError> {
    let mut library = Library::open(db)?;
    match library.add_folder(folder) {
        Ok(_) | Err(LibraryError::AlreadyCovered { .. }) => {}
        Err(error) => return Err(error),
    }
    let started = Instant::now();
    let summary = scan(&mut library, art, &AtomicBool::new(false), |_| {})?;
    let elapsed = started.elapsed();
    println!(
        "{} added, {} updated, {} removed, {} failed in {elapsed:.2?}",
        summary.added, summary.updated, summary.removed, summary.failed
    );
    for (path, reason) in &summary.failures {
        println!("  failed: {}: {reason}", path.display());
    }
    let albums = library.albums()?;
    let with_art = albums.iter().filter(|album| album.art.is_some()).count();
    println!("{} albums, {with_art} with artwork", albums.len());
    for album in albums.iter().take(15) {
        println!(
            "  {} — {} ({}) · {} tracks · {} {}",
            album.artist,
            album.title,
            album
                .year
                .map_or_else(|| "?".into(), |year| year.to_string()),
            album.tracks,
            album.best_codec,
            album.best_rate
        );
    }
    Ok(())
}
