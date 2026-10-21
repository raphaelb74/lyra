//! Signal behaviour of the converter, from synthetic DSD with a known
//! content.

// Measurement code: lengths and sample indexes convert to and from f64.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
#![expect(clippy::unwrap_used, reason = "test helpers fail loudly")]

use std::f64::consts::PI;

use dsd_pcm::Converter;

const DSD64: u32 = 2_822_400;
const SILENCE: u8 = 0x69;

/// 1-bit DSD of `signal(t)` (seconds → −1..1) from a second-order
/// delta-sigma modulator: bytes, oldest bit in bit 7.
fn modulate(dsd_rate: u32, seconds: f64, signal: impl Fn(f64) -> f64) -> Vec<u8> {
    let bits = (f64::from(dsd_rate) * seconds) as usize / 8 * 8;
    let (mut first, mut second, mut out) = (0.0, 0.0, -1.0);
    let mut bytes = vec![0u8; bits / 8];
    for n in 0..bits {
        let x = signal(n as f64 / f64::from(dsd_rate));
        first += x - out;
        second += first - out;
        out = if second >= 0.0 { 1.0 } else { -1.0 };
        if out > 0.0 {
            bytes[n / 8] |= 0x80 >> (n % 8);
        }
    }
    bytes
}

fn interleave(channels: &[Vec<u8>]) -> Vec<u8> {
    (0..channels[0].len())
        .flat_map(|i| channels.iter().map(move |channel| channel[i]))
        .collect()
}

fn convert(dsd_rate: u32, output: u32, channels: usize, gain_db: f64, dsd: &[u8]) -> Vec<i32> {
    let mut converter = Converter::new(dsd_rate, output, channels, gain_db).unwrap();
    let mut out = Vec::new();
    converter.push(dsd, &mut out);
    converter.finish(&mut out);
    out
}

/// One channel of interleaved samples, as −1..1.
fn channel(samples: &[i32], channels: usize, index: usize) -> Vec<f64> {
    samples
        .iter()
        .skip(index)
        .step_by(channels)
        .map(|&s| f64::from(s >> 8) / 8_388_608.0)
        .collect()
}

/// Amplitude of `freq` and RMS of everything else, over whole cycles in the
/// middle of `samples` (edges skipped).
fn analyse(samples: &[f64], rate: u32, freq: f64) -> (f64, f64) {
    let period = f64::from(rate) / freq;
    let skip = samples.len() / 8;
    let cycles = ((samples.len() - 2 * skip) as f64 / period).floor();
    let length = (cycles * period).round() as usize;
    let part = &samples[skip..skip + length];
    let w = 2.0 * PI * freq / f64::from(rate);
    let (mut a, mut b) = (0.0, 0.0);
    for (n, &y) in part.iter().enumerate() {
        let t = (skip + n) as f64;
        a += y * (w * t).sin();
        b += y * (w * t).cos();
    }
    let (a, b) = (2.0 * a / length as f64, 2.0 * b / length as f64);
    let residual = part
        .iter()
        .enumerate()
        .map(|(n, &y)| {
            let t = (skip + n) as f64;
            let fit = a * (w * t).sin() + b * (w * t).cos();
            (y - fit) * (y - fit)
        })
        .sum::<f64>()
        / length as f64;
    ((a * a + b * b).sqrt(), residual.sqrt())
}

fn db(ratio: f64) -> f64 {
    20.0 * ratio.log10()
}

fn sine(freq: f64, amplitude: f64) -> impl Fn(f64) -> f64 {
    move |t| amplitude * (2.0 * PI * freq * t).sin()
}

#[test]
fn a_tone_keeps_its_frequency_and_level() {
    for multiple in [1, 2, 4] {
        let dsd_rate = DSD64 * multiple;
        let dsd = modulate(dsd_rate, 0.1, sine(1_000.0, 0.5));
        for output in [88_200, 44_100] {
            let out = convert(dsd_rate, output, 1, 0.0, &dsd);
            let (amplitude, residual) = analyse(&channel(&out, 1, 0), output, 1_000.0);
            let error = db(amplitude / 0.5);
            assert!(
                error.abs() < 0.1,
                "DSD{} at {output}: {error} dB",
                64 * multiple
            );
            // Nothing but the tone: modulator noise and dither only.
            assert!(db(residual) < -60.0, "residual {} dB", db(residual));
        }
    }
}

#[test]
fn gain_shifts_the_level_exactly() {
    let dsd = modulate(DSD64, 0.1, sine(1_000.0, 0.5));
    let level = |gain| {
        let out = convert(DSD64, 88_200, 1, gain, &dsd);
        db(analyse(&channel(&out, 1, 0), 88_200, 1_000.0).0)
    };
    let reference = level(0.0);
    // A −6 dB modulation comes out at −6 dBFS: the mapping is proportional.
    assert!((reference - db(0.5)).abs() < 0.1, "{reference}");
    for gain in [-3.0, -6.0] {
        assert!((level(gain) - reference - gain).abs() < 0.01);
    }
}

