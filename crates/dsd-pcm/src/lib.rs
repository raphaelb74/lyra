//! DSD → PCM conversion, the opt-in last resort for devices that take
//! neither native DSD nor DoP. **Never bit-perfect.**
//!
//! Input is what `format-dsd` produces: DSD bytes, oldest bit in bit 7,
//! interleaved one byte per channel. Output is interleaved 24-bit PCM,
//! left-justified in `i32` (the convention of `format-pcm`'s packers).
//!
//! The signal is band-limited to the audio band (flat to 20 kHz, at least
//! 120 dB down from 30 kHz, or from the output Nyquist frequency at 44.1 kHz;
//! 150 dB for the first stage, which faces DSD's near full-scale noise)
//! and decimated in stages:
//!
//! 1. A filter over the raw bits decimates to 352.8 kHz. Each byte holds
//!    eight ±1 samples, so the contribution of eight taps depends only on the
//!    byte's value: it is a table lookup, not eight multiplications.
//! 2. Decimate-by-2 filters down to the output rate.
//!
//! All filters are linear phase and centred: output sample `j` is the
//! filtered signal at exactly the time of input byte `j × factor`, so there is
//! no delay to compensate. The first outputs see silence before the start
//! and [`Converter::finish`] feeds silence after the end, so nothing is lost.
//!
//! Level is proportional: an all-ones stream is +1.0 full scale and all
//! zeros −1.0. A fixed gain (usually a reduction, for headroom) is applied,
//! then TPDF dither at one 24-bit step; samples beyond full scale are clamped
//! and counted.

mod design;

use std::iter;

use design::{ATTENUATION_DB, BIT_ATTENUATION_DB, lowpass};

/// Output rates, preferred first. The audio band ends near 30 kHz (DSD's
/// shaped noise rises above it), so 88.2 kHz carries all of it; higher rates
/// would add only filtered silence at several times the cost.
pub const OUTPUT_RATES: [u32; 2] = [88_200, 44_100];

/// Rate after the bit filter.
const FIRST_RATE: u32 = 352_800;
/// Passband edge: flat up to here.
const PASS_HZ: f64 = 20_000.0;
/// Stopband edge, unless the output Nyquist frequency is lower.
const STOP_HZ: f64 = 30_000.0;
/// DSD idle pattern, fed before the start and after the end.
const SILENCE: u8 = 0x69;
/// 24-bit full scale.
const FULL_SCALE: f64 = 8_388_608.0;

/// Output rates available for a DSD rate: the 44.1 kHz family (DSD64 and
/// its multiples). Empty for anything else.
pub fn output_rates(dsd_rate: u32) -> &'static [u32] {
    if bytes_per_first_output(dsd_rate).is_some() {
        &OUTPUT_RATES
    } else {
        &[]
    }
}

fn bytes_per_first_output(dsd_rate: u32) -> Option<usize> {
    let per_byte = FIRST_RATE * 8;
    (dsd_rate >= per_byte && dsd_rate.is_multiple_of(per_byte))
        .then(|| usize::try_from(dsd_rate / per_byte).ok())
        .flatten()
}

/// Converts one DSD stream. Create it on the control plane (it designs its
/// filters); [`Self::push`] then runs without allocating once its buffers
/// have grown to the chunk size.
#[derive(Debug)]
pub struct Converter {
    dsd_rate: u32,
    output_rate: u32,
    bits: BitFilter,
    stages: Vec<Decimator>,
    lanes: Vec<Lane>,
    gain: f64,
    dither: Dither,
    clipped: u64,
    peak: f64,
}

