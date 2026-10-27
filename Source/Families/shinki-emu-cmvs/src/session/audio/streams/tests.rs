use super::super::tests::{OutputControl, OutputStep};
use super::*;
use crate::system_save::slot_globals::NativeResourceChannel;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

fn clip(values: impl IntoIterator<Item = f32>) -> DecodedClip {
    DecodedClip {
        sample_rate: OUTPUT_RATE,
        frames: Arc::new(values.into_iter().flat_map(|value| [value; 2]).collect()),
        source: None,
        native_channels: 2,
        indeterminate_tail: false,
        native_restart_rewinds: true,
        native_repeat: None,
        native_pcm: None,
    }
}

fn constant(value: f32, frames: usize) -> DecodedClip {
    clip(std::iter::repeat_n(value, frames))
}

fn resource(bridge: &mut AudioBridge, channel: u8, clip: DecodedClip, looping: bool, fade_ms: u32) {
    bridge
        .configure_native_resource(
            NativeResourceChannel {
                name: "retained.wav".into(),
                ..Default::default()
            },
            NativeChannelPlayback {
                slot: usize::from(channel),
                clip,
                looping,
                playing: true,
                native_gain: 256,
                native_volume: 256,
                native_pan: 0,
                volume: 1.0,
                stereo_gain: [1.0; 2],
            },
            fade_ms,
        )
        .unwrap();
}

// Enqueue the same worker query used by snapshot before releasing an output write.
// Its reply must remain blocked until that PCM has been accepted.
fn accepted_snapshot(
    bridge: &AudioBridge,
    control: &OutputControl,
    channel: u8,
) -> NativeStreamSnapshot {
    let (reply, result) = mpsc::channel();
    bridge
        .submit(Command::StreamSnapshot {
            channel,
            observe_position: false,
            reply,
        })
        .unwrap();
    assert!(matches!(result.try_recv(), Err(TryRecvError::Empty)));
    control.steps.send(OutputStep::Accept).unwrap();
    result.recv_timeout(Duration::from_secs(5)).unwrap()
}

#[test]
fn voice_eos_retains_decoded_object_and_restart_emits_pcm_again() {
    for channel in 5..9 {
        let (mut bridge, control) = OutputControl::new();
        bridge
            .configure_voice(
                channel,
                constant(0.5, 4).with_native_channels(1),
                false,
                256,
            )
            .unwrap();
        control.wait_for_write();
        let pcm = control.latest_pcm();
        assert_eq!(&pcm[..8], &[0.5; 8]);
        assert!(pcm[8..].iter().all(|sample| *sample == 0.0));
        let ended = accepted_snapshot(&bridge, &control, channel);
        assert!(ended.exists && !ended.playing);
        assert_eq!(ended.decoded_bytes, 8);
        assert_eq!(ended.levels, [0; 2]);
        assert_eq!(bridge.snapshot(channel, false).unwrap(), ended);
        assert_ne!(bridge.playing_channels().unwrap() & (1 << channel), 0);
        control.consume_to(ended.marker.accepted_end_frame.unwrap());
        assert_eq!(bridge.playing_channels().unwrap() & (1 << channel), 0);
        bridge
            .control_channel(channel, NativeStreamControl::Restart { looping: true })
            .unwrap();
        control.wait_for_write();
        assert!(control.latest_pcm().iter().all(|sample| *sample == 0.5));
        bridge
            .control_channel(channel, NativeStreamControl::Pause)
            .unwrap();
        let paused = accepted_snapshot(&bridge, &control, channel);
        assert_eq!(paused.decoded_bytes, 8 + CHUNK_FRAMES as u32 * 2);
        assert!(paused.exists && !paused.playing);
        control.finish(&mut bridge);
    }
}

#[test]
fn pause_retains_cursor_start_resumes_and_restart_seeks_without_resetting_byte_counter() {
    let (mut bridge, control) = OutputControl::new();
    let samples = (0..CHUNK_FRAMES * 8).map(|frame| (frame / CHUNK_FRAMES + 1) as f32 / 8.0);
    bridge.configure_voice(5, clip(samples), true, 256).unwrap();
    control.wait_for_write();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.125));
    bridge
        .control_channel(5, NativeStreamControl::Pause)
        .unwrap();
    let paused = accepted_snapshot(&bridge, &control, 5);
    assert_eq!(
        (paused.marker.accepted_output_frames, paused.decoded_bytes),
        (2400, 9600)
    );
    assert_eq!(paused.position_ms, None);
    assert_eq!(bridge.snapshot(5, false).unwrap(), paused);
    bridge
        .control_channel(5, NativeStreamControl::Start { looping: true })
        .unwrap();
    control.wait_for_write();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.25));
    bridge
        .control_channel(5, NativeStreamControl::Pause)
        .unwrap();
    let paused = accepted_snapshot(&bridge, &control, 5);
    assert_eq!(
        (paused.marker.accepted_output_frames, paused.decoded_bytes),
        (4800, 19200)
    );
    bridge
        .control_channel(5, NativeStreamControl::Restart { looping: true })
        .unwrap();
    control.wait_for_write();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.125));
    bridge
        .control_channel(5, NativeStreamControl::Pause)
        .unwrap();
    let restarted = accepted_snapshot(&bridge, &control, 5);
    assert_eq!(restarted.decoded_bytes, 28800);
    assert_eq!(restarted.position_ms, None);
    assert_eq!(restarted.marker.accepted_output_frames, 7200);
    control.finish(&mut bridge);
}

