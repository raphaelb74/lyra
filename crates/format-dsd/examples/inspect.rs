//! Reads DSD files completely and prints their properties.
//!
//! `cargo run --release -p format-dsd --example inspect -- a.dsf b.dff`

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use format_dsd::{DsdError, DsdReader};

fn main() -> ExitCode {
    let mut ok = true;
    for path in std::env::args().skip(1) {
        match inspect(Path::new(&path)) {
            Ok(report) => println!("{path}: {report}"),
            Err(error) => {
                ok = false;
                println!("{path}: {error}");
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn inspect(path: &Path) -> Result<String, DsdError> {
    let started = Instant::now();
    let mut reader = DsdReader::open(path)?;
    let info = *reader.info();
    let mut block = Vec::new();
    let mut bytes = 0_usize;
    // FNV-1a over the frames, to compare runs and containers.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    while reader.read_next(&mut block)? {
        bytes += block.len();
        for &byte in &block {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    let frames = bytes / info.channels;
    Ok(format!(
        "{:?} DSD{} {} ch, {frames} frames (header {}), hash {hash:016x}, read in {:?}",
        info.container,
        info.multiple(),
        info.channels,
        info.total_frames,
        started.elapsed()
    ))
}
