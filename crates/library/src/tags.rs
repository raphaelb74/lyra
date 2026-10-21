//! Reads what the library needs from one file: stream properties, tags and
//! an embedded cover. Tags and pictures are untrusted; sizes are capped.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use format_dsd::{Container, DsdReader};
use id3::TagLike;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::common::Limit;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::{MetadataOptions, StandardTag, StandardVisualKey, Visual};

/// Largest embedded picture kept (bigger ones are skipped).
pub const MAX_PICTURE_BYTES: usize = 16 * 1024 * 1024;
/// Largest ID3v2 tag read from a DSF/DFF file.
const MAX_ID3_BYTES: u32 = 32 * 1024 * 1024;
/// Longest text field kept, in bytes.
const MAX_TEXT_BYTES: usize = 1024;

/// One audio file, as the library stores it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scanned {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub year: Option<i32>,
    /// `flac`, `wav`, `dsf` or `dff`.
    pub codec: &'static str,
    /// PCM sample rate, or DSD bit rate per channel.
    pub sample_rate: u32,
    /// 1 for DSD.
    pub bits_per_sample: u32,
    pub channels: u32,
    pub duration_ms: Option<u64>,
    pub picture: Option<Picture>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// `image/jpeg`, `image/png` or `image/webp`.
    pub mime: &'static str,
    pub data: Vec<u8>,
}

/// Reads a FLAC, WAV, DSF or DFF file. The error is a short reason for the
/// scan report.
pub fn read(path: &Path) -> Result<Scanned, String> {
    let is_dsd = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("dsf") || extension.eq_ignore_ascii_case("dff")
        });
    if is_dsd {
        read_dsd(path)
    } else {
        read_pcm(path)
    }
}

fn read_pcm(path: &Path) -> Result<Scanned, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        hint.with_extension(extension);
    }
    let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let options = MetadataOptions::default()
        .limit_tag_bytes(Limit::Maximum(MAX_ID3_BYTES as usize))
        .limit_visual_bytes(Limit::Maximum(MAX_PICTURE_BYTES));
    let mut reader = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), options)
        .map_err(|error| error.to_string())?;

    let mut scanned = {
        let track = reader
            .default_track(TrackType::Audio)
            .ok_or("no audio track")?;
        let Some(CodecParameters::Audio(params)) = &track.codec_params else {
            return Err("no audio codec parameters".into());
        };
        let sample_rate = params.sample_rate.ok_or("sample rate missing")?;
        Scanned {
            codec: if reader.format_info().short_name == "wave" {
                "wav"
            } else {
                "flac"
            },
            sample_rate,
            bits_per_sample: params.bits_per_sample.unwrap_or(0),
            channels: params
                .channels
                .as_ref()
                .map_or(0, |channels| u32::try_from(channels.count()).unwrap_or(0)),
            duration_ms: track
                .num_frames
                .map(|frames| frames.saturating_mul(1000) / u64::from(sample_rate.max(1))),
            ..Scanned::default()
        }
    };

    let mut metadata = reader.metadata();
    while let Some(revision) = metadata.pop() {
        let containers = std::iter::once(revision.media)
            .chain(revision.per_track.into_iter().map(|track| track.metadata));
        for container in containers {
            for tag in container.tags {
                if let Some(std) = tag.std {
                    apply_standard(&mut scanned, std);
                }
            }
            if scanned.picture.is_none() {
                scanned.picture = best_visual(container.visuals);
            }
        }
    }
    Ok(scanned)
}

fn apply_standard(scanned: &mut Scanned, tag: StandardTag) {
    let set = |field: &mut Option<String>, value: &str| {
        if field.is_none() {
            *field = text(value);
        }
    };
    match tag {
        StandardTag::TrackTitle(value) => set(&mut scanned.title, &value),
        StandardTag::Artist(value) => set(&mut scanned.artist, &value),
        StandardTag::Album(value) => set(&mut scanned.album, &value),
        StandardTag::AlbumArtist(value) => set(&mut scanned.album_artist, &value),
        StandardTag::TrackNumber(number) => {
            scanned.track = scanned.track.or(u32::try_from(number).ok());
        }
        StandardTag::DiscNumber(number) => {
            scanned.disc = scanned.disc.or(u32::try_from(number).ok());
        }
        StandardTag::ReleaseYear(year) | StandardTag::RecordingYear(year) => {
            scanned.year = scanned.year.or(Some(i32::from(year)));
        }
        StandardTag::ReleaseDate(date) | StandardTag::RecordingDate(date) => {
            scanned.year = scanned.year.or_else(|| year_of(&date));
        }
        _ => {}
    }
}

