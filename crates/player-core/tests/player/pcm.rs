//! PCM playback: exact bytes, states, refusals, pause, seek, stop.

use format_pcm::SampleEncoding;
use player_core::mock::{MockSink, SinkCall};
use player_core::{
    DsdTransport, Encoding, ErrorKind, HardwareVolume, OutputFormat, PlaybackState, PlayerStatus,
    SinkError, SinkErrorKind,
};
use std::time::Duration;

use crate::{DEVICE, assert_same_bytes, fixture, reference, refuse, start, wait_for};

#[test]
fn plays_every_source_byte_through_the_states() {
    for (name, encoding, rate) in [
        ("s16_44100.wav", SampleEncoding::S16Le, 44_100),
        ("s24_96000.wav", SampleEncoding::S24In3Le, 96_000),
    ] {
        let sink = MockSink::new();
        let recorder = sink.recorder.clone();
        let (player, updates) = start(sink);
        player
            .play(fixture(name), DEVICE.into(), refuse(DsdTransport::Auto))
            .unwrap();

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
            encoding: Encoding::Pcm(encoding),
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
    sink.encodings = vec![Encoding::Pcm(SampleEncoding::S32Le)];
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    player
        .play(
            fixture("s16_44100.wav"),
            DEVICE.into(),
            refuse(DsdTransport::Auto),
        )
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
        .play(
            fixture("s24_96000.wav"),
            DEVICE.into(),
            refuse(DsdTransport::Auto),
        )
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
        .play(
            fixture("s16_44100.wav"),
            DEVICE.into(),
            refuse(DsdTransport::Auto),
        )
        .unwrap();

    let error = wait_for(&updates, PlaybackState::Error).0.error.unwrap();
    assert_eq!(error.kind, ErrorKind::DeviceBusy);
    assert!(error.message.contains("pipewire"));
}

#[test]
fn reports_a_missing_file() {
    let (player, updates) = start(MockSink::new());
    player
        .play(
            fixture("missing.flac"),
            DEVICE.into(),
            refuse(DsdTransport::Auto),
        )
        .unwrap();
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
        .play(
            fixture("s24_96000.wav"),
            DEVICE.into(),
            refuse(DsdTransport::Auto),
        )
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
        .play(
            fixture("s16_44100.wav"),
            DEVICE.into(),
            refuse(DsdTransport::Auto),
        )
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
        .play(
            fixture("s24_96000.wav"),
            DEVICE.into(),
            refuse(DsdTransport::Auto),
        )
        .unwrap();
    wait_for(&updates, PlaybackState::Playing);

    player.stop().unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Stopped);
    assert_eq!(
        status,
        PlayerStatus {
            state: PlaybackState::Stopped,
            track: None,
            error: None,
            queue_length: 1,
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