#[test]
fn stream_volume_and_pan_follow_wrapping_native_gain_not_signed_clamps() {
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(5, constant(0.5, CHUNK_FRAMES), true, 256)
        .unwrap();
    control.wait_for_write();
    bridge
        .control_channel(5, NativeStreamControl::Volume(128))
        .unwrap();
    bridge
        .control_channel(5, NativeStreamControl::Pan(128))
        .unwrap();
    control.accept_and_wait();
    assert!(
        control
            .latest_pcm()
            .as_chunks::<2>()
            .0
            .iter()
            .all(|frame| *frame == [0.125, 0.25])
    );
    bridge
        .control_channel(5, NativeStreamControl::Pan(512))
        .unwrap();
    control.accept_and_wait();
    assert!(
        control
            .latest_pcm()
            .as_chunks::<2>()
            .0
            .iter()
            .all(|frame| *frame == [0.5, 0.25])
    );
    bridge
        .control_channel(5, NativeStreamControl::Volume(u32::MAX))
        .unwrap();
    bridge
        .control_channel(5, NativeStreamControl::Pan(0))
        .unwrap();
    control.accept_and_wait();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.5));
    bridge
        .control_channel(5, NativeStreamControl::Volume(0x01000001))
        .unwrap();
    control.accept_and_wait();
    assert!(
        control
            .latest_pcm()
            .iter()
            .all(|sample| *sample == 1.0 / 512.0)
    );
    bridge
        .control_channel(5, NativeStreamControl::Volume(256))
        .unwrap();
    bridge
        .control_channel(5, NativeStreamControl::Pan(-256))
        .unwrap();
    control.accept_and_wait();
    assert!(
        control
            .latest_pcm()
            .as_chunks::<2>()
            .0
            .iter()
            .all(|frame| *frame == [0.5, 0.0])
    );
    control.finish(&mut bridge);
}

#[test]
fn software_stream_gain_is_linear_and_mixes_with_s16_saturation() {
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(5, constant(0.75, CHUNK_FRAMES), true, 128)
        .unwrap();
    control.wait_for_write();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.375));
    bridge
        .configure_voice(6, constant(0.75, CHUNK_FRAMES), true, 256)
        .unwrap();
    control.accept_and_wait();
    assert!(
        control
            .latest_pcm()
            .iter()
            .all(|sample| *sample == 32767.0 / 32768.0)
    );
    bridge
        .configure_voice(5, constant(-0.75, CHUNK_FRAMES), true, 256)
        .unwrap();
    bridge
        .configure_voice(6, constant(-0.75, CHUNK_FRAMES), true, 256)
        .unwrap();
    control.accept_and_wait();
    assert!(control.latest_pcm().iter().all(|sample| *sample == -1.0));
    control.finish(&mut bridge);
}

#[test]
fn negative_pcm_gain_uses_arithmetic_shift_rounding() {
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(5, constant(-3.0 / 32768.0, CHUNK_FRAMES), true, 128)
        .unwrap();
    control.wait_for_write();
    assert!(
        control
            .latest_pcm()
            .iter()
            .all(|sample| *sample == -2.0 / 32768.0)
    );
    control.finish(&mut bridge);
}

#[test]
fn ordered_queries_report_delayed_pregain_levels_even_when_muted() {
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(
            8,
            constant(0.5, CHUNK_FRAMES).with_native_channels(1),
            true,
            256,
        )
        .unwrap();
    control.wait_for_write();
    let first = accepted_snapshot(&bridge, &control, 8);
    assert_eq!(
        (
            first.marker.accepted_output_frames,
            first.decoded_bytes,
            first.levels
        ),
        (2400, 4800, [0; 2])
    );
    control.wait_for_write();
    bridge
        .control_channel(8, NativeStreamControl::Volume(0))
        .unwrap();
    let second = accepted_snapshot(&bridge, &control, 8);
    assert_eq!(
        (
            second.marker.accepted_output_frames,
            second.decoded_bytes,
            second.levels
        ),
        (4800, 9600, [0; 2])
    );
    control.wait_for_write();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.0));
    let third = accepted_snapshot(&bridge, &control, 8);
    assert_eq!(
        (
            third.marker.accepted_output_frames,
            third.decoded_bytes,
            third.levels
        ),
        (7200, 14400, [16384; 2])
    );
    control.wait_for_write();
    bridge
        .control_channel(8, NativeStreamControl::Pause)
        .unwrap();
    let paused = accepted_snapshot(&bridge, &control, 8);
    assert!(paused.exists && !paused.playing);
    assert_eq!(paused.levels, [0; 2]);
    control.finish(&mut bridge);
}

