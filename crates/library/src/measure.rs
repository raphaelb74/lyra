//! Measures a file's loudness by decoding it whole: the playback decoders
//! for FLAC and WAV, DSD through the PCM converter (for analysis only).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use dsd_pcm::Converter;
use format_dsd::DsdReader;
use format_pcm::Decoder;

use crate::meter::{Histogram, Meter};
use crate::tags::is_dsd;

/// Left-justified `i32` full scale.
const FULL_SCALE: f64 = 2_147_483_648.0;
/// DSD is measured against the Scarlet Book reference, 50 % modulation as
/// 0 dBFS, the level most DACs play it at: twice the converter's scale.
const DSD_REFERENCE: f64 = 2.0;

/// The file's blocks; `Ok(None)` when `stop` was raised meanwhile.
pub(crate) fn measure(path: &Path, stop: &AtomicBool) -> Result<Option<Histogram>, String> {
    if is_dsd(path) {
        measure_dsd(path, stop)
    } else {
        measure_pcm(path, stop)
    }
}

fn measure_pcm(path: &Path, stop: &AtomicBool) -> Result<Option<Histogram>, String> {
    let mut decoder = Decoder::open(path).map_err(|error| error.to_string())?;
    let info = *decoder.info();
    let mut meter = Meter::new(info.sample_rate, info.channels).ok_or("unsupported stream")?;
    let mut samples = Vec::new();
    let mut floats = Vec::new();
    while decoder
        .decode_next(&mut samples)
        .map_err(|error| error.to_string())?
    {
        if stop.load(Ordering::Relaxed) {
            return Ok(None);
        }
        add(&mut meter, &samples, 1.0, &mut floats);
    }
    Ok(Some(meter.finish()))
}

fn measure_dsd(path: &Path, stop: &AtomicBool) -> Result<Option<Histogram>, String> {
    let mut reader = DsdReader::open(path).map_err(|error| error.to_string())?;
    let info = *reader.info();
    let rate = *dsd_pcm::output_rates(info.dsd_rate)
        .last()
        .ok_or("DSD rate the converter does not take")?;
    let mut converter =
        Converter::for_analysis(info.dsd_rate, rate, info.channels).ok_or("unsupported stream")?;
    let mut meter = Meter::new(rate, info.channels).ok_or("unsupported stream")?;
    let (mut dsd, mut samples, mut floats) = (Vec::new(), Vec::new(), Vec::new());
    while reader
        .read_next(&mut dsd)
        .map_err(|error| error.to_string())?
    {
        if stop.load(Ordering::Relaxed) {
            return Ok(None);
        }
        samples.clear();
        converter.push(&dsd, &mut samples);
        add(&mut meter, &samples, DSD_REFERENCE, &mut floats);
    }
    samples.clear();
    converter.finish(&mut samples);
    add(&mut meter, &samples, DSD_REFERENCE, &mut floats);
    Ok(Some(meter.finish()))
}

#[expect(clippy::cast_possible_truncation, reason = "samples are within ±2")]
fn add(meter: &mut Meter, samples: &[i32], scale: f64, floats: &mut Vec<f32>) {
    floats.clear();
    floats.extend(
        samples
            .iter()
            .map(|&sample| (f64::from(sample) / FULL_SCALE * scale) as f32),
    );
    meter.add(floats);
}

/// Integrated loudness of one file in LUFS, for tools and tests.
pub fn measure_loudness(path: &Path) -> Result<Option<f64>, String> {
    Ok(measure(path, &AtomicBool::new(false))?.and_then(|blocks| blocks.loudness()))
}
