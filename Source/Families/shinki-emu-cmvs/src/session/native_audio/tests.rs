use super::*;
use crate::session::audio::tests::{OutputControl, OutputStep};
use crate::system_save::slot_globals::NativeGlobals;

const FORMAT: [u32; 2] = [44100, 8192];

fn wave(root: &std::path::Path, name: &str, channels: u16, frames: usize) -> u32 {
    let size = frames as u32 * u32::from(channels) * 2;
    let mut bytes = Vec::with_capacity(44 + size as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&44100u32.to_le_bytes());
    bytes.extend_from_slice(&(44100 * u32::from(channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for _ in 0..frames * usize::from(channels) {
        bytes.extend_from_slice(&16384i16.to_le_bytes());
    }
    std::fs::write(root.join(name), &bytes).unwrap();
    bytes.len() as u32
}

fn fixture() -> (tempfile::TempDir, CmvsArchive, CmvsPs2aVmState) {
    let (root, archive, mut vm) = crate::scene::parent_load_tests::fixture();
    for bank in [
        CmvsResourceSearchBank::Native0d14,
        CmvsResourceSearchBank::Native0d0c,
    ] {
        vm.resource_search_paths
            .insert(bank, BTreeMap::from([(0, String::new())]));
    }
    vm.process_string_slots.insert(0, b"voice.wav".to_vec());
    (root, archive, vm)
}

fn command(
    archive: &CmvsArchive,
    vm: &mut CmvsPs2aVmState,
    bridge: &mut audio::AudioBridge,
    id: u16,
    words: &[u32],
) {
    vm.dispatch_stopped = false;
    if let Some(action) = crate::scene::parent_load_tests::command(vm, id, words) {
        execute(archive, vm, bridge, FORMAT, action).unwrap();
    }
}

fn repeat(vm: &mut CmvsPs2aVmState, id: u16) {
    vm.dispatch_stopped = false;
    let pc = vm.program_counter;
    crate::execute_cmvs390_frame(
        vm,
        &CmvsPs2aInstructionFrame::Command {
            span: CmvsPs2aSourceSpan {
                offset: pc,
                byte_length: 2,
            },
            command_id: id,
        },
    )
    .unwrap();
}

fn read_register(
    vm: &mut CmvsPs2aVmState,
    register: u32,
) -> Result<u32, shinki_emu_sdk::CoreError> {
    vm.dispatch_stopped = false;
    let pc = vm.program_counter;
    crate::execute_cmvs390_frame(
        vm,
        &CmvsPs2aInstructionFrame::StackExpression(CmvsPs2aStackExpression {
            span: CmvsPs2aSourceSpan {
                offset: pc,
                byte_length: 10,
            },
            nodes: vec![CmvsPs2aStackExpressionNode::Value {
                span: CmvsPs2aSourceSpan {
                    offset: pc + 2,
                    byte_length: 6,
                },
                opcode: 0x10f,
                payload: register,
            }],
        }),
    )?;
    Ok(vm.current_value.unwrap())
}

#[test]
fn ordinary_voice_load_consumes_all_words_and_emits_native_linear_pcm_and_wav_extent() {
    let (root, archive, mut vm) = fixture();
    let bytes = wave(root.path(), "voice.wav", 1, 44100);
    let (mut bridge, control) = OutputControl::new();
    command(
        &archive,
        &mut vm,
        &mut bridge,
        175,
        &[3, 0x8000_0000, 128, 1, 1.0f32.to_bits()],
    );
    assert_eq!(vm.stack_cursor_bytes, 0);
    assert_eq!(read_register(&mut vm.clone(), 0).unwrap(), 1);
    assert_eq!(read_register(&mut vm.clone(), 4).unwrap(), bytes);
    control.wait_for_write();
    assert!(
        control
            .latest_pcm()
            .iter()
            .all(|sample| (*sample - 0.25).abs() < 0.0001)
    );
    command(&archive, &mut vm, &mut bridge, 168, &[3]);
    control.steps.send(OutputStep::Accept).unwrap();
    command(&archive, &mut vm, &mut bridge, 170, &[3]);
    assert_eq!(vm.interpreter_words[&81220], u32::MAX);
    control.finish(&mut bridge);
}

#[test]
fn native_voice_asset_failure_keeps_result_usable_but_extent_unknown_until_real_register_write() {
    let (_root, archive, mut vm) = fixture();
    let (mut bridge, control) = OutputControl::new();
    vm.write_interpreter_word(81236, 0x1234_5678);
    command(
        &archive,
        &mut vm,
        &mut bridge,
        175,
        &[0, 0x8000_0000, 256, 0, 1.0f32.to_bits()],
    );
    assert_eq!(read_register(&mut vm.clone(), 0).unwrap(), 0);
    assert!(vm.native_audio_extent_indeterminate);
    assert!(!vm.interpreter_words.contains_key(&81236));
    assert_eq!(
        read_register(&mut vm.clone(), 4).unwrap_err().code(),
        "SHINKI_EMU_CMVS_NATIVE_AUDIO_EXTENT_INDETERMINATE"
    );
    let encoded = postcard::to_allocvec(&vm).unwrap();
    let restored: CmvsPs2aVmState = postcard::from_bytes(&encoded).unwrap();
    assert_eq!(read_register(&mut restored.clone(), 0).unwrap(), 0);
    assert_eq!(
        read_register(&mut restored.clone(), 4).unwrap_err().code(),
        "SHINKI_EMU_CMVS_NATIVE_AUDIO_EXTENT_INDETERMINATE"
    );
    crate::session::test_support::write_program(_root.path(), "start.ps3", 0);
    vm.resource_search_paths.insert(
        CmvsResourceSearchBank::Script,
        BTreeMap::from([(0, String::new())]),
    );
    let script = archive.load_native_script("start.ps3", 0, &mut vm).unwrap();
    let image = crate::vm::native_slot::capture_native_program(&script, &vm).unwrap();
    assert_eq!(
        crate::vm::native_slot::capture_native_interpreter(&vm, &image)
            .unwrap_err()
            .code(),
        "SHINKI_EMU_CMVS_NATIVE_AUDIO_EXTENT_INDETERMINATE"
    );
    command(&archive, &mut vm, &mut bridge, 174, &[0]);
    assert_eq!(read_register(&mut vm.clone(), 4).unwrap(), 0);
    command(
        &archive,
        &mut vm,
        &mut bridge,
        175,
        &[0, 0x8000_0000, 256, 0, 1.0f32.to_bits()],
    );
    command(&archive, &mut vm, &mut bridge, 416, &[]);
    assert_eq!(read_register(&mut vm.clone(), 4).unwrap(), 0);
    control.finish(&mut bridge);
}

#[test]
fn disabled_voice_preserves_extent_and_disabled_resource_still_captures_all_descriptor_words() {
    let (_root, archive, mut vm) = fixture();
    let (mut bridge, control) = OutputControl::new();
    vm.write_interpreter_word(1552, 0);
    vm.write_interpreter_word(1556, 0);
    vm.write_interpreter_word(81236, 77);
    command(
        &archive,
        &mut vm,
        &mut bridge,
        175,
        &[0, 0x8000_0000, 256, 1, 2.0f32.to_bits()],
    );
    assert_eq!(read_register(&mut vm.clone(), 0).unwrap(), 0);
    assert_eq!(read_register(&mut vm.clone(), 4).unwrap(), 77);
    vm.write_interpreter_word(81220, 91);
    command(
        &archive,
        &mut vm,
        &mut bridge,
        186,
        &[5, 0x8000_0000, 128, 300, 1, 1],
    );
    assert_eq!(vm.stack_cursor_bytes, 0);
    assert_eq!(vm.interpreter_words[&81220], 91);
    let mut globals = NativeGlobals::default();
    native_save_audio::capture(Some(&bridge), &vm, &mut globals).unwrap();
    assert_eq!(globals.resource_channels.slots[5].name, "voice.wav");
    assert_eq!(globals.resource_channels.slots[5].volume, 128);
    assert_eq!(globals.resource_channels.slots[5].loop_playback, 1);
    assert_eq!(globals.resource_channels.slots[5].restart_mode, 1);
    assert!(!bridge.snapshot(14, false).unwrap().exists);
    control.finish(&mut bridge);
}

#[test]
fn six_word_resource_load_pause_raw_controls_capture_and_dispose_use_actual_pcm() {
    let (root, archive, mut vm) = fixture();
    wave(root.path(), "voice.wav", 2, 44100 * 2);
    let (mut bridge, control) = OutputControl::new();
    command(
        &archive,
        &mut vm,
        &mut bridge,
        186,
        &[5, 0x8000_0000, 256, 0, 1, 1],
    );
    control.wait_for_write();
    assert_eq!(vm.stack_cursor_bytes, 0);
    assert!(
        control
            .latest_pcm()
            .iter()
            .all(|sample| (*sample - 0.5).abs() < 0.0001)
    );
    vm.process_string_slots.insert(0, b"unrelated.wav".to_vec());
    command(&archive, &mut vm, &mut bridge, 184, &[5, 128]);
    command(&archive, &mut vm, &mut bridge, 185, &[5, 128]);
    control.accept_and_wait();
    for frame in control.latest_pcm().as_chunks::<2>().0 {
        assert!((frame[0] - 0.125).abs() < 0.0001);
        assert!((frame[1] - 0.25).abs() < 0.0001);
    }
    command(&archive, &mut vm, &mut bridge, 178, &[5]);
    control.steps.send(OutputStep::Accept).unwrap();
    command(&archive, &mut vm, &mut bridge, 183, &[5]);
    assert_eq!(vm.interpreter_words[&81220], u32::MAX);
    let mut globals = NativeGlobals::default();
    native_save_audio::capture(Some(&bridge), &vm, &mut globals).unwrap();
    let record = &globals.resource_channels.slots[5];
    assert_eq!(
        (
            record.name.as_str(),
            record.volume,
            record.channel_volume,
            record.stereo_pan,
            record.restart_mode
        ),
        ("voice.wav", 256, 128, 128, 0)
    );
    command(&archive, &mut vm, &mut bridge, 177, &[5, 1]);
    control.wait_for_write();
    command(&archive, &mut vm, &mut bridge, 179, &[5]);
    control.steps.send(OutputStep::Accept).unwrap();
    assert!(!bridge.snapshot(14, false).unwrap().exists);
    native_save_audio::capture(Some(&bridge), &vm, &mut globals).unwrap();
    assert_eq!(globals.resource_channels.slots[5].name, "");
    assert_eq!(globals.resource_channels.slots[5].restart_mode, 0);
    control.finish(&mut bridge);
}

#[test]
fn real_voice_generations_hold_339_and_344_until_consumed_end_then_native_postroll() {
    for id in [339, 344] {
        let (root, archive, mut vm) = fixture();
        wave(root.path(), "voice.wav", 1, 8);
        let (mut bridge, control) = OutputControl::new();
        let _ = crate::scene::parent_load_tests::command(
            &mut vm,
            349,
            &[0, 0, 0, 0, 640, 180, 1280, 720],
        );
        vm.write_interpreter_word(1500, 1);
        vm.native_frame_delta_ms = 100;
        command(
            &archive,
            &mut vm,
            &mut bridge,
            175,
            &[3, 0x8000_0000, 256, 0, 1.0f32.to_bits()],
        );
        control.wait_for_write();
        vm.native_audio_playing = bridge.playing_channels().unwrap();
        let words = if id == 339 {
            [0, u32::MAX, 3]
        } else {
            [0, 0, 3]
        };
        let _ = crate::scene::parent_load_tests::command(&mut vm, id, &words);
        let parked = (vm.program_counter, vm.stack_cursor_bytes);
        for _ in 0..70 {
            repeat(&mut vm, id);
            assert_eq!((vm.program_counter, vm.stack_cursor_bytes), parked);
        }
        control.steps.send(OutputStep::Accept).unwrap();
        assert!(!bridge.snapshot(8, false).unwrap().playing);
        vm.native_audio_playing = bridge.playing_channels().unwrap();
        assert_ne!(vm.native_audio_playing & (1 << 8), 0);
        assert_eq!(control.played_frames(), 0);
        assert!(control.accepted_frames() > 0);
        for _ in 0..70 {
            repeat(&mut vm, id);
            assert_eq!((vm.program_counter, vm.stack_cursor_bytes), parked);
        }
        control.consume_all();
        vm.native_audio_playing = bridge.playing_channels().unwrap();
        for _ in 0..2 {
            repeat(&mut vm, id);
            assert_eq!((vm.program_counter, vm.stack_cursor_bytes), parked);
        }
        repeat(&mut vm, id);
        assert_eq!(
            (vm.program_counter, vm.stack_cursor_bytes),
            (parked.0 + 2, 0)
        );
        control.finish(&mut bridge);
    }
}

#[test]
fn resource_source_failure_is_native_false_and_retains_descriptor_while_bridge_failure_is_fatal() {
    let (_root, archive, mut vm) = fixture();
    let (mut bridge, control) = OutputControl::new();
    command(
        &archive,
        &mut vm,
        &mut bridge,
        186,
        &[0, 0x8000_0000, 128, 0, 1, 1],
    );
    assert_eq!(read_register(&mut vm.clone(), 0).unwrap(), 0);
    let mut globals = NativeGlobals::default();
    native_save_audio::capture(Some(&bridge), &vm, &mut globals).unwrap();
    assert_eq!(globals.resource_channels.slots[0].name, "voice.wav");
    bridge.close().unwrap();
    vm.dispatch_stopped = false;
    let action =
        crate::scene::parent_load_tests::command(&mut vm, 186, &[0, 0x8000_0000, 128, 0, 1, 1])
            .unwrap();
    assert_eq!(
        execute(&archive, &mut vm, &mut bridge, FORMAT, action)
            .unwrap_err()
            .code
            .as_str(),
        "SHINKI_EMU_CMVS_AUDIO_WORKER"
    );
    drop(control);
}

#[test]
fn resource_fadeout_clears_only_name_and_disposal_poll_waits_for_retained_object_not_play_flag() {
    let (root, archive, mut vm) = fixture();
    wave(root.path(), "voice.wav", 1, 8);
    let (mut bridge, control) = OutputControl::new();
    command(
        &archive,
        &mut vm,
        &mut bridge,
        175,
        &[0, 0x8000_0000, 256, 0, 1.0f32.to_bits()],
    );
    control.wait_for_write();
    control.steps.send(OutputStep::Accept).unwrap();
    let ended = bridge.snapshot(5, false).unwrap();
    assert!(ended.exists && !ended.playing);
    vm.native_audio_present = u16::from(ended.exists) << 5;
    let _ = crate::scene::parent_load_tests::command(&mut vm, 189, &[0]);
    let parked = vm.program_counter;
    assert_eq!(vm.stack_cursor_bytes, 4);
    repeat(&mut vm, 189);
    assert_eq!(vm.program_counter, parked);
    bridge
        .control_channel(5, audio::NativeStreamControl::Dispose)
        .unwrap();
    vm.native_audio_present = u16::from(bridge.snapshot(5, false).unwrap().exists) << 5;
    repeat(&mut vm, 189);
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (parked + 2, 0));
    command(
        &archive,
        &mut vm,
        &mut bridge,
        186,
        &[0, 0x8000_0000, 128, 0, 1, 1],
    );
    control.wait_for_write();
    command(&archive, &mut vm, &mut bridge, 182, &[0, 0]);
    assert_eq!(vm.stack_cursor_bytes, 0);
    let mut globals = NativeGlobals::default();
    native_save_audio::capture(Some(&bridge), &vm, &mut globals).unwrap();
    assert_eq!(globals.resource_channels.slots[0].name, "");
    assert_eq!(globals.resource_channels.slots[0].restart_mode, 1);
    control.steps.send(OutputStep::Accept).unwrap();
    assert!(!bridge.snapshot(9, false).unwrap().exists);
    control.finish(&mut bridge);
}

#[test]
fn ordinary175_activates_real_mono_stretch_but_stereo_nan_and_replaced_rates_keep_raw_pcm() {
    for (channels, rate, expected) in [
        (1, 2.0f32, 16383.0 / 32768.0),
        (2, 2.0, 0.5),
        (1, f32::NAN, 0.5),
        (1, 4.0, 0.5),
    ] {
        let (root, archive, mut vm) = fixture();
        wave(root.path(), "voice.wav", channels, 44100);
        let (mut bridge, control) = OutputControl::new();
        command(
            &archive,
            &mut vm,
            &mut bridge,
            175,
            &[2, 0x8000_0000, 256, 1, rate.to_bits()],
        );
        control.wait_for_write();
        assert!(
            control
                .latest_pcm()
                .iter()
                .all(|sample| (*sample - expected).abs() < 0.00001)
        );
        command(&archive, &mut vm, &mut bridge, 168, &[2]);
        control.steps.send(OutputStep::Accept).unwrap();
        assert!(!bridge.snapshot(7, false).unwrap().exists);
        control.finish(&mut bridge);
    }
}

#[test]
fn ordinary_audio_uses_native_filename_decoder_not_generic_autodetection() {
    let (root, archive, mut vm) = fixture();
    wave(root.path(), "wrong.ogg", 1, 44100);
    vm.process_string_slots.insert(0, b"wrong.ogg".to_vec());
    let (mut bridge, control) = OutputControl::new();
    command(
        &archive,
        &mut vm,
        &mut bridge,
        175,
        &[0, 0x8000_0000, 256, 0, 1.0f32.to_bits()],
    );
    assert_eq!(read_register(&mut vm.clone(), 0).unwrap(), 0);
    assert!(vm.native_audio_extent_indeterminate);
    assert!(!bridge.snapshot(5, false).unwrap().exists);
    control.finish(&mut bridge);
}

#[test]
fn ordinary186_reads_the_full_six_word_native_contract_before_pop() {
    let mut vm = CmvsPs2aVmState::new(0);
    let span = CmvsPs2aSourceSpan {
        offset: 0,
        byte_length: 8,
    };
    crate::execute_cmvs390_frame(
        &mut vm,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span,
            stack_bytes: 4,
            value: 0,
        }),
    )
    .unwrap();
    let pc = vm.program_counter;
    let result = crate::execute_cmvs390_frame(
        &mut vm,
        &CmvsPs2aInstructionFrame::Command {
            span: CmvsPs2aSourceSpan {
                offset: pc,
                byte_length: 2,
            },
            command_id: 186,
        },
    );
    assert!(result.is_err());
    assert_eq!(vm.stack_cursor_bytes, 4);
}

