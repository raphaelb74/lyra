//! DSD as DoP and native DSD, and the transport choice.

use format_dsd::DsdLayout;
use format_pcm::SampleEncoding;
use unda::mock::{MockSink, SinkCall};
use unda::{DsdTransport, Encoding, ErrorKind, HardwareVolume, PlaybackState};

use crate::{
    DEVICE, assert_same_bytes, dsd_fixture, dsd_reference, play_dsd, refuse, start, wait_for,
    with_native,
};

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
