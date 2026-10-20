//! Playback engine: controller, bounded buffering, transport state.
//!
//! Independent of Tauri and of ALSA: output goes through [`AudioSink`], so the
//! engine is tested with [`mock::MockSink`] and runs on the DAC through
//! `audio-alsa`.

pub mod mock;
mod player;
mod sink;
mod source;
mod status;
mod writer;

pub use player::{POOL_SIZE, Player};
pub use sink::{AudioSink, HardwareVolume, OutputFormat, PcmStream, SinkError, SinkErrorKind};
pub use source::{SourceInfo, Transport};
pub use status::{BitPerfect, ErrorKind, NowPlaying, PlaybackState, PlayerError, PlayerStatus};
