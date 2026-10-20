use std::fmt;
use std::io;

#[derive(Debug)]
pub enum DsdError {
    Io(io::Error),
    /// Not a DSF or DFF file (e.g. a macOS `._` metadata file).
    NotDsd,
    /// Valid file Lyra does not play (DST compression, too many channels…).
    Unsupported(&'static str),
    /// Corrupt or truncated.
    Malformed(String),
    SeekOutOfRange,
}

impl fmt::Display for DsdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot read file: {error}"),
            Self::NotDsd => f.write_str("not a DSF or DFF file"),
            Self::Unsupported(what) => write!(f, "unsupported: {what}"),
            Self::Malformed(what) => write!(f, "invalid DSD file: {what}"),
            Self::SeekOutOfRange => f.write_str("seek position is past the end of the track"),
        }
    }
}

impl std::error::Error for DsdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}