#[test]
fn silence_converts_to_dither_only() {
    let dsd = vec![SILENCE; 40_000];
    let out = convert(DSD64, 88_200, 2, 0.0, &dsd);
    // 20 000 bytes per channel, one output per 4 bytes, two channels.
    assert_eq!(out.len(), 10_000);
    // TPDF dither is at most one step either way; no offset, no burst.
    assert!(out.iter().all(|&s| (s >> 8).abs() <= 1), "{:?}", &out[..8]);
    let mean = out.iter().map(|&s| f64::from(s >> 8)).sum::<f64>() / out.len() as f64;
    assert!(mean.abs() < 0.05, "{mean}");
}

#[test]
fn ultrasonic_tones_do_not_reach_the_output() {
    for freq in [35_000.0, 60_000.0, 100_000.0] {
        let dsd = modulate(DSD64, 0.05, sine(freq, 0.5));
        for output in [88_200, 44_100] {
            let out = convert(DSD64, output, 1, 0.0, &dsd);
            let samples = channel(&out, 1, 0);
            // The middle: a tone cut off at the edges spreads into the band.
            let middle = &samples[samples.len() / 8..samples.len() * 7 / 8];
            let rms = (middle.iter().map(|y| y * y).sum::<f64>() / middle.len() as f64).sqrt();
            // Bounded by the test modulator's own noise, not by the filter.
            assert!(db(rms) < -70.0, "{freq} Hz at {output}: {} dB", db(rms));
        }
    }
}

#[test]
fn channels_stay_independent() {
    let tone = modulate(DSD64, 0.05, sine(1_000.0, 0.5));
    let quiet = vec![SILENCE; tone.len()];
    let out = convert(DSD64, 88_200, 2, 0.0, &interleave(&[tone, quiet]));
    let (left, _) = analyse(&channel(&out, 2, 0), 88_200, 1_000.0);
    assert!((db(left / 0.5)).abs() < 0.1);
    assert!(
        channel(&out, 2, 1)
            .iter()
            .all(|y| y.abs() <= 1.0 / 8_388_608.0)
    );
}

#[test]
fn chunking_does_not_change_the_output() {
    let left = modulate(DSD128_RATE, 0.02, sine(997.0, 0.4));
    let right = modulate(DSD128_RATE, 0.02, sine(3_001.0, 0.3));
    let dsd = interleave(&[left, right]);
    let whole = convert(DSD128_RATE, 88_200, 2, -3.0, &dsd);

    let mut converter = Converter::new(DSD128_RATE, 88_200, 2, -3.0).unwrap();
    let mut pieces = Vec::new();
    let mut rest = dsd.as_slice();
    for frames in [1, 7, 13, 1, 1_001, 64].iter().cycle() {
        if rest.is_empty() {
            break;
        }
        let (chunk, tail) = rest.split_at((frames * 2).min(rest.len()));
        converter.push(chunk, &mut pieces);
        rest = tail;
    }
    converter.finish(&mut pieces);
    assert_eq!(whole, pieces);
}

const DSD128_RATE: u32 = 2 * DSD64;

#[test]
fn reset_starts_over_cleanly() {
    let dsd = modulate(DSD64, 0.02, sine(1_000.0, 0.5));
    let fresh = convert(DSD64, 88_200, 1, 0.0, &dsd);

    let mut converter = Converter::new(DSD64, 88_200, 1, 0.0).unwrap();
    let mut out = Vec::new();
    converter.push(&modulate(DSD64, 0.01, sine(5_000.0, 0.7)), &mut out);
    // A seek: whatever was in the filters is gone.
    converter.reset();
    out.clear();
    converter.push(&dsd, &mut out);
    converter.finish(&mut out);
    assert_eq!(out, fresh);
}

#[test]
fn every_input_reaches_the_output() {
    for (bytes, output) in [(35_280, 88_200), (35_280, 44_100), (35_281, 88_200)] {
        let out = convert(DSD64, output, 1, 0.0, &vec![SILENCE; bytes]);
        // One output per 352_800 / output bytes, rounded up: nothing is cut.
        let per_output = (DSD64 / 8 / output) as usize;
        assert_eq!(
            out.len(),
            bytes.div_ceil(per_output),
            "{bytes} bytes at {output}"
        );
    }
}

#[test]
fn overload_is_clamped_and_counted() {
    let mut converter = Converter::new(DSD64, 88_200, 1, 0.0).unwrap();
    let mut out = Vec::new();
    // All ones is +1.0, just past the largest 24-bit value.
    converter.push(&vec![0xFF; 20_000], &mut out);
    let settled = &out[out.len() / 2..];
    assert!(
        settled.iter().all(|&s| s == 8_388_607 << 8),
        "wrapped or unclamped"
    );
    assert!(converter.clipped() >= settled.len() as u64 / 2);

    let mut quiet = Converter::new(DSD64, 88_200, 1, -3.0).unwrap();
    quiet.push(&modulate(DSD64, 0.02, sine(1_000.0, 0.5)), &mut out);
    assert_eq!(quiet.clipped(), 0);
}
