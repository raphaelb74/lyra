//! Engine behavior against the mock sink, with the format-pcm fixtures.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail loudly"
)]

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use format_dsd::DsdLayout;
use format_pcm::SampleEncoding;
use player_core::mock::{MockSink, SinkCall};
use player_core::{
    BitPerfect, DsdFallback, DsdPolicy, DsdTransport, Encoding, ErrorKind, HardwareVolume,
    OutputFormat, PlaybackState, Player, PlayerStatus, QueueItem, SinkError, SinkErrorKind,
    Transport,
};

const DEVICE: &str = "hw:CARD=TEST,DEV=0";

/// The default DSD policy for `transport`: bit-perfect or refused.
fn refuse(transport: DsdTransport) -> DsdPolicy {
    DsdPolicy {
        transport,
        fallback: DsdFallback::Refuse,
    }
}

/// Auto transport, converting to PCM when neither DSD transport works.
const CONVERT: DsdPolicy = DsdPolicy {
    transport: DsdTransport::Auto,
    fallback: DsdFallback::Convert { gain_db: 0 },
};

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
        player
            .play(dsd_fixture(name), DEVICE.into(), refuse(DsdTransport::Auto))
            .unwrap();

        let (status, _) = wait_for(&updates, PlaybackState::Stopped);
        let output = status.track.unwrap().output.unwrap();
        assert_eq!(
            (output.encoding, output.sample_rate),
            (Encoding::Dop(SampleEncoding::S24In3Le), 176_400)
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
            .play(
                dsd_fixture("dsd64_stereo.dsf"),
                DEVICE.into(),
                refuse(DsdTransport::Auto),
            )
            .unwrap();

        let error = wait_for(&updates, PlaybackState::Error).0.error.unwrap();
        assert_eq!(error.kind, ErrorKind::Unsupported, "{volume:?}");
        let reason = match volume {
            HardwareVolume::Unknown => "would play loud noise",
            _ => "it is at −6.00 dB",
        };
        assert!(error.message.contains(reason), "{}", error.message);
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

/// Plays a DSD fixture to completion; returns the output and the bytes written.
fn play_dsd(sink: MockSink, name: &str, dsd: DsdPolicy) -> (OutputFormat, Vec<u8>) {
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    player.play(dsd_fixture(name), DEVICE.into(), dsd).unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Stopped);
    drop(player);
    let output = status.track.unwrap().output.unwrap();
    (output, recorder.with(|recording| recording.bytes.clone()))
}

/// The default mock plus one native DSD layout at its DSD64 rate.
fn with_native(layout: DsdLayout) -> MockSink {
    let mut sink = MockSink::new();
    sink.encodings.push(Encoding::Dsd(layout));
    sink.rates.push(layout.frame_rate(2_822_400));
    sink
}

#[test]
fn native_dsd_carries_every_dsd_bit_in_the_device_layout() {
    let reference = dsd_reference();
    for name in ["dsd64_stereo.dsf", "dsd64_stereo.dff"] {
        let (output, bytes) = play_dsd(
            with_native(DsdLayout::U32Be),
            name,
            refuse(DsdTransport::Auto),
        );
        assert_eq!(
            (output.encoding, output.sample_rate),
            (Encoding::Dsd(DsdLayout::U32Be), 88_200),
            "{name}"
        );
        // DSD_U32_BE stereo: 4 bytes of L, then 4 of R, oldest first.
        let mut dsd = Vec::new();
        for frame in bytes.as_chunks::<8>().0 {
            for k in 0..4 {
                dsd.extend_from_slice(&[frame[k], frame[4 + k]]);
            }
        }
        // The last word is filled up with DSD silence, never PCM zeros.
        let (music, padding) = dsd.split_at(reference.len().min(dsd.len()));
        assert_same_bytes(music, &reference, name);
        assert!(padding.len() < 8 && padding.iter().all(|&byte| byte == 0x69));
    }
}