#[test]
fn invalid_disposal_selector_raises_native_mask_and_keeps_a000_operands() {
    let mut vm = CmvsPs2aVmState::new(0);
    let _ = crate::scene::parent_load_tests::command(&mut vm, 189, &[u32::MAX]);
    assert_eq!(vm.native_audio_poll_channel().unwrap(), None);
    assert_eq!(vm.stack_cursor_bytes, 4);
    assert_eq!(vm.interpreter_flag_words.get(&10532), Some(&0x400));
    let pc = vm.program_counter;
    repeat(&mut vm, 189);
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (pc, 4));
}

fn query_with_queued_pcm(
    archive: &CmvsArchive,
    vm: &mut CmvsPs2aVmState,
    bridge: &mut audio::AudioBridge,
    control: &OutputControl,
    id: u16,
) {
    std::thread::scope(|scope| {
        let (done, finished) = std::sync::mpsc::channel();
        scope.spawn(move || {
            command(archive, vm, bridge, id, &[0]);
            done.send(()).unwrap();
        });
        // Only queue acceptance advances here. The test device's independently
        // consumed watermark stays fixed, regardless of command scheduling.
        for _ in 0..32 {
            control.steps.send(OutputStep::Accept).unwrap();
            match finished.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(()) => return,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => control.wait_for_write(),
                Err(cause) => panic!("native query failed: {cause}"),
            }
        }
        panic!("native query did not process its ordered PCM queue");
    });
}

