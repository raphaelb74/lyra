//! Native DSD: raw DSD bytes in the layouts ALSA defines.
//!
//! One ALSA frame holds 1, 2 or 4 DSD bytes per channel. Within a channel's
//! word the oldest byte is the most significant; the word is stored little- or
//! big-endian. Bits are never touched: ALSA's DSD formats are MSB-first like
//! the canonical layout, and drivers that need the bits reversed do it
//! themselves.

use crate::{MAX_CHANNELS, SILENCE};

/// Native DSD sample layouts ALSA can expose (`SND_PCM_FORMAT_DSD_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DsdLayout {
    U8,
    U16Le,
    U16Be,
    U32Le,
    U32Be,
}

impl DsdLayout {
    /// Widest first: fewer frames per second for the same DSD rate.
    pub const ALL: [Self; 5] = [Self::U32Be, Self::U32Le, Self::U16Be, Self::U16Le, Self::U8];

    /// DSD bytes per channel in one ALSA frame.
    pub const fn bytes(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16Le | Self::U16Be => 2,
            Self::U32Le | Self::U32Be => 4,
        }
    }

    /// ALSA frame rate carrying `dsd_rate` bits per second per channel
    /// (DSD64 as `DSD_U32_BE` runs at 88.2 kHz).
    pub const fn frame_rate(self, dsd_rate: u32) -> u32 {
        let bits_per_frame = match self {
            Self::U8 => 8,
            Self::U16Le | Self::U16Be => 16,
            Self::U32Le | Self::U32Be => 32,
        };
        dsd_rate / bits_per_frame
    }

    /// ALSA format name, e.g. `DSD_U32_BE`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::U8 => "DSD_U8",
            Self::U16Le => "DSD_U16_LE",
            Self::U16Be => "DSD_U16_BE",
            Self::U32Le => "DSD_U32_LE",
            Self::U32Be => "DSD_U32_BE",
        }
    }

    const fn little_endian(self) -> bool {
        matches!(self, Self::U16Le | Self::U32Le)
    }
}

/// Groups canonical DSD frames into a layout's words, keeping an incomplete
/// word across calls.
#[derive(Debug, Default)]
pub struct NativePacker {
    /// Canonical frames of an incomplete word, interleaved.
    pending: [u8; 4 * MAX_CHANNELS],
    pending_frames: usize,
}

impl NativePacker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `layout` frames for interleaved DSD `frames` to `out`. Frames
    /// that do not fill a word are held until the next call or [`Self::finish`].
    pub fn pack(&mut self, layout: DsdLayout, frames: &[u8], channels: usize, out: &mut Vec<u8>) {
        if channels == 0 || channels > MAX_CHANNELS {
            return;
        }
        let word = layout.bytes() * channels;
        let mut frames = &frames[..frames.len() - frames.len() % channels];
        out.reserve(frames.len() + word);

        if self.pending_frames > 0 {
            let have = self.pending_frames * channels;
            let take = (word - have).min(frames.len());
            self.pending[have..have + take].copy_from_slice(&frames[..take]);
            self.pending_frames += take / channels;
            frames = &frames[take..];
            if self.pending_frames < layout.bytes() {
                return;
            }
            let pending = self.pending;
            emit(layout, &pending[..word], channels, out);
            self.pending_frames = 0;
        }
        let mut words = frames.chunks_exact(word);
        for group in words.by_ref() {
            emit(layout, group, channels, out);
        }
        let rest = words.remainder();
        self.pending[..rest.len()].copy_from_slice(rest);
        self.pending_frames = rest.len() / channels;
    }

    /// Flushes an incomplete word, filled up with DSD silence.
    pub fn finish(&mut self, layout: DsdLayout, channels: usize, out: &mut Vec<u8>) {
        if self.pending_frames == 0 || !(1..=MAX_CHANNELS).contains(&channels) {
            return;
        }
        let word = layout.bytes() * channels;
        self.pending[self.pending_frames * channels..word].fill(SILENCE);
        let pending = self.pending;
        emit(layout, &pending[..word], channels, out);
        self.pending_frames = 0;
    }

    /// Drops held frames (after a seek).
    pub fn reset(&mut self) {
        self.pending_frames = 0;
    }
}

/// Writes one ALSA frame from `layout.bytes()` interleaved canonical frames.
fn emit(layout: DsdLayout, frames: &[u8], channels: usize, out: &mut Vec<u8>) {
    for channel in 0..channels {
        let oldest_first = frames[channel..].iter().step_by(channels).copied();
        if layout.little_endian() {
            out.extend(oldest_first.rev());
        } else {
            out.extend(oldest_first);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DsdLayout, NativePacker};
    use crate::SILENCE;

    /// Stereo frames (L, R): (0x11, 0x21), (0x12, 0x22), … oldest first.
    const DSD: [u8; 8] = [0x11, 0x21, 0x12, 0x22, 0x13, 0x23, 0x14, 0x24];

    fn packed(layout: DsdLayout, frames: &[u8]) -> Vec<u8> {
        let mut packer = NativePacker::new();
        let mut out = Vec::new();
        packer.pack(layout, frames, 2, &mut out);
        packer.finish(layout, 2, &mut out);
        out
    }

    #[test]
    fn each_layout_orders_bytes_as_alsa_defines() {
        assert_eq!(packed(DsdLayout::U8, &DSD), DSD);
        assert_eq!(
            packed(DsdLayout::U16Be, &DSD),
            [0x11, 0x12, 0x21, 0x22, 0x13, 0x14, 0x23, 0x24]
        );
        assert_eq!(
            packed(DsdLayout::U16Le, &DSD),
            [0x12, 0x11, 0x22, 0x21, 0x14, 0x13, 0x24, 0x23]
        );
        assert_eq!(
            packed(DsdLayout::U32Be, &DSD),
            [0x11, 0x12, 0x13, 0x14, 0x21, 0x22, 0x23, 0x24]
        );
        assert_eq!(
            packed(DsdLayout::U32Le, &DSD),
            [0x14, 0x13, 0x12, 0x11, 0x24, 0x23, 0x22, 0x21]
        );
    }

    #[test]
    fn frame_rates_match_the_layout_width() {
        assert_eq!(DsdLayout::U32Be.frame_rate(2_822_400), 88_200);
        assert_eq!(DsdLayout::U16Le.frame_rate(5_644_800), 352_800);
        assert_eq!(DsdLayout::U8.frame_rate(2_822_400), 352_800);
        assert_eq!(DsdLayout::U32Le.frame_rate(11_289_600), 352_800);
    }

    #[test]
    fn words_span_calls_and_the_tail_is_padded_with_silence() {
        let dsd: Vec<u8> = (0..22).collect(); // 11 stereo frames
        let whole = packed(DsdLayout::U32Be, &dsd);

        let mut packer = NativePacker::new();
        let mut split = Vec::new();
        for part in [&dsd[..2], &dsd[2..8], &dsd[8..10], &dsd[10..14], &dsd[14..]] {
            packer.pack(DsdLayout::U32Be, part, 2, &mut split);
        }
        packer.finish(DsdLayout::U32Be, 2, &mut split);
        assert_eq!(split, whole);
        assert_eq!(whole.len(), 3 * 8);
        // Last word: frames 8–10, then one frame of silence, per channel.
        assert_eq!(whole[16..], [16, 18, 20, SILENCE, 17, 19, 21, SILENCE]);
    }
}
