use super::*;

fn run(vm: &mut CmvsPs2aVmState, command_id: u16, args: &[u32]) {
    let _ = run_action(vm, command_id, args);
}

fn run_action(
    vm: &mut CmvsPs2aVmState,
    command_id: u16,
    args: &[u32],
) -> Option<crate::CmvsPs2aVmAction> {
    // Each call represents a fresh scheduler dispatch after any C004 yield.
    vm.dispatch_stopped = false;
    let span = crate::CmvsPs2aSourceSpan {
        offset: 0,
        byte_length: 2,
    };
    for value in args.iter().rev() {
        crate::vm::execute_cmvs390_frame(
            vm,
            &crate::CmvsPs2aInstructionFrame::Control(
                crate::CmvsPs2aControlInstruction::PushImmediate {
                    span,
                    stack_bytes: 4,
                    value: *value,
                },
            ),
        )
        .unwrap();
    }
    crate::vm::execute_cmvs390_frame(
        vm,
        &crate::CmvsPs2aInstructionFrame::Command { span, command_id },
    )
    .unwrap()
}

fn fixture() -> CmvsPs2aVmState {
    let mut vm = CmvsPs2aVmState::new(0);
    vm.native_stage_size = [8, 4];
    run(&mut vm, 349, &[0, 0, 1, 1, 8, 4, 8, 4]);
    run(&mut vm, 386, &[0, u32::MAX, 0, 0, 1, 1]);
    run(&mut vm, 401, &[0, 0, 0, 0, 1, 1]);
    run(&mut vm, 402, &[0, 0]);
    run(&mut vm, 346, &[0, 5, 2]);
    vm
}

#[test]
fn view_query_reports_the_global_flag_and_invalid_playback_cannot_change_suppression() {
    let mut vm = fixture();
    run(&mut vm, 748, &[1, 7]);
    run(&mut vm, 748, &[0, 0]);
    // Native 4292C0 reads DWORD47 even in window mode with DWORD43 set.
    assert_eq!(vm.interpreter_words[&81220], 1);
    run(&mut vm, 279, &[12, 0]);
    assert_eq!(vm.interpreter_flag_words[&10532] & 0x1000, 0x1000);
    run(&mut vm, 347, &[0, 1]);
    run(&mut vm, 356, &[1]);
    crate::vm::native_view::apply_channel_origins(&mut vm).unwrap();
    assert_eq!(vm.effect_channels[&0].origin(), (1, 1));
    // Any occupied playback record controls the one renderer flag.
    run(&mut vm, 279, &[0, 0]);
    crate::vm::native_view::apply_channel_origins(&mut vm).unwrap();
    vm.native_pointer_present = true;
    vm.pointer_x = 5;
    vm.pointer_y = 2;
    run(&mut vm, 404, &[0]);
    assert_eq!(vm.interpreter_words[&81236], 0);
    run(&mut vm, 748, &[1, 0]);
    run(&mut vm, 748, &[0, 1]);
    assert_eq!(vm.interpreter_words[&81220], 0);
    crate::vm::native_view::apply_channel_origins(&mut vm).unwrap();
    run(&mut vm, 404, &[0]);
    assert_eq!(vm.interpreter_words[&81236], u32::MAX);
}

#[test]
fn active_view_swap_rejects_a_missing_text_owner_before_changing_live_geometry() {
    let mut vm = fixture();
    run(&mut vm, 279, &[0, 0]);
    run(&mut vm, 748, &[1, 1]);
    run(&mut vm, 347, &[0, 1]);
    run(&mut vm, 356, &[1]);
    vm.native_text.remove(&0);
    let error = crate::vm::native_view::apply_channel_origins(&mut vm).unwrap_err();
    assert_eq!(error.code(), "SHINKI_EMU_CMVS_VIEW_TEXT");
    assert_eq!(vm.effect_channels[&0].origin(), (1, 1));
}