#[test]
fn ordinary170_and183_report_consumed_frames_not_queued_pcm_or_vm_ticks() {
    for resource in [false, true] {
        let (root, archive, mut vm) = fixture();
        wave(root.path(), "voice.wav", 2, 44100);
        let (mut bridge, control) = OutputControl::new();
        if resource {
            command(
                &archive,
                &mut vm,
                &mut bridge,
                186,
                &[0, 0x8000_0000, 256, 0, 1, 1],
            );
        } else {
            command(
                &archive,
                &mut vm,
                &mut bridge,
                175,
                &[0, 0x8000_0000, 256, 1, 1.0f32.to_bits()],
            );
        }
        control.wait_for_write();
        control.accept_and_wait();
        control.consume_to(480);
        assert!(control.accepted_frames() > control.played_frames());
        let query = if resource { 183 } else { 170 };
        query_with_queued_pcm(&archive, &mut vm, &mut bridge, &control, query);
        assert_eq!(vm.interpreter_words[&81220], 10);
        control.consume_to(720);
        query_with_queued_pcm(&archive, &mut vm, &mut bridge, &control, query);
        assert_eq!(vm.interpreter_words[&81220], 15);
        command(
            &archive,
            &mut vm,
            &mut bridge,
            if resource { 179 } else { 168 },
            &[0],
        );
        control.steps.send(OutputStep::Accept).unwrap();
        assert!(
            !bridge
                .snapshot(if resource { 9 } else { 5 }, false)
                .unwrap()
                .exists
        );
        control.finish(&mut bridge);
    }
}

