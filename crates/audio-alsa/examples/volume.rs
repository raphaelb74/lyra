//! Shows, or sets, the main hardware volume of a card.
//!
//! `cargo run -p audio-alsa --example volume -- hw:CARD=PCH,DEV=0 [percent]`

use std::{env, process};

fn main() {
    let mut args = env::args().skip(1);
    let Some(device) = args.next() else {
        eprintln!("usage: volume <hw:CARD=…,DEV=…> [0-100]");
        process::exit(2);
    };
    let result = match args.next().map(|value| value.parse::<u8>()) {
        Some(Ok(percent)) => audio_alsa::set_volume(&device, percent),
        Some(Err(_)) => {
            eprintln!("the volume is a percentage, 0 to 100");
            process::exit(2);
        }
        None => audio_alsa::volume(&device),
    };
    match result {
        Ok(Some(level)) => println!("{level:?}"),
        Ok(None) => println!("{device} has no playback volume control"),
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    }
}
