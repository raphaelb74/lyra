//! Measures files' loudness and how long it takes.
//!
//! `cargo run --release -p library --example loudness -- FILE…`

use std::path::Path;
use std::time::Instant;

fn main() {
    for path in std::env::args().skip(1) {
        let start = Instant::now();
        match library::measure_loudness(Path::new(&path)) {
            Ok(lufs) => println!("{lufs:?} LUFS in {:.2?}  {path}", start.elapsed()),
            Err(error) => println!("{error}  {path}"),
        }
    }
}