impl Converter {
    /// `None` when `dsd_rate` or `output_rate` is not supported (see
    /// [`output_rates`]) or `channels` is 0. `gain_db` is applied to every
    /// sample, e.g. −3.0 for 3 dB of headroom.
    pub fn new(dsd_rate: u32, output_rate: u32, channels: usize, gain_db: f64) -> Option<Self> {
        let bytes_per_output = bytes_per_first_output(dsd_rate)?;
        if channels == 0 || !OUTPUT_RATES.contains(&output_rate) {
            return None;
        }
        let band_edge = STOP_HZ.min(f64::from(output_rate) / 2.0);
        let bits = BitFilter::new(
            f64::from(dsd_rate),
            bytes_per_output,
            f64::from(FIRST_RATE) - band_edge,
        );

        let mut stages = Vec::new();
        let mut rate = FIRST_RATE;
        while rate > output_rate {
            let next = rate / 2;
            // Intermediate stages only stop what would fold into the band
            // the later stages keep; the last stage also removes everything
            // above `band_edge`.
            let stage_stop = if next == output_rate {
                band_edge
            } else {
                f64::from(next) - band_edge
            };
            stages.push(Decimator {
                taps: lowpass(f64::from(rate), PASS_HZ, stage_stop, ATTENUATION_DB, 1),
            });
            rate = next;
        }

        let mut converter = Self {
            dsd_rate,
            output_rate,
            lanes: (0..channels).map(|_| Lane::default()).collect(),
            bits,
            stages,
            gain: 10f64.powf(gain_db / 20.0),
            dither: Dither::new(),
            clipped: 0,
            peak: 0.0,
        };
        converter.reset();
        Some(converter)
    }

    pub fn output_rate(&self) -> u32 {
        self.output_rate
    }

    pub fn channels(&self) -> usize {
        self.lanes.len()
    }

    /// Whether this converter can continue into a stream of this shape, for
    /// gapless playback: the filter state carries over unchanged.
    pub fn continues(&self, dsd_rate: u32, output_rate: u32, channels: usize) -> bool {
        (self.dsd_rate, self.output_rate, self.lanes.len()) == (dsd_rate, output_rate, channels)
    }

    /// Samples clamped at full scale since creation.
    pub fn clipped(&self) -> u64 {
        self.clipped
    }

    /// Highest absolute level since creation, after the gain and before
    /// clamping: 1.0 is full scale, above it clips.
    pub fn peak(&self) -> f64 {
        self.peak
    }

    /// Converts interleaved DSD bytes, appending interleaved samples to
    /// `out`. A trailing partial frame is ignored.
    pub fn push(&mut self, dsd: &[u8], out: &mut Vec<i32>) {
        let channels = self.lanes.len();
        let dsd = &dsd[..dsd.len() / channels * channels];
        for (channel, lane) in self.lanes.iter_mut().enumerate() {
            lane.bytes
                .extend(dsd.iter().skip(channel).step_by(channels).copied());
        }
        self.run(false, out);
    }

    /// Flushes the filters at the end of the stream: the last input reaches
    /// the output. Call [`Self::reset`] before reusing the converter.
    pub fn finish(&mut self, out: &mut Vec<i32>) {
        self.run(true, out);
    }

    /// Back to the start state, as before the first input (after a seek).
    pub fn reset(&mut self) {
        self.dither = Dither::new();
        let lead = self.bits.lead;
        for lane in &mut self.lanes {
            lane.bytes.clear();
            lane.bytes.extend(iter::repeat_n(SILENCE, lead));
            lane.history.resize_with(self.stages.len(), Vec::new);
            for (history, stage) in lane.history.iter_mut().zip(&self.stages) {
                history.clear();
                history.extend(iter::repeat_n(0.0, stage.lead()));
            }
        }
    }

