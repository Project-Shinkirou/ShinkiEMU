use super::test_support::{command, game, open, picture, pixel};
use super::*;

#[test]
#[ignore = "requires a hardware GPU"]
fn retiring_the_last_image_presents_black_without_a_text_object() {
    let (root, archive) = game();
    let mut session = open(root.path(), archive);
    picture(&mut session, "red.pb3", 0);
    assert_eq!(&pixel(&session, 4, 4)[..3], &[240, 0, 0]);
    command(&mut session, 47, &[]);
    session.tick().unwrap();
    assert!(
        session
            .frame
            .rgba8
            .as_slice()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[..3] == [0, 0, 0])
    );
}

#[test]
#[ignore = "requires a hardware GPU"]
fn story_pointer_release_does_not_run_the_registered_menu_callback() {
    let (root, archive) = game();
    let mut session = open(root.path(), archive);
    let script = Arc::make_mut(session.scripts.get_mut(&0).unwrap());
    // Main parks; the menu callback at 16 resets child 0 of image 60.
    script.bytecode[..8].copy_from_slice(&[0x00, 0x20, 0x00, 0x04, 0, 0, 0, 0]);
    script.bytecode[16..36].copy_from_slice(&[
        0x40, 0x04, 4, 0, 0, 0, 0, 0, 0x40, 0x04, 4, 0, 60, 0, 0, 0, 0x22, 0x20, 0x00, 0x20,
    ]);
    let record = 0x33dc + 29 * 28;
    session.vm.write_interpreter_word(record, 0);
    session.vm.write_interpreter_word(record + 4, 16);
    command(&mut session, 137, &[1]);

    session
        .vm
        .process_string_slots
        .insert(0, b"red.pb3".to_vec());
    command(&mut session, 32, &[60]);
    command(&mut session, 64, &[60]);
    command(&mut session, 48, &[60, 0x8000_0000]);
    command(&mut session, 68, &[60, u32::MAX, 0, 0, 16, 16]);
    command(&mut session, 34, &[60, 0]);
    session
        .vm
        .process_string_slots
        .insert(0, b"blue.pb3".to_vec());
    command(&mut session, 56, &[60, 0, 0x8000_0000]);
    command(&mut session, 80, &[60, 0]);
    command(&mut session, 68, &[60, 0, 0, 0, 16, 16]);
    session.tick().unwrap();
    assert_eq!(&pixel(&session, 6, 6)[..3], &[0, 0, 240]);

    session
        .advance_inner(
            0,
            &[
                RuntimeEvent::PointerMove { x: 6.0, y: 6.0 },
                RuntimeEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Pressed,
                },
            ],
        )
        .unwrap();
    session
        .advance_inner(
            0,
            &[RuntimeEvent::PointerButton {
                button: PointerButton::Primary,
                state: KeyState::Released,
            }],
        )
        .unwrap();
    assert_eq!(&pixel(&session, 6, 6)[..3], &[0, 0, 240]);

    // A real menu callback remains executable; it must not be inferred
    // merely from a story surface using the same image/child identifiers.
    session.vm.forced_dialog_record = Some((0, 16, 1));
    session.tick().unwrap();
    assert_eq!(&pixel(&session, 6, 6)[..3], &[240, 0, 0]);
}