#[test]
fn dsd_transport_choice_is_explicit() {
    let device_with_both = || with_native(DsdLayout::U8);
    let encoding = |sink, dsd| play_dsd(sink, "dsd64_stereo.dsf", dsd).0.encoding;
    assert_eq!(
        encoding(device_with_both(), refuse(DsdTransport::Dop)),
        Encoding::Dop(SampleEncoding::S24In3Le)
    );
    assert_eq!(
        encoding(device_with_both(), refuse(DsdTransport::Native)),
        Encoding::Dsd(DsdLayout::U8)
    );

    // Native only, on a device without native DSD: refused, nothing written.
    let sink = MockSink::new();
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    player
        .play(
            dsd_fixture("dsd64_stereo.dsf"),
            DEVICE.into(),
            refuse(DsdTransport::Native),
        )
        .unwrap();
    let error = wait_for(&updates, PlaybackState::Error).0.error.unwrap();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert!(
        error.message.contains("DSD64 as native DSD"),
        "{}",
        error.message
    );
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

#[test]
fn native_dsd_plays_at_any_dac_volume() {
    // The device is in a DSD format; there are no markers for a volume to break.
    let mut sink = with_native(DsdLayout::U32Be);
    sink.hardware_volume = HardwareVolume::Unknown;
    let (output, bytes) = play_dsd(sink, "dsd64_stereo.dsf", refuse(DsdTransport::Auto));
    assert_eq!(output.encoding, Encoding::Dsd(DsdLayout::U32Be));
    assert_eq!(bytes.len(), dsd_reference().len().next_multiple_of(8));
}

fn queue(paths: &[PathBuf]) -> Vec<QueueItem> {
    paths
        .iter()
        .zip(1..)
        .map(|(path, id)| QueueItem {
            path: path.clone(),
            id: Some(id),
        })
        .collect()
}

fn opens(calls: &[SinkCall]) -> usize {
    calls
        .iter()
        .filter(|call| matches!(call, SinkCall::Open(_)))
        .count()
}

#[test]
fn same_format_tracks_play_gaplessly() {
    let sink = MockSink::new();
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    let tracks = [fixture("s16_44100.wav"), fixture("s16_44100.flac")];
    player
        .play_queue(queue(&tracks), 0, DEVICE.into(), refuse(DsdTransport::Auto))
        .unwrap();

    // The status follows the device into the second track.
    let mut shown = Vec::new();
    loop {
        let status = updates.recv_timeout(Duration::from_secs(10)).unwrap();
        if let Some(track) = &status.track
            && shown.last() != Some(&track.id)
        {
            shown.push(track.id);
        }
        if status.state == PlaybackState::Stopped {
            assert_eq!(status.queue_length, 2);
            break;
        }
    }
    assert_eq!(shown, [Some(1), Some(2)]);
    drop(player);

    let reference = reference("s16_44100.wav");
    recorder.with(|recording| {
        assert_eq!(opens(&recording.calls), 1, "no reopen between tracks");
        let drains = recording
            .calls
            .iter()
            .filter(|call| **call == SinkCall::Drain)
            .count();
        assert_eq!(drains, 1, "drained only at the end of the queue");
        // Both tracks, back to back, every byte.
        assert_same_bytes(
            &recording.bytes,
            &[reference.clone(), reference].concat(),
            "queue",
        );
    });
}

#[test]
fn a_format_change_reopens_the_device_between_tracks() {
    let sink = MockSink::new();
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    let tracks = [fixture("s16_44100.wav"), fixture("s24_96000.wav")];
    player
        .play_queue(queue(&tracks), 0, DEVICE.into(), refuse(DsdTransport::Auto))
        .unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Stopped);
    let output = status.track.unwrap().output.unwrap();
    assert_eq!(
        (output.encoding, output.sample_rate),
        (Encoding::Pcm(SampleEncoding::S24In3Le), 96_000)
    );
    drop(player);
    recorder.with(|recording| {
        assert_eq!(opens(&recording.calls), 2);
        // The second open saw exactly the second file.
        assert_same_bytes(
            &recording.bytes,
            &reference("s24_96000.wav"),
            "second track",
        );
    });
}

#[test]
fn dop_markers_continue_across_a_gapless_boundary() {
    let mut sink = MockSink::new();
    sink.hardware_volume = HardwareVolume::Unity;
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    let tracks = [
        dsd_fixture("dsd64_stereo.dsf"),
        dsd_fixture("dsd64_stereo.dff"),
    ];
    player
        .play_queue(queue(&tracks), 0, DEVICE.into(), refuse(DsdTransport::Dop))
        .unwrap();
    wait_for(&updates, PlaybackState::Stopped);
    drop(player);
    recorder.with(|recording| {
        assert_eq!(opens(&recording.calls), 1);
        let frames = recording.bytes.as_chunks::<6>().0;
        // 9426 DSD frames per file: 4713 DoP frames each, an odd count, so
        // the second file must start on the opposite marker.
        assert_eq!(frames.len(), 2 * 4713);
        for (index, frame) in frames.iter().enumerate() {
            let marker = if index % 2 == 0 { 0x05 } else { 0xFA };
            assert_eq!((frame[2], frame[5]), (marker, marker), "DoP frame {index}");
        }
    });
}

