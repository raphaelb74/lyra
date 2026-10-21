//! Lists ALSA playback devices and what each accepts.
//!
//! `cargo run -p audio-alsa --example devices`

use std::process::ExitCode;

use audio_alsa::{AlsaError, list_playback_devices, probe};

fn main() -> ExitCode {
    let devices = match list_playback_devices() {
        Ok(devices) => devices,
        Err(error) => {
            eprintln!("cannot list ALSA devices: {error}");
            return ExitCode::FAILURE;
        }
    };
    for device in &devices {
        println!(
            "{}  {} — {}",
            device.name, device.pcm_name, device.card_name
        );
        match probe(device) {
            Ok(caps) => {
                println!("  channels {}–{}", caps.channels_min, caps.channels_max);
                for support in &caps.pcm {
                    println!("  {:?}: {}", support.encoding, khz(&support.rates));
                }
                for (layout, rates) in &caps.dsd {
                    println!("  native DSD {}: {}", layout.name(), khz(rates));
                }
            }
            Err(error @ AlsaError::Busy { .. }) => println!("  {error}"),
            Err(error) => println!("  probe failed: {error}"),
        }
    }
    if devices.is_empty() {
        println!("no ALSA playback devices");
    }
    ExitCode::SUCCESS
}

fn khz(rates: &[u32]) -> String {
    let list: Vec<String> = rates
        .iter()
        .map(|rate| format!("{}", f64::from(*rate) / 1000.0))
        .collect();
    format!("{} kHz", list.join(", "))
}