    fn run(&mut self, flush: bool, out: &mut Vec<i32>) {
        for lane in &mut self.lanes {
            lane.out.clear();
            if flush {
                lane.bytes.extend(iter::repeat_n(SILENCE, self.bits.lead));
            }
            self.bits.run(&mut lane.bytes, &mut lane.out);
            for (history, stage) in lane.history.iter_mut().zip(&self.stages) {
                history.extend_from_slice(&lane.out);
                if flush {
                    history.extend(iter::repeat_n(0.0, stage.lead()));
                }
                lane.out.clear();
                stage.run(history, &mut lane.out);
            }
        }
        // Every lane received the same number of bytes, so the same count.
        let frames = self
            .lanes
            .iter()
            .map(|lane| lane.out.len())
            .min()
            .unwrap_or(0);
        out.reserve(frames * self.lanes.len());
        for frame in 0..frames {
            for lane in &self.lanes {
                let sample = lane.out[frame] * self.gain;
                self.peak = self.peak.max(sample.abs());
                out.push(quantize(sample, &mut self.dither, &mut self.clipped));
            }
        }
    }
}

/// One channel's filter state.
#[derive(Debug, Default)]
struct Lane {
    /// Bytes not yet consumed by the bit filter (its window history first).
    bytes: Vec<u8>,
    /// Per decimator: samples not yet consumed (window history first).
    history: Vec<Vec<f64>>,
    /// Scratch for the samples between stages, then the lane's output.
    out: Vec<f64>,
}

/// The first stage, over bits: `table[g * 256 + byte]` is the sum of taps
/// `8g..8g+8`, each times ±1 for the byte's bits (bit 7 first).
#[derive(Debug)]
struct BitFilter {
    table: Vec<f64>,
    /// Window length in bytes.
    groups: usize,
    /// Bytes per output sample.
    step: usize,
    /// Bytes before the centre of the window (the history to prime).
    lead: usize,
}

impl BitFilter {
    fn new(rate: f64, step: usize, stop_hz: f64) -> Self {
        let taps = lowpass(rate, PASS_HZ, stop_hz, BIT_ATTENUATION_DB, 8);
        // Centred on bit 7 of byte `lead`; trailing bits of the last byte are 0.
        let lead = (taps.len() - 1) / 2 / 8;
        let groups = taps.len().div_ceil(8);
        let mut table = vec![0.0; groups * 256];
        for (group, entries) in table.as_chunks_mut::<256>().0.iter_mut().enumerate() {
            for (byte, entry) in entries.iter_mut().enumerate() {
                *entry = (0..8)
                    .filter_map(|bit| {
                        let tap = taps.get(group * 8 + bit)?;
                        let one = byte & (0x80 >> bit) != 0;
                        Some(if one { *tap } else { -*tap })
                    })
                    .sum();
            }
        }
        Self {
            table,
            groups,
            step,
            lead,
        }
    }

    fn run(&self, bytes: &mut Vec<u8>, out: &mut Vec<f64>) {
        let mut start = 0;
        while start + self.groups <= bytes.len() {
            let window = &bytes[start..start + self.groups];
            let sum = window
                .iter()
                .zip(self.table.as_chunks::<256>().0)
                .map(|(&byte, entries)| entries[usize::from(byte)])
                .sum();
            out.push(sum);
            start += self.step;
        }
        bytes.drain(..start.min(bytes.len()));
    }
}

/// A decimate-by-2 low-pass.
#[derive(Debug)]
struct Decimator {
    taps: Vec<f64>,
}

impl Decimator {
    /// Samples before the centre of the window.
    fn lead(&self) -> usize {
        (self.taps.len() - 1) / 2
    }

    fn run(&self, history: &mut Vec<f64>, out: &mut Vec<f64>) {
        let length = self.taps.len();
        let mut start = 0;
        while start + length <= history.len() {
            out.push(dot(&self.taps, &history[start..start + length]));
            start += 2;
        }
        history.drain(..start.min(history.len()));
    }
}

/// Four independent sums, so the compiler can vectorise.
fn dot(a: &[f64], b: &[f64]) -> f64 {
    let mut sums = [0.0; 4];
    let (a4, a_rest) = a.as_chunks::<4>();
    let (b4, b_rest) = b.as_chunks::<4>();
    for (x, y) in a4.iter().zip(b4) {
        for lane in 0..4 {
            sums[lane] += x[lane] * y[lane];
        }
    }
    let rest: f64 = a_rest.iter().zip(b_rest).map(|(x, y)| x * y).sum();
    (sums[0] + sums[1]) + (sums[2] + sums[3]) + rest
}

