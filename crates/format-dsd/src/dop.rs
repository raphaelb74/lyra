//! DoP 1.0: DSD carried in 24-bit PCM frames.
//!
//! Each PCM frame carries 16 DSD bits per channel: a marker byte in the top
//! 8 bits, then the older DSD byte, then the newer one. Markers alternate
//! `0x05`, `0xFA` from frame to frame and are identical across channels. The
//! carrier runs at DSD rate / 16 (DSD64 → 176.4 kHz).
//!
//! Samples come out left-justified in `i32`, the same convention as
//! `format-pcm`, so any 24-bit-or-wider [`format_pcm::SampleEncoding`] packs
//! them unchanged.

use crate::{DsdInfo, MAX_CHANNELS, SILENCE};

pub const MARKERS: [u8; 2] = [0x05, 0xFA];

/// PCM carrier rate for a DSD stream.
pub fn carrier_rate(info: &DsdInfo) -> u32 {
    info.dsd_rate / 16
}

/// Packs DSD frames into DoP samples, keeping marker phase across calls.
#[derive(Debug, Default)]
pub struct DopPacker {
    /// Index into [`MARKERS`] for the next DoP frame.
    marker: usize,
    /// An odd DSD frame left over from the last call.
    pending: Option<[u8; MAX_CHANNELS]>,
}

impl DopPacker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends DoP samples for interleaved DSD `frames` to `out`. An odd
    /// trailing frame is held until the next call or [`Self::finish`].
    pub fn pack(&mut self, frames: &[u8], channels: usize, out: &mut Vec<i32>) {
        if channels == 0 || channels > MAX_CHANNELS {
            return;
        }
        let mut frames = &frames[..frames.len() - frames.len() % channels];
        out.reserve((frames.len() / channels / 2 + 1) * channels);

        if let Some(held) = self.pending.take() {
            if frames.is_empty() {
                self.pending = Some(held);
                return;
            }
            let (newer, rest) = frames.split_at(channels);
            self.push(&held[..channels], newer, out);
            frames = rest;
        }
        let mut pairs = frames.chunks_exact(2 * channels);
        for pair in pairs.by_ref() {
            let (older, newer) = pair.split_at(channels);
            self.push(older, newer, out);
        }
        let odd = pairs.remainder();
        if !odd.is_empty() {
            let mut held = [0; MAX_CHANNELS];
            held[..channels].copy_from_slice(odd);
            self.pending = Some(held);
        }
    }

    /// Flushes a held odd frame, paired with DSD silence.
    pub fn finish(&mut self, channels: usize, out: &mut Vec<i32>) {
        if let Some(held) = self.pending.take()
            && (1..=MAX_CHANNELS).contains(&channels)
        {
            self.push(&held[..channels], &[SILENCE; MAX_CHANNELS][..channels], out);
        }
    }

    /// Drops a held frame (after a seek). Marker phase continues.
    pub fn reset(&mut self) {
        self.pending = None;
    }

    fn push(&mut self, older: &[u8], newer: &[u8], out: &mut Vec<i32>) {
        let marker = MARKERS[self.marker];
        self.marker ^= 1;
        for (&old, &new) in older.iter().zip(newer) {
            let word = (u32::from(marker) << 24) | (u32::from(old) << 16) | (u32::from(new) << 8);
            out.push(word.cast_signed());
        }
    }
}

#[cfg(test)]
mod tests {
    use format_pcm::SampleEncoding;

    use super::{DopPacker, SILENCE};

    fn packed(encoding: SampleEncoding, samples: &[i32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        encoding.pack(samples, &mut bytes);
        bytes
    }

    #[test]
    fn frames_carry_marker_then_older_then_newer_byte() {
        // Stereo DSD frames: (L, R) = (0x11, 0x21), (0x12, 0x22), (0x13, 0x23), (0x14, 0x24).
        let dsd = [0x11, 0x21, 0x12, 0x22, 0x13, 0x23, 0x14, 0x24];
        let mut samples = Vec::new();
        DopPacker::new().pack(&dsd, 2, &mut samples);
        assert_eq!(
            packed(SampleEncoding::S24In3Le, &samples),
            [
                0x12, 0x11, 0x05, 0x22, 0x21, 0x05, // frame 0: marker 0x05 on both channels
                0x14, 0x13, 0xFA, 0x24, 0x23, 0xFA, // frame 1: marker 0xFA
            ]
        );
        assert_eq!(
            packed(SampleEncoding::S32Le, &samples[..1]),
            [0x00, 0x12, 0x11, 0x05]
        );
        // S24_LE sign-extends the marker byte into the unused top byte.
        assert_eq!(
            packed(SampleEncoding::S24In4Le, &samples[2..3]),
            [0x14, 0x13, 0xFA, 0xFF]
        );
    }

    #[test]
    fn markers_alternate_across_calls_and_odd_splits() {
        let dsd: Vec<u8> = (0..20).collect(); // 20 mono DSD frames → 10 DoP frames
        let mut whole = Vec::new();
        DopPacker::new().pack(&dsd, 1, &mut whole);

        let mut packer = DopPacker::new();
        let mut split = Vec::new();
        for part in [&dsd[..3], &dsd[3..4], &dsd[4..11], &dsd[11..]] {
            packer.pack(part, 1, &mut split);
        }
        assert_eq!(split, whole);
        let markers: Vec<u32> = whole.iter().map(|&s| s.cast_unsigned() >> 24).collect();
        assert_eq!(
            markers,
            [0x05, 0xFA, 0x05, 0xFA, 0x05, 0xFA, 0x05, 0xFA, 0x05, 0xFA]
        );
    }

    #[test]
    fn finish_pads_an_odd_frame_with_dsd_silence() {
        let mut packer = DopPacker::new();
        let mut samples = Vec::new();
        packer.pack(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF], 2, &mut samples);
        assert_eq!(samples.len(), 2);
        packer.finish(2, &mut samples);
        assert_eq!(
            packed(SampleEncoding::S24In3Le, &samples[2..]),
            [SILENCE, 0xEE, 0xFA, SILENCE, 0xFF, 0xFA]
        );
    }
}
