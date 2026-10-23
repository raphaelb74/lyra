//! Direct ALSA hardware output: device discovery and capability probing.
//!
//! Only `hw:` devices are used. No `plughw`, `default`, dmix, PulseAudio or
//! PipeWire: what the DAC receives is exactly what Lyra writes.

mod device;
mod diagnostics;
mod error;
mod reserve;
mod sink;
mod volume;

pub use device::{Capabilities, Device, PcmSupport, card_list, list_playback_devices, probe};
pub use diagnostics::{KernelReport, SinkReport, SinkStats, kernel_report};
pub use error::{AlsaError, Holder};
pub use sink::{AlsaSink, BufferConfig};
pub use volume::{VolumeLevel, hardware_volume, set_volume, set_volume_db, volume};