#[test]
fn every_ordinary_slot_generation_survives_stale_eos_then_disposes() {
    for channel in 5..15 {
        let (mut bridge, control) = OutputControl::new();
        if channel < 9 {
            bridge
                .configure_voice(channel, constant(0.5, 4), false, 256)
                .unwrap();
        } else {
            resource(&mut bridge, channel, constant(0.5, 4), false, 0);
        }
        control.wait_for_write();
        assert_eq!(control.latest_pcm()[0], 0.5);
        let old = bridge.channel_generations[usize::from(channel)].load(Ordering::Acquire);
        if channel < 9 {
            bridge
                .configure_voice(channel, constant(0.25, CHUNK_FRAMES), true, 256)
                .unwrap();
        } else {
            resource(&mut bridge, channel, constant(0.25, CHUNK_FRAMES), true, 0);
        }
        let replacement = bridge.channel_generations[usize::from(channel)].load(Ordering::Acquire);
        assert_ne!(old, replacement);
        control.accept_and_wait();
        assert!(control.latest_pcm().iter().all(|sample| *sample == 0.25));
        assert_eq!(
            bridge.channel_generations[usize::from(channel)].load(Ordering::Acquire),
            replacement
        );
        bridge
            .control_channel(channel, NativeStreamControl::Dispose)
            .unwrap();
        assert!(!accepted_snapshot(&bridge, &control, channel).exists);
        assert!(!bridge.snapshot(channel, false).unwrap().exists);
        assert_eq!(
            bridge.channel_generations[usize::from(channel)].load(Ordering::Acquire),
            0
        );
        control.finish(&mut bridge);
    }
}

#[test]
fn resource_fade_in_is_atomic_and_natural_end_keeps_descriptor() {
    let (mut bridge, control) = OutputControl::new();
    resource(&mut bridge, 14, constant(0.5, CHUNK_FRAMES * 2), false, 100);
    control.wait_for_write();
    let pcm = control.latest_pcm();
    assert_eq!(pcm[0], 0.0);
    assert_eq!(pcm[CHUNK_FRAMES * 2 - 2], 0.25);
    assert!(accepted_snapshot(&bridge, &control, 14).playing);
    control.wait_for_write();
    assert_eq!(control.latest_pcm()[CHUNK_FRAMES * 2 - 2], 0.5);
    let ended = accepted_snapshot(&bridge, &control, 14);
    assert!(ended.exists && !ended.playing);
    let mut globals = crate::system_save::slot_globals::NativeGlobals::default();
    bridge.capture_native_audio(&mut globals).unwrap();
    assert_eq!(globals.resource_channels.slots[5].name, "retained.wav");
    bridge
        .control_channel(14, NativeStreamControl::Restart { looping: true })
        .unwrap();
    control.wait_for_write();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.5));
    control.finish(&mut bridge);
}

#[test]
fn fade_out_disposes_playing_or_paused_objects_and_unsigned_duration_is_preserved() {
    for paused in [false, true] {
        let (mut bridge, control) = OutputControl::new();
        resource(&mut bridge, 9, constant(0.5, CHUNK_FRAMES), true, 0);
        control.wait_for_write();
        if paused {
            bridge
                .control_channel(9, NativeStreamControl::Pause)
                .unwrap();
        }
        bridge
            .control_channel(9, NativeStreamControl::FadeOut { duration_ms: 100 })
            .unwrap();
        control.accept_and_wait();
        let pcm = control.latest_pcm();
        assert_eq!(pcm[0], if paused { 0.0 } else { 0.5 });
        assert_eq!(pcm[CHUNK_FRAMES * 2 - 2], if paused { 0.0 } else { 0.25 });
        control.accept_and_wait();
        assert!(!accepted_snapshot(&bridge, &control, 9).exists);
        control.finish(&mut bridge);
    }
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(5, constant(0.5, CHUNK_FRAMES), true, 256)
        .unwrap();
    control.wait_for_write();
    bridge
        .control_channel(
            5,
            NativeStreamControl::FadeOut {
                duration_ms: u32::MAX,
            },
        )
        .unwrap();
    control.accept_and_wait();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.5));
    bridge
        .control_channel(5, NativeStreamControl::FadeOut { duration_ms: 0 })
        .unwrap();
    //48C0E0 does not restart an already active fade-out, even with a new duration.
    control.accept_and_wait();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.5));
    bridge
        .control_channel(5, NativeStreamControl::Dispose)
        .unwrap();
    assert!(!accepted_snapshot(&bridge, &control, 5).exists);
    control.finish(&mut bridge);
}

#[test]
fn output_failure_cannot_return_a_successful_ordered_stream_query() {
    for failure in [OutputStep::Fail, OutputStep::Close] {
        let (mut bridge, control) = OutputControl::new();
        bridge
            .configure_voice(5, constant(0.5, CHUNK_FRAMES), true, 256)
            .unwrap();
        control.wait_for_write();
        assert!(control.latest_pcm().iter().any(|sample| *sample != 0.0));
        let (reply, result) = mpsc::channel();
        bridge
            .submit(Command::StreamSnapshot {
                channel: 5,
                observe_position: false,
                reply,
            })
            .unwrap();
        control.steps.send(failure).unwrap();
        assert!(result.recv_timeout(Duration::from_secs(5)).is_err());
        assert!(bridge.close().is_err());
        assert!(bridge.snapshot(5, false).is_err());
        assert!(
            bridge
                .control_channel(5, NativeStreamControl::Restart { looping: true })
                .is_err()
        );
        assert_eq!(bridge.channel_generations[5].load(Ordering::Acquire), 0);
    }
}