fn custom_adpcm_wave(root: &std::path::Path, name: &str) -> u32 {
    let mut bytes = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    bytes.extend_from_slice(&26u32.to_le_bytes());
    let mut format = [0u8; 26];
    format[..2].copy_from_slice(&0x55u16.to_le_bytes()); // Still490730, not MP3-in-WAV.
    format[2..4].copy_from_slice(&1u16.to_le_bytes());
    format[4..8].copy_from_slice(&44100u32.to_le_bytes());
    format[12..14].copy_from_slice(&56u16.to_le_bytes());
    format[18..20].copy_from_slice(&100u16.to_le_bytes());
    format[20..22].copy_from_slice(&1u16.to_le_bytes());
    format[22..24].copy_from_slice(&256u16.to_le_bytes());
    bytes.extend_from_slice(&format);
    bytes.extend_from_slice(b"fact\x04\0\0\0");
    bytes.extend_from_slice(&22050u32.to_le_bytes()); // Native fact*4 =44100 mono s16 frames.
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(441u32 * 56).to_le_bytes());
    let mut block = [0u8; 56];
    block[1..3].copy_from_slice(&16u16.to_le_bytes());
    block[3..5].copy_from_slice(&16384i16.to_le_bytes());
    block[5..7].copy_from_slice(&16384i16.to_le_bytes());
    for _ in 0..441 {
        bytes.extend_from_slice(&block);
    }
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&(size - 8).to_le_bytes());
    std::fs::write(root.join(name), bytes).unwrap();
    size
}

