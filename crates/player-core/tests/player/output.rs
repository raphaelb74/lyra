//! The device around the music: live volume changes and format-change silence.

use format_dsd::DsdLayout;
use player_core::mock::{MockSink, SinkCall};
use player_core::{BitPerfect, DsdTransport, HardwareVolume, PlaybackState};
use std::time::Duration;

use crate::{
    DEVICE, assert_same_bytes, dsd_fixture, fixture, reference, refuse, start, wait_for,
    with_native,
};

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
