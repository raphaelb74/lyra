//! Programme loudness after ITU-R BS.1770-4 / EBU R128: K-weighting,
//! 400 ms blocks every 100 ms, an absolute gate at −70 LUFS and a relative
//! gate 10 LU below the ungated level.
//!
//! Blocks are counted in a [`Histogram`] of 0.1 LU steps (as libebur128's
//! histogram mode; within 0.05 LU of exact). Tracks' histograms add up, so
//! an album is gated as one programme from what its tracks stored.

use std::f64::consts::PI;

/// Loudness of a block whose weighted mean square is `energy`.
fn lufs(energy: f64) -> f64 {
    -0.691 + 10.0 * energy.log10()
}

/// The energy of a block of loudness `lufs`.
fn energy(lufs: f64) -> f64 {
    10f64.powf((lufs + 0.691) / 10.0)
}

const ABSOLUTE_GATE: f64 = -70.0;
const RELATIVE_GATE: f64 = -10.0;
/// Histogram steps per LU, and the loudest block counted apart (+10 LUFS:
/// full-scale square waves on every channel stay below it).
const STEPS_PER_LU: f64 = 10.0;
const BINS: usize = 800;
/// Sub-blocks of 100 ms in one 400 ms block.
const SUB_BLOCKS: usize = 4;

/// One second-order section, direct form I.
#[derive(Clone, Copy, Debug)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
}

impl Biquad {
    /// Stage 1 of the K-weighting: a high shelf for the head's effect.
    fn shelf(rate: f64) -> Self {
        let (f0, gain, q) = (
            1_681.974_450_955_533,
            3.999_843_853_973_347,
            0.707_175_236_955_419_6,
        );
        let k = (PI * f0 / rate).tan();
        let vh = 10f64.powf(gain / 20.0);
        let vb = vh.powf(0.499_666_774_154_541_6);
        let a0 = 1.0 + k / q + k * k;
        Self {
            b: [
                (vh + vb * k / q + k * k) / a0,
                2.0 * (k * k - vh) / a0,
                (vh - vb * k / q + k * k) / a0,
            ],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        }
    }

    /// Stage 2: the RLB high pass.
    fn high_pass(rate: f64) -> Self {
        let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
        let k = (PI * f0 / rate).tan();
        let a0 = 1.0 + k / q + k * k;
        Self {
            b: [1.0, -2.0, 1.0],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        }
    }
}

/// Filter state of one channel through both stages.
#[derive(Clone, Copy, Debug, Default)]
struct State {
    x: [f64; 2],
    mid: [f64; 2],
    y: [f64; 2],
}

/// Measures one programme. Feed it interleaved samples in −1.0..1.0.
#[derive(Debug)]
pub struct Meter {
    shelf: Biquad,
    high_pass: Biquad,
    weights: Vec<f64>,
    states: Vec<State>,
    /// Frames per 100 ms sub-block.
    sub_frames: usize,
    /// Weighted sum of squares so far in the current sub-block.
    sum: f64,
    frames: usize,
    /// The last sub-blocks' mean squares, oldest first.
    recent: Vec<f64>,
    histogram: Histogram,
}

impl Meter {
    /// `None` for no channels or a rate below 8 kHz.
    pub fn new(rate: u32, channels: usize) -> Option<Self> {
        if channels == 0 || rate < 8_000 {
            return None;
        }
        let rate_f = f64::from(rate);
        Some(Self {
            shelf: Biquad::shelf(rate_f),
            high_pass: Biquad::high_pass(rate_f),
            weights: weights(channels),
            states: vec![State::default(); channels],
            sub_frames: (rate as usize).div_ceil(10),
            sum: 0.0,
            frames: 0,
            recent: Vec::with_capacity(SUB_BLOCKS),
            histogram: Histogram::default(),
        })
    }

    /// Adds whole frames; a trailing partial frame is ignored.
    pub fn add(&mut self, samples: &[f32]) {
        let channels = self.states.len();
        for frame in samples.chunks_exact(channels) {
            let mut weighted = 0.0;
            for ((sample, state), weight) in frame.iter().zip(&mut self.states).zip(&self.weights) {
                let y = filter(&self.shelf, &self.high_pass, state, f64::from(*sample));
                weighted += weight * y * y;
            }
            self.sum += weighted;
            self.frames += 1;
            if self.frames == self.sub_frames {
                self.end_sub_block();
            }
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "a sub-block is under 10⁶ frames"
    )]
    fn end_sub_block(&mut self) {
        if self.recent.len() == SUB_BLOCKS {
            self.recent.remove(0);
        }
        self.recent.push(self.sum / self.sub_frames as f64);
        self.sum = 0.0;
        self.frames = 0;
        if self.recent.len() == SUB_BLOCKS {
            let mean = self.recent.iter().sum::<f64>() / SUB_BLOCKS as f64;
            if mean > 0.0 {
                self.histogram.add(lufs(mean));
            }
        }
    }

