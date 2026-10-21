//! DSD file readers (DSF, DSDIFF/DFF), DoP packing and native DSD layouts.
//!
//! Both readers produce the same layout: **DSD bytes, most significant bit
//! first (oldest bit in bit 7), interleaved one byte per channel**. A "frame"
//! is one byte per channel, i.e. 8 DSD samples per channel. Raw DSD bits are
//! never altered: DSF only needs its bit order and block layout undone.
//!
//! Parsing is defensive: header fields are validated, sizes taken from the
//! file never drive an allocation, and truncated or corrupt data returns an
//! error instead of panicking.

mod dff;
pub mod dop;
mod dsf;
mod error;
pub mod native;

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub use error::DsdError;
pub use native::{DsdLayout, NativePacker};

/// DSD64 bit rate per channel (64 × 44.1 kHz).
pub const DSD64_RATE: u32 = 2_822_400;

/// DSD idle pattern: digital silence that keeps the DAC in DSD mode.
pub const SILENCE: u8 = 0x69;

/// Most channels read. Multichannel files open, so the player can refuse
/// them with a clear reason when the DAC has fewer outputs.
pub const MAX_CHANNELS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    Dsf,
    Dff,
}

/// Properties of a DSD stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DsdInfo {
    pub container: Container,
    /// DSD bit rate per channel, e.g. 2 822 400 for DSD64.
    pub dsd_rate: u32,
    pub channels: usize,
    /// Length in frames (bytes per channel).
    pub total_frames: u64,
}

impl DsdInfo {
    /// 64 for DSD64, 128 for DSD128, … (of 44.1 or 48 kHz).
    pub fn multiple(&self) -> u32 {
        multiple(self.dsd_rate)
    }

    /// Frames per second (bytes per channel per second).
    pub fn frame_rate(&self) -> u32 {
        self.dsd_rate / 8
    }
}

/// 64 for 2.8224 MHz (DSD64), 128 for 5.6448 MHz, … (of 44.1 or 48 kHz).
pub fn multiple(dsd_rate: u32) -> u32 {
    let base = if dsd_rate.is_multiple_of(44_100) {
        44_100
    } else {
        48_000
    };
    dsd_rate / base
}

/// Any seekable byte source.
pub trait Source: Read + Seek + Send {}
impl<T: Read + Seek + Send> Source for T {}

/// Reads one DSD file as interleaved MSB-first frames.
pub struct DsdReader {
    inner: Inner,
}

enum Inner {
    Dsf(dsf::Dsf),
    Dff(dff::Dff),
}

impl std::fmt::Debug for DsdReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DsdReader")
            .field("info", self.info())
            .finish_non_exhaustive()
    }
}

impl DsdReader {
    pub fn open(path: &Path) -> Result<Self, DsdError> {
        let file = File::open(path).map_err(DsdError::Io)?;
        Self::from_reader(Box::new(io::BufReader::new(file)))
    }

    /// Detects the container from its first bytes, never from a file name.
    pub fn from_reader(mut source: Box<dyn Source>) -> Result<Self, DsdError> {
        let mut magic = [0; 4];
        source
            .read_exact(&mut magic)
            .map_err(|_| DsdError::NotDsd)?;
        source.seek(SeekFrom::Start(0)).map_err(DsdError::Io)?;
        let inner = match &magic {
            b"DSD " => Inner::Dsf(dsf::Dsf::open(source)?),
            b"FRM8" => Inner::Dff(dff::Dff::open(source)?),
            _ => return Err(DsdError::NotDsd),
        };
        Ok(Self { inner })
    }

    pub fn info(&self) -> &DsdInfo {
        match &self.inner {
            Inner::Dsf(dsf) => dsf.info(),
            Inner::Dff(dff) => dff.info(),
        }
    }

    /// Replaces `out` with the next frames (a whole number, interleaved).
    /// Returns `false` at the end, with `out` empty.
    pub fn read_next(&mut self, out: &mut Vec<u8>) -> Result<bool, DsdError> {
        out.clear();
        match &mut self.inner {
            Inner::Dsf(dsf) => dsf.read_next(out),
            Inner::Dff(dff) => dff.read_next(out),
        }
    }

    /// Moves to `frame`; the next read starts exactly there.
    pub fn seek(&mut self, frame: u64) -> Result<(), DsdError> {
        if frame > self.info().total_frames {
            return Err(DsdError::SeekOutOfRange);
        }
        match &mut self.inner {
            Inner::Dsf(dsf) => dsf.seek(frame),
            Inner::Dff(dff) => dff.seek(frame),
        }
    }
}

/// Where the file's ID3v2 tag starts, if it has one: the DSF metadata
/// pointer, or a DFF `ID3 ` chunk. Only the position: the caller checks the
/// tag itself.
pub fn tag_offset(source: &mut dyn Source) -> Result<Option<u64>, DsdError> {
    let mut magic = [0; 4];
    source.seek(SeekFrom::Start(0)).map_err(DsdError::Io)?;
    source
        .read_exact(&mut magic)
        .map_err(|_| DsdError::NotDsd)?;
    match &magic {
        b"DSD " => dsf::tag_offset(source),
        b"FRM8" => dff::tag_offset(source),
        _ => Err(DsdError::NotDsd),
    }
}

/// Checks a DSD rate: a power-of-two multiple of DSD64, up to DSD512.
fn valid_rate(rate: u32) -> bool {
    [1, 2, 4, 8]
        .iter()
        .any(|&factor| rate == DSD64_RATE * factor)
        || [1, 2, 4, 8]
            .iter()
            .any(|&factor| rate == 3_072_000 * factor)
}

/// Reads a fixed-size header field group.
fn read_array<const N: usize>(source: &mut dyn Source) -> Result<[u8; N], DsdError> {
    let mut bytes = [0; N];
    source.read_exact(&mut bytes).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            DsdError::Malformed("header is truncated".into())
        } else {
            DsdError::Io(error)
        }
    })?;
    Ok(bytes)
}

/// Reads up to `buffer.len()` bytes, stopping early only at end of file.
fn read_full(source: &mut dyn Source, buffer: &mut [u8]) -> Result<usize, DsdError> {
    let mut filled = 0;
    while filled < buffer.len() {
        match source.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(DsdError::Io(error)),
        }
    }
    Ok(filled)
}