#[test]
fn next_previous_and_skip_move_through_the_queue() {
    let mut sink = MockSink::new();
    sink.write_delay = Duration::from_millis(20);
    let (player, updates) = start(sink);
    let tracks = [
        fixture("s16_44100.wav"),
        fixture("s24_96000.wav"),
        fixture("s16_44100.flac"),
    ];
    player
        .play_queue(queue(&tracks), 0, DEVICE.into(), refuse(DsdTransport::Auto))
        .unwrap();
    let shown_id = |updates: &Receiver<PlayerStatus>| {
        wait_for(updates, PlaybackState::Playing)
            .0
            .track
            .and_then(|track| track.id)
    };
    assert_eq!(shown_id(&updates), Some(1));

    player.next().unwrap();
    assert_eq!(shown_id(&updates), Some(2));
    player.skip_to(2).unwrap();
    assert_eq!(shown_id(&updates), Some(3));
    // Early in a track, previous goes back one entry.
    player.previous().unwrap();
    assert_eq!(shown_id(&updates), Some(2));
    assert_eq!(player.queue().len(), 3);

    // Next on the last entry stops.
    player.skip_to(2).unwrap();
    assert_eq!(shown_id(&updates), Some(3));
    player.next().unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Stopped);
    assert_eq!(status.track, None);
}

/// Built-in audio as Lyra sees it: PCM and the DoP format accepted, volume
/// unreadable, no native DSD.
fn built_in() -> MockSink {
    let mut sink = MockSink::new();
    sink.hardware_volume = HardwareVolume::Unknown;
    sink
}

/// The fixture converted in one go, as the device must receive it.
fn converted_reference(tracks: usize) -> Vec<u8> {
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

#[test]
fn conversion_plays_dsd_on_a_device_without_dsd_support() {
    let sink = built_in();
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    player
        .play(dsd_fixture("dsd64_stereo.dsf"), DEVICE.into(), CONVERT)
        .unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Stopped);
    let track = status.track.unwrap();
    let output = track.output.unwrap();
    assert_eq!(
        (output.encoding, output.sample_rate),
        (Encoding::Converted(SampleEncoding::S24In3Le), 88_200)
    );
    assert_eq!(output.encoding.transport(), Transport::Converted);
    match track.bit_perfect() {
        Some(BitPerfect::No { reason }) => assert!(reason.contains("converted to PCM")),
        other => panic!("converted audio reported as {other:?}"),
    }
    drop(player);
    // Every chunk, the filters' tail included: identical to converting the
    // whole file at once.
    recorder.with(|recording| {
        assert_same_bytes(&recording.bytes, &converted_reference(1), "converted");
    });
}

#[test]
fn converted_tracks_join_without_a_seam() {
    let sink = built_in();
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    let tracks = [
        dsd_fixture("dsd64_stereo.dsf"),
        dsd_fixture("dsd64_stereo.dff"),
    ];
    player
        .play_queue(queue(&tracks), 0, DEVICE.into(), CONVERT)
        .unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Stopped);
    assert_eq!(status.track.unwrap().id, Some(2));
    drop(player);
    recorder.with(|recording| {
        assert_eq!(opens(&recording.calls), 1, "no reopen between tracks");
        // The filters run straight through the boundary: one stream.
        assert_same_bytes(&recording.bytes, &converted_reference(2), "two tracks");
    });
}

#[test]
fn conversion_is_never_chosen_over_a_bit_perfect_transport() {
    // A USB DAC at 0 dB takes DoP; native DSD wins over both.
    let mut dop_dac = MockSink::new();
    dop_dac.rates.push(176_400);
    dop_dac.hardware_volume = HardwareVolume::Unity;
    let (output, _) = play_dsd(dop_dac, "dsd64_stereo.dsf", CONVERT);
    assert!(matches!(output.encoding, Encoding::Dop(_)));
    let (output, _) = play_dsd(with_native(DsdLayout::U32Be), "dsd64_stereo.dsf", CONVERT);
    assert_eq!(output.encoding, Encoding::Dsd(DsdLayout::U32Be));

    // An explicit DoP choice is kept even where conversion would play.
    let sink = built_in();
    let (player, updates) = start(sink);
    let dop_only = DsdPolicy {
        transport: DsdTransport::Dop,
        ..CONVERT
    };
    player
        .play(dsd_fixture("dsd64_stereo.dsf"), DEVICE.into(), dop_only)
        .unwrap();
    let error = wait_for(&updates, PlaybackState::Error).0.error.unwrap();
    assert_eq!(error.kind, ErrorKind::Unsupported);
}

