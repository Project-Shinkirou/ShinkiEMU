use super::*;
use crate::vm::native_slot::*;

/// Decoded C layout from4782F0: header48, one index, four code bytes,
/// nine writable data bytes, then one NUL-terminated Shift-JIS pool entry.
fn program() -> Vec<u8> {
    let mut bytes = b"PS2A".to_vec();
    for word in [48u32, 0x11223344, 7, 1, 4, 9, 4, 0, 1, 19, 0x55667788] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9]);
    bytes.extend_from_slice(b"x\0");
    bytes
}
fn fixture() -> NativeSlot {
    let program = program();
    let script = decode_native_program(&program).unwrap();
    let mut state = NativeInterpreter::default();
    state.scalars.insert(274, program.len() as u32);
    state.script_data = script.data_segment;
    let header =
        capture_native_slot_header_at(&crate::CmvsPs2aVmState::new(0), [2026, 10, 25, 12, 34, 56])
            .unwrap();
    NativeSlot {
        header,
        interpreter: state,
        program_image: program,
        preview_bmp: Vec::new(),
        images: BTreeMap::new(),
        channels: BTreeMap::new(),
        text: BTreeMap::new(),
        cmv: BTreeMap::new(),
        scene_workers: BTreeMap::new(),
    }
}

#[test]
fn native_mixed_width_fixture_distinguishes_frame_bases_from_values() {
    let mut slot = fixture();
    slot.interpreter.frame_bases[3] = 0x11223344;
    slot.interpreter.value_stack[12..16].copy_from_slice(&0xaabbccddu32.to_le_bytes());
    let mut snow_wire = vec![1, 0, 2, 0];
    for word in [
        2.0_f32.to_bits(),
        3.0_f32.to_bits(),
        1000.0_f32.to_bits(),
        20.0_f32.to_bits(),
        2,
        0,
    ] {
        snow_wire.extend_from_slice(&word.to_le_bytes());
    }
    let mut name = [0; 32];
    name[..7].copy_from_slice(b"red.pb3");
    snow_wire.extend_from_slice(&name);
    slot.scene_workers.insert(
        3,
        crate::vm::native_scene_worker::SavedNativeSceneWorker::decode(&snow_wire).unwrap(),
    );
    let bytes = encode_state(&slot).unwrap();
    // Six empty u16 strings, then seven eight-byte scalar records.
    assert_eq!(&bytes[..5], &[0x00, 0x01, 0x01, 0x00, 0x00]);
    let frame = 30 + 7 * 8 + 44 + 8 + 1800 + 6 * 8;
    assert_eq!(
        &bytes[frame..frame + 6],
        &[0x42, 0x02, 0x00, 0x01, 0x00, 0x00]
    );
    assert_eq!(
        &bytes[frame + 6 + 12..frame + 6 + 16],
        &[0x44, 0x33, 0x22, 0x11]
    );
    let values = frame + 6 + 256;
    assert_eq!(
        &bytes[values..values + 6],
        &[0x43, 0x02, 0x00, 0x00, 0x01, 0x00]
    );
    assert_eq!(
        &bytes[values + 6 + 12..values + 6 + 16],
        &[0xdd, 0xcc, 0xbb, 0xaa]
    );
    let mut decoded = fixture();
    decode_state(&bytes, &mut decoded).unwrap();
    assert_eq!(decoded.scene_workers, slot.scene_workers);
    assert_eq!(decoded.interpreter.frame_bases[3], 0x11223344);
    assert_eq!(
        &decoded.interpreter.value_stack[12..16],
        &[0xdd, 0xcc, 0xbb, 0xaa]
    );
}

