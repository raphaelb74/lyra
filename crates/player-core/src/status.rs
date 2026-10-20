use std::fmt;
use std::path::PathBuf;

use format_pcm::{DecodeError, StreamInfo};

use crate::sink::{OutputFormat, SinkError, SinkErrorKind};

/// Playback states. Every change is published as a [`PlayerStatus`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    /// Opening and probing the file.
    Loading,
    /// Opening the device with the file's exact format.
    Negotiating,
    /// Device open, first audio on its way.
    Buffering,
    Playing,
    Paused,
    Error,
}

/// Machine-readable error category (the IPC `code`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    DeviceBusy,
    DeviceNotFound,
    /// File or device cannot be played without conversion.
    Unsupported,
    InvalidFile,
    /// Output failed while playing (underrun recovery failed, unplugged…).
    Output,
    /// A player thread stopped unexpectedly.
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerError {
    pub kind: ErrorKind,
    pub message: String,
}

impl fmt::Display for PlayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PlayerError {}

impl From<SinkError> for PlayerError {
    fn from(error: SinkError) -> Self {
        let kind = match error.kind {
            SinkErrorKind::Busy => ErrorKind::DeviceBusy,
            SinkErrorKind::NotFound => ErrorKind::DeviceNotFound,
            SinkErrorKind::Unsupported => ErrorKind::Unsupported,
            SinkErrorKind::Failed => ErrorKind::Output,
        };
        Self {
            kind,
            message: error.message,
        }
    }
}

impl From<DecodeError> for PlayerError {
    fn from(error: DecodeError) -> Self {
        let kind = match error {
            DecodeError::Unsupported(_) => ErrorKind::Unsupported,
            DecodeError::Io(_) | DecodeError::Malformed(_) | DecodeError::SeekOutOfRange => {
                ErrorKind::InvalidFile
            }
        };
        Self {
            kind,
            message: error.to_string(),
        }
    }
}

/// The track being played and how it reaches the DAC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NowPlaying {
    pub path: PathBuf,
    pub device: String,
    pub source: StreamInfo,
    /// Set once the device accepted the stream.
    pub output: Option<OutputFormat>,
}

/// Immutable snapshot published on every state change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerStatus {
    pub state: PlaybackState,
    pub track: Option<NowPlaying>,
    /// Why the state is [`PlaybackState::Error`].
    pub error: Option<PlayerError>,
}

impl PlayerStatus {
    pub(crate) const STOPPED: Self = Self {
        state: PlaybackState::Stopped,
        track: None,
        error: None,
    };
}