/// To a left-justified 24-bit sample, with TPDF dither; clamps and counts.
fn quantize(sample: f64, dither: &mut Dither, clipped: &mut u64) -> i32 {
    let max = FULL_SCALE - 1.0;
    let value = (sample * FULL_SCALE + dither.next_tpdf()).round();
    let value = if value > max {
        *clipped += 1;
        max
    } else if value < -FULL_SCALE {
        *clipped += 1;
        -FULL_SCALE
    } else {
        value
    };
    // In range: |value| ≤ 2^23, so the cast is exact.
    #[allow(clippy::cast_possible_truncation)]
    let value = value as i32;
    value << 8
}

/// Triangular dither of ±1 step from a fixed-seed xorshift generator, so
/// output is repeatable.
#[derive(Debug)]
struct Dither(u64);

impl Dither {
    fn new() -> Self {
        Self(0x9E37_79B9_7F4A_7C15)
    }

    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        // 53 random bits: exactly representable.
        #[allow(clippy::cast_precision_loss)]
        let value = (self.0 >> 11) as f64;
        value / 9_007_199_254_740_992.0
    }

    fn next_tpdf(&mut self) -> f64 {
        self.uniform() - self.uniform()
    }
}

#[cfg(test)]
mod tests {
    use super::design::response::{gain_db, worst_db};
    use super::{Converter, FIRST_RATE, OUTPUT_RATES, PASS_HZ, output_rates};

    #[test]
    fn supports_the_44k_family_only() {
        assert_eq!(output_rates(2_822_400), &OUTPUT_RATES);
        assert_eq!(output_rates(22_579_200), &OUTPUT_RATES);
        assert_eq!(output_rates(3_072_000), &[] as &[u32]);
        assert!(Converter::new(2_822_400, 176_400, 2, 0.0).is_none());
        assert!(Converter::new(2_822_400, 88_200, 0, 0.0).is_none());
    }

    /// Every stage keeps the passband flat and stops, over its whole
    /// stopband, what would fold into the band kept after it.
    #[test]
    fn each_stage_meets_its_specification() {
        for multiple in [64, 128, 256, 512] {
            for output in OUTPUT_RATES {
                let dsd_rate = 44_100 * multiple;
                let converter = Converter::new(dsd_rate, output, 2, 0.0).unwrap();
                let stop = 30_000f64.min(f64::from(output) / 2.0);
                let label = format!("DSD{multiple} to {output} Hz");

                // One bit set versus none: that tap counted +1, then −1.
                let bit_taps: Vec<f64> = (0..converter.bits.groups * 8)
                    .map(|n| {
                        let entries = &converter.bits.table[n / 8 * 256..][..256];
                        (entries[0x80 >> (n % 8)] - entries[0]) / 2.0
                    })
                    .collect();
                let rate = f64::from(dsd_rate);
                let mut pass_db = gain_db(&bit_taps, rate, PASS_HZ);
                let alias = f64::from(FIRST_RATE) - stop;
                let worst = worst_db(&bit_taps, rate, alias, rate / 2.0);
                assert!(worst < -150.0, "{label}, bits: {worst} dB");

                let mut rate = f64::from(FIRST_RATE);
                for (index, stage) in converter.stages.iter().enumerate() {
                    let next = rate / 2.0;
                    let last = index + 1 == converter.stages.len();
                    let stage_stop = if last { stop } else { next - stop };
                    pass_db += gain_db(&stage.taps, rate, PASS_HZ);
                    let worst = worst_db(&stage.taps, rate, stage_stop, rate / 2.0);
                    assert!(worst < -120.0, "{label}, stage {index}: {worst} dB");
                    rate = next;
                }
                assert!(pass_db.abs() < 0.05, "{label}: {pass_db} dB at 20 kHz");
            }
        }
    }
}