#[test]
fn ordinary175_custom_non_pcm_consumer_preserves_pcm_gain_rate_and_physical_extent() {
    for (name, rate, volume, expected) in [
        ("voice.wma", 1.0f32, 128, 0.25f32),
        ("voice.wav", 2.0, 256, 16383.0 / 32768.0),
    ] {
        let (root, archive, mut vm) = fixture();
        let size = custom_adpcm_wave(root.path(), name);
        vm.process_string_slots.insert(0, name.as_bytes().to_vec());
        let (mut bridge, control) = OutputControl::new();
        command(
            &archive,
            &mut vm,
            &mut bridge,
            175,
            &[0, 0x8000_0000, volume, 1, rate.to_bits()],
        );
        assert_eq!(read_register(&mut vm.clone(), 0).unwrap(), 1);
        assert_eq!(read_register(&mut vm.clone(), 4).unwrap(), size);
        control.wait_for_write();
        assert!(
            control
                .latest_pcm()
                .iter()
                .all(|sample| (*sample - expected).abs() < 0.00001)
        );
        command(&archive, &mut vm, &mut bridge, 168, &[0]);
        control.steps.send(OutputStep::Accept).unwrap();
        control.finish(&mut bridge);
    }
}

#[test]
fn pcm_selected_names_accept_only_native_wave_consumer_and_keep_source_metadata() {
    let (root, archive, vm) = fixture();
    for name in ["voice.wav", "voice.wma", "voice.mv", "voice.xyz", "voice"] {
        let size = wave(root.path(), name, 1, 32);
        let (clip, extent) = native_save_audio::decode_with_extent(
            &archive,
            &vm,
            name,
            CmvsResourceSearchBank::Native0d14,
        )
        .unwrap();
        let clip = clip.unwrap();
        assert_eq!(clip.native_channels(), 1);
        assert_eq!(clip.source_sample_rate(), 44100);
        assert_eq!(extent, Some(size));
    }
}

#[test]
fn riff_under_mp3_or_mv2_name_reaches_explicit_decoder_boundary_without_a_wav_extent() {
    for (name, expected) in [
        ("false.mp3", "SHINKI_EMU_CMVS_NATIVE_MP3_HEADER"),
        ("false.mv2", "SHINKI_EMU_CMVS_NATIVE_AUDIO_CODEC"),
    ] {
        let (root, archive, mut vm) = fixture();
        wave(root.path(), name, 1, 32);
        vm.process_string_slots.insert(0, name.as_bytes().to_vec());
        let (mut bridge, control) = OutputControl::new();
        let action = crate::scene::parent_load_tests::command(
            &mut vm,
            175,
            &[0, 0x8000_0000, 256, 0, 1.0f32.to_bits()],
        )
        .unwrap();
        assert_eq!(
            execute(&archive, &mut vm, &mut bridge, FORMAT, action)
                .unwrap_err()
                .code(),
            expected
        );
        assert_eq!(read_register(&mut vm.clone(), 0).unwrap(), 1); //48DB70 ignores callback return.
        assert!(vm.native_audio_extent_indeterminate);
        assert!(!bridge.snapshot(5, false).unwrap().exists);
        control.finish(&mut bridge);
    }
}

