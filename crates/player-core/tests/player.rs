//! Engine behavior against the mock sink, with the format-pcm fixtures.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail loudly"
)]

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use format_pcm::SampleEncoding;
use player_core::mock::{MockSink, SinkCall};
use player_core::{
    ErrorKind, HardwareVolume, OutputFormat, PlaybackState, Player, PlayerStatus, SinkError,
    SinkErrorKind,
};

const DEVICE: &str = "hw:CARD=TEST,DEV=0";

/// Asserts equal bytes, reporting the first difference instead of dumping both.
fn assert_same_bytes(actual: &[u8], expected: &[u8], what: &str) {
    if let Some(at) = actual.iter().zip(expected).position(|(a, b)| a != b) {
        panic!("{what}: first difference at byte {at}");
    }
    assert_eq!(actual.len(), expected.len(), "{what}: length");
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../format-pcm/tests/fixtures")
        .join(name)
}

/// The data chunk of a fixture WAV: what the device must receive.
fn reference(name: &str) -> Vec<u8> {
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

fn start(sink: MockSink) -> (Player, Receiver<PlayerStatus>) {
    let (tx, rx) = mpsc::channel();
    let player = Player::new(sink, move |status| {
        let _ = tx.send(status.clone());
    })
    .unwrap();
    (player, rx)
}

/// Waits for a published status in `state`; returns the states seen on the way.
fn wait_for(
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

#[test]
fn plays_every_source_byte_through_the_states() {
    for (name, encoding, rate) in [
        ("s16_44100.wav", SampleEncoding::S16Le, 44_100),
        ("s24_96000.wav", SampleEncoding::S24In3Le, 96_000),
    ] {
        let sink = MockSink::new();
        let recorder = sink.recorder.clone();
        let (player, updates) = start(sink);
        player.play(fixture(name), DEVICE.into()).unwrap();

        let (status, seen) = wait_for(&updates, PlaybackState::Stopped);
        assert_eq!(
            seen,
            [
                PlaybackState::Loading,
                PlaybackState::Negotiating,
                PlaybackState::Buffering,
                PlaybackState::Playing,
                PlaybackState::Stopped
            ],
            "{name}"
        );
        let output = OutputFormat {
            encoding,
            sample_rate: rate,
            channels: 2,
            hardware_volume: HardwareVolume::None,
        };
        assert_eq!(status.track.unwrap().output, Some(output));
        drop(player); // joins the threads, so every sink call has happened
        recorder.with(|recording| {
            assert_same_bytes(&recording.bytes, &reference(name), name);
            assert_eq!(recording.calls.first(), Some(&SinkCall::Open(output)));
            assert_eq!(
                recording.calls[recording.calls.len() - 2..],
                [SinkCall::Drain, SinkCall::Close]
            );
        });
    }
}

#[test]
fn zero_pads_into_a_wider_layout_when_needed() {
    let mut sink = MockSink::new();
    sink.encodings = vec![SampleEncoding::S32Le];
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    player
        .play(fixture("s16_44100.wav"), DEVICE.into())
        .unwrap();
    wait_for(&updates, PlaybackState::Stopped);

    let source = reference("s16_44100.wav");
    recorder.with(|recording| {
        assert_eq!(recording.bytes.len(), source.len() * 2);
        for (wide, narrow) in recording.bytes.chunks(4).zip(source.chunks(2)) {
            assert_eq!(wide, [0, 0, narrow[0], narrow[1]]);
        }
    });
}

#[test]
fn refuses_a_rate_the_device_lacks() {
    let mut sink = MockSink::new();
    sink.rates = vec![44_100, 48_000];
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    player
        .play(fixture("s24_96000.wav"), DEVICE.into())
        .unwrap();

    let (status, _) = wait_for(&updates, PlaybackState::Error);
    assert_eq!(status.error.unwrap().kind, ErrorKind::Unsupported);
    recorder.with(|recording| {
        assert!(
            !recording
                .calls
                .iter()
                .any(|call| matches!(call, SinkCall::Write { .. }))
        );
    });
}

#[test]
fn reports_a_busy_device() {
    let mut sink = MockSink::new();
    sink.open_error = Some(SinkError::new(
        SinkErrorKind::Busy,
        "hw:CARD=TEST,DEV=0 is in use by pipewire (pid 42)",
    ));
    let (player, updates) = start(sink);
    player
        .play(fixture("s16_44100.wav"), DEVICE.into())
        .unwrap();

    let error = wait_for(&updates, PlaybackState::Error).0.error.unwrap();
    assert_eq!(error.kind, ErrorKind::DeviceBusy);
    assert!(error.message.contains("pipewire"));
}

#[test]
fn reports_a_missing_file() {
    let (player, updates) = start(MockSink::new());
    player.play(fixture("missing.flac"), DEVICE.into()).unwrap();
    assert_eq!(
        wait_for(&updates, PlaybackState::Error)
            .0
            .error
            .unwrap()
            .kind,
        ErrorKind::InvalidFile
    );
}

#[test]
fn pause_holds_output_and_resume_loses_nothing() {
    let mut sink = MockSink::new();
    sink.write_delay = Duration::from_millis(5);
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    player
        .play(fixture("s24_96000.wav"), DEVICE.into())
        .unwrap();
    wait_for(&updates, PlaybackState::Playing);

    player.pause().unwrap();
    wait_for(&updates, PlaybackState::Paused);
    std::thread::sleep(Duration::from_millis(50)); // let an in-flight write finish
    let writes = || recorder.with(|recording| recording.bytes.len());
    let before = writes();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(writes(), before, "audio written while paused");

    player.resume().unwrap();
    wait_for(&updates, PlaybackState::Stopped);
    recorder.with(|recording| {
        assert_same_bytes(
            &recording.bytes,
            &reference("s24_96000.wav"),
            "after resume",
        );
        assert!(recording.calls.contains(&SinkCall::Paused(true)));
        assert!(recording.calls.contains(&SinkCall::Paused(false)));
    });
}

#[test]
fn seek_discards_queued_audio_and_resumes_at_the_frame() {
    const TARGET: usize = 2_000;
    let mut sink = MockSink::new();
    sink.write_delay = Duration::from_millis(20);
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    player
        .play(fixture("s16_44100.wav"), DEVICE.into())
        .unwrap();
    wait_for(&updates, PlaybackState::Playing);

    player.seek(TARGET as u64).unwrap();
    wait_for(&updates, PlaybackState::Stopped);
    let source = reference("s16_44100.wav");
    recorder.with(|recording| {
        assert!(recording.calls.contains(&SinkCall::Discard));
        // Bytes since the discard are exactly the source from the target frame.
        assert_same_bytes(&recording.bytes, &source[TARGET * 4..], "after seek");
    });
}

#[test]
fn stop_releases_the_device() {
    let mut sink = MockSink::new();
    sink.write_delay = Duration::from_millis(20);
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    player
        .play(fixture("s24_96000.wav"), DEVICE.into())
        .unwrap();
    wait_for(&updates, PlaybackState::Playing);

    player.stop().unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Stopped);
    assert_eq!(
        status,
        PlayerStatus {
            state: PlaybackState::Stopped,
            track: None,
            error: None
        }
    );
    assert_eq!(player.position(), 0);
    drop(player); // joins the threads, so every sink call has happened
    recorder.with(|recording| {
        assert!(recording.calls.contains(&SinkCall::Close));
        assert!(
            !recording.calls.contains(&SinkCall::Drain),
            "stop must not wait for the track to finish"
        );
    });
}

fn dsd_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../format-dsd/tests/fixtures")
        .join(name)
}

