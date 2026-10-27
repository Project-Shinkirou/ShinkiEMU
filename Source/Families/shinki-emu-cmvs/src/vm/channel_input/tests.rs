use super::*;
use crate::{CmvsPs2aInstructionFrame, CmvsPs2aSourceSpan};

fn fixture() -> CmvsPs2aVmState {
    let mut state = CmvsPs2aVmState::new(0);
    state.slot_objects.insert(742 << 8, 1);
    let object = state.effect_channels.entry(0).or_default();
    object.set_origin(5, 7);
    object.write_dword(1264, u32::MAX);
    object.write_quad_rect(0, [5, 10, 20, 30]);
    object.set_quad_visible(0, true);
    state.native_stage_size = [1280, 720];
    state.pointer_x = 10;
    state.pointer_y = 20;
    state
}

fn query(state: &mut CmvsPs2aVmState) -> (u32, u32, u32) {
    stack::push_word(state, 0).unwrap();
    dispatch::execute_cmvs390_frame(
        state,
        &CmvsPs2aInstructionFrame::Command {
            span: CmvsPs2aSourceSpan {
                offset: 0,
                byte_length: 2,
            },
            command_id: 404,
        },
    )
    .unwrap();
    (
        state.interpreter_words[&81236],
        state.interpreter_words[&81240],
        state.interpreter_words[&81244],
    )
}

#[test]
fn keyboard_edges_are_not_mapped_to_a_message_quad_or_consumed() {
    let mut state = fixture();
    state.input_advance_press = true;
    state.input_advance_release = true;
    state.input_confirm_held = true;
    assert_eq!(query(&mut state), (u32::MAX, 0, 0));
    assert!(state.input_advance_release && state.input_advance_press);
}

#[test]
fn physical_press_release_confirms_only_its_quad_and_leaves_hotkey_selection_unchanged() {
    let mut state = fixture();
    state.native_pointer_present = true;
    state.input_selection_mirror = 9;
    state.menu_session_live = true;
    state.input_advance_press = true;
    state.input_confirm_held = true;
    assert_eq!(query(&mut state), (0, 0, 1));
    state.input_advance_press = false;
    state.input_confirm_held = false;
    state.input_advance_release = true;
    assert_eq!(query(&mut state), (0, 1, 0));
    assert!(!state.input_advance_release);
    assert_eq!(state.input_selection_mirror, 9);
    state.input_advance_release = true;
    assert_eq!(query(&mut state), (0, 0, 0));
}

#[test]
fn native_unsigned_rect_words_do_not_create_a_negative_coordinate_hit() {
    let mut state = fixture();
    state.native_pointer_present = true;
    state.pointer_x = 0;
    state.pointer_y = 0;
    let object = state.effect_channels.get_mut(&0).unwrap();
    object.set_origin(0, 0);
    object.write_quad_rect(0, [-1, 0, 1, 10]);
    assert_eq!(query(&mut state), (u32::MAX, 0, 0));
}
