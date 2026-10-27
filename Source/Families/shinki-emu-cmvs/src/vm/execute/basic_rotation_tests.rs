use super::*;

fn command(vm: &mut CmvsPs2aVmState, id: u16, words: &[u32]) {
    for &word in words.iter().rev() {
        push_word(vm, word).unwrap();
    }
    execute_cmvs390_frame(
        vm,
        &CmvsPs2aInstructionFrame::Command {
            span: crate::CmvsPs2aSourceSpan {
                offset: 0,
                byte_length: 2,
            },
            command_id: id,
        },
    )
    .unwrap();
}

#[test]
fn absolute_angle_setter_preserves_native_wrapping_abs_and_signed_hundredths() {
    let mut vm = CmvsPs2aVmState::new(0);
    command(&mut vm, 32, &[0]);
    command(&mut vm, 64, &[0]);
    for (degrees, hundredths) in [
        (-5_i32, 500),
        (721, 100),
        (i32::MIN, -12800),
        (i32::MAX, 12700),
    ] {
        command(&mut vm, 74, &[0, u32::MAX, degrees as u32]);
        assert_eq!(vm.texture_parents[&0].rotation_hundredths, hundredths);
    }
    assert_eq!(vm.stack_cursor_bytes, 0);
}