/// The DFF sound chunk: canonical DSD frames (MSB first, interleaved).
fn dsd_reference() -> Vec<u8> {
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

#[test]
fn dop_carries_every_dsd_bit_with_alternating_markers() {
    for name in ["dsd64_stereo.dsf", "dsd64_stereo.dff"] {
        let mut sink = MockSink::new();
        sink.rates.push(176_400);
        sink.hardware_volume = HardwareVolume::Unity;
        let recorder = sink.recorder.clone();
        let (player, updates) = start(sink);
        player.play(dsd_fixture(name), DEVICE.into()).unwrap();

        let (status, _) = wait_for(&updates, PlaybackState::Stopped);
        let output = status.track.unwrap().output.unwrap();
        assert_eq!(
            (output.encoding, output.sample_rate),
            (SampleEncoding::S24In3Le, 176_400)
        );
        drop(player);

        recorder.with(|recording| {
            // S24_3LE stereo: [newer, older, marker] per channel.
            let mut dsd = Vec::new();
            for (index, frame) in recording.bytes.as_chunks::<6>().0.iter().enumerate() {
                let marker = if index % 2 == 0 { 0x05 } else { 0xFA };
                assert_eq!(
                    (frame[2], frame[5]),
                    (marker, marker),
                    "{name}: marker at DoP frame {index}"
                );
                dsd.extend_from_slice(&[frame[1], frame[4], frame[0], frame[3]]);
            }
            assert_same_bytes(&dsd, &dsd_reference(), name);
        });
    }
}

#[test]
fn dop_is_refused_unless_the_dac_volume_is_known_to_be_untouched() {
    for volume in [
        HardwareVolume::Unknown,
        HardwareVolume::Attenuated { centibels: -600 },
    ] {
        let mut sink = MockSink::new();
        sink.rates.push(176_400);
        sink.hardware_volume = volume;
        let recorder = sink.recorder.clone();
        let (player, updates) = start(sink);
        player
            .play(dsd_fixture("dsd64_stereo.dsf"), DEVICE.into())
            .unwrap();

        let error = wait_for(&updates, PlaybackState::Error).0.error.unwrap();
        assert_eq!(error.kind, ErrorKind::Unsupported, "{volume:?}");
        drop(player);
        recorder.with(|recording| {
            assert!(
                !recording
                    .calls
                    .iter()
                    .any(|call| matches!(call, SinkCall::Write { .. }))
            );
        });
    }
}
