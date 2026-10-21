//! Converts DSD files to PCM (nothing is played or written) and reports each
//! file's peak level and the conversion speed. The peaks decide how much
//! headroom the converter needs by default.
//!
//! ```sh
//! cargo run --release -p dsd-pcm --example survey -- ~/Music [--rate 44100]
//! ```

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{env, fs, process};

use dsd_pcm::Converter;
use format_dsd::DsdReader;

fn main() {
    let mut rate = 88_200;
    let mut paths = Vec::new();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--rate" {
            rate = args.next().and_then(|r| r.parse().ok()).unwrap_or(0);
        } else {
            paths.push(PathBuf::from(arg));
        }
    }
    if paths.is_empty() {
        eprintln!("usage: survey <file or folder>... [--rate 88200|44100]");
        process::exit(2);
    }
    let mut files = Vec::new();
    for path in &paths {
        collect(path, &mut files);
    }
    files.sort();

    let mut peaks = Vec::new();
    let (mut audio, mut busy) = (Duration::ZERO, Duration::ZERO);
    for file in &files {
        match convert(file, rate) {
            Ok(result) => {
                println!(
                    "{:+6.2} dBFS  {:>8} clipped  DSD{:<4} {}",
                    20.0 * result.peak.log10(),
                    result.clipped,
                    result.multiple,
                    file.display()
                );
                peaks.push(result.peak);
                audio += result.audio;
                busy += result.busy;
            }
            Err(message) => eprintln!("skipped {}: {message}", file.display()),
        }
    }
    if peaks.is_empty() {
        return;
    }
    peaks.sort_by(f64::total_cmp);
    let db = |peak: f64| 20.0 * peak.log10();
    let at = |percent: usize| peaks[(peaks.len() - 1) * percent / 100];
    println!(
        "\n{} files. Peak: median {:+.2}, 90% {:+.2}, 99% {:+.2}, max {:+.2} dBFS",
        peaks.len(),
        db(at(50)),
        db(at(90)),
        db(at(99)),
        db(at(100))
    );
    for headroom in [0.0, -3.0, -6.0] {
        let clipping = peaks.iter().filter(|&&p| db(p) + headroom > 0.0).count();
        println!("At {headroom:+} dB: {clipping} files would clip");
    }
    println!(
        "Speed: {:.1}× real time on one thread ({:.0} s of audio in {:.1} s)",
        audio.as_secs_f64() / busy.as_secs_f64(),
        audio.as_secs_f64(),
        busy.as_secs_f64()
    );
}

struct Result {
    peak: f64,
    clipped: u64,
    multiple: u32,
    audio: Duration,
    busy: Duration,
}

fn convert(path: &Path, rate: u32) -> std::result::Result<Result, String> {
    let mut reader = DsdReader::open(path).map_err(|e| e.to_string())?;
    let info = *reader.info();
    let mut converter = Converter::new(info.dsd_rate, rate, info.channels, 0.0)
        .ok_or_else(|| format!("DSD rate {} is not supported", info.dsd_rate))?;
    let (mut bytes, mut out) = (Vec::new(), Vec::new());
    let mut busy = Duration::ZERO;
    loop {
        let more = reader.read_next(&mut bytes).map_err(|e| e.to_string())?;
        let start = Instant::now();
        out.clear();
        converter.push(&bytes, &mut out);
        if !more {
            converter.finish(&mut out);
        }
        busy += start.elapsed();
        if !more {
            break;
        }
    }
    Ok(Result {
        peak: converter.peak(),
        clipped: converter.clipped(),
        multiple: info.multiple(),
        audio: Duration::from_millis(info.total_frames * 1000 / u64::from(info.frame_rate())),
        busy,
    })
}

fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            collect(&entry.path(), files);
        }
    } else if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("dsf") || e.eq_ignore_ascii_case("dff"))
    {
        files.push(path.to_owned());
    }
}
