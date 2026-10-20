//! PCM decoding for bit-perfect playback.
//!
//! [`Decoder`] reads WAV and FLAC into interleaved `i32` samples that are
//! *left-justified*: the file's integer samples sit in the high bits and the
//! low `32 - bits_per_sample` bits are zero. No float, dither, gain or
//! resampling is involved, so every source bit is preserved.
//!
//! [`SampleEncoding::pack`] writes those samples in an ALSA byte layout.

mod decode;
mod pack;

pub use decode::{Codec, DecodeError, Decoder, StreamInfo};
pub use pack::SampleEncoding;