#[test]
fn stretch_emits_initialized_pcm_then_reports_first_unwritten_sample() {
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(
            5,
            constant(0.25, CHUNK_FRAMES + 4).with_indeterminate_tail(),
            true,
            256,
        )
        .unwrap();
    control.wait_for_write();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.25));
    assert!(accepted_snapshot(&bridge, &control, 5).playing);
    control.wait_for_write();
    assert_eq!(control.latest_pcm(), vec![0.25; 8]);
    let worker = bridge.worker.take().unwrap();
    control.steps.send(OutputStep::Accept).unwrap();
    worker.join().unwrap();
    let failure = bridge.close().unwrap_err();
    assert_eq!(
        failure.code.as_str(),
        "SHINKI_EMU_CMVS_NATIVE_AUDIO_STRETCH_INDETERMINATE"
    );
    assert_eq!(
        bridge.snapshot(5, false).unwrap_err().code.as_str(),
        failure.code.as_str()
    );
}

#[test]
fn disposing_before_stretch_boundary_never_consumes_unwritten_pcm() {
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(
            5,
            constant(0.25, CHUNK_FRAMES * 2).with_indeterminate_tail(),
            true,
            256,
        )
        .unwrap();
    control.wait_for_write();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.25));
    bridge
        .control_channel(5, NativeStreamControl::Dispose)
        .unwrap();
    assert!(!accepted_snapshot(&bridge, &control, 5).exists);
    control.finish(&mut bridge);
}

#[test]
fn codec_noop_restart_keeps_decoder_cursor_instead_of_rewinding_pcm() {
    let (mut bridge, control) = OutputControl::new();
    let samples = (0..CHUNK_FRAMES * 4).map(|frame| (frame / CHUNK_FRAMES + 1) as f32 / 8.0);
    bridge
        .configure_voice(
            5,
            clip(samples).with_native_restart_rewinds(false),
            false,
            256,
        )
        .unwrap();
    control.wait_for_write();
    assert_eq!(control.latest_pcm()[0], 0.125);
    bridge
        .control_channel(5, NativeStreamControl::Restart { looping: false })
        .unwrap();
    control.accept_and_wait();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.25));
    control.finish(&mut bridge);
}

#[test]
fn native_restore_publishes_resource_generations_and_retires_old_voice_objects() {
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(5, constant(0.5, 4), false, 256)
        .unwrap();
    control.wait_for_write();
    let playback = PreparedNativePlayback::new(
        [None, None],
        256,
        vec![
            NativeChannelPlayback {
                slot: 9,
                clip: constant(0.125, CHUNK_FRAMES),
                looping: true,
                playing: true,
                native_gain: 256,
                native_volume: 256,
                native_pan: 0,
                volume: 1.0,
                stereo_gain: [1.0; 2],
            },
            NativeChannelPlayback {
                slot: 14,
                clip: constant(0.25, CHUNK_FRAMES),
                looping: true,
                playing: true,
                native_gain: 256,
                native_volume: 256,
                native_pan: 0,
                volume: 1.0,
                stereo_gain: [1.0; 2],
            },
        ],
    );
    bridge
        .commit_native_audio(playback, Default::default())
        .unwrap();
    assert_ne!(bridge.channel_generations[9].load(Ordering::Acquire), 0);
    assert_ne!(bridge.channel_generations[14].load(Ordering::Acquire), 0);
    control.accept_and_wait();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.375));
    bridge
        .control_channel(9, NativeStreamControl::Pause)
        .unwrap();
    bridge
        .control_channel(14, NativeStreamControl::Pause)
        .unwrap();
    assert!(!accepted_snapshot(&bridge, &control, 5).exists);
    assert!(bridge.snapshot(9, false).unwrap().exists);
    assert!(bridge.snapshot(14, false).unwrap().exists);
    assert_eq!(bridge.channel_generations[5].load(Ordering::Acquire), 0);
    assert_eq!(bridge.channel_generations[9].load(Ordering::Acquire), 0);
    assert_eq!(bridge.channel_generations[14].load(Ordering::Acquire), 0);
    control.finish(&mut bridge);
}

