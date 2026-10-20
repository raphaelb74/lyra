//! Plays one file on one ALSA device and prints what the kernel negotiated.
//!
//! `cargo run -p audio-alsa --example play -- hw:CARD=ONE,DEV=0 track.flac`
//!
//! While playing, prints `/proc/asound/<card>/pcm<dev>p/sub0/hw_params`: the
//! format and rate the device is running at, independent of Lyra's own report.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use audio_alsa::{AlsaSink, list_playback_devices};
use player_core::{PlaybackState, Player};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(device), Some(path)) = (args.next(), args.next()) else {
        eprintln!("usage: play <hw:CARD=…,DEV=…> <file.flac|file.wav>");
        return ExitCode::FAILURE;
    };

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
    if let Err(error) = player.play(PathBuf::from(path), device.clone()) {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }

    let mut printed_params = false;
    loop {
        let Ok(status) = updates.recv_timeout(Duration::from_millis(500)) else {
            continue;
        };
        println!("{:?}", status.state);
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
