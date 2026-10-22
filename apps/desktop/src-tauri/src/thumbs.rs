//! Small copies of album covers for grids and lists.
//!
//! Embedded covers are often several megabytes at 1500 px or more; a grid
//! of them decodes slowly while scrolling. A thumbnail is made on first
//! request with gdk-pixbuf (already in the app through GTK) and kept beside
//! the originals in `thumbs/`, which the library clears with them.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gdk_pixbuf::Pixbuf;

/// Longest side of a thumbnail: sharp at 2× for the largest grid cell.
const SIZE: i32 = 512;
const JPEG_QUALITY: &str = "85";

/// Makes temporary names unique when two requests make the same thumbnail.
static PARTS: AtomicU64 = AtomicU64::new(0);

/// The thumbnail of artwork file `name` (already checked to be a cache
/// name), made if needed. The original when it is small already or cannot
/// be scaled; `None` when there is no such artwork.
pub fn thumbnail(art_dir: &Path, name: &str) -> Option<PathBuf> {
    let original = art_dir.join(name);
    let thumb = art_dir.join(library::THUMB_DIR).join(name);
    if thumb.is_file() {
        return Some(thumb);
    }
    let (_, width, height) = Pixbuf::file_info(&original)?;
    if width <= SIZE && height <= SIZE {
        return Some(original);
    }
    match make(&original, &thumb, name) {
        Some(()) => Some(thumb),
        None => Some(original),
    }
}

fn make(original: &Path, thumb: &Path, name: &str) -> Option<()> {
    // Cache names are lower case: `<album>-<time>.<jpg|png|webp>`.
    let png = Path::new(name).extension().is_some_and(|ext| ext == "png");
    let format = if png { "png" } else { "jpeg" };
    let options: &[(&str, &str)] = if format == "jpeg" {
        &[("quality", JPEG_QUALITY)]
    } else {
        &[]
    };
    let bytes = Pixbuf::from_file_at_scale(original, SIZE, SIZE, true)
        .ok()?
        .save_to_bufferv(format, options)
        .ok()?;
    let folder = thumb.parent()?;
    fs::create_dir_all(folder).ok()?;
    let part = folder.join(format!(
        "{name}.{}.part",
        PARTS.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&part, bytes).ok()?;
    fs::rename(&part, thumb).ok().or_else(|| {
        let _ = fs::remove_file(&part);
        None
    })
}
