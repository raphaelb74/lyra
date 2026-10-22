//! The queue: gapless continuation, reopening on format change, navigation.

use format_pcm::SampleEncoding;
use player_core::mock::{MockSink, SinkCall};
use player_core::{DsdTransport, Encoding, HardwareVolume, PlaybackState, PlayerStatus, Repeat};
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::{Duration, Instant};

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
    player
        .skip_to(2, DEVICE.into(), refuse(DsdTransport::Auto))
        .unwrap();
    assert_eq!(shown_id(&updates), Some(3));
    // Early in a track, previous goes back one entry.
    player.previous().unwrap();
    assert_eq!(shown_id(&updates), Some(2));
    assert_eq!(player.queue().len(), 3);

    // Next on the last entry stops.
    player
        .skip_to(2, DEVICE.into(), refuse(DsdTransport::Auto))
        .unwrap();
    assert_eq!(shown_id(&updates), Some(3));
    player.next().unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Stopped);
    assert_eq!(status.track, None);
}

#[test]
fn shuffle_keeps_the_playing_track_and_off_restores_the_order() {
    let mut sink = MockSink::new();
    sink.write_delay = Duration::from_millis(20);
    let (player, updates) = start(sink);
    let tracks: Vec<_> = (0..12).map(|_| fixture("s16_44100.wav")).collect();
    player
        .play_queue(queue(&tracks), 4, DEVICE.into(), refuse(DsdTransport::Auto))
        .unwrap();
    wait_for(&updates, PlaybackState::Playing);
    // Paused, the shown track stays put while the order changes.
    player.pause().unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Paused);
    let playing = status.track.map(|track| track.index).unwrap();
    let ids = |player: &player_core::Player| -> Vec<i64> {
        player.queue().iter().filter_map(|item| item.id).collect()
    };
    let given = ids(&player);

    player.set_shuffle(true).unwrap();
    let status = wait_until(&updates, |status| status.shuffle);
    let shuffled = ids(&player);
    assert_ne!(shuffled, given);
    assert_eq!(status.track.map(|track| track.index), Some(0));
    assert_eq!(
        shuffled[0], given[playing],
        "the playing track moves to the front"
    );

    player.set_shuffle(false).unwrap();
    let status = wait_until(&updates, |status| !status.shuffle);
    assert_eq!(ids(&player), given);
    assert_eq!(status.track.map(|track| track.index), Some(playing));
}

#[test]
fn shuffle_stays_on_for_the_next_queue() {
    let mut sink = MockSink::new();
    sink.write_delay = Duration::from_millis(20);
    let (player, updates) = start(sink);
    player.set_shuffle(true).unwrap();
    let tracks: Vec<_> = (0..12).map(|_| fixture("s16_44100.wav")).collect();
    player
        .play_queue(queue(&tracks), 7, DEVICE.into(), refuse(DsdTransport::Auto))
        .unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Playing);
    assert!(status.shuffle);
    let track = status.track.unwrap();
    assert_eq!(
        (track.index, track.id),
        (0, Some(8)),
        "the chosen track plays first"
    );
}

#[test]
fn repeat_all_starts_the_queue_again() {
    let mut sink = MockSink::new();
    sink.write_delay = Duration::from_millis(20);
    let (player, updates) = start(sink);
    player.set_repeat(Repeat::All).unwrap();
    let tracks = [fixture("s16_44100.wav"), fixture("s16_44100.flac")];
    player
        .play_queue(queue(&tracks), 1, DEVICE.into(), refuse(DsdTransport::Auto))
        .unwrap();
    let status = wait_until(&updates, |status| {
        status.state == PlaybackState::Playing
            && status.track.as_ref().and_then(|track| track.id) == Some(1)
    });
    assert_eq!(status.repeat, Repeat::All);
}