    /// The blocks so far.
    pub fn finish(self) -> Histogram {
        self.histogram
    }
}

/// How many 400 ms blocks fell in each 0.1 LU step above the absolute gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Histogram {
    counts: Vec<u32>,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            counts: vec![0; BINS],
        }
    }
}

impl Histogram {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "checked above the gate and capped to the last bin"
    )]
    fn add(&mut self, lufs: f64) {
        if lufs > ABSOLUTE_GATE {
            let bin = (((lufs - ABSOLUTE_GATE) * STEPS_PER_LU) as usize).min(BINS - 1);
            self.counts[bin] = self.counts[bin].saturating_add(1);
        }
    }

    /// Adds `other`'s blocks: the two measured as one programme.
    pub fn merge(&mut self, other: &Histogram) {
        for (count, more) in self.counts.iter_mut().zip(&other.counts) {
            *count = count.saturating_add(*more);
        }
    }

    /// Integrated loudness in LUFS; `None` when no block is above the
    /// absolute gate (silence, or under 400 ms).
    pub fn loudness(&self) -> Option<f64> {
        let ungated = self.mean_energy(0)?;
        let gate = lufs(ungated) + RELATIVE_GATE;
        self.mean_energy(first_bin_above(gate)).map(lufs)
    }

    /// Mean block energy from bin `from` up.
    fn mean_energy(&self, from: usize) -> Option<f64> {
        let (sum, count) = self.counts.iter().enumerate().skip(from).fold(
            (0.0, 0u64),
            |(sum, count), (bin, &n)| {
                (
                    sum + f64::from(n) * energy(centre(bin)),
                    count + u64::from(n),
                )
            },
        );
        #[expect(clippy::cast_precision_loss, reason = "block counts are far below 2⁵²")]
        (count > 0).then(|| sum / count as f64)
    }

    /// Sparse little-endian form for storage: (bin u16, count u32) pairs.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (bin, &count) in self.counts.iter().enumerate().filter(|(_, n)| **n > 0) {
            bytes.extend_from_slice(&u16::try_from(bin).unwrap_or(u16::MAX).to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
        }
        bytes
    }

    /// `None` when `bytes` is not what [`Self::to_bytes`] writes.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if !bytes.len().is_multiple_of(6) {
            return None;
        }
        let mut histogram = Self::default();
        for &[b0, b1, c0, c1, c2, c3] in bytes.as_chunks::<6>().0 {
            let bin = usize::from(u16::from_le_bytes([b0, b1]));
            let count = u32::from_le_bytes([c0, c1, c2, c3]);
            *histogram.counts.get_mut(bin)? = count;
        }
        Some(histogram)
    }
}

/// Loudness at the middle of `bin`.
#[expect(clippy::cast_precision_loss, reason = "bins are under 1000")]
fn centre(bin: usize) -> f64 {
    ABSOLUTE_GATE + (bin as f64 + 0.5) / STEPS_PER_LU
}

/// The first bin whose middle is above `gate`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0 first; past the last bin means none"
)]
fn first_bin_above(gate: f64) -> usize {
    (((gate - ABSOLUTE_GATE) * STEPS_PER_LU - 0.5).floor() + 1.0).max(0.0) as usize
}

fn filter(shelf: &Biquad, high_pass: &Biquad, state: &mut State, x: f64) -> f64 {
    let mid = shelf.b[0] * x + shelf.b[1] * state.x[0] + shelf.b[2] * state.x[1]
        - shelf.a[0] * state.mid[0]
        - shelf.a[1] * state.mid[1];
    let y = high_pass.b[0] * mid + high_pass.b[1] * state.mid[0] + high_pass.b[2] * state.mid[1]
        - high_pass.a[0] * state.y[0]
        - high_pass.a[1] * state.y[1];
    state.x = [x, state.x[0]];
    state.mid = [mid, state.mid[0]];
    state.y = [y, state.y[0]];
    y
}

