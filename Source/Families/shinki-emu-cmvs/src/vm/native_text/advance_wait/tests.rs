use super::*;
use crate::{CmvsPs2aInstructionFrame, CmvsPs2aSourceSpan};

fn fixture() -> CmvsPs2aVmState {
    let mut vm = CmvsPs2aVmState::new(0);
    rebuild_channel(&mut vm, 0, [0, 0], [600, 200], Some([1280, 720])).unwrap();
    let mut object = CmvsEffectChannel::default();
    object.write_dword(0, 0);
    object.write_dword(1264, u32::MAX);
    vm.effect_channels.insert(0, object);
    vm.slot_objects.insert(742 << 8, 99);
    vm.native_stage_size = [1280, 720];
    vm.native_frame_delta_ms = 100;
    layout::append(vm.native_text.get_mut(&0).unwrap(), "AB", None, None).unwrap();
    for child in [2, 3] {
        vm.effect_elements.entry((0, child)).or_default();
    }
    vm
}

fn arm(vm: &mut CmvsPs2aVmState, words: [u32; 3]) {
    for word in words.into_iter().rev() {
        stack::push_word(vm, word).unwrap();
    }
    vm.program_counter = 80;
}

fn execute(vm: &mut CmvsPs2aVmState, command_id: u16) {
    vm.dispatch_stopped = false;
    dispatch::execute_cmvs390_frame(
        vm,
        &CmvsPs2aInstructionFrame::Command {
            span: CmvsPs2aSourceSpan {
                offset: 80,
                byte_length: 2,
            },
            command_id,
        },
    )
    .unwrap();
}

#[test]
fn manual_wait_holds_operands_then_release_advances_once_without_stopping_dispatch() {
    let mut vm = fixture();
    arm(&mut vm, [0, u32::MAX, u32::MAX]);
    for _ in 0..20 {
        execute(&mut vm, 339);
        assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (80, 12));
        assert!(vm.dispatch_stopped);
        assert_eq!(vm.effect_elements[&(0, 2)].visible, 1);
    }
    vm.input_advance_release = true;
    vm.input_advance_press = true;
    execute(&mut vm, 339);
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (82, 0));
    assert!(!vm.dispatch_stopped);
    assert!(!vm.input_advance_release && !vm.input_advance_press);
    assert_eq!(vm.effect_elements[&(0, 2)].visible, 0);
    assert_eq!(vm.effect_elements[&(0, 3)].visible, 0);
}

#[test]
fn context_gate_blocks_held_confirmation_and_auto_time_until_cleared() {
    let mut vm = fixture();
    vm.settings_words.insert(88, 4);
    vm.write_interpreter_word(1500, 1);
    vm.input_shift_held = true;
    vm.input_advance_release = true;
    arm(&mut vm, [0, u32::MAX, u32::MAX]);
    for _ in 0..20 {
        execute(&mut vm, 339);
    }
    assert_eq!(vm.program_counter, 80);
    assert_eq!(timer(&vm, CONFIRM_TIMER), 0);
    assert!(vm.input_advance_release);
    vm.settings_words.insert(88, u32::MAX);
    execute(&mut vm, 339);
    assert_eq!(vm.program_counter, 82);
    // The held branch does not consume release edges.
    assert!(vm.input_advance_release);
}

#[test]
fn unread_skip_is_disabled_but_read_skip_can_complete() {
    let mut vm = fixture();
    vm.write_interpreter_word(1468, 1);
    vm.write_interpreter_word(1464, 1);
    vm.input_control_held = true;
    arm(&mut vm, [0, 55, u32::MAX]);
    execute(&mut vm, 339);
    assert_eq!(vm.program_counter, 80);
    assert_eq!(vm.interpreter_words[&1464], 0);
    vm.process_flag_bits.insert(55);
    execute(&mut vm, 339);
    assert_eq!(vm.program_counter, 82);
}

