//! Decodes files completely and checks them against their stored checksum
//! (FLAC MD5 of the original samples).
//!
//! `cargo run -p format-pcm --example verify -- a.flac b.flac`

use std::path::Path;
use std::process::ExitCode;

use format_pcm::Decoder;

fn main() -> ExitCode {
    let mut ok = true;
    for path in std::env::args().skip(1) {
        match verify(Path::new(&path)) {
            Ok(report) => println!("{path}: {report}"),
            Err(error) => {
                ok = false;
                println!("{path}: FAILED, {error}");
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn verify(path: &Path) -> Result<String, format_pcm::DecodeError> {
    let mut decoder = Decoder::open(path)?;
    let info = *decoder.info();
    let mut block = Vec::new();
    let mut samples = 0_usize;
    while decoder.decode_next(&mut block)? {
        samples += block.len();
    }
    let frames = samples / info.channels;
    let checksum = match decoder.verify() {
        Some(true) => "MD5 matches",
        Some(false) => "MD5 MISMATCH",
        None => "no checksum stored",
    };
    Ok(format!(
        "{:?} {}-bit {} Hz {} ch, {frames} frames, {checksum}",
        info.codec, info.bits_per_sample, info.sample_rate, info.channels
    ))
}