#[test]
fn stateful_native_stretch_reaches_existing_pcm_bus_and_preserves_native_byte_counter() {
    use crate::audio::native_stretch::NativeMono175;
    let state = NativeMono175::new(44100, 1.5, true, 44100, 8192).unwrap();
    let mut source = constant(0.5, 44100);
    source.sample_rate = 44100;
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(
            5,
            source
                .with_native_channels(1)
                .with_native_format(44100, 8192)
                .with_native_stretch(state),
            true,
            256,
        )
        .unwrap();
    control.wait_for_write();
    assert!(
        control
            .latest_pcm()
            .iter()
            .all(|sample| *sample == 16383.0 / 32768.0)
    );
    let snapshot = accepted_snapshot(&bridge, &control, 5);
    assert_eq!(snapshot.position_ms, None);
    assert_eq!(snapshot.marker.accepted_output_frames, CHUNK_FRAMES as u64);
    assert_eq!(snapshot.decoded_bytes, 8192);
    assert_eq!(snapshot.levels, [0; 2]);
    control.wait_for_write();
    bridge
        .control_channel(5, NativeStreamControl::Volume(128))
        .unwrap();
    let second = accepted_snapshot(&bridge, &control, 5);
    assert_eq!(second.levels, [16383; 2]);
    control.wait_for_write();
    assert!(
        control
            .latest_pcm()
            .iter()
            .all(|sample| *sample == 8191.0 / 32768.0)
    );
    bridge
        .control_channel(5, NativeStreamControl::Pause)
        .unwrap();
    let paused = accepted_snapshot(&bridge, &control, 5);
    assert!(paused.exists && !paused.playing);
    bridge
        .control_channel(5, NativeStreamControl::Start { looping: true })
        .unwrap();
    control.wait_for_write();
    assert!(
        control
            .latest_pcm()
            .iter()
            .all(|sample| *sample == 8191.0 / 32768.0)
    );
    bridge
        .control_channel(5, NativeStreamControl::Dispose)
        .unwrap();
    let disposed = accepted_snapshot(&bridge, &control, 5);
    assert!(!disposed.exists);
    control.finish(&mut bridge);
}

#[test]
fn stopping_at_partial_known_prefix_keeps_other_channel_pcm_cursor_exact() {
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(
            5,
            constant(0.25, CHUNK_FRAMES + 4).with_indeterminate_tail(),
            true,
            256,
        )
        .unwrap();
    control.wait_for_write();
    let other = (0..CHUNK_FRAMES * 4).map(|frame| (frame / CHUNK_FRAMES + 1) as f32 / 8.0);
    bridge.configure_voice(6, clip(other), true, 256).unwrap();
    assert!(accepted_snapshot(&bridge, &control, 5).playing);
    control.wait_for_write();
    assert_eq!(control.latest_pcm(), vec![0.375; 8]);
    bridge
        .control_channel(5, NativeStreamControl::Dispose)
        .unwrap();
    let disposed = accepted_snapshot(&bridge, &control, 5);
    assert!(!disposed.exists);
    assert_eq!(disposed.marker.accepted_bus_frames, CHUNK_FRAMES as u64 + 4);
    assert_eq!(
        disposed.marker.accepted_end_frame,
        Some(CHUNK_FRAMES as u64 + 4)
    );
    control.wait_for_write();
    let pcm = control.latest_pcm();
    assert!(
        pcm[..(CHUNK_FRAMES - 4) * 2]
            .iter()
            .all(|sample| *sample == 0.125)
    );
    assert!(
        pcm[(CHUNK_FRAMES - 4) * 2..]
            .iter()
            .all(|sample| *sample == 0.25)
    );
    bridge
        .control_channel(6, NativeStreamControl::Dispose)
        .unwrap();
    let other = accepted_snapshot(&bridge, &control, 6);
    assert!(!other.exists);
    assert_eq!(other.marker.accepted_start_frame, Some(CHUNK_FRAMES as u64));
    assert_eq!(
        other.marker.accepted_generation_frames,
        CHUNK_FRAMES as u64 + 4
    );
    assert_eq!(
        other.marker.accepted_end_frame,
        Some(CHUNK_FRAMES as u64 * 2 + 4)
    );
    assert_eq!(
        other.marker.accepted_bus_frames,
        CHUNK_FRAMES as u64 * 2 + 4
    );
    control.finish(&mut bridge);
}

#[test]
fn noop_codec_restart_at_eos_does_not_seek_even_when_looping_changes() {
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(
            5,
            constant(0.5, 4).with_native_restart_rewinds(false),
            false,
            256,
        )
        .unwrap();
    control.wait_for_write();
    assert!(!accepted_snapshot(&bridge, &control, 5).playing);
    bridge
        .control_channel(5, NativeStreamControl::Restart { looping: true })
        .unwrap();
    control.wait_for_write();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.0));
    let ended = accepted_snapshot(&bridge, &control, 5);
    assert!(ended.exists && !ended.playing);
    assert_eq!(ended.decoded_bytes, 16);
    control.finish(&mut bridge);
}

#[test]
fn completed_stretcher_is_retired_and_rewinding_codec_restarts_raw_decoder() {
    use crate::audio::native_stretch::NativeMono175;
    let state = NativeMono175::new(22050, 2.0, false, 44100, 8192).unwrap();
    let mut source = constant(32767.0 / 32768.0, 1);
    source.sample_rate = 22050;
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(
            5,
            source.with_native_channels(1).with_native_stretch(state),
            false,
            256,
        )
        .unwrap();
    control.wait_for_write();
    assert_eq!(control.latest_pcm()[0], 32766.0 / 32768.0);
    bridge
        .control_channel(5, NativeStreamControl::Pause)
        .unwrap();
    assert!(!accepted_snapshot(&bridge, &control, 5).playing);
    bridge
        .control_channel(5, NativeStreamControl::Restart { looping: false })
        .unwrap();
    control.wait_for_write();
    assert_eq!(control.latest_pcm()[0], 32767.0 / 32768.0);
    let ended = accepted_snapshot(&bridge, &control, 5);
    assert!(ended.exists && !ended.playing);
    control.finish(&mut bridge);
}

