//! Plays one file on one ALSA device and prints what the kernel negotiated.
//!
//! `cargo run -p audio-alsa --example play -- hw:CARD=ONE,DEV=0 a.flac [b.flac …] [auto|native|dop|convert]`
//!
//! Several files play as a queue, gaplessly where the format allows; each
//! track change is printed. DSD files take an optional transport: `auto`
//! (default: native DSD, else DoP), `native`, `dop`, or `convert` (auto, then
//! conversion to PCM, not bit-perfect). `seconds=N` plays N seconds of each
//! track, then skips to the next as the app does, and stops after the last.
//!
//! While playing, prints `/proc/asound/<card>/pcm<dev>p/sub0/hw_params`: the
//! format and rate the device is running at, independent of Lyra's own report.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use audio_alsa::{AlsaSink, list_playback_devices};
use unda::{DsdFallback, DsdPolicy, DsdTransport, PlaybackState, Player, QueueItem};

const USAGE: &str =
    "usage: play <hw:CARD=…,DEV=…> <file.flac|wav|dsf|dff>… [auto|native|dop|convert] [seconds=N]";

struct Args {
    device: String,
    items: Vec<QueueItem>,
    dsd: DsdPolicy,
    per_track: Option<Duration>,
}

fn parse_args() -> Option<Args> {
    let mut args = std::env::args().skip(1);
    let device = args.next()?;
    let mut dsd = DsdPolicy::default();
    let mut items = Vec::new();
    let mut per_track = None;
    for arg in args {
        if let Some(seconds) = arg.strip_prefix("seconds=") {
            per_track = Some(Duration::from_secs(seconds.parse().ok()?));
            continue;
        }
        match arg.as_str() {
            "auto" => dsd.transport = DsdTransport::Auto,
            "native" => dsd.transport = DsdTransport::Native,
            "dop" => dsd.transport = DsdTransport::Dop,
            "convert" => dsd.fallback = DsdFallback::Convert { gain_db: 0 },
            path => items.push(QueueItem {
                path: PathBuf::from(path),
                id: None,
            }),
        }
    }
    (!items.is_empty()).then_some(Args {
        device,
        items,
        dsd,
        per_track,
    })
}

fn main() -> ExitCode {
    let Some(Args {
        device,
        items,
        dsd,
        per_track,
    }) = parse_args()
    else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };

    let count = items.len();
    let sink = AlsaSink::new();
    let sink_stats = sink.stats();
    let (tx, updates) = mpsc::channel();
    let player = match Player::new(sink, move |status| {
        let _ = tx.send(status.clone());
    }) {
        Ok(player) => player,
        Err(error) => {
            eprintln!("cannot start the player: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = player.play_queue(items, 0, device.clone(), dsd) {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }

    let mut printed_params = false;
    let mut shown = None;
    let mut track_started = Instant::now();
    // Counted per open device; sampled while it is open.
    let mut last_underruns = 0;
    loop {
        if let Some(report) = sink_stats.report() {
            last_underruns = report.underruns;
        }
        if let Some(limit) = per_track
            && track_started.elapsed() >= limit
        {
            track_started = Instant::now();
            let last = shown.is_some_and(|index| index + 1 >= count);
            let _ = if last { player.stop() } else { player.next() };
        }
        let Ok(status) = updates.recv_timeout(Duration::from_millis(100)) else {
            continue;
        };
        println!("{:?}", status.state);
        if let Some(track) = &status.track
            && shown != Some(track.index)
        {
            shown = Some(track.index);
            track_started = Instant::now();
            printed_params = false;
            println!("track {}: {}", track.index + 1, track.path.display());
        }
        match status.state {
            PlaybackState::Playing if !printed_params => {
                if let Some(track) = &status.track {
                    println!("source: {:?}", track.source);
                    println!("output: {:?}", track.output);
                }
                match kernel_hw_params(&device) {
                    Some(params) => println!("kernel hw_params:\n{params}"),
                    None => println!("kernel hw_params: unavailable"),
                }
                printed_params = true;
            }
            PlaybackState::Stopped => break,
            PlaybackState::Error => {
                if let Some(error) = status.error {
                    eprintln!("error ({:?}): {}", error.kind, error.message);
                }
                return ExitCode::FAILURE;
            }
            _ => {}
        }
    }
    println!("underruns (last open): {last_underruns}");
    ExitCode::SUCCESS
}

/// The kernel's view of the open substream.
fn kernel_hw_params(device: &str) -> Option<String> {
    let device = list_playback_devices()
        .ok()?
        .into_iter()
        .find(|candidate| candidate.name == device)?;
    let path = format!(
        "/proc/asound/card{}/pcm{}p/sub0/hw_params",
        device.card_index, device.device
    );
    std::fs::read_to_string(path).ok()
}
