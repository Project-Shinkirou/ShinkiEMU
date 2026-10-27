use super::*;
use crate::{CmvsPs2aInstructionFrame, CmvsPs2aSourceSpan};

fn fixture(source: &str) -> CmvsPs2aVmState {
    let mut vm = CmvsPs2aVmState::new(0);
    rebuild_channel(&mut vm, 0, [10, 20], [300, 80], Some([1280, 720])).unwrap();
    let mut channel = CmvsEffectChannel::default();
    channel.write_dword(0, 0);
    vm.effect_channels.insert(0, channel);
    vm.slot_objects.insert(742 << 8, 99);
    let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(source);
    assert!(!invalid);
    let mut bytes = bytes.into_owned();
    bytes.push(0);
    vm.frame_string_pool_bytes.insert(0, bytes);
    vm
}

fn prepare(vm: &mut CmvsPs2aVmState, read_flag: u32) {
    for word in [0, read_flag, 0] {
        stack::push_word(vm, word).unwrap();
    }
    vm.program_counter = 80;
}

fn execute(vm: &mut CmvsPs2aVmState) -> Option<CmvsPs2aVmAction> {
    vm.dispatch_stopped = false;
    dispatch::execute_cmvs390_frame(
        vm,
        &CmvsPs2aInstructionFrame::Command {
            span: CmvsPs2aSourceSpan {
                offset: 80,
                byte_length: 2,
            },
            command_id: 338,
        },
    )
    .unwrap()
}

fn begin(vm: &mut CmvsPs2aVmState, read_flag: u32) {
    prepare(vm, read_flag);
    let Some(CmvsPs2aVmAction::NativeText(CmvsNativeTextAction::DrawDialogue {
        channel,
        slot,
        source,
    })) = execute(vm)
    else {
        panic!("expected font-dependent dialogue layout");
    };
    let options = options(vm, slot);
    let boundary = vm
        .effect_playback
        .get(&slot)
        .map(|record| &record.text_surface);
    let reveals = dialogue_layout::append(
        vm.native_text.get_mut(&slot).unwrap(),
        &source,
        options,
        boundary,
        None,
    )
    .unwrap();
    layout_complete(vm, channel, slot, reveals).unwrap();
}

#[test]
fn repoll_keeps_operands_and_glyphs_until_natural_completion() {
    let mut vm = fixture("AB");
    vm.write_interpreter_word(1480, 0);
    vm.write_interpreter_word(1488, 10);
    begin(&mut vm, u32::MAX);
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (80, 12));
    advance(&mut vm, 0);
    assert_eq!(visual(&vm, 0, 0, 27), Some([1.0; 2]));
    assert_eq!(visual(&vm, 0, 1, 27), None);
    advance(&mut vm, 29);
    assert!(execute(&mut vm).is_none());
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (80, 12));
    assert_eq!(vm.native_text[&0].glyphs.len(), 2);
    advance(&mut vm, 1);
    assert!(execute(&mut vm).is_none());
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (82, 0));
    assert_eq!(
        vm.interpreter_words[&schedule::SCRIPT_FRAME_COUNTER_FIELD],
        0
    );
    assert!(vm.dispatch_stopped);
}

#[test]
fn fade_survives_snapshot_and_completes_after_remaining_time() {
    let mut vm = fixture("A");
    begin(&mut vm, u32::MAX);
    advance(&mut vm, 175);
    assert_eq!(visual(&vm, 0, 0, 27), Some([127.0 / 255.0, 1.0]));
    let bytes = postcard::to_allocvec(&vm).unwrap();
    let mut restored: CmvsPs2aVmState = postcard::from_bytes(&bytes).unwrap();
    validate(&restored).unwrap();
    advance(&mut restored, 174);
    assert!(execute(&mut restored).is_none());
    assert_eq!(restored.program_counter, 80);
    advance(&mut restored, 1);
    assert!(execute(&mut restored).is_none());
    assert_eq!(
        (restored.program_counter, restored.stack_cursor_bytes),
        (82, 0)
    );
    assert_eq!(visual(&restored, 0, 0, 27), Some([1.0; 2]));
}

#[test]
fn unread_text_disables_forced_skip_but_read_text_is_immediate() {
    for read in [false, true] {
        let mut vm = fixture("A");
        vm.write_interpreter_word(1464, 1);
        vm.write_interpreter_word(1468, 1);
        if read {
            vm.process_flag_bits.insert(55);
        }
        begin(&mut vm, 55);
        assert_eq!(vm.interpreter_words[&1464], u32::from(read));
        assert_eq!(vm.program_counter, if read { 82 } else { 80 });
        assert_eq!(vm.stack_cursor_bytes, if read { 0 } else { 12 });
    }
}