/// BS.1770 channel weights. Five or more channels follow the WAV/FLAC
/// order (L, R, C, LFE, Ls, Rs, …): the LFE is left out and the surrounds
/// weigh 1.41 (+1.5 dB).
fn weights(channels: usize) -> Vec<f64> {
    (0..channels)
        .map(|channel| match (channels, channel) {
            (6.., 3) => 0.0,
            (5.., 4..) => 1.41,
            _ => 1.0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Histogram, Meter};

    /// `tenths` of a second of a 997 Hz sine at `dbfs` on every one of
    /// `channels`.
    #[expect(clippy::cast_possible_truncation, reason = "samples are in −1..1")]
    fn sine(rate: u32, channels: usize, dbfs: f64, tenths: u32) -> Vec<f32> {
        let amplitude = 10f64.powf(dbfs / 20.0);
        (0..rate / 10 * tenths)
            .flat_map(|n| {
                let phase = 2.0 * std::f64::consts::PI * 997.0 * f64::from(n) / f64::from(rate);
                std::iter::repeat_n((amplitude * phase.sin()) as f32, channels)
            })
            .collect()
    }

    fn measure(rate: u32, channels: usize, samples: &[f32]) -> Option<f64> {
        let mut meter = Meter::new(rate, channels).unwrap();
        meter.add(samples);
        meter.finish().loudness()
    }

    #[test]
    fn a_stereo_sine_at_minus_23_dbfs_is_minus_23_lufs() {
        // EBU Tech 3341, test 1 and 2, at the common rates.
        for rate in [44_100, 48_000, 96_000, 192_000] {
            let loudness = measure(rate, 2, &sine(rate, 2, -23.0, 200)).unwrap();
            assert!((loudness + 23.0).abs() < 0.1, "{rate} Hz: {loudness}");
        }
    }

    #[test]
    fn one_channel_alone_is_3_db_quieter() {
        let loudness = measure(48_000, 1, &sine(48_000, 1, -20.0, 100)).unwrap();
        assert!((loudness + 23.01).abs() < 0.1, "{loudness}");
    }

    #[test]
    fn silence_and_short_audio_have_no_loudness() {
        assert_eq!(measure(48_000, 2, &vec![0.0; 48_000 * 4]), None);
        assert_eq!(measure(48_000, 2, &sine(48_000, 2, -20.0, 3)), None);
        assert_eq!(Meter::new(48_000, 0).map(|_| ()), None);
    }

    #[test]
    fn silence_does_not_lower_the_level() {
        let mut samples = sine(48_000, 2, -20.0, 100);
        samples.extend(vec![0.0; 48_000 * 2 * 30]);
        let loudness = measure(48_000, 2, &samples).unwrap();
        assert!((loudness + 20.0).abs() < 0.1, "{loudness}");
    }

    #[test]
    fn quiet_passages_below_the_relative_gate_are_left_out() {
        // EBU Tech 3341 test 3 shape: −36 dB then −23 dB then −36 dB.
        let rate = 48_000;
        let mut samples = sine(rate, 2, -36.0, 100);
        samples.extend(sine(rate, 2, -23.0, 600));
        samples.extend(sine(rate, 2, -36.0, 100));
        let loudness = measure(rate, 2, &samples).unwrap();
        assert!((loudness + 23.0).abs() < 0.1, "{loudness}");
    }

    #[test]
    fn an_album_is_gated_as_one_programme() {
        let rate = 44_100;
        let mut loud = Meter::new(rate, 2).unwrap();
        loud.add(&sine(rate, 2, -14.0, 300));
        let mut quiet = Meter::new(rate, 2).unwrap();
        quiet.add(&sine(rate, 2, -20.0, 300));
        let mut album = loud.finish();
        album.merge(&quiet.finish());
        let stored = Histogram::from_bytes(&album.to_bytes()).unwrap();
        assert_eq!(stored, album);
        let album = album.loudness().unwrap();
        // Energy mean of −14 and −20: about −16.03.
        assert!((album + 16.03).abs() < 0.1, "{album}");
    }

    #[test]
    fn stored_histograms_are_checked() {
        assert_eq!(Histogram::from_bytes(&[1, 2, 3]), None);
        assert_eq!(Histogram::from_bytes(&[0xff, 0xff, 1, 0, 0, 0]), None);
        assert_eq!(Histogram::from_bytes(&[]).unwrap().loudness(), None);
    }

    #[test]
    fn the_lfe_does_not_count() {
        let rate = 48_000;
        let mut samples = sine(rate, 6, -23.0, 100);
        for frame in samples.as_chunks_mut::<6>().0 {
            frame[2] = 0.0;
            frame[4] = 0.0;
            frame[5] = 0.0;
        }
        let with_lfe = measure(rate, 6, &samples).unwrap();
        for frame in samples.as_chunks_mut::<6>().0 {
            frame[3] = 0.0;
        }
        assert!((with_lfe - measure(rate, 6, &samples).unwrap()).abs() < 0.001);
        assert!((with_lfe + 23.0).abs() < 0.1, "{with_lfe}");
    }
}
