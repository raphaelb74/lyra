//! Readers against committed fixtures (`scripts/make-dsd-fixtures.py`).
//! The DFF data chunk is the reference: both containers must reproduce it.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail loudly"
)]

use std::io::Cursor;
use std::path::PathBuf;

use format_dsd::{Container, DsdError, DsdInfo, DsdReader};

const FRAMES: u64 = 2 * 4096 + 1234;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(path).unwrap()
}

fn reader(bytes: Vec<u8>) -> Result<DsdReader, DsdError> {
    DsdReader::from_reader(Box::new(Cursor::new(bytes)))
}

/// The DFF `DSD ` sound chunk: canonical frames.
fn reference() -> Vec<u8> {
    let bytes = fixture("dsd64_stereo.dff");
    let mut at = 16;
    loop {
        let size = usize::try_from(u64::from_be_bytes(
            bytes[at + 4..at + 12].try_into().unwrap(),
        ))
        .unwrap();
        if &bytes[at..at + 4] == b"DSD " {
            return bytes[at + 12..at + 12 + size].to_vec();
        }
        at += 12 + size + size % 2;
    }
}

fn read_all(reader: &mut DsdReader) -> Result<Vec<u8>, DsdError> {
    let mut all = Vec::new();
    let mut block = Vec::new();
    while reader.read_next(&mut block)? {
        assert_eq!(block.len() % reader.info().channels, 0, "partial frame");
        all.extend_from_slice(&block);
    }
    Ok(all)
}

fn assert_same_bytes(actual: &[u8], expected: &[u8], what: &str) {
    if let Some(at) = actual.iter().zip(expected).position(|(a, b)| a != b) {
        panic!("{what}: first difference at byte {at}");
    }
    assert_eq!(actual.len(), expected.len(), "{what}: length");
}

#[test]
fn dsf_and_dff_reproduce_the_same_dsd_bits() {
    let reference = reference();
    for (name, container) in [
        ("dsd64_stereo.dsf", Container::Dsf),
        ("dsd64_stereo.dff", Container::Dff),
    ] {
        let mut reader = reader(fixture(name)).unwrap();
        assert_eq!(
            *reader.info(),
            DsdInfo {
                container,
                dsd_rate: 2_822_400,
                channels: 2,
                total_frames: FRAMES
            }
        );
        assert_eq!(reader.info().multiple(), 64);
        assert_same_bytes(&read_all(&mut reader).unwrap(), &reference, name);
    }
}

#[test]
fn seek_is_frame_exact_across_dsf_blocks() {
    let reference = reference();
    for name in ["dsd64_stereo.dsf", "dsd64_stereo.dff"] {
        for target in [0, 1, 4095, 4096, 4097, 9000, FRAMES] {
            let mut reader = reader(fixture(name)).unwrap();
            reader.seek(target).unwrap();
            let start = usize::try_from(target).unwrap() * 2;
            assert_same_bytes(
                &read_all(&mut reader).unwrap(),
                &reference[start..],
                &format!("{name} from {target}"),
            );
        }
        let mut reader = reader(fixture(name)).unwrap();
        assert!(matches!(
            reader.seek(FRAMES + 1),
            Err(DsdError::SeekOutOfRange)
        ));
    }
}

#[test]
fn truncated_files_fail_instead_of_ending_early() {
    for name in ["dsd64_stereo.dsf", "dsd64_stereo.dff"] {
        let mut bytes = fixture(name);
        bytes.truncate(bytes.len() * 2 / 3);
        let mut reader = reader(bytes).unwrap();
        assert!(
            matches!(read_all(&mut reader), Err(DsdError::Malformed(_))),
            "{name}: a cut file must not look complete"
        );
    }
    // Cut inside the header.
    for name in ["dsd64_stereo.dsf", "dsd64_stereo.dff"] {
        assert!(matches!(
            reader(fixture(name)[..40].to_vec()),
            Err(DsdError::Malformed(_))
        ));
    }
}

#[test]
fn rejects_non_dsd_data() {
    // macOS AppleDouble file, as found next to real DSF files.
    let apple_double = b"\x00\x05\x16\x07\x00\x02\x00\x00Mac OS X        ".to_vec();
    assert!(matches!(reader(apple_double), Err(DsdError::NotDsd)));
    assert!(matches!(reader(Vec::new()), Err(DsdError::NotDsd)));
}

/// Writes a little-endian u32 into the DSF fmt chunk.
fn dsf_with(offset: usize, value: u32) -> Vec<u8> {
    let mut bytes = fixture("dsd64_stereo.dsf");
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    bytes
}

#[test]
fn validates_dsf_headers() {
    // fmt fields: version @40, format id @44, channels @52, rate @56, bits @60, block @72.
    assert!(matches!(
        reader(dsf_with(40, 2)),
        Err(DsdError::Unsupported(_))
    ));
    assert!(matches!(
        reader(dsf_with(44, 1)),
        Err(DsdError::Unsupported(_))
    ));
    assert!(matches!(
        reader(dsf_with(52, 0)),
        Err(DsdError::Malformed(_))
    ));
    assert!(matches!(
        reader(dsf_with(52, 7)),
        Err(DsdError::Malformed(_))
    ));
    assert!(matches!(
        reader(dsf_with(56, 44_100)),
        Err(DsdError::Unsupported(_))
    ));
    assert!(matches!(
        reader(dsf_with(60, 2)),
        Err(DsdError::Malformed(_))
    ));
    assert!(matches!(
        reader(dsf_with(72, 3000)),
        Err(DsdError::Malformed(_))
    ));
    // A huge block size never turns into a huge allocation.
    assert!(matches!(
        reader(dsf_with(72, u32::MAX)),
        Err(DsdError::Malformed(_))
    ));
}

#[test]
fn refuses_dst_compressed_dff() {
    let mut bytes = fixture("dsd64_stereo.dff");
    // The compression type follows the CMPR chunk header.
    let cmpr = bytes
        .windows(4)
        .position(|window| window == b"CMPR")
        .unwrap();
    bytes[cmpr + 12..cmpr + 16].copy_from_slice(b"DST ");
    assert!(matches!(reader(bytes), Err(DsdError::Unsupported(_))));
}