#[test]
fn native_mono_22050_to_44100_holds_samples_before_host_resampling() {
    let mut source = clip([0.5, -0.5]);
    source.sample_rate = 22050;
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(
            5,
            source
                .with_native_channels(1)
                .with_native_format(44100, 8192),
            true,
            256,
        )
        .unwrap();
    control.wait_for_write();
    let pcm = control.latest_pcm();
    assert_eq!(&pcm[..4], &[0.5; 4]);
    assert_eq!(&pcm[6..8], &[-0.5; 2]);
    bridge
        .control_channel(5, NativeStreamControl::Pause)
        .unwrap();
    let paused = accepted_snapshot(&bridge, &control, 5);
    assert_eq!(paused.decoded_bytes, 4096);
    control.finish(&mut bridge);
}

#[test]
fn native_stereo_44100_to_22050_discards_odd_frames_instead_of_interpolating() {
    let mut source = clip([]);
    source.sample_rate = 44100;
    source.frames = Arc::new(vec![0.125, 0.25, 0.75, -0.75]);
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(5, source.with_native_format(22050, 3072), true, 256)
        .unwrap();
    control.wait_for_write();
    assert!(
        control
            .latest_pcm()
            .as_chunks::<2>()
            .0
            .iter()
            .all(|frame| *frame == [0.125, 0.25])
    );
    bridge
        .control_channel(5, NativeStreamControl::Pause)
        .unwrap();
    let paused = accepted_snapshot(&bridge, &control, 5);
    assert_eq!(paused.decoded_bytes, 12288);
    control.finish(&mut bridge);
}

#[test]
fn native_raw_final_buffer_is_initialized_silence_not_an_unknown_stretch_tail() {
    let mut source = constant(0.5, 1);
    source.sample_rate = 44100;
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(
            5,
            source
                .with_native_channels(1)
                .with_native_restart_rewinds(false)
                .with_native_format(44100, 8192),
            false,
            256,
        )
        .unwrap();
    control.wait_for_write();
    let pcm = control.latest_pcm();
    assert_eq!(&pcm[..2], &[0.5; 2]);
    assert!(pcm[4..].iter().all(|sample| *sample == 0.0));
    let ended = accepted_snapshot(&bridge, &control, 5);
    assert!(ended.exists && !ended.playing);
    assert_eq!(ended.decoded_bytes, 2);
    bridge
        .control_channel(5, NativeStreamControl::Restart { looping: true })
        .unwrap();
    control.wait_for_write();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.0));
    assert!(!accepted_snapshot(&bridge, &control, 5).playing);
    control.finish(&mut bridge);
}

#[test]
fn oversized_initial_gain_is_zero_but_oversized_live_gain_retains_previous_pcm_level() {
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(5, constant(0.5, CHUNK_FRAMES), true, 512)
        .unwrap();
    control.wait_for_write();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.0));
    let mut gains = [None; 15];
    gains[5] = Some(128);
    bridge.apply_native_gains(None, gains).unwrap();
    control.accept_and_wait();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.25));
    gains[5] = Some(512);
    bridge.apply_native_gains(None, gains).unwrap();
    control.accept_and_wait();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.25));
    control.finish(&mut bridge);
}

#[test]
fn native_bgm_intro_joins_loop_inside_one_decoder_notification_without_zero_padding() {
    let mut intro = constant(0.5, 1);
    intro.sample_rate = 44100;
    let mut continuation = constant(0.25, 1);
    continuation.sample_rate = 44100;
    let (mut bridge, control) = OutputControl::new();
    bridge
        .play_crossfade(
            intro
                .with_native_channels(1)
                .with_native_format(44100, 8192),
            Some(
                continuation
                    .with_native_channels(1)
                    .with_native_format(44100, 8192),
            ),
            0,
            true,
            256,
        )
        .unwrap();
    control.wait_for_write();
    let pcm = control.latest_pcm();
    assert_eq!(&pcm[..2], &[0.5; 2]);
    assert!(pcm[4..].iter().all(|sample| *sample == 0.25));
    control.accept_and_wait();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.25));
    control.finish(&mut bridge);
}

#[test]
fn accepted_eos_markers_keep_exact_source_end_after_the_object_is_disposed() {
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(5, constant(0.5, 4), false, 256)
        .unwrap();
    control.wait_for_write();
    assert_eq!(&control.latest_pcm()[..8], &[0.5; 8]);
    let ended = accepted_snapshot(&bridge, &control, 5);
    assert!(ended.exists && !ended.playing);
    let marker = ended.marker;
    assert_ne!(marker.generation, 0);
    assert_eq!(marker.accepted_bus_frames, CHUNK_FRAMES as u64);
    assert_eq!(marker.accepted_start_frame, Some(0));
    assert_eq!(marker.accepted_end_frame, Some(4));
    assert_eq!(marker.accepted_generation_frames, 4);
    assert_eq!(marker.generation_output_frame_base, 0);
    assert_eq!(marker.accepted_output_frames, 4);
    assert_eq!(f64::from_bits(marker.decoder_source_frame_bits), 4.0);
    assert_eq!(
        f64::from_bits(marker.generation_decoder_source_frame_bits),
        0.0
    );
    assert_eq!(marker.original_source_rate, OUTPUT_RATE);
    assert_eq!(marker.input_sample_rate, OUTPUT_RATE);
    assert!(!marker.looping);
    bridge
        .control_channel(5, NativeStreamControl::Dispose)
        .unwrap();
    let disposed = bridge.snapshot(5, false).unwrap();
    assert!(!disposed.exists);
    assert_eq!(disposed.marker, marker);
    control.finish(&mut bridge);
}

