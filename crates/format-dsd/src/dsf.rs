//! DSF (Sony DSD Stream File), little endian.
//!
//! Layout: `DSD ` chunk (28 bytes), `fmt ` chunk (52 bytes), `data` chunk.
//! Audio is stored in groups: for each channel in turn, one block of
//! `block_size` bytes. With 1 bit per sample the oldest bit is the least
//! significant one. The last group is zero-padded past `sample_count`.

use std::io::SeekFrom;

use crate::{
    Container, DsdError, DsdInfo, MAX_CHANNELS, Source, read_array, read_full, valid_rate,
};

pub(crate) struct Dsf {
    source: Box<dyn Source>,
    info: DsdInfo,
    data_start: u64,
    /// Bytes per channel per block.
    block: usize,
    /// Bit order is LSB first (1 bit per sample); MSB first for 8.
    lsb_first: bool,
    /// Frame index of the next group's first frame.
    next_group: u64,
    /// Frames to drop from the next group, after a seek.
    skip: usize,
    /// One group, reused.
    group: Vec<u8>,
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    let mut field = [0; 4];
    field.copy_from_slice(&bytes[at..at + 4]);
    u32::from_le_bytes(field)
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    let mut field = [0; 8];
    field.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(field)
}

/// The ID3v2 tag position from the `DSD ` chunk's metadata pointer.
pub(crate) fn tag_offset(source: &mut dyn Source) -> Result<Option<u64>, DsdError> {
    source.seek(SeekFrom::Start(0)).map_err(DsdError::Io)?;
    let header: [u8; 28] = read_array(source)?;
    if &header[..4] != b"DSD " {
        return Err(malformed("bad DSD chunk"));
    }
    let pointer = u64_at(&header, 20);
    Ok((pointer != 0).then_some(pointer))
}

fn malformed(what: &str) -> DsdError {
    DsdError::Malformed(what.into())
}

impl Dsf {
    pub fn open(mut source: Box<dyn Source>) -> Result<Self, DsdError> {
        let header: [u8; 28] = read_array(&mut *source)?;
        if &header[..4] != b"DSD " || u64_at(&header, 4) != 28 {
            return Err(malformed("bad DSD chunk"));
        }

        let fmt: [u8; 52] = read_array(&mut *source)?;
        if &fmt[..4] != b"fmt " {
            return Err(malformed("missing fmt chunk"));
        }
        let fmt_size = u64_at(&fmt, 4);
        let version = u32_at(&fmt, 12);
        let format_id = u32_at(&fmt, 16);
        let channels = usize::try_from(u32_at(&fmt, 24)).unwrap_or(usize::MAX);
        let dsd_rate = u32_at(&fmt, 28);
        let bits_per_sample = u32_at(&fmt, 32);
        let sample_count = u64_at(&fmt, 36);
        let block = usize::try_from(u32_at(&fmt, 44)).unwrap_or(usize::MAX);

        if fmt_size < 52 {
            return Err(malformed("fmt chunk too short"));
        }
        if version != 1 {
            return Err(DsdError::Unsupported("DSF format version other than 1"));
        }
        if format_id != 0 {
            return Err(DsdError::Unsupported("compressed DSF"));
        }
        if channels == 0 || channels > MAX_CHANNELS {
            return Err(malformed("channel count outside 1–6"));
        }
        if !valid_rate(dsd_rate) {
            return Err(DsdError::Unsupported("DSD rate is not DSD64–DSD512"));
        }
        if bits_per_sample != 1 && bits_per_sample != 8 {
            return Err(malformed("bits per sample is not 1 or 8"));
        }
        // The spec fixes 4096; accept other sane powers of two, bounded.
        if !block.is_power_of_two() || !(256..=65_536).contains(&block) {
            return Err(malformed("block size"));
        }

        // Skip any fmt extension, then find the data chunk.
        source
            .seek(SeekFrom::Start(28 + fmt_size))
            .map_err(DsdError::Io)?;
        let data: [u8; 12] = read_array(&mut *source)?;
        if &data[..4] != b"data" || u64_at(&data, 4) < 12 {
            return Err(malformed("missing data chunk"));
        }
        let data_start = 28 + fmt_size + 12;

        let info = DsdInfo {
            container: Container::Dsf,
            dsd_rate,
            channels,
            total_frames: sample_count.div_ceil(8),
        };
        Ok(Self {
            source,
            info,
            data_start,
            block,
            lsb_first: bits_per_sample == 1,
            next_group: 0,
            skip: 0,
            group: vec![0; block * channels],
        })
    }

    pub fn info(&self) -> &DsdInfo {
        &self.info
    }

    pub fn read_next(&mut self, out: &mut Vec<u8>) -> Result<bool, DsdError> {
        let channels = self.info.channels;
        let block = self.block as u64;
        while out.is_empty() {
            if self.next_group >= self.info.total_frames {
                return Ok(false);
            }
            if read_full(&mut *self.source, &mut self.group)? < self.group.len() {
                return Err(malformed("file is truncated"));
            }
            let left = self.info.total_frames - self.next_group;
            let valid = usize::try_from(left.min(block)).unwrap_or(self.block);
            let skip = std::mem::take(&mut self.skip).min(valid);
            self.next_group += block;

            out.resize((valid - skip) * channels, 0);
            for (channel, block_bytes) in self.group.chunks_exact(self.block).enumerate() {
                for (frame, &byte) in out
                    .chunks_exact_mut(channels)
                    .zip(&block_bytes[skip..valid])
                {
                    frame[channel] = if self.lsb_first {
                        byte.reverse_bits()
                    } else {
                        byte
                    };
                }
            }
        }
        Ok(true)
    }

    pub fn seek(&mut self, frame: u64) -> Result<(), DsdError> {
        let block = self.block as u64;
        let group = frame / block;
        let offset = group
            .checked_mul(block * self.info.channels as u64)
            .and_then(|bytes| bytes.checked_add(self.data_start))
            .ok_or(DsdError::SeekOutOfRange)?;
        self.source
            .seek(SeekFrom::Start(offset))
            .map_err(DsdError::Io)?;
        self.next_group = group * block;
        self.skip = usize::try_from(frame % block).unwrap_or(0);
        Ok(())
    }
}
