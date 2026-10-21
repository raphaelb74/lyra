//! Linear-phase low-pass design by the Kaiser window method.
//!
//! Runs once when a converter is created (control plane), never while audio
//! flows.

use std::f64::consts::PI;

/// Stopband attenuation of the filters on PCM samples.
pub(crate) const ATTENUATION_DB: f64 = 120.0;
/// Stopband attenuation of the filter on bits. DSD's shaped noise is near
/// full scale, and the idle pattern sits exactly on the first output rate,
/// so its leak lands on DC: this keeps both below one 24-bit step.
pub(crate) const BIT_ATTENUATION_DB: f64 = 150.0;
/// Kaiser's estimates fall a few dB short for short filters: design with margin.
const MARGIN_DB: f64 = 8.0;

/// Taps of an odd-length symmetric low-pass at `rate` Hz, flat to `pass` Hz
/// and at least `attenuation` dB down from `stop` Hz, with DC gain 1.
///
/// The half-length (taps either side of the centre) is rounded up to a
/// multiple of `align`, so a filter over bits can centre on a byte boundary.
pub(crate) fn lowpass(rate: f64, pass: f64, stop: f64, attenuation: f64, align: usize) -> Vec<f64> {
    let design = attenuation + MARGIN_DB;
    // Kaiser's length estimate: N ≈ (A − 7.95) / (14.36 · Δf / fs).
    let estimate = (design - 7.95) / (14.36 * (stop - pass) / rate);
    let half = ceil_to_usize(estimate / 2.0).max(1).next_multiple_of(align);
    let beta = 0.1102 * (design - 8.7);
    let cutoff = f64::midpoint(pass, stop) / rate;
    let centre = to_f64(half);
    let window_scale = bessel_i0(beta);

    let mut taps: Vec<f64> = (0..=2 * half)
        .map(|n| {
            let m = to_f64(n) - centre;
            let ideal = if n == half {
                2.0 * cutoff
            } else {
                (2.0 * PI * cutoff * m).sin() / (PI * m)
            };
            let ratio = m / centre;
            let window = bessel_i0(beta * (1.0 - ratio * ratio).max(0.0).sqrt()) / window_scale;
            ideal * window
        })
        .collect();
    let sum: f64 = taps.iter().sum();
    for tap in &mut taps {
        *tap /= sum;
    }
    taps
}

/// Modified Bessel function of the first kind, order 0 (power series).
fn bessel_i0(x: f64) -> f64 {
    let quarter = x * x / 4.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    let mut k = 1.0;
    while term > sum * 1e-17 {
        term *= quarter / (k * k);
        sum += term;
        k += 1.0;
    }
    sum
}

/// Filter lengths stay in the hundreds, so the conversion is exact.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn to_f64(n: usize) -> f64 {
    n as f64
}

/// `x` is a positive length estimate well below `usize::MAX`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn ceil_to_usize(x: f64) -> usize {
    x.ceil().max(0.0) as usize
}

#[cfg(test)]
pub(crate) mod response {
    use std::f64::consts::PI;

    /// Gain in dB of `taps` at `freq` Hz for a filter running at `rate` Hz.
    pub fn gain_db(taps: &[f64], rate: f64, freq: f64) -> f64 {
        let w = 2.0 * PI * freq / rate;
        let (re, im) = taps
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(re, im), (n, &tap)| {
                let phase = w * super::to_f64(n);
                (re + tap * phase.cos(), im - tap * phase.sin())
            });
        20.0 * (re * re + im * im).sqrt().max(1e-300).log10()
    }

    /// Highest gain in dB over `from..=to` Hz.
    pub fn worst_db(taps: &[f64], rate: f64, from: f64, to: f64) -> f64 {
        (0..=1000)
            .map(|step| from + (to - from) * f64::from(step) / 1000.0)
            .map(|freq| gain_db(taps, rate, freq))
            .fold(f64::MIN, f64::max)
    }
}

#[cfg(test)]
mod tests {
    use super::response::{gain_db, worst_db};
    use super::{ATTENUATION_DB, lowpass};

    #[test]
    fn meets_the_passband_and_stopband() {
        let rate = 176_400.0;
        let taps = lowpass(rate, 20_000.0, 30_000.0, ATTENUATION_DB, 1);
        assert_eq!(taps.len() % 2, 1);
        for step in 0..=200 {
            let freq = 20_000.0 * f64::from(step) / 200.0;
            assert!(gain_db(&taps, rate, freq).abs() < 0.001, "{freq} Hz");
        }
        assert!(worst_db(&taps, rate, 30_000.0, rate / 2.0) < -ATTENUATION_DB);
    }

    #[test]
    fn half_length_is_aligned() {
        let taps = lowpass(2_822_400.0, 20_000.0, 322_800.0, ATTENUATION_DB, 8);
        assert_eq!((taps.len() - 1) / 2 % 8, 0);
    }
}