#[test]
fn visible_quad_consumes_its_release_in_channel_virtual_coordinates() {
    let mut vm = fixture();
    vm.native_pointer_present = true;
    vm.native_text.get_mut(&0).unwrap().virtual_size = [1920, 1440];
    let object = vm.effect_channels.get_mut(&0).unwrap();
    object.write_quad_rect(0, [150, 150, 100, 100]);
    object.set_quad_visible(0, true);
    vm.pointer_x = 120;
    vm.pointer_y = 100;
    vm.input_advance_release = true;
    arm(&mut vm, [0, u32::MAX, u32::MAX]);
    execute(&mut vm, 339);
    assert_eq!(vm.program_counter, 80);
    assert!(!vm.input_advance_release);
    vm.pointer_x = -20;
    vm.pointer_y = -20;
    vm.input_advance_release = true;
    execute(&mut vm, 339);
    assert_eq!(vm.program_counter, 82);
}

#[test]
fn keyboard_only_input_does_not_hover_the_default_zero_position() {
    let mut vm = fixture();
    let object = vm.effect_channels.get_mut(&0).unwrap();
    object.write_quad_rect(0, [0, 0, 100, 100]);
    object.set_quad_visible(0, true);
    vm.input_advance_release = true;
    arm(&mut vm, [0, u32::MAX, u32::MAX]);
    execute(&mut vm, 339);
    assert_eq!(vm.program_counter, 82);
}

#[test]
fn drag_release_outside_the_pressed_quad_does_not_advance_the_line() {
    let mut vm = fixture();
    vm.native_pointer_present = true;
    let object = vm.effect_channels.get_mut(&0).unwrap();
    object.write_quad_rect(0, [0, 0, 100, 100]);
    object.set_quad_visible(0, true);
    vm.input_advance_press = true;
    vm.input_confirm_held = true;
    arm(&mut vm, [0, u32::MAX, u32::MAX]);
    execute(&mut vm, 339);
    vm.input_advance_press = false;
    vm.input_confirm_held = false;
    vm.input_advance_release = true;
    vm.pointer_x = 200;
    execute(&mut vm, 339);
    assert_eq!(vm.program_counter, 80);
    assert!(!vm.input_advance_release);
    vm.input_advance_release = true;
    execute(&mut vm, 339);
    assert_eq!(vm.program_counter, 82);
}

#[test]
fn fourth_voice_slot_waits_for_real_end_and_native_postroll() {
    let mut vm = fixture();
    vm.native_audio_playing = 1 << 8;
    vm.write_interpreter_word(81220, 7);
    vm.write_interpreter_word(81224, 8);
    arm(&mut vm, [0, 123, 3]);
    for _ in 0..8 {
        execute(&mut vm, 344);
    }
    assert_eq!(vm.program_counter, 80);
    assert_eq!(timer(&vm, AUTO_TIMER), 0);
    vm.native_audio_playing = 0;
    execute(&mut vm, 344);
    execute(&mut vm, 344);
    assert_eq!(vm.program_counter, 80);
    execute(&mut vm, 344);
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (82, 0));
    assert!(!vm.dispatch_stopped);
    assert_eq!(vm.interpreter_words[&81220], 7);
    assert_eq!(vm.interpreter_words[&81224], 8);
}

#[test]
fn automatic_wait_uses_text_time_without_consuming_manual_input() {
    let mut vm = fixture();
    vm.native_frame_delta_ms = 70;
    vm.write_interpreter_word(1504, 1);
    vm.write_interpreter_word(1508, 4);
    vm.settings_words.insert(88, 7);
    vm.input_confirm_held = true;
    vm.input_advance_release = true;
    arm(&mut vm, [0, 99, u32::MAX]);
    execute(&mut vm, 344);
    assert_eq!(vm.program_counter, 80);
    execute(&mut vm, 344);
    assert_eq!(vm.program_counter, 82);
    assert!(vm.input_advance_release);
}