#[test]
fn start_and_restart_get_new_accepted_segments_and_preserve_distinct_seek_anchors() {
    let (mut bridge, control) = OutputControl::new();
    let source = (0..CHUNK_FRAMES * 8).map(|frame| (frame / CHUNK_FRAMES + 1) as f32 / 8.0);
    bridge.configure_voice(5, clip(source), true, 256).unwrap();
    control.wait_for_write();
    bridge
        .control_channel(5, NativeStreamControl::Pause)
        .unwrap();
    let first = accepted_snapshot(&bridge, &control, 5);
    assert_eq!(first.marker.accepted_start_frame, Some(0));
    assert_eq!(first.marker.accepted_end_frame, None);
    assert_eq!(first.marker.accepted_generation_frames, CHUNK_FRAMES as u64);
    bridge
        .control_channel(5, NativeStreamControl::Restart { looping: true })
        .unwrap();
    control.wait_for_write();
    assert_eq!(control.latest_pcm()[0], 0.125);
    bridge
        .control_channel(5, NativeStreamControl::Pause)
        .unwrap();
    let restarted = accepted_snapshot(&bridge, &control, 5);
    assert_ne!(restarted.marker.generation, first.marker.generation);
    assert_eq!(
        restarted.marker.accepted_start_frame,
        Some(CHUNK_FRAMES as u64)
    );
    assert_eq!(
        restarted.marker.accepted_generation_frames,
        CHUNK_FRAMES as u64
    );
    assert_eq!(
        restarted.marker.generation_output_frame_base,
        CHUNK_FRAMES as u64
    );
    assert_eq!(
        restarted.marker.accepted_output_frames,
        CHUNK_FRAMES as u64 * 2
    );
    assert_eq!(
        f64::from_bits(restarted.marker.generation_decoder_source_frame_bits),
        0.0
    );
    assert_eq!(restarted.decoded_bytes, CHUNK_FRAMES as u32 * 8);
    bridge
        .control_channel(5, NativeStreamControl::Start { looping: true })
        .unwrap();
    control.wait_for_write();
    assert_eq!(control.latest_pcm()[0], 0.25);
    bridge
        .control_channel(5, NativeStreamControl::Pause)
        .unwrap();
    let resumed = accepted_snapshot(&bridge, &control, 5);
    assert_ne!(resumed.marker.generation, restarted.marker.generation);
    assert_eq!(
        resumed.marker.accepted_start_frame,
        Some(CHUNK_FRAMES as u64 * 2)
    );
    assert_eq!(
        resumed.marker.accepted_generation_frames,
        CHUNK_FRAMES as u64
    );
    assert_eq!(
        resumed.marker.generation_output_frame_base,
        CHUNK_FRAMES as u64 * 2
    );
    assert_eq!(
        resumed.marker.accepted_output_frames,
        CHUNK_FRAMES as u64 * 3
    );
    assert_eq!(
        f64::from_bits(resumed.marker.generation_decoder_source_frame_bits),
        CHUNK_FRAMES as f64
    );
    assert_eq!(resumed.marker.accepted_bus_frames, CHUNK_FRAMES as u64 * 3);
    assert_eq!(resumed.decoded_bytes, CHUNK_FRAMES as u32 * 12);
    control.finish(&mut bridge);
}

#[test]
fn native_level_ring_uses_device_notifications_not_host_50ms_chunks() {
    for (device_rate, chunk_bytes, source_chunk_frames, third_level) in
        [(44100, 8192, 2048, 8192), (22050, 3072, 1536, 12288)]
    {
        let values = (0..source_chunk_frames * 8)
            .map(|frame| (frame / source_chunk_frames + 1) as f32 / 8.0);
        let mut source = clip(values);
        source.sample_rate = 44100;
        let (mut bridge, control) = OutputControl::new();
        bridge
            .configure_voice(
                5,
                source
                    .with_native_channels(1)
                    .with_native_format(device_rate, chunk_bytes),
                true,
                0,
            )
            .unwrap();
        control.wait_for_write();
        assert!(control.latest_pcm().iter().all(|sample| *sample == 0.0));
        let first = accepted_snapshot(&bridge, &control, 5);
        assert_eq!(first.levels, [0; 2]);
        assert_eq!(first.marker.original_source_rate, 44100);
        assert_eq!(first.marker.input_sample_rate, device_rate);
        control.wait_for_write();
        let second = accepted_snapshot(&bridge, &control, 5);
        assert_eq!(second.levels, [4096; 2]);
        control.wait_for_write();
        let third = accepted_snapshot(&bridge, &control, 5);
        assert_eq!(third.levels, [third_level; 2]);
        control.wait_for_write();
        bridge
            .control_channel(5, NativeStreamControl::Pause)
            .unwrap();
        assert_eq!(accepted_snapshot(&bridge, &control, 5).levels, [0; 2]);
        control.finish(&mut bridge);
    }
}

