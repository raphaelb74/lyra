//! Reads what the library needs from one file: stream properties, tags and
//! an embedded cover. Tags and pictures are untrusted; sizes are capped.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use format_dsd::{Container, DsdReader};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::VerificationCheck;
use symphonia::core::common::Limit;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::{
    MetadataOptions, MetadataRevision, RawTag, RawValue, StandardTag, StandardVisualKey, Visual,
};

use crate::values::{self, Field, FileValue};

/// Largest embedded picture kept (bigger ones are skipped).
pub const MAX_PICTURE_BYTES: usize = 16 * 1024 * 1024;
/// Largest ID3v2 tag read from a DSF/DFF file.
const MAX_ID3_BYTES: u32 = 32 * 1024 * 1024;

/// One audio file, as the library stores it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scanned {
    /// What the tags say, exactly, in file order.
    pub values: Vec<FileValue>,
    /// `flac`, `wav`, `dsf` or `dff`.
    pub codec: &'static str,
    /// PCM sample rate, or DSD bit rate per channel.
    pub sample_rate: u32,
    /// 1 for DSD.
    pub bits_per_sample: u32,
    pub channels: u32,
    pub duration_ms: Option<u64>,
    /// Audio frames (samples per channel), when the header says.
    pub frames: Option<u64>,
    /// FLAC: MD5 of the decoded audio, from STREAMINFO; `None` when the
    /// encoder left it blank.
    pub audio_md5: Option<[u8; 16]>,
    pub picture: Option<Picture>,
}

impl Scanned {
    /// What the library shows for `field` from this file alone.
    pub fn shown(&self, field: Field) -> Option<String> {
        values::file_shown(&self.values, field)
    }
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
            frames: track.num_frames,
            audio_md5: match params.verification_check {
                Some(VerificationCheck::Md5(md5)) if md5 != [0; 16] => Some(md5),
                _ => None,
            },
            ..Scanned::default()
        }
    };

    // Oldest revision first. `pop` hands over every revision but the
    // latest, which stays behind as `current`: a file's only tags are there.
    let mut metadata = reader.metadata();
    while let Some(revision) = metadata.pop() {
        read_revision(&mut scanned, &revision);
    }
    if let Some(latest) = metadata.current() {
        read_revision(&mut scanned, latest);
    }
    Ok(scanned)
}

fn read_revision(scanned: &mut Scanned, revision: &MetadataRevision) {
    let containers = std::iter::once(&revision.media)
        .chain(revision.per_track.iter().map(|track| &track.metadata));
    for container in containers {
        // Symphonia gives `TRACKNUMBER=3/12` twice: as the track number and
        // as the track total. The value belongs once to the field its key
        // names.
        let mut last: Option<&RawTag> = None;
        for tag in &container.tags {
            let by_key = key_field(&tag.raw.key);
            let field = match (tag.std.as_ref().and_then(std_field), by_key) {
                (Some(std), Some(key)) if std != key => {
                    if last.is_some_and(|raw| same_raw(raw, &tag.raw)) {
                        continue;
                    }
                    key
                }
                (Some(field), _) | (None, Some(field)) => field,
                (None, None) => continue,
            };
            push_raw(&mut scanned.values, field, &tag.raw.value);
            last = Some(&tag.raw);
        }
        if scanned.picture.is_none() {
            scanned.picture = best_visual(&container.visuals);
        }
    }
}

fn push_raw(values: &mut Vec<FileValue>, field: Field, raw: &RawValue) {
    match raw {
        RawValue::String(value) => values.push(FileValue::new(field, value)),
        RawValue::StringList(list) => {
            values.extend(list.iter().map(|value| FileValue::new(field, value)));
        }
        RawValue::UnsignedInt(n) => values.push(FileValue::new(field, &n.to_string())),
        RawValue::SignedInt(n) => values.push(FileValue::new(field, &n.to_string())),
        RawValue::Boolean(on) => values.push(FileValue::new(field, if *on { "1" } else { "0" })),
        _ => {}
    }
}

/// Same key and same text value.
fn same_raw(a: &RawTag, b: &RawTag) -> bool {
    a.key == b.key
        && match (&a.value, &b.value) {
            (RawValue::String(a), RawValue::String(b)) => a == b,
            _ => false,
        }
}