#[test]
fn confirm_auto_mode_uses_the_configured_glyph_count_duration() {
    let mut vm = fixture();
    vm.native_frame_delta_ms = 50;
    vm.write_interpreter_word(1500, 1);
    vm.write_interpreter_word(1504, 0);
    vm.write_interpreter_word(1508, 10);
    arm(&mut vm, [0, u32::MAX, u32::MAX]);
    execute(&mut vm, 339);
    assert_eq!(vm.program_counter, 80);
    execute(&mut vm, 339);
    assert_eq!(vm.program_counter, 82);
}

#[test]
fn timers_are_independent_and_snapshot_retains_elapsed_auto_wait() {
    let mut vm = fixture();
    arm(&mut vm, [0, 0, u32::MAX]);
    execute(&mut vm, 344);
    assert_eq!(timer(&vm, AUTO_TIMER), 100);
    vm.write_interpreter_word(1500, 1);
    execute(&mut vm, 339);
    assert_eq!(timer(&vm, AUTO_TIMER), 100);
    assert_eq!(timer(&vm, CONFIRM_TIMER), 100);
    let bytes = postcard::to_allocvec(&vm).unwrap();
    let mut restored: CmvsPs2aVmState = postcard::from_bytes(&bytes).unwrap();
    restored.native_frame_delta_ms = 100;
    for _ in 0..8 {
        execute(&mut restored, 344);
    }
    assert_eq!(restored.program_counter, 80);
    execute(&mut restored, 344);
    assert_eq!(restored.program_counter, 82);
    assert_eq!(timer(&restored, CONFIRM_TIMER), 100);
    assert_eq!(timer(&restored, AUTO_TIMER), 0);
}

#[test]
fn busy_channel_does_not_consume_input_or_run_the_auto_clock() {
    let mut vm = fixture();
    vm.effect_channels.get_mut(&0).unwrap().write_dword(4, 1);
    vm.input_confirm_held = true;
    arm(&mut vm, [0, 0, u32::MAX]);
    execute(&mut vm, 344);
    assert_eq!(vm.program_counter, 80);
    assert_eq!(timer(&vm, AUTO_TIMER), 0);
    assert_eq!(vm.effect_elements[&(0, 2)].visible, 0);
}

#[test]
fn indicator_mode_tracks_cursor_and_can_be_cleared_without_clobbering_quads() {
    let mut vm = fixture();
    for word in [u32::MAX - 3, 3, 3, 0] {
        stack::push_word(&mut vm, word).unwrap();
    }
    execute(&mut vm, 337);
    let cursor = vm.native_text[&0].cursor;
    arm(&mut vm, [0, u32::MAX, u32::MAX]);
    execute(&mut vm, 339);
    assert_eq!(
        vm.effect_elements[&(0, 2)].position,
        Some([cursor[0] + 3, cursor[1] - 4])
    );
    assert_eq!(
        vm.effect_elements[&(0, 3)].position,
        Some([cursor[0] + 3, cursor[1] - 4])
    );
    vm.input_advance_release = true;
    execute(&mut vm, 339);
    assert_eq!(vm.effect_elements[&(0, 2)].visible, 0);
}

#[test]
fn missing_channel_sets_native_error_and_continues_without_a_fake_wait() {
    let mut vm = fixture();
    vm.interpreter_flag_words.insert(10532, 0x20);
    arm(&mut vm, [8, 0, 0]);
    execute(&mut vm, 339);
    assert_eq!(vm.interpreter_flag_words[&10532], 0x10020);
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (82, 0));
    assert!(!vm.dispatch_stopped);
}

#[test]
fn holding_enter_does_not_skip_and_one_release_cannot_complete_the_next_line() {
    let mut vm = fixture();
    vm.input_confirm_held = true;
    arm(&mut vm, [0, u32::MAX, u32::MAX]);
    for _ in 0..5 {
        execute(&mut vm, 339);
    }
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (80, 12));
    vm.input_confirm_held = false;
    vm.input_advance_release = true;
    execute(&mut vm, 339);
    assert_eq!(vm.program_counter, 82);
    arm(&mut vm, [0, u32::MAX, u32::MAX]);
    execute(&mut vm, 339);
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (80, 12));
}
