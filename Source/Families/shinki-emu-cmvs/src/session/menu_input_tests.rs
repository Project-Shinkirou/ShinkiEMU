use super::*;
use crate::session::test_support::{game, open};

/// A system-menu key-quad click only publishes the selected id for the
/// record-17 poll body: the next frame-entry rebuild must resume the poll
/// through the engine request dwords without laying an extra dispatcher
/// frame. An engine-laid frame base outlives the poll body's own dispatcher
/// call and its unwind returns into the script with an underfilled stack.
#[test]
fn menu_quad_click_publishes_selection_without_engine_laid_dispatch_frame() {
    let (root, archive) = game();
    let mut session = open(root.path(), archive);
    session.input.menu_opened = true;
    session.vm.input_selection_mirror = 5;
    let bases = session.vm.call_frame_bases.clone();

    session
        .advance_inner(
            0,
            &[RuntimeEvent::PointerButton {
                button: PointerButton::Primary,
                state: KeyState::Released,
            }],
        )
        .unwrap();
    assert_eq!(session.vm.process_indexed_words.get(&10), Some(&5));

    crate::vm::rebuild_frame_entry_queue(&mut session.vm).unwrap();
    assert_eq!(session.vm.program_counter, 179_768);
    assert_eq!(session.vm.current_frame, 1);
    assert_eq!(session.vm.call_frame_bases, bases);
    assert_eq!(session.vm.dialog_dispatch_base_depth, None);
}