/// The field a standard tag fills.
fn std_field(tag: &StandardTag) -> Option<Field> {
    Some(match tag {
        StandardTag::TrackTitle(_) => Field::Title,
        StandardTag::Artist(_) => Field::Artist,
        StandardTag::Album(_) => Field::Album,
        StandardTag::AlbumArtist(_) => Field::AlbumArtist,
        StandardTag::TrackNumber(_) => Field::Track,
        StandardTag::TrackTotal(_) => Field::TrackTotal,
        StandardTag::DiscNumber(_) => Field::Disc,
        StandardTag::DiscTotal(_) => Field::DiscTotal,
        StandardTag::ReleaseDate(_)
        | StandardTag::ReleaseYear(_)
        | StandardTag::RecordingDate(_)
        | StandardTag::RecordingYear(_) => Field::Date,
        StandardTag::OriginalReleaseDate(_)
        | StandardTag::OriginalReleaseYear(_)
        | StandardTag::OriginalRecordingDate(_) => Field::OriginalDate,
        StandardTag::Genre(_) => Field::Genre,
        StandardTag::Composer(_) => Field::Composer,
        StandardTag::CompilationFlag(_) => Field::Compilation,
        StandardTag::SortTrackTitle(_) => Field::TitleSort,
        StandardTag::SortArtist(_) => Field::ArtistSort,
        StandardTag::SortAlbum(_) => Field::AlbumSort,
        StandardTag::SortAlbumArtist(_) => Field::AlbumArtistSort,
        StandardTag::MusicBrainzAlbumId(_) => Field::ReleaseId,
        StandardTag::MusicBrainzReleaseGroupId(_) => Field::ReleaseGroupId,
        StandardTag::MusicBrainzRecordingId(_) | StandardTag::MusicBrainzTrackId(_) => {
            Field::RecordingId
        }
        StandardTag::MusicBrainzReleaseTrackId(_) => Field::ReleaseTrackId,
        StandardTag::MusicBrainzArtistId(_) => Field::ArtistId,
        StandardTag::MusicBrainzAlbumArtistId(_) => Field::AlbumArtistId,
        _ => return None,
    })
}

/// The field of a tag key whose value the reader could not parse (` 3`
/// as a track number): Vorbis comment names, RIFF INFO chunks and ID3
/// frames, without case.
fn key_field(key: &str) -> Option<Field> {
    Some(match key.to_ascii_lowercase().as_str() {
        "title" | "inam" | "tit2" => Field::Title,
        "artist" | "iart" | "tpe1" => Field::Artist,
        "album" | "iprd" | "talb" => Field::Album,
        "albumartist" | "album artist" | "tpe2" => Field::AlbumArtist,
        "tracknumber" | "itrk" | "trck" => Field::Track,
        "tracktotal" | "totaltracks" => Field::TrackTotal,
        "discnumber" | "tpos" => Field::Disc,
        "disctotal" | "totaldiscs" => Field::DiscTotal,
        "date" | "year" | "icrd" | "tdrc" | "tyer" | "tdrl" => Field::Date,
        "originaldate" | "originalyear" | "tdor" | "tory" => Field::OriginalDate,
        "genre" | "ignr" | "tcon" => Field::Genre,
        "composer" | "tcom" => Field::Composer,
        "compilation" | "tcmp" => Field::Compilation,
        "titlesort" | "tsot" => Field::TitleSort,
        "artistsort" | "tsop" => Field::ArtistSort,
        "albumsort" | "tsoa" => Field::AlbumSort,
        "albumartistsort" | "tso2" => Field::AlbumArtistSort,
        _ => return None,
    })
}

/// MusicBrainz IDs in ID3 user text frames, by description (Picard's).
fn txxx_field(description: &str) -> Option<Field> {
    Some(match description.to_ascii_lowercase().as_str() {
        "musicbrainz album id" => Field::ReleaseId,
        "musicbrainz release group id" => Field::ReleaseGroupId,
        "musicbrainz release track id" => Field::ReleaseTrackId,
        "musicbrainz artist id" => Field::ArtistId,
        "musicbrainz album artist id" => Field::AlbumArtistId,
        _ => return None,
    })
}

/// The front cover if there is one, else the first picture.
fn best_visual(visuals: &[Visual]) -> Option<Picture> {
    let usable = |visual: &&Visual| visual.media_type.as_deref().and_then(image_mime).is_some();
    let chosen = visuals
        .iter()
        .filter(usable)
        .find(|visual| visual.usage == Some(StandardVisualKey::FrontCover))
        .or_else(|| visuals.iter().find(usable))?;
    Some(Picture {
        mime: chosen.media_type.as_deref().and_then(image_mime)?,
        data: chosen.data.to_vec(),
    })
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
        frames: Some(info.total_frames),
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
    for frame in tag.frames() {
        let content = frame.content();
        if let Some(text) = content.extended_text() {
            if let Some(field) = txxx_field(&text.description) {
                push_id3(&mut scanned.values, field, &text.value);
            }
        } else if let Some(id) = content.unique_file_identifier() {
            if id.owner_identifier == "http://musicbrainz.org"
                && let Ok(value) = std::str::from_utf8(&id.identifier)
            {
                scanned
                    .values
                    .push(FileValue::new(Field::RecordingId, value));
            }
        } else if let (Some(field), Some(text)) = (key_field(frame.id()), content.text()) {
            push_id3(&mut scanned.values, field, text);
        }
    }
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

/// ID3 v2.4 separates values with NUL.
fn push_id3(values: &mut Vec<FileValue>, field: Field, text: &str) {
    values.extend(
        text.split('\0')
            .filter(|value| !value.is_empty())
            .map(|value| FileValue::new(field, value)),
    );
}

#[cfg(test)]
mod tests {
    use super::image_mime;

    #[test]
    fn only_shown_image_types_are_kept() {
        assert_eq!(image_mime("IMAGE/JPG"), Some("image/jpeg"));
        assert_eq!(image_mime("image/tiff"), None);
    }
}