#[test]
fn wav_non_pcm_failure_preserves_defined_extent_in_the_real175_register_consumer() {
    let (root, archive, mut vm) = fixture();
    let size = wave(root.path(), "float.wma", 1, 32);
    let path = root.path().join("float.wma");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[20..22].copy_from_slice(&3u16.to_le_bytes()); // Generic float decoding is not the native path.
    bytes[28..32].copy_from_slice(&(44100u32 * 4).to_le_bytes());
    bytes[32..34].copy_from_slice(&4u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&32u16.to_le_bytes());
    for sample in bytes[44..].as_chunks_mut::<4>().0 {
        sample.copy_from_slice(&0.5f32.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
    vm.process_string_slots.insert(0, b"float.wma".to_vec());
    let (mut bridge, control) = OutputControl::new();
    let action = crate::scene::parent_load_tests::command(
        &mut vm,
        175,
        &[0, 0x8000_0000, 256, 0, 1.0f32.to_bits()],
    )
    .unwrap();
    assert_eq!(
        execute(&archive, &mut vm, &mut bridge, FORMAT, action)
            .unwrap_err()
            .code(),
        "SHINKI_EMU_CMVS_NATIVE_PCM_BOUNDARY"
    );
    assert_eq!(read_register(&mut vm.clone(), 0).unwrap(), 1);
    assert_eq!(read_register(&mut vm.clone(), 4).unwrap(), size);
    assert!(!vm.native_audio_extent_indeterminate);
    assert!(!bridge.snapshot(5, false).unwrap().exists);
    control.finish(&mut bridge);
}

#[test]
fn ordinary175_mv2x_uses_real_subband_pcm_and_header_extent_not_file_size() {
    let (root, archive, mut vm) = fixture();
    let mut source = b"MV2X".to_vec();
    source.extend_from_slice(&44100u32.to_le_bytes());
    source.extend_from_slice(&0u16.to_le_bytes());
    source.extend_from_slice(&44100u16.to_le_bytes());
    source.extend_from_slice(&[1, 4]);
    source.extend_from_slice(&138u32.to_le_bytes());
    let mut bits = Vec::new();
    let mut push = |value: u32, width: usize| {
        for bit in (0..width).rev() {
            bits.push((value >> bit) & 1);
        }
    };
    for block in 0..138 {
        push(u32::from(block == 0), 10);
        push(200, 9);
        if block == 0 {
            push(2, 2);
            push(1, 1);
        }
    }
    for chunk in bits.chunks(32) {
        let word = chunk
            .iter()
            .enumerate()
            .fold(0u32, |word, (bit, value)| word | (value << bit));
        source.extend_from_slice(&word.to_le_bytes());
    }
    assert_ne!(source.len(), 88200);
    std::fs::write(root.path().join("voice.mv2"), source).unwrap();
    vm.process_string_slots.insert(0, b"voice.mv2".to_vec());
    let (mut bridge, control) = OutputControl::new();
    command(
        &archive,
        &mut vm,
        &mut bridge,
        175,
        &[0, 0x8000_0000, 128, 1, 1.0f32.to_bits()],
    );
    assert_eq!(read_register(&mut vm.clone(), 0).unwrap(), 1);
    assert_eq!(read_register(&mut vm.clone(), 4).unwrap(), 88200);
    control.wait_for_write();
    let pcm = control.latest_pcm();
    assert!(pcm.iter().any(|sample| *sample != 0.0));
    assert!(pcm.iter().all(|sample| sample.abs() <= 0.5));
    command(&archive, &mut vm, &mut bridge, 168, &[0]);
    control.steps.send(OutputStep::Accept).unwrap();
    control.finish(&mut bridge);
}

#[test]
fn fixed_stream15_and_static0_are_independent_load_only_objects() {
    let (root, archive, mut vm) = fixture();
    wave(root.path(), "voice.wav", 2, 44100);
    let (mut bridge, control) = OutputControl::new();
    vm.resource_search_paths.insert(
        CmvsResourceSearchBank::Native0d14,
        BTreeMap::from([(0, "missing/".into())]),
    );
    vm.write_interpreter_word(81236, 71);
    vm.write_interpreter_word(81240, 72);
    vm.write_interpreter_word(1556, 0);
    command(
        &archive,
        &mut vm,
        &mut bridge,
        576,
        &[0, 0x8000_0000, 256, 100, 1, 0],
    );
    assert_eq!(vm.stack_cursor_bytes, 0);
    assert_eq!(vm.interpreter_words[&81220], 1);
    assert_eq!(
        (vm.interpreter_words[&81236], vm.interpreter_words[&81240]),
        (71, 72)
    );
    let fixed = bridge.snapshot(audio::FIXED_STREAM_CHANNEL, false).unwrap();
    assert!(fixed.exists && !fixed.playing);
    command(&archive, &mut vm, &mut bridge, 584, &[0, 0x8000_0000]);
    assert_eq!(vm.interpreter_words[&81220], 1);
    let mut globals = NativeGlobals::default();
    native_save_audio::capture(Some(&bridge), &vm, &mut globals).unwrap();
    assert_eq!(globals.static_sounds.entries[0].name, "voice.wav");
    assert_eq!(globals.static_sounds.entries[0].playing, 0);
    command(&archive, &mut vm, &mut bridge, 585, &[0]);
    assert!(
        bridge
            .snapshot(audio::FIXED_STREAM_CHANNEL, false)
            .unwrap()
            .exists
    );
    command(&archive, &mut vm, &mut bridge, 578, &[0]);
    assert_eq!(vm.interpreter_words[&81220], u32::MAX);
    command(&archive, &mut vm, &mut bridge, 579, &[0]);
    assert_eq!(vm.interpreter_words[&81220], u32::MAX);
    assert_eq!(
        (vm.interpreter_words[&81236], vm.interpreter_words[&81240]),
        (0, 0)
    );
    command(&archive, &mut vm, &mut bridge, 577, &[0]);
    assert!(
        !bridge
            .snapshot(audio::FIXED_STREAM_CHANNEL, false)
            .unwrap()
            .exists
    );
    control.finish(&mut bridge);
}

#[test]
fn fixed576_uses_master_only_linear_gain_and578_observes_played_pcm() {
    let (root, archive, mut vm) = fixture();
    wave(root.path(), "voice.wav", 2, 44100);
    let (mut bridge, control) = OutputControl::new();
    vm.write_interpreter_word(1560, 128);
    vm.write_interpreter_word(1568, 0);
    vm.write_interpreter_word(1556, 0);
    vm.write_interpreter_word(81236, 99);
    command(
        &archive,
        &mut vm,
        &mut bridge,
        576,
        &[0, 0x8000_0000, 128, 0, 1, 1],
    );
    assert_eq!(vm.interpreter_words[&81236], 99);
    control.wait_for_write();
    assert!(
        control
            .latest_pcm()
            .iter()
            .all(|sample| (*sample - 0.125).abs() < 0.00001)
    );
    control.accept_and_wait();
    control.consume_to(480);
    query_with_queued_pcm(&archive, &mut vm, &mut bridge, &control, 578);
    assert_eq!(vm.interpreter_words[&81220], 10);
    query_with_queued_pcm(&archive, &mut vm, &mut bridge, &control, 579);
    assert_eq!(vm.interpreter_words[&81220], 10);
    assert!(vm.interpreter_words[&81236] > 0);
    assert_eq!(vm.interpreter_words[&81236], vm.interpreter_words[&81240]);
    command(&archive, &mut vm, &mut bridge, 577, &[0]);
    control.steps.send(OutputStep::Accept).unwrap();
    command(&archive, &mut vm, &mut bridge, 578, &[0]);
    assert_eq!(vm.interpreter_words[&81220], u32::MAX);
    control.finish(&mut bridge);
}

#[test]
fn fixed580_volume_applies_after_queued_pcm_without_replacing_the_result() {
    let (root, archive, mut vm) = fixture();
    wave(root.path(), "voice.wav", 2, 44100);
    let (mut bridge, control) = OutputControl::new();
    command(
        &archive,
        &mut vm,
        &mut bridge,
        576,
        &[0, 0x8000_0000, 256, 0, 1, 1],
    );
    control.wait_for_write();
    assert!(
        control
            .latest_pcm()
            .iter()
            .all(|sample| (*sample - 0.5).abs() < 0.00001)
    );
    command(&archive, &mut vm, &mut bridge, 580, &[0, 0]);
    control.accept_and_wait();
    assert!(control.latest_pcm().iter().all(|sample| *sample == 0.0));
    assert_eq!(vm.interpreter_words[&81220], 1);
    command(&archive, &mut vm, &mut bridge, 577, &[0]);
    control.steps.send(OutputStep::Accept).unwrap();
    assert!(
        !bridge
            .snapshot(audio::FIXED_STREAM_CHANNEL, false)
            .unwrap()
            .exists
    );
    control.finish(&mut bridge);
}

#[test]
fn fixed576_zero_and_wrapping_volume_are_not_signed_default_gain() {
    for volume in [0, 0x8000_0000] {
        let (root, archive, mut vm) = fixture();
        wave(root.path(), "voice.wav", 2, 44100);
        let (mut bridge, control) = OutputControl::new();
        vm.write_interpreter_word(1560, 256);
        command(
            &archive,
            &mut vm,
            &mut bridge,
            576,
            &[0, 0x8000_0000, volume, 0, 1, 1],
        );
        control.wait_for_write();
        assert!(control.latest_pcm().iter().all(|sample| *sample == 0.0));
        command(&archive, &mut vm, &mut bridge, 577, &[0]);
        control.steps.send(OutputStep::Accept).unwrap();
        assert!(
            !bridge
                .snapshot(audio::FIXED_STREAM_CHANNEL, false)
                .unwrap()
                .exists
        );
        control.finish(&mut bridge);
    }
}

#[test]
fn static584_loads_only_and586_uses_signed_gain_and_se_gate() {
    for (volume, expected) in [(128, 0.03125), (0, 0.125), (u32::MAX, 0.125)] {
        let (root, archive, mut vm) = fixture();
        wave(root.path(), "voice.wav", 2, 44100);
        let (mut bridge, control) = OutputControl::new();
        vm.write_interpreter_word(1560, 128);
        vm.write_interpreter_word(1568, 256);
        command(&archive, &mut vm, &mut bridge, 584, &[23, 0x8000_0000]);
        assert_eq!(vm.interpreter_words[&81220], 1);
        vm.write_interpreter_word(81220, 73);
        vm.write_interpreter_word(1556, 0);
        command(&archive, &mut vm, &mut bridge, 586, &[23, volume, 1]);
        let mut globals = NativeGlobals::default();
        native_save_audio::capture(Some(&bridge), &vm, &mut globals).unwrap();
        assert_eq!(globals.static_sounds.entries[0].playing, 0);
        vm.write_interpreter_word(1556, 1);
        command(&archive, &mut vm, &mut bridge, 586, &[23, volume, 1]);
        assert_eq!(vm.stack_cursor_bytes, 0);
        assert_eq!(vm.interpreter_words[&81220], 73);
        control.wait_for_write();
        assert!(
            control
                .latest_pcm()
                .iter()
                .all(|sample| (*sample - expected).abs() < 0.00001)
        );
        command(&archive, &mut vm, &mut bridge, 587, &[23]);
        control.steps.send(OutputStep::Accept).unwrap();
        bridge.snapshot(audio::FIXED_STREAM_CHANNEL, false).unwrap();
        native_save_audio::capture(Some(&bridge), &vm, &mut globals).unwrap();
        assert_eq!(globals.static_sounds.entries[0].playing, 0);
        assert_eq!(vm.interpreter_words[&81220], 73);
        control.finish(&mut bridge);
    }
}

#[test]
fn fixed_and_static_failures_publish_false_without_touching_the_level_pair() {
    let (root, archive, mut vm) = fixture();
    wave(root.path(), "voice.wav", 2, 44100);
    let (mut bridge, control) = OutputControl::new();
    vm.write_interpreter_word(81236, 81);
    vm.write_interpreter_word(81240, 82);
    command(
        &archive,
        &mut vm,
        &mut bridge,
        576,
        &[0, 0x8000_0000, 256, 0, 1, 0],
    );
    command(&archive, &mut vm, &mut bridge, 584, &[0, 0x8000_0000]);
    vm.process_string_slots.insert(0, b"missing.wav".to_vec());
    for (id, words) in [
        (576, vec![0, 0x8000_0000, 256, 0, 1, 1]),
        (584, vec![0, 0x8000_0000]),
    ] {
        vm.write_interpreter_word(81220, 17);
        command(&archive, &mut vm, &mut bridge, id, &words);
        assert_eq!(vm.interpreter_words[&81220], 0);
        assert_eq!(
            (vm.interpreter_words[&81236], vm.interpreter_words[&81240]),
            (81, 82)
        );
    }
    assert!(
        !bridge
            .snapshot(audio::FIXED_STREAM_CHANNEL, false)
            .unwrap()
            .exists
    );
    let mut globals = NativeGlobals::default();
    native_save_audio::capture(Some(&bridge), &vm, &mut globals).unwrap();
    assert!(globals.static_sounds.entries.is_empty());
    control.finish(&mut bridge);
}

#[test]
fn all_fixed_and_static_invalid_selectors_preserve_results_even_when_se_is_disabled() {
    let (_root, archive, mut vm) = fixture();
    let (mut bridge, control) = OutputControl::new();
    vm.write_interpreter_word(1556, 0);
    for (id, words) in [
        (576, vec![1, 0x8000_0000, 256, 0, 1, 1]),
        (577, vec![1]),
        (578, vec![1]),
        (579, vec![1]),
        (580, vec![1, 0]),
        (584, vec![24, 0x8000_0000]),
        (585, vec![24]),
        (586, vec![24, 256, 1]),
        (587, vec![24]),
    ] {
        vm.interpreter_flag_words.insert(10532, 0x40);
        vm.write_interpreter_word(81220, 91);
        vm.write_interpreter_word(81236, 92);
        vm.write_interpreter_word(81240, 93);
        command(&archive, &mut vm, &mut bridge, id, &words);
        assert_eq!(vm.stack_cursor_bytes, 0);
        assert_eq!(vm.interpreter_flag_words[&10532], 0x240, "command {id}");
        assert_eq!(vm.interpreter_words[&81220], 91);
        assert_eq!(
            (vm.interpreter_words[&81236], vm.interpreter_words[&81240]),
            (92, 93)
        );
    }
    control.finish(&mut bridge);
}

#[test]
fn fixed576_and_static584_resolve_tagged_names_before_selector_guards() {
    for (id, words) in [
        (576, vec![1, 0x8000_0000, 256, 0, 0, 0]),
        (584, vec![24, 0x8000_0000]),
    ] {
        let (_root, _archive, mut vm) = fixture();
        vm.process_string_slots.insert(0, vec![0x81]);
        vm.write_interpreter_word(10532, 0x40);
        for value in words.iter().rev() {
            let span = CmvsPs2aSourceSpan {
                offset: vm.program_counter,
                byte_length: 8,
            };
            crate::execute_cmvs390_frame(
                &mut vm,
                &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                    span,
                    stack_bytes: 4,
                    value: *value,
                }),
            )
            .unwrap();
        }
        let span = CmvsPs2aSourceSpan {
            offset: vm.program_counter,
            byte_length: 2,
        };
        let cause = crate::execute_cmvs390_frame(
            &mut vm,
            &CmvsPs2aInstructionFrame::Command {
                span,
                command_id: id,
            },
        )
        .unwrap_err();
        assert_eq!(cause.code(), "SHINKI_EMU_CMVS_VM_STRING_ENCODING");
        assert_eq!(vm.interpreter_words[&10532], 0x40);
    }
}

