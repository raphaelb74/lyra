//! DSDIFF / DFF (Philips), big endian.
//!
//! `FRM8` form of type `DSD ` holding chunks: `FVER`, `PROP` (with `FS  `,
//! `CHNL`, `CMPR`), then the `DSD ` sound data, already interleaved one byte
//! per channel, MSB first. Chunks are padded to even sizes. DST-compressed
//! files (`DST ` sound chunk) are refused.

use std::io::SeekFrom;

use crate::{
    Container, DsdError, DsdInfo, MAX_CHANNELS, Source, read_array, read_full, valid_rate,
};

/// Frames per read: ~11.6 ms at DSD64, like one DSF block.
const FRAMES_PER_READ: u64 = 4096;

pub(crate) struct Dff {
    source: Box<dyn Source>,
    info: DsdInfo,
    data_start: u64,
    /// Next frame to read.
    position: u64,
}

fn malformed(what: &str) -> DsdError {
    DsdError::Malformed(what.into())
}

/// Chunk id and size.
fn chunk_header(source: &mut dyn Source) -> Result<([u8; 4], u64), DsdError> {
    let header: [u8; 12] = read_array(source)?;
    let mut id = [0; 4];
    id.copy_from_slice(&header[..4]);
    let mut size = [0; 8];
    size.copy_from_slice(&header[4..]);
    Ok((id, u64::from_be_bytes(size)))
}

/// Seeks past a chunk body of `size` bytes plus its pad byte.
fn skip_body(source: &mut dyn Source, body_start: u64, size: u64) -> Result<(), DsdError> {
    let end = body_start
        .checked_add(size)
        .and_then(|end| end.checked_add(size & 1))
        .ok_or_else(|| malformed("chunk size"))?;
    source.seek(SeekFrom::Start(end)).map_err(DsdError::Io)?;
    Ok(())
}

impl Dff {
    pub fn open(mut source: Box<dyn Source>) -> Result<Self, DsdError> {
        let (id, form_size) = chunk_header(&mut *source)?;
        let form_type: [u8; 4] = read_array(&mut *source)?;
        if &id != b"FRM8" || &form_type != b"DSD " {
            return Err(malformed("not a DSDIFF form"));
        }
        let form_end = 12_u64.saturating_add(form_size);

        let mut rate = None;
        let mut channels = None;
        let mut at = 16;
        while at < form_end {
            let (id, size) = chunk_header(&mut *source)?;
            let body = at + 12;
            match &id {
                b"PROP" => {
                    let (fs, count) = read_properties(&mut *source, body, size)?;
                    rate = fs;
                    channels = count;
                }
                b"DSD " => {
                    let dsd_rate = rate.ok_or_else(|| malformed("sound data before properties"))?;
                    let channels = channels.ok_or_else(|| malformed("no channel count"))?;
                    if !valid_rate(dsd_rate) {
                        return Err(DsdError::Unsupported("DSD rate is not DSD64–DSD512"));
                    }
                    if channels == 0 || channels > MAX_CHANNELS {
                        return Err(malformed("channel count outside 1–6"));
                    }
                    if !size.is_multiple_of(channels as u64) {
                        return Err(malformed("sound data is not whole frames"));
                    }
                    let info = DsdInfo {
                        container: Container::Dff,
                        dsd_rate,
                        channels,
                        total_frames: size / channels as u64,
                    };
                    return Ok(Self {
                        source,
                        info,
                        data_start: body,
                        position: 0,
                    });
                }
                b"DST " => return Err(DsdError::Unsupported("DST-compressed DFF")),
                _ => skip_body(&mut *source, body, size)?,
            }
            at = body
                .checked_add(size + (size & 1))
                .ok_or_else(|| malformed("chunk size"))?;
            source.seek(SeekFrom::Start(at)).map_err(DsdError::Io)?;
        }
        Err(malformed("no sound data"))
    }

    pub fn info(&self) -> &DsdInfo {
        &self.info
    }

    pub fn read_next(&mut self, out: &mut Vec<u8>) -> Result<bool, DsdError> {
        let frames = FRAMES_PER_READ.min(self.info.total_frames - self.position);
        if frames == 0 {
            return Ok(false);
        }
        let bytes = usize::try_from(frames).unwrap_or(0) * self.info.channels;
        out.resize(bytes, 0);
        if read_full(&mut *self.source, out)? < bytes {
            out.clear();
            return Err(malformed("file is truncated"));
        }
        self.position += frames;
        Ok(true)
    }

    pub fn seek(&mut self, frame: u64) -> Result<(), DsdError> {
        let offset = frame
            .checked_mul(self.info.channels as u64)
            .and_then(|bytes| bytes.checked_add(self.data_start))
            .ok_or(DsdError::SeekOutOfRange)?;
        self.source
            .seek(SeekFrom::Start(offset))
            .map_err(DsdError::Io)?;
        self.position = frame;
        Ok(())
    }
}

/// Start of the body of a top-level `ID3 ` chunk: not in the DSDIFF spec,
/// but where taggers put an ID3v2 tag, usually after the sound data.
pub(crate) fn tag_offset(source: &mut dyn Source) -> Result<Option<u64>, DsdError> {
    /// Real files have a handful of chunks; stop on anything stranger.
    const MAX_CHUNKS: usize = 64;
    source.seek(SeekFrom::Start(0)).map_err(DsdError::Io)?;
    let (id, form_size) = chunk_header(source)?;
    let form_type: [u8; 4] = read_array(source)?;
    if &id != b"FRM8" || &form_type != b"DSD " {
        return Err(malformed("not a DSDIFF form"));
    }
    let form_end = 12_u64.saturating_add(form_size);
    let mut at = 16;
    for _ in 0..MAX_CHUNKS {
        if at >= form_end {
            break;
        }
        // A truncated chunk header ends the search; the audio may still play.
        let Ok((id, size)) = chunk_header(source) else {
            break;
        };
        let body = at + 12;
        if &id == b"ID3 " {
            return Ok(Some(body));
        }
        at = body
            .checked_add(size)
            .and_then(|end| end.checked_add(size & 1))
            .ok_or_else(|| malformed("chunk size"))?;
        source.seek(SeekFrom::Start(at)).map_err(DsdError::Io)?;
    }
    Ok(None)
}

/// Reads `FS  `, `CHNL` and `CMPR` from a `PROP` chunk; skips the rest.
fn read_properties(
    source: &mut dyn Source,
    body: u64,
    size: u64,
) -> Result<(Option<u32>, Option<usize>), DsdError> {
    let prop_type: [u8; 4] = read_array(source)?;
    if &prop_type != b"SND " {
        return Err(malformed("property chunk is not SND"));
    }
    let end = body
        .checked_add(size)
        .ok_or_else(|| malformed("chunk size"))?;
    let mut rate = None;
    let mut channels = None;
    let mut at = body + 4;
    while at < end {
        let (id, sub_size) = chunk_header(source)?;
        let sub_body = at + 12;
        match &id {
            b"FS  " => rate = Some(u32::from_be_bytes(read_array(source)?)),
            b"CHNL" => channels = Some(usize::from(u16::from_be_bytes(read_array(source)?))),
            b"CMPR" => {
                let compression: [u8; 4] = read_array(source)?;
                if &compression != b"DSD " {
                    return Err(DsdError::Unsupported("DST-compressed DFF"));
                }
            }
            _ => {}
        }
        skip_body(source, sub_body, sub_size)?;
        at = sub_body
            .checked_add(sub_size + (sub_size & 1))
            .ok_or_else(|| malformed("chunk size"))?;
    }
    Ok((rate, channels))
}
