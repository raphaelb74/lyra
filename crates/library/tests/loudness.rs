//! Loudness for volume levelling: tags, background measuring, albums.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "helpers fail loudly; test signals are small"
)]

mod common;

use std::f64::consts::PI;
use std::fs;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use library::{Library, MeasureEvent, Measurer};

use common::{flac_with_comments, run_scan, temp_dir, track_ids};

/// `seconds` of a 997 Hz stereo sine at `dbfs`, 16-bit 44.1 kHz WAV.
fn wav(path: &Path, seconds: u32, dbfs: f64) {
    let rate = 44_100u32;
    let amplitude = 10f64.powf(dbfs / 20.0) * 32_767.0;
    let mut data = Vec::new();
    for n in 0..rate * seconds {
        let value = (amplitude * (2.0 * PI * 997.0 * f64::from(n) / f64::from(rate)).sin()) as i16;
        data.extend_from_slice(&value.to_le_bytes());
        data.extend_from_slice(&value.to_le_bytes());
    }
    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&2u16.to_le_bytes());
    file.extend_from_slice(&rate.to_le_bytes());
    file.extend_from_slice(&(rate * 4).to_le_bytes());
    file.extend_from_slice(&4u16.to_le_bytes());
    file.extend_from_slice(&16u16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&(data.len() as u32).to_le_bytes());
    file.extend_from_slice(&data);
    fs::write(path, file).unwrap();
}

/// `blocks` × 4096 bytes per channel of DSD64 stereo: a 997 Hz sine whose
/// modulation swings ±`depth` (1.0 is all ones to all zeros), from a
/// first-order sigma-delta modulator. DSF stores bits oldest first in bit 0.
fn dsf(path: &Path, blocks: usize, depth: f64) {
    const BLOCK: usize = 4096;
    let rate = 2_822_400u32;
    let samples = blocks * BLOCK * 8;
    let mut channel = vec![0u8; blocks * BLOCK];
    let mut integrator = 0.0;
    for n in 0..samples {
        let input = depth * (2.0 * PI * 997.0 * n as f64 / f64::from(rate)).sin();
        let bit = integrator >= 0.0;
        integrator += input - if bit { 1.0 } else { -1.0 };
        if bit {
            channel[n / 8] |= 1 << (n % 8);
        }
    }
    let mut data = Vec::with_capacity(channel.len() * 2);
    for block in channel.chunks(BLOCK) {
        data.extend_from_slice(block);
        data.extend_from_slice(block);
    }
    let total = 28 + 52 + 12 + data.len() as u64;
    let mut file = b"DSD ".to_vec();
    file.extend_from_slice(&28u64.to_le_bytes());
    file.extend_from_slice(&total.to_le_bytes());
    file.extend_from_slice(&0u64.to_le_bytes());
    file.extend_from_slice(b"fmt ");
    file.extend_from_slice(&52u64.to_le_bytes());
    for value in [1u32, 0, 2, 2, rate, 1] {
        file.extend_from_slice(&value.to_le_bytes());
    }
    file.extend_from_slice(&(samples as u64).to_le_bytes());
    file.extend_from_slice(&(BLOCK as u32).to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&(12 + data.len() as u64).to_le_bytes());
    file.extend_from_slice(&data);
    fs::write(path, file).unwrap();
}

/// Measures what `first` needs, and waits for it.
fn measure_all(db: &Path, first: &[i64], expected: usize) {
    let (events, received) = mpsc::channel();
    let measurer = Measurer::new(db.to_owned(), move |event| {
        let _ = events.send(event);
    });
    measurer.prioritise(first.to_vec());
    measurer.set_enabled(true);
    for _ in 0..expected {
        match received.recv_timeout(Duration::from_secs(60)).unwrap() {
            MeasureEvent::Measured { .. } => {}
            MeasureEvent::Failed(error) => panic!("{error}"),
        }
    }
    measurer.set_enabled(false);
}

fn close(actual: Option<f64>, expected: f64) {
    let actual = actual.unwrap();
    assert!((actual - expected).abs() < 0.15, "{actual} vs {expected}");
}

#[test]
fn measures_untagged_albums_and_keeps_tagged_ones() {
    let dir = temp_dir("measures_untagged_albums_and_keeps_tagged_ones");
    let root = dir.join("music");
    let tones = root.join("Tones");
    fs::create_dir_all(&tones).unwrap();
    wav(&tones.join("1 loud.wav"), 2, -14.0);
    wav(&tones.join("2 quiet.wav"), 2, -20.0);
    let dsd = root.join("DSD");
    fs::create_dir_all(&dsd).unwrap();
    // 0.1 of full modulation: −20 dB of the converter's scale, −14 dB
    // against the 50 % reference.
    dsf(&dsd.join("sine.dsf"), 172, 0.1);
    let tagged = root.join("Tagged");
    fs::create_dir_all(&tagged).unwrap();
    fs::write(
        tagged.join("t.flac"),
        flac_with_comments(&[
            b"ALBUM=Tagged",
            b"REPLAYGAIN_TRACK_GAIN=-2.00 dB",
            b"REPLAYGAIN_ALBUM_GAIN=-3.00 dB",
        ]),
    )
    .unwrap();

    let db = dir.join("library.db");
    let mut library = Library::open(&db).unwrap();
    library.add_folder(&root).unwrap();
    run_scan(&mut library, &dir.join("art"));
    let ids = track_ids(&library);
    let id = |name: &str| ids.iter().find(|(_, title)| title == name).unwrap().0;
    let all: Vec<i64> = ids.iter().map(|(id, _)| *id).collect();

    // Tagged album complete: nothing to measure there. Otherwise the track
    // asked for first, then its album.
    assert_eq!(library.next_unmeasured(&[id("t.flac")]).unwrap(), None);
    let next = |first: &[i64]| library.next_unmeasured(first).unwrap().unwrap().track_id;
    assert_eq!(next(&[id("2 quiet.wav")]), id("2 quiet.wav"));
    assert_eq!(next(&[id("t.flac"), id("sine.dsf")]), id("sine.dsf"));

    measure_all(&db, &all, 3);
    assert_eq!(library.next_unmeasured(&all).unwrap(), None);

    let loudness = library.loudness(&all).unwrap();
    close(loudness[&id("1 loud.wav")].track, -14.0);
    close(loudness[&id("2 quiet.wav")].track, -20.0);
    // Gated together: the energy mean of −14 and −20.
    close(loudness[&id("1 loud.wav")].album, -16.03);
    close(loudness[&id("2 quiet.wav")].album, -16.03);
    close(loudness[&id("sine.dsf")].track, -14.0);
    // −18 LUFS reference minus the gain.
    close(loudness[&id("t.flac")].track, -16.0);
    close(loudness[&id("t.flac")].album, -15.0);
}
