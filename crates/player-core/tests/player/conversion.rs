//! Opt-in DSD to PCM conversion on devices without DSD support.

use format_dsd::DsdLayout;
use format_pcm::SampleEncoding;
use player_core::mock::MockSink;
use player_core::{
    BitPerfect, DsdPolicy, DsdTransport, Encoding, ErrorKind, HardwareVolume, PlaybackState,
    Transport,
};

use crate::{
    CONVERT, DEVICE, assert_same_bytes, built_in, converted_reference, dsd_fixture, opens,
    play_dsd, queue, start, wait_for, with_native,
};

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
fn conversion_reports_its_peak_while_it_plays() {
    let mut sink = built_in();
    sink.write_delay = std::time::Duration::from_millis(50);
    let (player, updates) = start(sink);
    assert_eq!(player.conversion(), None);
    player
        .play(dsd_fixture("dsd64_stereo.dsf"), DEVICE.into(), CONVERT)
        .unwrap();
    wait_for(&updates, PlaybackState::Playing);
    let stats = player.conversion().unwrap();
    assert_eq!(stats.clipped, 0);
    assert!(stats.peak > 0.0 && stats.peak < 1.0, "{stats:?}");
    wait_for(&updates, PlaybackState::Stopped);
    assert_eq!(player.conversion(), None, "nothing converts once stopped");
}

#[test]
fn native_dsd_reports_no_conversion() {
    let mut sink = with_native(DsdLayout::U32Be);
    sink.write_delay = std::time::Duration::from_millis(50);
    let (player, updates) = start(sink);
    player
        .play(dsd_fixture("dsd64_stereo.dsf"), DEVICE.into(), CONVERT)
        .unwrap();
    wait_for(&updates, PlaybackState::Playing);
    assert_eq!(player.conversion(), None);
}
