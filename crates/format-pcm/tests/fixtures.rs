//! Bit-exactness against committed fixtures (`scripts/make-pcm-fixtures.py`).
//! The WAV data chunk is the reference: decoding WAV or FLAC and packing in
//! the source layout must reproduce it byte for byte.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail loudly"
)]

use std::io::Cursor;
use std::path::PathBuf;

use format_pcm::{Codec, DecodeError, Decoder, SampleEncoding, StreamInfo};

/// Asserts equal bytes, reporting the first difference instead of dumping both.
fn assert_same_bytes(actual: &[u8], expected: &[u8], what: &str) {
    if let Some(at) = actual.iter().zip(expected).position(|(a, b)| a != b) {
        panic!("{what}: first difference at byte {at}");
    }
    assert_eq!(actual.len(), expected.len(), "{what}: length");
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The `data` chunk of a RIFF/WAVE file.
fn wav_data(bytes: &[u8]) -> &[u8] {
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if &bytes[at..at + 4] == b"data" {
            return &bytes[at + 8..at + 8 + size];
        }
        at += 8 + size + size % 2;
    }
    panic!("no data chunk");
}

/// Decodes everything, packs it, and returns (bytes, frames).
fn decode_all(
    decoder: &mut Decoder,
    encoding: SampleEncoding,
) -> Result<(Vec<u8>, usize), DecodeError> {
    let mut block = Vec::new();
    let mut bytes = Vec::new();
    let mut samples = 0;
    while decoder.decode_next(&mut block)? {
        samples += block.len();
        encoding.pack(&block, &mut bytes);
    }
    Ok((bytes, samples / decoder.info().channels))
}

struct Case {
    stem: &'static str,
    bits: u32,
    rate: u32,
    encoding: SampleEncoding,
}

const CASES: [Case; 2] = [
    Case {
        stem: "s16_44100",
        bits: 16,
        rate: 44_100,
        encoding: SampleEncoding::S16Le,
    },
    Case {
        stem: "s24_96000",
        bits: 24,
        rate: 96_000,
        encoding: SampleEncoding::S24In3Le,
    },
];

#[test]
fn wav_and_flac_reproduce_source_bytes() {
    for case in &CASES {
        let wav_bytes = std::fs::read(fixture(&format!("{}.wav", case.stem))).unwrap();
        let reference = wav_data(&wav_bytes);
        let frames = reference.len() / (2 * case.encoding.bytes());

        for (extension, codec) in [("wav", Codec::Wav), ("flac", Codec::Flac)] {
            let mut decoder =
                Decoder::open(&fixture(&format!("{}.{extension}", case.stem))).unwrap();
            assert_eq!(
                *decoder.info(),
                StreamInfo {
                    codec,
                    sample_rate: case.rate,
                    channels: 2,
                    bits_per_sample: case.bits,
                    total_frames: Some(frames as u64),
                },
                "{}.{extension}",
                case.stem
            );
            let (bytes, frame_count) = decode_all(&mut decoder, case.encoding).unwrap();
            assert_eq!(frame_count, frames, "{}.{extension} frame count", case.stem);
            assert_same_bytes(&bytes, reference, &format!("{}.{extension}", case.stem));
            if codec == Codec::Flac {
                assert_eq!(decoder.verify(), Some(true), "{}.flac MD5", case.stem);
            }
        }
    }
}

#[test]
fn wider_layouts_keep_every_bit() {
    let mut decoder = Decoder::open(&fixture("s16_44100.flac")).unwrap();
    let mut block = Vec::new();
    assert!(decoder.decode_next(&mut block).unwrap());
    let mut s16 = Vec::new();
    let mut s32 = Vec::new();
    SampleEncoding::S16Le.pack(&block, &mut s16);
    SampleEncoding::S32Le.pack(&block, &mut s32);
    for (narrow, wide) in s16.as_chunks::<2>().0.iter().zip(s32.as_chunks::<4>().0) {
        assert_eq!(*wide, [0, 0, narrow[0], narrow[1]]);
    }
}

#[test]
fn seek_is_frame_exact_across_blocks() {
    for case in &CASES {
        for extension in ["wav", "flac"] {
            let path = fixture(&format!("{}.{extension}", case.stem));
            let (all, _) = decode_all(&mut Decoder::open(&path).unwrap(), case.encoding).unwrap();
            let frame_bytes = 2 * case.encoding.bytes();
            // 4100 crosses the first 4096-frame FLAC block.
            for target in [0_usize, 1, 4100, all.len() / frame_bytes - 1] {
                let mut decoder = Decoder::open(&path).unwrap();
                decoder.seek(target as u64).unwrap();
                let (tail, _) = decode_all(&mut decoder, case.encoding).unwrap();
                assert_same_bytes(
                    &tail,
                    &all[target * frame_bytes..],
                    &format!("{}.{extension} seek to {target}", case.stem),
                );
            }
        }
    }
}

#[test]
fn truncated_flac_never_reports_success() {
    let bytes = std::fs::read(fixture("s24_96000.flac")).unwrap();
    let truncated = bytes[..bytes.len() / 2].to_vec();
    let Ok(mut decoder) = Decoder::from_reader(Cursor::new(truncated), Some("flac")) else {
        return;
    };
    let complete = decode_all(&mut decoder, SampleEncoding::S24In3Le).is_ok()
        && decoder.verify() == Some(true);
    assert!(!complete, "half a file must not decode as a verified track");
}

#[test]
fn float_wav_is_refused() {
    // 32-bit IEEE float, stereo, 48 kHz, one frame.
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&44_u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&3_u16.to_le_bytes()); // WAVE_FORMAT_IEEE_FLOAT
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&48_000_u32.to_le_bytes());
    wav.extend_from_slice(&(48_000_u32 * 8).to_le_bytes());
    wav.extend_from_slice(&8_u16.to_le_bytes());
    wav.extend_from_slice(&32_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&8_u32.to_le_bytes());
    wav.extend_from_slice(&[0; 8]);

    let opened = Decoder::from_reader(Cursor::new(wav), None);
    let refused = match opened {
        Err(DecodeError::Unsupported(_)) => true,
        Err(other) => panic!("expected Unsupported, got {other}"),
        Ok(mut decoder) => matches!(
            decoder.decode_next(&mut Vec::new()),
            Err(DecodeError::Unsupported(_))
        ),
    };
    assert!(refused);
}
