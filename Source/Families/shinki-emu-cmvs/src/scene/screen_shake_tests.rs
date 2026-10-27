use super::parent_load_tests::{command, fixture};
use super::*;

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_shake_clock_pause_and_texture_header_gate_reach_pixels() {
    let (_, archive, mut vm) = fixture();
    vm.native_stage_size = [8, 4];
    vm.native_render_size = [4, 2];
    vm.native_frame_delta_ms = 10;
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    for (parent, x, color) in [(0, 2, 0x00ff_0000), (1, 6, 0x0000_ff00)] {
        command(&mut vm, 32, &[parent]);
        command(&mut vm, 64, &[parent]);
        command(&mut vm, 67, &[parent, u32::MAX, 1, 1, color]);
        command(&mut vm, 69, &[parent, u32::MAX, x, 1]);
    }
    command(&mut vm, 46, &[1, u32::MAX]);
    command(&mut vm, 549, &[0, 1, 10, u32::MAX]);
    crate::vm::screen_shake::advance(&mut vm).unwrap();
    for (phase, red_x) in [(0, 4), (1, 2), (2, 0), (3, 2)] {
        match phase {
            1 => {
                command(&mut vm, 551, &[1]);
            }
            2 => {
                command(&mut vm, 551, &[0]);
                crate::vm::screen_shake::advance(&mut vm).unwrap();
            }
            3 => {
                command(&mut vm, 553, &[]);
            }
            _ => {}
        }
        let frame = scene
            .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (8, 4), None)
            .unwrap()
            .unwrap();
        for y in 0..4 {
            for x in 0..8 {
                let expected = if y == 1 && x == red_x {
                    [255, 0, 0, 255]
                } else if y == 1 && x == 6 {
                    [0, 255, 0, 255]
                } else {
                    [0, 0, 0, 255]
                };
                let offset = (y * 8 + x) * 4;
                assert_eq!(
                    &frame.rgba8[offset..offset + 4],
                    &expected,
                    "phase {phase} pixel {x},{y}"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_channel_root_383_disables_native_global_offset_without_moving_its_child_locally() {
    let mut vm = CmvsPs2aVmState::new(0);
    vm.native_stage_size = [8, 4];
    vm.native_render_size = [4, 2];
    vm.native_frame_delta_ms = 10;
    command(&mut vm, 320, &[0, 0, 2, 1, 8, 4]);
    command(&mut vm, 384, &[0, u32::MAX]);
    command(&mut vm, 387, &[0, u32::MAX, 2, 1]);
    command(&mut vm, 376, &[0, 0]);
    command(&mut vm, 384, &[0, 0]);
    command(&mut vm, 386, &[0, 0, 0, 0, 1, 1]);
    command(&mut vm, 387, &[0, 0, 1, 0]);
    command(&mut vm, 378, &[0, 0, 1]);
    command(&mut vm, 549, &[0, 1, 10, u32::MAX]);
    crate::vm::screen_shake::advance(&mut vm).unwrap();
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    scene.clear(8, 4).unwrap();
    let atlas = TextureFrame::from_vec(1, 1, vec![0, 0, 255, 255]).unwrap();
    for (disabled, blue_x) in [(false, 5), (true, 3)] {
        if disabled {
            command(&mut vm, 383, &[0, u32::MAX]);
        }
        let mut commands = Vec::new();
        scene
            .append_channel_images(&vm, 0, "fixture:shake", &atlas, &mut commands)
            .unwrap();
        let frame = scene.draw(8, 4, commands).unwrap();
        for y in 0..4 {
            for x in 0..8 {
                let expected = if y == 1 && x == blue_x {
                    [0, 0, 255, 255]
                } else {
                    [0, 0, 0, 255]
                };
                let offset = (y * 8 + x) * 4;
                assert_eq!(
                    &frame.rgba8[offset..offset + 4],
                    &expected,
                    "disabled {disabled} pixel {x},{y}"
                );
            }
        }
    }
}

#[test]
fn native_shake_wait_keeps_operands_until_the_frame_clock_retires_the_owner() {
    let mut vm = CmvsPs2aVmState::new(0);
    vm.native_frame_delta_ms = 10;
    command(&mut vm, 549, &[0, 1, 10, 0]);
    command(&mut vm, 552, &[1]);
    let pc = vm.program_counter;
    let stack = vm.stack_cursor_bytes;
    assert!(vm.dispatch_stopped);
    let poll = |vm: &mut CmvsPs2aVmState| {
        vm.dispatch_stopped = false;
        crate::execute_cmvs390_frame(
            vm,
            &crate::CmvsPs2aInstructionFrame::Command {
                span: crate::CmvsPs2aSourceSpan {
                    offset: vm.program_counter,
                    byte_length: 2,
                },
                command_id: 552,
            },
        )
        .unwrap();
    };
    crate::vm::screen_shake::advance(&mut vm).unwrap();
    poll(&mut vm);
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (pc, stack));
    assert!(vm.dispatch_stopped);
    crate::vm::screen_shake::advance(&mut vm).unwrap();
    poll(&mut vm);
    assert_eq!(
        (vm.program_counter, vm.stack_cursor_bytes),
        (pc + 2, stack - 4)
    );
    assert_eq!(vm.interpreter_words[&81220], 1);
    assert!(!vm.dispatch_stopped);
}