#[test]
fn static_free_and_failed_reload_destroy_all_duplicate_voices() {
    for free in [false, true] {
        let (root, archive, mut vm) = fixture();
        wave(root.path(), "voice.wav", 2, 44100);
        let (mut bridge, control) = OutputControl::new();
        command(&archive, &mut vm, &mut bridge, 584, &[0, 0x8000_0000]);
        bridge.play_static_sound(0, 256, None).unwrap();
        control.wait_for_write();
        for _ in 1..32 {
            bridge.play_static_sound(0, 256, None).unwrap();
        }
        if free {
            command(&archive, &mut vm, &mut bridge, 585, &[0]);
            assert_eq!(vm.interpreter_words[&81220], 1);
        } else {
            vm.process_string_slots.insert(0, b"missing.wav".to_vec());
            command(&archive, &mut vm, &mut bridge, 584, &[0, 0x8000_0000]);
            assert_eq!(vm.interpreter_words[&81220], 0);
        }
        control.steps.send(OutputStep::Accept).unwrap();
        bridge.snapshot(audio::FIXED_STREAM_CHANNEL, false).unwrap();
        let mut globals = NativeGlobals::default();
        native_save_audio::capture(Some(&bridge), &vm, &mut globals).unwrap();
        assert!(globals.static_sounds.entries.is_empty());
        vm.process_string_slots.insert(0, b"voice.wav".to_vec());
        command(&archive, &mut vm, &mut bridge, 584, &[0, 0x8000_0000]);
        command(&archive, &mut vm, &mut bridge, 586, &[0, 256, 1]);
        control.wait_for_write();
        assert!(
            control
                .latest_pcm()
                .iter()
                .all(|sample| (*sample - 0.5).abs() < 0.00001)
        );
        command(&archive, &mut vm, &mut bridge, 585, &[0]);
        control.steps.send(OutputStep::Accept).unwrap();
        bridge.snapshot(audio::FIXED_STREAM_CHANNEL, false).unwrap();
        control.finish(&mut bridge);
    }
}

#[test]
fn fixed576_fades_real_pcm_only_when_started_and_keeps_extent_register() {
    let (root, archive, mut vm) = fixture();
    wave(root.path(), "voice.wav", 2, 44100);
    let (mut bridge, control) = OutputControl::new();
    vm.write_interpreter_word(81236, 123);
    command(
        &archive,
        &mut vm,
        &mut bridge,
        576,
        &[0, 0x8000_0000, 256, 100, 1, 1],
    );
    control.wait_for_write();
    let pcm = control.latest_pcm();
    assert_eq!(pcm[0], 0.0);
    assert!(pcm.last().copied().unwrap() > 0.0);
    assert!(pcm.iter().all(|sample| *sample < 0.5));
    assert_eq!(vm.interpreter_words[&81236], 123);
    command(&archive, &mut vm, &mut bridge, 577, &[0]);
    control.steps.send(OutputStep::Accept).unwrap();
    assert!(
        !bridge
            .snapshot(audio::FIXED_STREAM_CHANNEL, false)
            .unwrap()
            .exists
    );
    control.finish(&mut bridge);
}
