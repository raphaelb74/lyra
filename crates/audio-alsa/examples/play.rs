//! Plays one file on one ALSA device and prints what the kernel negotiated.
//!
//! `cargo run -p audio-alsa --example play -- hw:CARD=ONE,DEV=0 a.flac [b.flac …] [auto|native|dop]`
//!
//! Several files play as a queue, gaplessly where the format allows; each
//! track change is printed. DSD files take an optional transport: `auto`
//! (default: native DSD, else DoP), `native` or `dop`.
//!
//! While playing, prints `/proc/asound/<card>/pcm<dev>p/sub0/hw_params`: the
//! format and rate the device is running at, independent of Lyra's own report.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use audio_alsa::{AlsaSink, list_playback_devices};
use player_core::{DsdTransport, PlaybackState, Player, QueueItem};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let usage = "usage: play <hw:CARD=…,DEV=…> <file.flac|wav|dsf|dff>… [auto|native|dop]";
    let Some(device) = args.next() else {
        eprintln!("{usage}");
        return ExitCode::FAILURE;
    };
    let mut dsd = DsdTransport::Auto;
    let mut items = Vec::new();
    for arg in args {
        match arg.as_str() {
            "auto" => dsd = DsdTransport::Auto,
            "native" => dsd = DsdTransport::Native,
            "dop" => dsd = DsdTransport::Dop,
            path => items.push(QueueItem {
                path: PathBuf::from(path),
                id: None,
            }),
        }
    }
    if items.is_empty() {
        eprintln!("{usage}");
        return ExitCode::FAILURE;
    }

    let sink = AlsaSink::new();
    let underruns = sink.underruns();
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
    loop {
        let Ok(status) = updates.recv_timeout(Duration::from_millis(500)) else {
            continue;
        };
        println!("{:?}", status.state);
        if let Some(track) = &status.track
            && shown != Some(track.index)
        {
            shown = Some(track.index);
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
    println!(
        "underruns: {}",
        underruns.load(std::sync::atomic::Ordering::Relaxed)
    );
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