#[test]
fn policy_shadow_preserves_279_override_until_a_changed_native_policy_reapplies_it() {
    let mut vm = fixture();
    run(&mut vm, 748, &[1, 1]);
    run(&mut vm, 347, &[0, 1]);
    run(&mut vm, 356, &[1]);
    run(&mut vm, 279, &[0, 0]);
    vm.native_pointer_present = true;
    vm.pointer_x = 5;
    vm.pointer_y = 2;
    let hit = |vm: &mut CmvsPs2aVmState| {
        crate::vm::native_view::apply_channel_origins(vm).unwrap();
        run(vm, 404, &[0]);
        vm.interpreter_words[&81236]
    };
    assert_eq!(hit(&mut vm), 0);
    run(&mut vm, 714, &[]);
    assert_eq!(vm.interpreter_words[&81220], 1);
    assert!(run_action(&mut vm, 713, &[1]).is_none());
    assert!(!vm.dispatch_stopped);
    assert_eq!(hit(&mut vm), 0);
    // An unchanged raw mode also leaves the renderer-only 279 override alone.
    assert!(run_action(&mut vm, 765, &[1]).is_none());
    assert_eq!(hit(&mut vm), 0);
    // Raw UINT_MAX differs before clamping, even though its stored mode
    // becomes the same 1. This changed path reapplies the policy's true flag.
    assert!(run_action(&mut vm, 765, &[u32::MAX]).is_none());
    assert!(!vm.dispatch_stopped);
    assert_eq!(hit(&mut vm), u32::MAX);
    run(&mut vm, 764, &[]);
    assert_eq!(vm.interpreter_words[&81220], 1);
    assert!(matches!(
        run_action(&mut vm, 713, &[0]),
        Some(crate::CmvsPs2aVmAction::SetNativeFullscreen { enabled: true })
    ));
    assert!(vm.dispatch_stopped);
    assert_eq!(vm.interpreter_words[&10708], 1);
    assert_eq!(hit(&mut vm), 0);
    run(&mut vm, 714, &[]);
    assert_eq!(vm.interpreter_words[&81220], 0);
    run(&mut vm, 279, &[0, 1]);
    assert_eq!(hit(&mut vm), u32::MAX);
    assert!(run_action(&mut vm, 713, &[0]).is_none());
    assert_eq!(hit(&mut vm), u32::MAX);
    run(&mut vm, 765, &[2]);
    assert_eq!(hit(&mut vm), 0);
    run(&mut vm, 764, &[]);
    assert_eq!(vm.interpreter_words[&81220], 2);
    run(&mut vm, 356, &[0]);
    assert!(run_action(&mut vm, 713, &[1]).is_none());
    assert!(!vm.dispatch_stopped);
    assert_eq!(hit(&mut vm), u32::MAX);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_script_fullscreen_view_flags_move_image_text_and_pointer_hit_together() {
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    let mut vm = fixture();
    let atlas = TextureFrame::from_vec(1, 1, vec![192, 64, 128, 255]).unwrap();
    vm.native_pointer_present = true;

    let check = |scene: &mut CmvsScene, vm: &mut CmvsPs2aVmState, expected: [i32; 2]| {
        crate::vm::native_view::apply_channel_origins(vm).unwrap();
        let mut commands = Vec::new();
        scene
            .append_channel_images(vm, 0, "fixture:native-view", &atlas, &mut commands)
            .unwrap();
        let frame = scene.draw(8, 4, commands).unwrap();
        for y in 0..4 {
            for x in 0..8 {
                let color = if [x as i32, y as i32] == expected {
                    [192, 64, 128, 255]
                } else {
                    [0, 0, 0, 255]
                };
                let offset = (y * 8 + x) * 4;
                assert_eq!(&frame.rgba8[offset..offset + 4], &color);
            }
        }
        assert_eq!(vm.native_text[&0].position, expected);
        vm.pointer_x = expected[0];
        vm.pointer_y = expected[1];
        vm.input_advance_press = true;
        vm.input_advance_release = false;
        vm.input_confirm_held = true;
        run(vm, 404, &[0]);
        assert_eq!(vm.interpreter_words[&81236], 0);
        assert_eq!(vm.interpreter_words[&81244], 1);
        vm.input_advance_press = false;
        vm.input_advance_release = true;
        vm.input_confirm_held = false;
        run(vm, 404, &[0]);
        assert_eq!(vm.interpreter_words[&81240], 1);
    };

    run(&mut vm, 748, &[1, 1]);
    run(&mut vm, 748, &[0, 0]);
    assert_eq!(vm.interpreter_words[&81220], 1);
    run(&mut vm, 279, &[0, 0]);
    // 356 is the real producer. The 347 gate initially prevents a swap.
    run(&mut vm, 356, &[1]);
    check(&mut scene, &mut vm, [1, 1]);
    run(&mut vm, 347, &[0, 1]);
    check(&mut scene, &mut vm, [5, 2]);

    // The cached predicate deliberately does not reapply edited origins.
    run(&mut vm, 346, &[0, 6, 3]);
    run(&mut vm, 336, &[0, 0, 2, 0]);
    check(&mut scene, &mut vm, [2, 0]);
    run(&mut vm, 356, &[0]);
    check(&mut scene, &mut vm, [2, 0]);
    run(&mut vm, 356, &[1]);
    check(&mut scene, &mut vm, [6, 3]);

    // Disabled 347 preserves both current origin and cached predicate.
    run(&mut vm, 347, &[0, 0]);
    run(&mut vm, 356, &[0]);
    check(&mut scene, &mut vm, [6, 3]);
    run(&mut vm, 347, &[0, 1]);
    check(&mut scene, &mut vm, [2, 0]);
    run(&mut vm, 356, &[1]);
    check(&mut scene, &mut vm, [6, 3]);
    run(&mut vm, 279, &[0, 1]);
    check(&mut scene, &mut vm, [2, 0]);
    run(&mut vm, 279, &[0, 0]);
    check(&mut scene, &mut vm, [6, 3]);
    run(&mut vm, 748, &[1, 0]);
    check(&mut scene, &mut vm, [2, 0]);
    run(&mut vm, 748, &[1, 1]);
    check(&mut scene, &mut vm, [6, 3]);
    // Policy comparisons use retained policy words, not 279's override.
    run(&mut vm, 713, &[1]);
    check(&mut scene, &mut vm, [6, 3]);
    run(&mut vm, 765, &[u32::MAX]);
    check(&mut scene, &mut vm, [2, 0]);
    run(&mut vm, 279, &[0, 0]);
    check(&mut scene, &mut vm, [6, 3]);
    run(&mut vm, 713, &[0]);
    run(&mut vm, 279, &[0, 1]);
    check(&mut scene, &mut vm, [2, 0]);
    run(&mut vm, 713, &[0]);
    check(&mut scene, &mut vm, [2, 0]);
    run(&mut vm, 765, &[2]);
    check(&mut scene, &mut vm, [6, 3]);
}
