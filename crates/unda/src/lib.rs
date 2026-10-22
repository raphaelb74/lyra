//! UNDA (Unified Native Digital Audio), Lyra's playback engine: controller,
//! bounded buffering, transport state.
//!
//! Independent of Tauri and of ALSA: output goes through [`AudioSink`], so the
//! engine is tested with [`mock::MockSink`] and runs on the DAC through
//! `audio-alsa`.

mod controller;
mod dop;
pub mod mock;
mod order;
mod player;
mod sink;
mod source;
mod status;
mod writer;

pub use order::Repeat;
pub use player::{ConversionStats, POOL_SIZE, Player, QueueItem};
pub use sink::{
    AudioSink, DsdFallback, DsdPolicy, DsdTransport, Encoding, HardwareVolume, OutputFormat,
    SinkError, SinkErrorKind, StreamFormat, Transport,
};
pub use source::SourceInfo;
pub use status::{
    BitPerfect, ErrorKind, NowPlaying, PlaybackState, PlayerError, PlayerStatus, Processing,
};