/// The front cover if there is one, else the first picture.
fn best_visual(visuals: Vec<Visual>) -> Option<Picture> {
    let mut first = None;
    for visual in visuals {
        let Some(mime) = visual.media_type.as_deref().and_then(image_mime) else {
            continue;
        };
        let front = visual.usage == Some(StandardVisualKey::FrontCover);
        let picture = Picture {
            mime,
            data: visual.data.into_vec(),
        };
        if front {
            return Some(picture);
        }
        first.get_or_insert(picture);
    }
    first
}

fn read_dsd(path: &Path) -> Result<Scanned, String> {
    let reader = DsdReader::open(path).map_err(|error| error.to_string())?;
    let info = *reader.info();
    drop(reader);
    let mut scanned = Scanned {
        codec: match info.container {
            Container::Dsf => "dsf",
            Container::Dff => "dff",
        },
        sample_rate: info.dsd_rate,
        bits_per_sample: 1,
        channels: u32::try_from(info.channels).unwrap_or(0),
        duration_ms: Some(
            info.total_frames.saturating_mul(1000) / u64::from(info.frame_rate().max(1)),
        ),
        ..Scanned::default()
    };
    // A broken or missing tag leaves the file playable, just untitled.
    if let Ok(Some(tag)) = read_id3(path) {
        apply_id3(&mut scanned, &tag);
    }
    Ok(scanned)
}

fn read_id3(path: &Path) -> io::Result<Option<id3::Tag>> {
    let mut file = File::open(path)?;
    let Some(offset) = format_dsd::tag_offset(&mut file).ok().flatten() else {
        return Ok(None);
    };
    file.seek(SeekFrom::Start(offset))?;
    let mut header = [0; 10];
    file.read_exact(&mut header)?;
    // ID3v2 header: "ID3", version, flags, then a 28-bit synchsafe size.
    if &header[..3] != b"ID3" || header[6..].iter().any(|byte| byte & 0x80 != 0) {
        return Ok(None);
    }
    let size = header[6..]
        .iter()
        .fold(0_u32, |size, &byte| (size << 7) | u32::from(byte));
    if size > MAX_ID3_BYTES {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(offset))?;
    let limited = io::BufReader::new(file.take(u64::from(size) + 20));
    Ok(id3::Tag::read_from2(SeekableTake(limited)).ok())
}

/// `id3` wants `Seek` but only reads forward from the start; the reader is
/// bounded to the tag so a bad size cannot make it read the whole file.
struct SeekableTake<R>(R);

impl<R: Read> Read for SeekableTake<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}

impl<R: Read> Seek for SeekableTake<R> {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "tag reader is forward-only",
        ))
    }
}

fn apply_id3(scanned: &mut Scanned, tag: &id3::Tag) {
    scanned.title = tag.title().and_then(text);
    scanned.artist = tag.artist().and_then(text);
    scanned.album = tag.album().and_then(text);
    scanned.album_artist = tag.album_artist().and_then(text);
    scanned.track = tag.track();
    scanned.disc = tag.disc();
    scanned.year = tag
        .year()
        .or_else(|| tag.date_released().map(|date| date.year))
        .or_else(|| tag.date_recorded().map(|date| date.year));
    let mut first = None;
    for picture in tag.pictures() {
        let Some(mime) = image_mime(&picture.mime_type) else {
            continue;
        };
        if picture.data.len() > MAX_PICTURE_BYTES {
            continue;
        }
        let candidate = || Picture {
            mime,
            data: picture.data.clone(),
        };
        if picture.picture_type == id3::frame::PictureType::CoverFront {
            scanned.picture = Some(candidate());
            return;
        }
        if first.is_none() {
            first = Some(candidate());
        }
    }
    scanned.picture = first;
}

/// Image types the webview shows; anything else is ignored.
pub fn image_mime(mime: &str) -> Option<&'static str> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/png" => Some("image/png"),
        "image/webp" => Some("image/webp"),
        _ => None,
    }
}

/// Trimmed, non-empty, capped at [`MAX_TEXT_BYTES`] on a character boundary.
fn text(value: &str) -> Option<String> {
    let value = value.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    if value.is_empty() {
        return None;
    }
    let mut end = value.len().min(MAX_TEXT_BYTES);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    Some(value[..end].to_owned())
}

/// `1973`, `1973-04-13` → 1973.
fn year_of(date: &str) -> Option<i32> {
    let digits = date.trim().get(..4)?;
    digits
        .bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| digits.parse().ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::{image_mime, text, year_of};

    #[test]
    fn cleans_untrusted_text() {
        assert_eq!(text("  Aladdin Sane\0"), Some("Aladdin Sane".into()));
        assert_eq!(text(" \0 "), None);
        let long = "é".repeat(600); // 1200 bytes
        assert_eq!(text(&long).map(|value| value.len()), Some(1024));
        assert_eq!(year_of("1973-04-13"), Some(1973));
        assert_eq!(year_of("73"), None);
        assert_eq!(image_mime("IMAGE/JPG"), Some("image/jpeg"));
        assert_eq!(image_mime("image/tiff"), None);
    }
}