#[test]
fn repeat_one_plays_the_track_again_gaplessly() {
    let mut sink = MockSink::new();
    sink.write_delay = Duration::from_millis(5);
    let recorder = sink.recorder.clone();
    let (player, updates) = start(sink);
    player.set_repeat(Repeat::One).unwrap();
    let tracks = [fixture("s16_44100.wav"), fixture("s24_96000.wav")];
    player
        .play_queue(queue(&tracks), 0, DEVICE.into(), refuse(DsdTransport::Auto))
        .unwrap();
    let once = reference("s16_44100.wav").len();
    let deadline = Instant::now() + Duration::from_secs(10);
    while recorder.with(|recording| recording.bytes.len()) < 3 * once {
        assert!(Instant::now() < deadline, "the track did not repeat");
        thread::sleep(Duration::from_millis(10));
    }
    player.stop().unwrap();
    wait_for(&updates, PlaybackState::Stopped);
    let shown: Vec<_> = updates
        .try_iter()
        .filter_map(|status| status.track.and_then(|track| track.id))
        .collect();
    assert!(!shown.contains(&2), "repeating one must not move on");
    recorder.with(|recording| assert_eq!(opens(&recording.calls), 1, "no reopen"));
}

#[test]
fn the_queue_can_be_edited_while_it_plays() {
    let mut sink = MockSink::new();
    sink.write_delay = Duration::from_millis(20);
    let (player, updates) = start(sink);
    // The format changes after the first track, so the decoder cannot run
    // ahead into the next ones: only the playing track is committed.
    let mut tracks = vec![fixture("s16_44100.wav")];
    tracks.extend((0..3).map(|_| fixture("s24_96000.wav")));
    let items = queue(&tracks);
    player
        .play_queue(
            items[..3].to_vec(),
            0,
            DEVICE.into(),
            refuse(DsdTransport::Auto),
        )
        .unwrap();
    wait_for(&updates, PlaybackState::Playing);
    player.pause().unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Paused);
    let playing = status.track.and_then(|track| track.id).unwrap();
    let ids = |player: &player_core::Player| -> Vec<i64> {
        player.queue().iter().filter_map(|item| item.id).collect()
    };
    let mut revision = status.queue_revision;
    let mut changed = |updates: &Receiver<PlayerStatus>| {
        let status = wait_until(updates, |status| status.queue_revision > revision);
        revision = status.queue_revision;
        status
    };

    // Added at the end, then promoted to play right after the playing track.
    let dsd = refuse(DsdTransport::Auto);
    player
        .enqueue(vec![items[3].clone()], false, DEVICE.into(), dsd)
        .unwrap();
    changed(&updates);
    assert_eq!(ids(&player).last(), Some(&4));
    let at = ids(&player).iter().position(|&id| id == 4).unwrap();
    player.move_next(at).unwrap();
    changed(&updates);
    let order = ids(&player);
    let here = order.iter().position(|&id| id == playing).unwrap();
    assert_eq!(order.get(here + 1), Some(&4), "{order:?}");

    player.remove(here + 1).unwrap();
    let status = changed(&updates);
    assert!(!ids(&player).contains(&4));
    assert_eq!(status.queue_length, 3);

    player.clear_queue().unwrap();
    let status = changed(&updates);
    assert_eq!(ids(&player), [playing], "only the playing track is left");
    assert_eq!(status.track.map(|track| track.index), Some(0));
}

#[test]
fn enqueue_plays_when_nothing_plays() {
    let (player, updates) = start(MockSink::new());
    player
        .enqueue(
            queue(&[fixture("s16_44100.wav")]),
            true,
            DEVICE.into(),
            refuse(DsdTransport::Auto),
        )
        .unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Playing);
    assert_eq!(status.track.and_then(|track| track.id), Some(1));
}

#[test]
fn a_stopped_queue_starts_again_from_the_chosen_entry() {
    let (player, updates) = start(MockSink::new());
    let tracks = [fixture("s16_44100.wav"), fixture("s16_44100.flac")];
    let dsd = refuse(DsdTransport::Auto);
    player
        .play_queue(queue(&tracks), 0, DEVICE.into(), dsd)
        .unwrap();
    wait_for(&updates, PlaybackState::Playing);
    player.stop().unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Stopped);
    assert!(!status.has_next(), "nothing is next while stopped");

    player.skip_to(1, DEVICE.into(), dsd).unwrap();
    let (status, _) = wait_for(&updates, PlaybackState::Playing);
    assert_eq!(status.track.and_then(|track| track.id), Some(2));
}

/// Waits for a status matching `done`.
fn wait_until(
    updates: &Receiver<PlayerStatus>,
    done: impl Fn(&PlayerStatus) -> bool,
) -> PlayerStatus {
    loop {
        let status = updates.recv_timeout(Duration::from_secs(10)).unwrap();
        if done(&status) {
            return status;
        }
    }
}
