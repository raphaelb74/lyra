//! Engine behavior against the mock sink, with the format-pcm and
//! format-dsd fixtures. One test binary; shared helpers live here.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail loudly"
)]

mod conversion;
mod dsd;
mod output;
mod pcm;
mod queue;

use format_dsd::DsdLayout;
use format_pcm::SampleEncoding;
use player_core::mock::{MockSink, SinkCall};
use player_core::{
    DsdFallback, DsdPolicy, DsdTransport, Encoding, HardwareVolume, OutputFormat, PlaybackState,
    Player, PlayerStatus, QueueItem,
};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

pub(crate) const DEVICE: &str = "hw:CARD=TEST,DEV=0";

/// The default DSD policy for `transport`: bit-perfect or refused.
pub(crate) fn refuse(transport: DsdTransport) -> DsdPolicy {
    DsdPolicy {
        transport,
        fallback: DsdFallback::Refuse,
    }
}

/// Auto transport, converting to PCM when neither DSD transport works.
pub(crate) const CONVERT: DsdPolicy = DsdPolicy {
    transport: DsdTransport::Auto,
    fallback: DsdFallback::Convert { gain_db: 0 },
};

/// Asserts equal bytes, reporting the first difference instead of dumping both.
pub(crate) fn assert_same_bytes(actual: &[u8], expected: &[u8], what: &str) {
    if let Some(at) = actual.iter().zip(expected).position(|(a, b)| a != b) {
        panic!("{what}: first difference at byte {at}");
    }
    assert_eq!(actual.len(), expected.len(), "{what}: length");
}

pub(crate) fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../format-pcm/tests/fixtures")
        .join(name)
}

/// The data chunk of a fixture WAV: what the device must receive.
pub(crate) fn reference(name: &str) -> Vec<u8> {
    let bytes = std::fs::read(fixture(name)).unwrap();
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if &bytes[at..at + 4] == b"data" {
            return bytes[at + 8..at + 8 + size].to_vec();
        }
        at += 8 + size + size % 2;
    }
    panic!("no data chunk in {name}");
}

pub(crate) fn start(sink: MockSink) -> (Player, Receiver<PlayerStatus>) {
    let (tx, rx) = mpsc::channel();
    let player = Player::new(sink, move |status| {
        let _ = tx.send(status.clone());
    })
    .unwrap();
    (player, rx)
}

/// Waits for a published status in `state`; returns the states seen on the way.
pub(crate) fn wait_for(
    updates: &Receiver<PlayerStatus>,
    state: PlaybackState,
) -> (PlayerStatus, Vec<PlaybackState>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let status = updates
            .recv_timeout(left)
            .unwrap_or_else(|_| panic!("no {state:?}; saw {seen:?}"));
        seen.push(status.state);
        if status.state == state {
            return (status, seen);
        }
    }
}

pub(crate) fn dsd_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../format-dsd/tests/fixtures")
        .join(name)
}

/// The DFF sound chunk: canonical DSD frames (MSB first, interleaved).
pub(crate) fn dsd_reference() -> Vec<u8> {
    let bytes = std::fs::read(dsd_fixture("dsd64_stereo.dff")).unwrap();
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

/// Plays a DSD fixture to completion; returns the output and the bytes written.
pub(crate) fn play_dsd(sink: MockSink, name: &str, dsd: DsdPolicy) -> (OutputFormat, Vec<u8>) {
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    player.play(dsd_fixture(name), DEVICE.into(), dsd).unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Stopped);
    drop(player);
    let output = status.track.unwrap().output.unwrap();
    (output, recorder.with(|recording| recording.bytes.clone()))
}

/// The default mock plus one native DSD layout at its DSD64 rate.
pub(crate) fn with_native(layout: DsdLayout) -> MockSink {
    let mut sink = MockSink::new();
    sink.encodings.push(Encoding::Dsd(layout));
    sink.rates.push(layout.frame_rate(2_822_400));
    sink
}

pub(crate) fn queue(paths: &[PathBuf]) -> Vec<QueueItem> {
    paths
        .iter()
        .zip(1..)
        .map(|(path, id)| QueueItem {
            path: path.clone(),
            id: Some(id),
        })
        .collect()
}

pub(crate) fn opens(calls: &[SinkCall]) -> usize {
    calls
        .iter()
        .filter(|call| matches!(call, SinkCall::Open(_)))
        .count()
}

/// Built-in audio as Lyra sees it: PCM and the DoP format accepted, volume
/// unreadable, no native DSD.
pub(crate) fn built_in() -> MockSink {
    let mut sink = MockSink::new();
    sink.hardware_volume = HardwareVolume::Unknown;
    sink
}

/// The fixture converted in one go, as the device must receive it.
pub(crate) fn converted_reference(tracks: usize) -> Vec<u8> {
    let mut converter = dsd_pcm::Converter::new(2_822_400, 88_200, 2, 0.0).unwrap();
    let mut samples = Vec::new();
    for _ in 0..tracks {
        converter.push(&dsd_reference(), &mut samples);
    }
    converter.finish(&mut samples);
    let mut bytes = Vec::new();
    SampleEncoding::S24In3Le.pack(&samples, &mut bytes);
    bytes
}