#[test]
fn a_volume_change_updates_the_verdict_while_playing() {
    let mut sink = MockSink::new();
    sink.hardware_volume = HardwareVolume::Unity;
    sink.write_delay = Duration::from_millis(50);
    let (player, updates) = start(sink);
    player
        .play(
            fixture("s24_96000.wav"),
            DEVICE.into(),
            refuse(DsdTransport::Auto),
        )
        .unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Playing);
    assert_eq!(status.track.unwrap().bit_perfect(), Some(BitPerfect::Yes));

    let lowered = HardwareVolume::Attenuated { centibels: -1_000 };
    // Another device's volume is not this one's business.
    player
        .volume_changed("hw:CARD=OTHER,DEV=0".into(), lowered)
        .unwrap();
    player.volume_changed(DEVICE.into(), lowered).unwrap();
    let verdict = loop {
        let status = updates.recv_timeout(Duration::from_secs(10)).unwrap();
        let output = status.track.as_ref().and_then(|track| track.output);
        if output.is_some_and(|output| output.hardware_volume == lowered) {
            break status.track.unwrap().bit_perfect();
        }
    };
    assert!(
        matches!(verdict, Some(BitPerfect::No { .. })),
        "{verdict:?}"
    );
}

#[test]
fn a_volume_change_during_dop_stops_playback() {
    let mut sink = MockSink::new();
    sink.rates.push(176_400);
    sink.hardware_volume = HardwareVolume::Unity;
    sink.write_delay = Duration::from_millis(50);
    let (player, updates) = start(sink);
    player
        .play(
            dsd_fixture("dsd64_stereo.dsf"),
            DEVICE.into(),
            refuse(DsdTransport::Dop),
        )
        .unwrap();
    wait_for(&updates, PlaybackState::Playing);
    player
        .volume_changed(
            DEVICE.into(),
            HardwareVolume::Attenuated { centibels: -300 },
        )
        .unwrap();
    let error = wait_for(&updates, PlaybackState::Error).0.error.unwrap();
    assert!(
        error.message.contains("moved during DoP"),
        "{}",
        error.message
    );
}

#[test]
fn silence_comes_first_when_the_format_changes() {
    let sink = MockSink::new();
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    player.set_resync_delay(Duration::from_millis(100));
    let reference = reference("s16_44100.wav");

    // First open since start: the DAC's state is unknown, so silence first.
    let play = |name: &str| {
        player
            .play(fixture(name), DEVICE.into(), refuse(DsdTransport::Auto))
            .unwrap();
        wait_for(&updates, PlaybackState::Stopped);
    };
    play("s16_44100.wav");
    recorder.with(|recording| {
        // 100 ms at 44.1 kHz: 4410 stereo 16-bit frames of zeros.
        let silence = vec![0; 4_410 * 4];
        assert_same_bytes(
            &recording.bytes,
            &[silence, reference.clone()].concat(),
            "first",
        );
    });

    // The same format again: straight into the music.
    play("s16_44100.wav");
    recorder.with(|recording| assert_same_bytes(&recording.bytes, &reference, "again"));
}

#[test]
fn a_refused_dop_device_never_gets_silence_either() {
    let mut sink = MockSink::new();
    sink.hardware_volume = HardwareVolume::Unknown;
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    player.set_resync_delay(Duration::from_millis(500));
    player
        .play(
            dsd_fixture("dsd64_stereo.dsf"),
            DEVICE.into(),
            refuse(DsdTransport::Dop),
        )
        .unwrap();
    wait_for(&updates, PlaybackState::Error);
    drop(player);
    recorder.with(|recording| {
        assert!(
            !recording
                .calls
                .iter()
                .any(|call| matches!(call, SinkCall::Write { .. })),
            "DoP silence is still DoP: it plays as noise where DoP is refused"
        );
    });
}

#[test]
fn stopping_dsd_ends_on_dsd_silence() {
    let mut sink = with_native(DsdLayout::U32Be);
    sink.write_delay = Duration::from_millis(50);
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    player
        .play(
            dsd_fixture("dsd64_stereo.dsf"),
            DEVICE.into(),
            refuse(DsdTransport::Auto),
        )
        .unwrap();
    wait_for(&updates, PlaybackState::Playing);
    player.stop().unwrap();
    wait_for(&updates, PlaybackState::Stopped);
    drop(player);
    recorder.with(|recording| {
        let tail: Vec<&SinkCall> = recording.calls.iter().rev().take(4).rev().collect();
        assert!(
            matches!(
                tail.as_slice(),
                [
                    SinkCall::Discard,
                    SinkCall::Write { .. },
                    SinkCall::Drain,
                    SinkCall::Close
                ]
            ),
            "{tail:?}"
        );
        // After the discard: 100 ms of the idle pattern, nothing else.
        assert_eq!(recording.bytes.len(), 8_820 * 8);
        assert!(recording.bytes.iter().all(|&byte| byte == 0x69));
    });
}
