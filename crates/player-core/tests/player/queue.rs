//! The queue: gapless continuation, reopening on format change, navigation.

use format_pcm::SampleEncoding;
use player_core::mock::{MockSink, SinkCall};
use player_core::{DsdTransport, Encoding, HardwareVolume, PlaybackState, PlayerStatus};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crate::{
    DEVICE, assert_same_bytes, dsd_fixture, fixture, opens, queue, reference, refuse, start,
    wait_for,
};

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