#[test]
fn native_order_last_scalar_string_and_record_override_prior_state() {
    let mut slot = fixture();
    let mut bytes = encode_state(&slot).unwrap();
    // Native scalar262 is frame,263 PC,264 counter;265 is a different block.
    bytes.extend_from_slice(&[0x08, 0x01, 0x04, 0x00, 0x77, 0, 0, 0]);
    bytes.extend_from_slice(&[0x00, 0x01, 0x04, 0x00, b'n', b'e', b'w', 0]);
    let mut records = vec![0u8; 1796];
    records[..4].copy_from_slice(&99u32.to_le_bytes());
    records[4..32].copy_from_slice(&word_bytes(&[7, 2, 3, 4, 5, 6, 7]));
    write_short(&mut bytes, 267, &records);
    decode_state(&bytes, &mut slot).unwrap();
    assert_eq!(slot.interpreter.scalars[&264], 0x77);
    assert_eq!(slot.interpreter.suspend_words, [0; 10]);
    assert_eq!(slot.interpreter.strings[0], b"new");
    assert_eq!(slot.interpreter.record_prefix, 99);
    assert_eq!(slot.interpreter.records[0], [7, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn container_extracts_all_three_independently_compressed_streams() {
    let mut slot = fixture();
    let mut bmp = vec![0; 58];
    bmp[..2].copy_from_slice(b"BM");
    for (offset, value) in [(2, 58u32), (10, 54), (14, 40), (18, 1), (22, 1)] {
        bmp[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bmp[26..30].copy_from_slice(&[1, 0, 24, 0]);
    bmp[54..57].copy_from_slice(&[4, 5, 6]);
    slot.preview_bmp = bmp;
    slot.header[548..550].copy_from_slice(&1u16.to_le_bytes());
    let bytes = encode_native_slot(&slot).unwrap();
    let restored = decode_native_slot(&bytes).unwrap();
    assert_eq!(restored.program_image, slot.program_image);
    assert_eq!(restored.preview_bmp, slot.preview_bmp);
    assert_eq!(restored.interpreter, slot.interpreter);
    let without_preview = encode_native_slot_with_preview(&slot, &[]).unwrap();
    assert!(
        decode_native_slot(&without_preview)
            .unwrap()
            .preview_bmp
            .is_empty()
    );
    assert_eq!(slot.preview_bmp.len(), 58);
    let a_length = u32::from_le_bytes(bytes[560..564].try_into().unwrap()) as usize;
    let end = bytes.len() - 2;
    let checksum = bytes[600..600 + a_length]
        .iter()
        .fold(0u16, |sum, b| sum.wrapping_add(u16::from(*b)));
    assert_eq!(&bytes[end..], &checksum.to_le_bytes());
    let mut wrong = bytes.clone();
    wrong[600] ^= 1;
    assert!(decode_native_slot(&wrong).is_err());
    // B is not part of encrypted-A checksum; corrupt its signature, not the sum.
    let mut wrong = bytes.clone();
    wrong[600 + a_length + 1] ^= 1;
    assert!(decode_native_slot(&wrong).is_err());
    for (offset, value) in [(540, u32::MAX), (560, u32::MAX), (580, u32::MAX), (588, 0)] {
        let mut wrong = bytes.clone();
        wrong[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(decode_native_slot(&wrong).is_err());
    }
    let mut wrong = bytes;
    wrong[546..548].copy_from_slice(&2u16.to_le_bytes());
    assert!(decode_native_slot(&wrong).is_err());
}

#[test]
fn unknown_records_and_wrong_fixed_widths_fail_explicitly() {
    let slot = fixture();
    let bytes = encode_state(&slot).unwrap();
    let mut wrong = bytes.clone();
    wrong.extend_from_slice(&[0xff, 0x7f, 0, 0, 0, 0]);
    assert!(decode_state(&wrong, &mut fixture()).is_err());
    let mut wrong = bytes;
    wrong.extend_from_slice(&[0x42, 0x02, 4, 0, 0, 0, 0, 0, 0, 0]);
    assert!(decode_state(&wrong, &mut fixture()).is_err());
    assert!(
        NativeSlot::new(
            &crate::CmvsPs2aVmState::new(0),
            NativeInterpreter::default(),
            Vec::new()
        )
        .is_err()
    );
}

#[test]
fn saved_c_replaces_menu_code_and_restores_runtime_data() {
    let saved = program();
    let mut vm = crate::CmvsPs2aVmState::new(88);
    let script = install_native_program(&saved, "cmvs:/fixture/story.ps3", &mut vm).unwrap();
    vm.script_data_segment_words
        .get_mut(&0)
        .unwrap()
        .insert(4, 0x11223344);
    let captured = capture_native_program(&script, &vm).unwrap();
    let mut menu = saved.clone();
    menu[52..56].copy_from_slice(&[1, 0, 2, 0]);
    install_native_program(&menu, "cmvs:/fixture/menu.ps3", &mut vm).unwrap();
    let restored = install_native_program(&captured, "cmvs:/fixture/story.ps3", &mut vm).unwrap();
    assert_eq!(restored.bytecode, [0, 0, 0, 0]);
    assert_eq!(&captured[..48], &saved[..48]);
    assert_eq!(&restored.data_segment[4..8], &[0x44, 0x33, 0x22, 0x11]);
    assert_eq!(restored.data_segment[8], 9);
    let before = vm.clone();
    let mut invalid = captured;
    invalid[4..8].copy_from_slice(&16u32.to_le_bytes());
    assert!(install_native_program(&invalid, "cmvs:/fixture/story.ps3", &mut vm).is_err());
    assert_eq!(vm, before);
}

#[test]
fn interpreter_apply_preserves_system_globals_and_unsaved_expression_flags() {
    let program = program();
    let mut vm = crate::CmvsPs2aVmState::new(0);
    install_native_program(&program, "cmvs:/fixture/story.ps3", &mut vm).unwrap();
    vm.write_interpreter_word(81212, 1);
    vm.write_interpreter_word(81216, 55);
    vm.write_interpreter_word(13204, 987);
    vm.process_indexed_words.insert(2048, 88);
    vm.process_float_words.insert(1024, 77);
    vm.process_flag_bits.insert(2048);
    vm.process_string_slots.insert(64, b"system".to_vec());
    let mut state = capture_native_interpreter(&vm, &program).unwrap();
    state.scalars.insert(264, 123);
    state.scalars.insert(275, 456);
    state.indexed_words[0] = 22;
    state.flags[0] = 0x81;
    state.records[2] = [9, 0, 100, 1, 2, 3, 4];
    apply_native_interpreter(&mut vm, &state).unwrap();
    assert_eq!(vm.interpreter_words[&81212], 1);
    assert_eq!(vm.interpreter_words[&81216], 55);
    assert_eq!(vm.interpreter_words[&13204], 987);
    assert_eq!(vm.process_indexed_words[&0], 22);
    assert_eq!(vm.process_indexed_words[&2048], 88);
    assert_eq!(vm.process_float_words[&1024], 77);
    assert!(vm.process_flag_bits.contains(&2048));
    assert!(vm.process_flag_bits.contains(&0));
    assert!(vm.process_flag_bits.contains(&7));
    assert_eq!(vm.process_string_slots[&64], b"system");
    assert_eq!(vm.interpreter_words[&13224], 123);
    assert_eq!(vm.interpreter_words[&(13272 + 2 * 28)], 9);
    let before = vm.clone();
    state.scalars.insert(577, 64);
    assert!(apply_native_interpreter(&mut vm, &state).is_err());
    assert_eq!(vm, before);
}

#[test]
fn captures_actual_native_header_title_identity_and_metadata() {
    let mut vm = crate::CmvsPs2aVmState::new(0);
    vm.interpreter_words.extend([
        (10556, 0x11223344),
        (10312, 11),
        (10316, 22),
        (10320, 33),
        (10292, 1),
    ]);
    vm.interpreter_string_buffers.insert(
        10324,
        vec![crate::CmvsStringSegment::OwnedString("chapter".into())],
    );
    let mut header = capture_native_slot_header_at(&vm, [2026, 10, 25, 12, 34, 56]).unwrap();
    assert_eq!(&header[8..12], &0x11223344u32.to_le_bytes());
    assert_eq!(&header[16..44], b"2026-10-25 12:34:56 chapter\0");
    assert!(header[272..528].iter().all(|byte| *byte == 0));
    assert_eq!(&header[528..540], &[11, 0, 0, 0, 22, 0, 0, 0, 33, 0, 0, 0]);
    assert_eq!(&header[544..550], &[4, 0, 1, 0, 1, 0]);
    refresh_native_slot_header_time(&mut header, [2027, 2, 3, 4, 5, 6]).unwrap();
    assert_eq!(&header[16..44], b"2027-02-03 04:05:06 chapter\0");
    assert!(capture_native_slot_header_at(&vm, [2026, 2, 30, 0, 0, 0]).is_err());
}

#[test]
fn native_record_merge_and29_push_keeps_exact_suspended_context() {
    let mut vm = crate::CmvsPs2aVmState::new(1234);
    install_native_program(&program(), "cmvs:/fixture/story.ps3", &mut vm).unwrap();
    vm.program_counter = 1234;
    vm.stack_cursor_bytes = 4;
    vm.stack_bytes = vec![1, 2, 3, 4];
    vm.stack_initialized = vec![true; 4];
    vm.write_interpreter_word(13224, 77);
    let mut prior = [[0, 0, u32::MAX, 0, 0, 0, 0]; 64];
    prior[29] = [9, 0, 70, 4, 5, 6, 7];
    // A root-frame row is restored, while nonzero pre-load frame rows win.
    for (index, row) in [
        (29, [3, 0, 90, 1, 2, 3, 4]),
        (5, [8, 2, 100, 11, 12, 13, 14]),
    ] {
        for (field, value) in row.into_iter().enumerate() {
            vm.write_interpreter_word(13272 + index * 28 + field as u32 * 4, value);
        }
    }
    prior[6] = [7, 1, 222, 9, 8, 7, 6];
    // Frame1 must really be installed before the preserved menu row can run.
    vm.script_frames.insert(1, vm.script_frames[&0].clone());
    assert!(reconcile_native_records_and_request29(&mut vm, &prior).unwrap());
    assert_eq!(vm.program_counter, 90);
    assert_eq!(vm.current_frame, 0);
    assert_eq!(vm.stack_cursor_bytes, 16);
    assert_eq!(vm.call_frame_bases, vec![0]);
    assert_eq!(
        &vm.stack_bytes[4..28],
        &word_bytes(&[77, 1234, 0, 0, 90, 0])
    );
    assert_eq!(vm.interpreter_words[&13224], 0);
    assert_eq!(vm.interpreter_words[&(13272 + 5 * 28)], 0);
    assert_eq!(vm.interpreter_words[&(13272 + 5 * 28 + 8)], u32::MAX);
    assert_eq!(vm.interpreter_words[&(13272 + 6 * 28)], 7);
    assert_eq!(vm.interpreter_words[&(13272 + 6 * 28 + 4)], 1);
    assert_eq!(vm.interpreter_words[&14060], 0);
    assert_eq!(vm.interpreter_words[&14064], u32::MAX);
    assert_eq!(vm.interpreter_words[&14056], 0);
    let before = vm.clone();
    prior[29] = [1, 1, 72, 0, 0, 0, 0];
    vm.stack_cursor_bytes = 65524;
    let overflow_before = vm.clone();
    assert!(reconcile_native_records_and_request29(&mut vm, &prior).is_err());
    assert_eq!(vm, overflow_before);
    vm = before;
    prior[29] = [0, 0, u32::MAX, 0, 0, 0, 0];
    vm.write_interpreter_word(13272 + 29 * 28 + 8, u32::MAX);
    let cursor = vm.stack_cursor_bytes;
    assert!(!reconcile_native_records_and_request29(&mut vm, &prior).unwrap());
    assert_eq!(vm.stack_cursor_bytes, cursor);
}