fn observed_snapshot(
    bridge: &AudioBridge,
    control: &OutputControl,
    channel: u8,
) -> NativeStreamSnapshot {
    let (reply, result) = mpsc::channel();
    bridge
        .submit(Command::StreamSnapshot {
            channel,
            observe_position: true,
            reply,
        })
        .unwrap();
    assert!(matches!(result.try_recv(), Err(TryRecvError::Empty)));
    control.steps.send(OutputStep::Accept).unwrap();
    result.recv_timeout(Duration::from_secs(5)).unwrap()
}

#[test]
fn accepted_voice_backlog_remains_pending_until_explicit_test_consumption_reaches_its_end() {
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(8, constant(0.5, 8), false, 256)
        .unwrap();
    control.wait_for_write();
    assert_eq!(&control.latest_pcm()[..16], &[0.5; 16]);
    let ended = accepted_snapshot(&bridge, &control, 8);
    assert!(!ended.playing);
    assert_eq!(ended.position_ms, None);
    assert_eq!(control.accepted_frames(), CHUNK_FRAMES as u64);
    assert_eq!(control.played_frames(), 0);
    for _ in 0..70 {
        assert_ne!(bridge.playing_channels().unwrap() & (1 << 8), 0);
    }
    control.consume_to(7);
    assert_ne!(bridge.playing_channels().unwrap() & (1 << 8), 0);
    control.consume_to(8);
    assert_eq!(bridge.playing_channels().unwrap() & (1 << 8), 0);
    assert_eq!(control.accepted_frames(), CHUNK_FRAMES as u64);
    control.finish(&mut bridge);
}

#[test]
fn observed_position_tracks_consumed_pcm_and_excludes_pause_gaps_and_other_channels() {
    let (mut bridge, control) = OutputControl::new();
    bridge
        .configure_voice(5, constant(0.5, CHUNK_FRAMES * 12), true, 256)
        .unwrap();
    control.wait_for_write();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.5));
    assert_eq!(accepted_snapshot(&bridge, &control, 5).position_ms, None);
    control.wait_for_write();
    control.consume_to(1200);
    let first = observed_snapshot(&bridge, &control, 5);
    assert_eq!(first.position_ms, Some(25));
    assert_eq!(control.accepted_frames(), 4800);
    control.wait_for_write();
    control.consume_to(3600);
    let next = observed_snapshot(&bridge, &control, 5);
    assert_eq!(next.position_ms, Some(75));
    assert_eq!(control.accepted_frames(), 7200);
    control.wait_for_write();
    bridge
        .control_channel(5, NativeStreamControl::Pause)
        .unwrap();
    bridge
        .configure_voice(6, constant(0.5, CHUNK_FRAMES), true, 256)
        .unwrap();
    assert_eq!(accepted_snapshot(&bridge, &control, 5).position_ms, None);
    control.wait_for_write();
    let _ = accepted_snapshot(&bridge, &control, 5);
    control.wait_for_write();
    control.consume_to(12000);
    bridge
        .control_channel(5, NativeStreamControl::Start { looping: true })
        .unwrap();
    let resumed = observed_snapshot(&bridge, &control, 5);
    assert_eq!(resumed.position_ms, Some(200));
    control.wait_for_write();
    assert!(
        control
            .latest_pcm()
            .iter()
            .all(|sample| *sample == 32767.0 / 32768.0)
    );
    let started = accepted_snapshot(&bridge, &control, 5);
    assert_eq!(started.marker.accepted_start_frame, Some(14400));
    control.wait_for_write();
    control.consume_to(15600);
    assert_eq!(
        observed_snapshot(&bridge, &control, 5).position_ms,
        Some(225)
    );
    control.wait_for_write();
    bridge
        .control_channel(5, NativeStreamControl::Pause)
        .unwrap();
    bridge
        .control_channel(6, NativeStreamControl::Dispose)
        .unwrap();
    assert_eq!(accepted_snapshot(&bridge, &control, 5).position_ms, None);
    control.finish(&mut bridge);
}

#[test]
fn native_repeat_pcm_switches_only_on_a_real_loop_and_preserves_first_pass_eof() {
    for looping in [false, true] {
        let mut source = constant(0.5, 2).with_native_channels(1);
        source.sample_rate = 44100;
        let repeat = kira::sound::static_sound::StaticSoundData {
            sample_rate: 44100,
            frames: vec![kira::Frame::new(-0.25, -0.25); 2].into(),
            settings: Default::default(),
            slice: None,
        };
        let mut source = source
            .with_native_repeat(Some(repeat))
            .with_native_format(44100, 8192);
        let mut pcm = source.native_pcm.take().unwrap();
        pcm.start(looping);
        let mut frames = Vec::new();
        let mut position = 0.0;
        assert!(!pcm.refill(&mut frames, &mut position));
        assert_eq!(&frames[..4], &[0.5; 4]);
        if looping {
            assert!(frames[4..].iter().all(|sample| *sample == -0.25));
            assert!(!pcm.finished);
        } else {
            assert!(frames[4..].iter().all(|sample| *sample == 0.0));
            assert!(pcm.finished);
            assert_eq!(pcm.decoded_bytes, 4);
        }
    }
}