#[test]
fn release_flushes_pending_glyphs_and_consumes_only_the_release_branch() {
    for held in [false, true] {
        let mut vm = fixture("AB");
        begin(&mut vm, u32::MAX);
        vm.input_shift_held = held;
        vm.input_advance_release = true;
        vm.input_advance_press = true;
        assert!(execute(&mut vm).is_none());
        assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (82, 0));
        assert_eq!(vm.input_advance_release, held);
        assert_eq!(vm.input_advance_press, held);
        assert_eq!(visual(&vm, 0, 1, 27), Some([1.0; 2]));
    }
}

#[test]
fn empty_and_suppressed_text_complete_the_first_poll_without_an_extra_wait() {
    for source in ["", "\\wcA"] {
        let mut vm = fixture(source);
        begin(&mut vm, u32::MAX);
        assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (82, 0));
        assert!(!vm.wait_boundary_hit);
        assert_eq!(
            vm.interpreter_words[&schedule::SCRIPT_FRAME_COUNTER_FIELD],
            0
        );
        assert_eq!(
            vm.native_text[&0].glyphs.len(),
            usize::from(!source.is_empty())
        );
    }
}

#[test]
fn absent_channel_raises_native_mask_and_completes_without_rendering() {
    let mut vm = fixture("A");
    vm.slot_objects.remove(&(742 << 8));
    vm.interpreter_flag_words.insert(10532, 0x20);
    prepare(&mut vm, u32::MAX);
    assert!(execute(&mut vm).is_none());
    assert_eq!(vm.interpreter_flag_words[&10532], 0x10020);
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (82, 0));
    assert!(vm.native_text[&0].glyphs.is_empty());
}

#[test]
fn horizontal_reveal_uses_integer_native_box_width_and_stops_at_full_size() {
    let mut vm = fixture("A");
    vm.write_interpreter_word(1480, 2);
    vm.write_interpreter_word(1484, 1);
    begin(&mut vm, u32::MAX);
    advance(&mut vm, 100);
    assert_eq!(visual(&vm, 0, 0, 27), Some([1.0, 15.0 / 31.0]));
    advance(&mut vm, 100);
    assert_eq!(visual(&vm, 0, 0, 27), Some([1.0; 2]));
    assert!(execute(&mut vm).is_none());
    assert_eq!(vm.program_counter, 82);
}

#[test]
fn first_poll_publishes_a_continuation_only_when_text_actually_waits() {
    for source in ["", "\\wcA", "A"] {
        let mut vm = fixture(source);
        vm.write_interpreter_word(13216, 7);
        vm.write_interpreter_word(13220, 600);
        begin(&mut vm, u32::MAX);
        let waiting = source == "A";
        assert_eq!(vm.interpreter_words[&13216], if waiting { 0 } else { 7 });
        assert_eq!(vm.interpreter_words[&13220], if waiting { 80 } else { 600 });
    }
}

#[test]
fn channel_clear_retires_pending_glyphs_and_resets_the_next_line_cursor() {
    let mut vm = fixture("AB");
    begin(&mut vm, u32::MAX);
    advance(&mut vm, 100);
    assert!(!vm.native_text_reveals[&0].is_empty());
    vm.dispatch_stopped = false;
    stack::push_word(&mut vm, 0).unwrap();
    dispatch::execute_cmvs390_frame(
        &mut vm,
        &CmvsPs2aInstructionFrame::Command {
            span: CmvsPs2aSourceSpan {
                offset: 80,
                byte_length: 2,
            },
            command_id: 330,
        },
    )
    .unwrap();
    assert!(vm.native_text[&0].glyphs.is_empty());
    assert_eq!(vm.native_text[&0].cursor, [0, 0]);
    assert!(!vm.native_text_reveals.contains_key(&0));
    let options = options(&vm, 0);
    dialogue_layout::append(
        vm.native_text.get_mut(&0).unwrap(),
        "C",
        options,
        None,
        None,
    )
    .unwrap();
    assert_eq!(vm.native_text[&0].glyphs[0].position, [0, 0]);
}

#[test]
fn ordinary_enter_hold_does_not_flush_typewriter_without_a_release() {
    let mut vm = fixture("AB");
    vm.input_confirm_held = true;
    begin(&mut vm, u32::MAX);
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (80, 12));
    advance(&mut vm, 50);
    assert!(execute(&mut vm).is_none());
    assert_eq!(vm.program_counter, 80);
    vm.input_advance_release = true;
    assert!(execute(&mut vm).is_none());
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (82, 0));
}
