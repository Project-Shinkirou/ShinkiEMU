use super::*;

fn run(vm: &mut CmvsPs2aVmState, command_id: u16, args: &[u32]) {
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
    .unwrap();
}

fn fixture() -> CmvsPs2aVmState {
    let mut vm = CmvsPs2aVmState::new(0);
    vm.slot_objects.insert(742 << 8, 1);
    vm.effect_channels.entry(0).or_default();
    vm.effect_elements.entry((0, u32::MAX)).or_default().visible = 1;
    vm.effect_elements.get_mut(&(0, u32::MAX)).unwrap().source = Some([0, 0, 0, 0]);
    run(&mut vm, 336, &[0, 0, 1, 0]);
    run(&mut vm, 376, &[0, 31]);
    run(&mut vm, 387, &[0, 31, 3, 0]);
    // The final atlas frame and hit rectangle of quad 15 occupy the words
    // formerly clobbered by treating DWORD618/619/622 as u16 indices.
    run(&mut vm, 400, &[0, 15, 4, 1, 0, 1, 1, 0, 0]);
    run(&mut vm, 401, &[0, 15, 3, 0, 1, 1]);
    run(&mut vm, 402, &[0, 15]);
    vm.native_stage_size = [8, 2];
    vm
}

#[test]
fn alternate_origin_commands_preserve_the_visible_quad_input_location() {
    let mut vm = fixture();
    vm.native_pointer_present = true;
    vm.pointer_x = 4;
    vm.pointer_y = 0;
    run(&mut vm, 346, &[0, 123, 456]);
    for enabled in [1, 0] {
        run(&mut vm, 347, &[0, enabled]);
        vm.input_advance_press = true;
        vm.input_advance_release = false;
        vm.input_confirm_held = true;
        run(&mut vm, 404, &[0]);
        assert_eq!(vm.interpreter_words[&81236], 15);
        assert_eq!(vm.interpreter_words[&81240], 0);
        assert_eq!(vm.interpreter_words[&81244], 1);
        vm.input_advance_press = false;
        vm.input_advance_release = true;
        vm.input_confirm_held = false;
        run(&mut vm, 404, &[0]);
        assert_eq!(vm.interpreter_words[&81236], 15);
        assert_eq!(vm.interpreter_words[&81240], 1);
        assert!(!vm.input_advance_release);
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_alternate_origin_commands_preserve_the_final_quad_atlas_frame() {
    static INSTANCE: std::sync::LazyLock<wgpu::Instance> =
        std::sync::LazyLock::new(|| roukaku_platform_common::native_wgpu_instance().unwrap());
    let mut scene = CmvsScene::new(&INSTANCE).unwrap();
    let mut vm = fixture();
    run(&mut vm, 346, &[0, 123, 456]);
    run(&mut vm, 347, &[0, 1]);
    // Select after writing the alternate origin, so the real frame reader
    // and sprite consumer observe any accidental atlas geometry overwrite.
    run(&mut vm, 406, &[0, 15, 4]);
    let atlas = TextureFrame {
        width: 4,
        height: 1,
        rgba8: vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ]
        .into(),
    };
    let mut commands = Vec::new();
    scene
        .append_channel_images(&vm, 0, "fixture:origin-atlas", &atlas, &mut commands)
        .unwrap();
    let frame = scene.draw(8, 2, commands).unwrap();
    let mut expected = [0, 0, 0, 255].repeat(16);
    expected[16..20].copy_from_slice(&[0, 255, 0, 255]);
    assert_eq!(frame.rgba8.as_slice(), expected.as_slice());
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_atlas_owner_without_quad_geometry_does_not_draw_the_resource_sheet() {
    static INSTANCE: std::sync::LazyLock<wgpu::Instance> =
        std::sync::LazyLock::new(|| roukaku_platform_common::native_wgpu_instance().unwrap());
    let mut scene = CmvsScene::new(&INSTANCE).unwrap();
    let mut vm = CmvsPs2aVmState::new(0);
    run(&mut vm, 320, &[0, 0, 0, 0, 2, 2]);
    let atlas = TextureFrame {
        width: 2,
        height: 2,
        rgba8: [255, 0, 0, 255].repeat(4).into(),
    };
    let mut commands = Vec::new();
    scene
        .append_channel_images(&vm, 0, "fixture:unconfigured-atlas", &atlas, &mut commands)
        .unwrap();
    let before = scene.draw(2, 2, commands).unwrap();
    for pixel in before.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [0, 0, 0, 255]);
    }
    vm.effect_elements.get_mut(&(0, u32::MAX)).unwrap().source = Some([0, 0, 2, 2]);
    let mut commands = Vec::new();
    scene
        .append_channel_images(&vm, 0, "fixture:unconfigured-atlas", &atlas, &mut commands)
        .unwrap();
    let after = scene.draw(2, 2, commands).unwrap();
    for pixel in after.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [255, 0, 0, 255]);
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_indexed_child_requires_root_basic_even_when_detached() {
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    scene.clear(2, 2).unwrap();
    let mut vm = CmvsPs2aVmState::new(0);
    run(&mut vm, 320, &[0, 0, 0, 0, 2, 2]);
    run(&mut vm, 376, &[0, 0]);
    run(&mut vm, 378, &[0, 0, 1]);
    run(&mut vm, 386, &[0, 0, 0, 0, 2, 2]);
    run(&mut vm, 382, &[0, 0]);
    let atlas = TextureFrame::from_vec(2, 2, [255, 0, 0, 255].repeat(4)).unwrap();
    for has_basic in [false, true, false] {
        run(&mut vm, if has_basic { 384 } else { 385 }, &[0, u32::MAX]);
        let mut commands = Vec::new();
        scene
            .append_channel_images(&vm, 0, "fixture:root-basic-gate", &atlas, &mut commands)
            .unwrap();
        let frame = scene.draw(2, 2, commands).unwrap();
        let expected = if has_basic {
            [255, 0, 0, 255]
        } else {
            [0, 0, 0, 255]
        };
        for pixel in frame.rgba8.as_chunks::<4>().0 {
            assert_eq!(*pixel, expected);
        }
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_channel_reference_basis_scales_window_geometry_before_viewport_clipping() {
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    scene.clear(16, 16).unwrap();
    let mut vm = CmvsPs2aVmState::new(0);
    run(&mut vm, 320, &[0, 0, 0, 0, 16, 16]);
    run(&mut vm, 380, &[0, u32::MAX, 8, 32]);
    run(&mut vm, 386, &[0, u32::MAX, 0, 0, 2, 2]);
    run(&mut vm, 387, &[0, u32::MAX, 7, 7]);
    let atlas = TextureFrame {
        width: 2,
        height: 2,
        rgba8: [255, 0, 0, 255].repeat(4).into(),
    };
    let mut commands = Vec::new();
    scene
        .append_channel_images(&vm, 0, "fixture:clipped-window", &atlas, &mut commands)
        .unwrap();
    let frame = scene.draw(16, 16, commands).unwrap();
    for y in 0..16 {
        for x in 0..16 {
            let offset = (y * 16 + x) * 4;
            let expected = if (14..16).contains(&x) && (14..16).contains(&y) {
                [255, 0, 0, 255]
            } else {
                [0, 0, 0, 255]
            };
            assert_eq!(&frame.rgba8[offset..offset + 4], &expected);
        }
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_channel_child_reference_inherits_until_native382_detaches_the_basis() {
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    scene.clear(16, 16).unwrap();
    let mut vm = CmvsPs2aVmState::new(0);
    run(&mut vm, 320, &[0, 0, 0, 0, 16, 16]);
    run(&mut vm, 384, &[0, u32::MAX]);
    run(&mut vm, 380, &[0, u32::MAX, 8, 16]);
    run(&mut vm, 376, &[0, 0]);
    run(&mut vm, 378, &[0, 0, 1]);
    run(&mut vm, 380, &[0, 0, 4, 16]);
    run(&mut vm, 386, &[0, 0, 0, 0, 1, 1]);
    run(&mut vm, 387, &[0, 0, 2, 2]);
    let atlas = TextureFrame {
        width: 1,
        height: 1,
        rgba8: vec![255, 0, 0, 255].into(),
    };
    for (detached, range) in [(false, 4..6), (true, 8..12)] {
        if detached {
            run(&mut vm, 382, &[0, 0]);
        }
        let mut commands = Vec::new();
        scene
            .append_channel_images(&vm, 0, "fixture:window-basis", &atlas, &mut commands)
            .unwrap();
        let frame = scene.draw(16, 16, commands).unwrap();
        for y in 0..16 {
            for x in 0..16 {
                let offset = (y * 16 + x) * 4;
                let expected = if range.contains(&x) && range.contains(&y) {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 0, 255]
                };
                assert_eq!(&frame.rgba8[offset..offset + 4], &expected);
            }
        }
    }
}

fn basic_channel() -> CmvsPs2aVmState {
    let mut vm = CmvsPs2aVmState::new(0);
    vm.slot_objects.insert(742 << 8, 1);
    vm.effect_channels.entry(0).or_default();
    vm.effect_elements.entry((0, u32::MAX)).or_default().visible = 1;
    run(&mut vm, 384, &[0, u32::MAX]);
    vm
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_channel_basic_pivot_scale_and_descending_child_order_preserve_atlas_uv() {
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    let mut vm = basic_channel();
    run(&mut vm, 336, &[0, 0, 2, 1]);
    run(
        &mut vm,
        395,
        &[0, u32::MAX, 2.0_f32.to_bits(), 1.0_f32.to_bits()],
    );
    for (child, source_x, order) in [(0, 1, 1), (1, 0, 5)] {
        run(&mut vm, 376, &[0, child]);
        run(&mut vm, 378, &[0, child, 1]);
        run(&mut vm, 386, &[0, child, source_x, 0, 2, 1]);
        run(&mut vm, 387, &[0, child, 3, 1]);
        run(&mut vm, 388, &[0, child, order]);
        run(&mut vm, 389, &[0, child, 1, 0]);
        run(
            &mut vm,
            395,
            &[0, child, 0.5_f32.to_bits(), 2.0_f32.to_bits()],
        );
    }
    let atlas =
        TextureFrame::from_vec(3, 1, vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255]).unwrap();
    let mut commands = Vec::new();
    scene
        .append_channel_images(&vm, 0, "fixture:basic-pivot", &atlas, &mut commands)
        .unwrap();
    let frame = scene.draw(12, 8, commands).unwrap();
    for y in 0..8 {
        for x in 0..12 {
            let expected = match (x, y) {
                (7, 3 | 4) => [0, 255, 0, 255],
                (8, 3 | 4) => [0, 0, 255, 255],
                _ => [0, 0, 0, 255],
            };
            let offset = (y * 12 + x) * 4;
            assert_eq!(&frame.rgba8[offset..offset + 4], &expected, "pixel {x},{y}");
        }
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_channel_basic_alpha_and_blend_modes_consume_live_parent_and_local_tint() {
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    let mut vm = basic_channel();
    run(&mut vm, 376, &[0, 0]);
    run(&mut vm, 378, &[0, 0, 1]);
    run(&mut vm, 386, &[0, 0, 0, 0, 1, 1]);
    run(&mut vm, 390, &[0, u32::MAX, 128]);
    run(&mut vm, 390, &[0, 0, 128]);
    let atlas = TextureFrame::from_vec(1, 1, vec![200, 120, 40, 64]).unwrap();
    for (mode, expected) in [
        (1, [13_u8, 8, 3, 255]),
        (0, [200, 120, 40, 64]),
        (2, [13, 8, 3, 255]),
        (3, [100, 60, 20, 255]),
    ] {
        run(&mut vm, 391, &[0, 0, mode]);
        let mut commands = Vec::new();
        scene
            .append_channel_images(&vm, 0, "fixture:basic-blends", &atlas, &mut commands)
            .unwrap();
        let frame = scene.draw(1, 1, commands).unwrap();
        for (actual, expected) in frame.rgba8.iter().zip(expected) {
            assert!(
                actual.abs_diff(expected) <= 1,
                "blend {mode}: {:?}",
                frame.rgba8
            );
        }
    }
    run(&mut vm, 390, &[0, u32::MAX, u32::MAX]);
    let mut commands = Vec::new();
    scene
        .append_channel_images(&vm, 0, "fixture:basic-blends", &atlas, &mut commands)
        .unwrap();
    assert_eq!(
        scene.draw(1, 1, commands).unwrap().rgba8.as_slice(),
        &[0, 0, 0, 255]
    );
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_channel_provider_swap_and_restore_rebind_preserve_pixels_and_source_alpha() {
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    let mut vm = basic_channel();
    run(&mut vm, 376, &[0, 0]);
    run(&mut vm, 378, &[0, 0, 1]);
    run(&mut vm, 386, &[0, 0, 0, 0, 1, 1]);
    run(&mut vm, 391, &[0, 0, 2]);
    for (index, (uri, has_alpha, alpha, expected)) in [
        ("fixture:alpha", true, 64, [70_u8, 110, 170, 255]),
        ("fixture:opaque", false, 255, [200, 120, 40, 255]),
        ("fixture:opaque", false, 255, [200, 120, 40, 255]),
        ("fixture:alpha", true, 64, [70, 110, 170, 255]),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 2 {
            scene = scene.native_restore_candidate().unwrap();
        }
        vm.effect_channels.get_mut(&0).unwrap().image_has_alpha = has_alpha;
        let atlas = TextureFrame::from_vec(1, 1, vec![200, 120, 40, alpha]).unwrap();
        let mut commands = vec![SceneCommand::Rect {
            id: "background".into(),
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            rgba: [20, 80, 160, 255],
        }];
        scene
            .append_channel_images(&vm, 0, uri, &atlas, &mut commands)
            .unwrap();
        let frame = scene.draw(1, 1, commands).unwrap();
        for (actual, expected) in frame.rgba8.iter().zip(expected) {
            assert!(
                actual.abs_diff(expected) <= 1,
                "replacement {index}: {:?}",
                frame.rgba8
            );
        }
    }
    let (_, archive, _) = super::super::parent_load_tests::fixture();
    vm.effect_channels.clear();
    let commands = scene.channel_image_commands(&vm, &archive).unwrap();
    assert_eq!(
        scene.draw(1, 1, commands).unwrap().rgba8.as_slice(),
        &[0, 0, 0, 255]
    );
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_window_tile_cleanup_resize_and_basic_rebuild_leave_no_previous_tail() {
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    let mut vm = basic_channel();
    run(&mut vm, 384, &[0, u32::MAX]);
    run(&mut vm, 387, &[0, u32::MAX, 1, 1]);
    let atlas = TextureFrame::from_vec(1, 1, vec![128, 64, 192, 255]).unwrap();

    // intproc67774 removes ids64..255 before62016 rebuilds the window.
    // A smaller next window must not retain the old right/bottom tiles.
    for (width, height) in [(6, 3), (3, 2), (5, 1)] {
        for child in 64..256 {
            run(&mut vm, 377, &[0, child]);
        }
        for y in 0..height {
            for x in 0..width {
                let child = 64 + y * width + x;
                run(&mut vm, 376, &[0, child]);
                run(&mut vm, 384, &[0, child]);
                run(&mut vm, 386, &[0, child, 0, 0, 1, 1]);
                run(&mut vm, 387, &[0, child, x, y]);
                run(&mut vm, 378, &[0, child, 1]);
            }
        }
        let mut commands = Vec::new();
        scene
            .append_channel_images(&vm, 0, "fixture:window-rebuild", &atlas, &mut commands)
            .unwrap();
        let frame = scene.draw(8, 5, commands).unwrap();
        for y in 0..5 {
            for x in 0..8 {
                let expected = if (1..1 + width).contains(&x) && (1..1 + height).contains(&y) {
                    [128, 64, 192, 255]
                } else {
                    [0, 0, 0, 255]
                };
                let offset = ((y * 8 + x) * 4) as usize;
                assert_eq!(
                    &frame.rgba8[offset..offset + 4],
                    &expected,
                    "{width}x{height}: {x},{y}"
                );
            }
        }
    }

    // 385 releases a basic, not its element or inherited atlas provider.
    run(&mut vm, 385, &[0, 64]);
    run(&mut vm, 379, &[0, 64]);
    assert_eq!(vm.interpreter_words[&81220], 1);
    let mut commands = Vec::new();
    scene
        .append_channel_images(&vm, 0, "fixture:window-rebuild", &atlas, &mut commands)
        .unwrap();
    let frame = scene.draw(8, 5, commands).unwrap();
    assert_eq!(&frame.rgba8[36..40], &[0, 0, 0, 255]);
    assert_eq!(&frame.rgba8[40..44], &[128, 64, 192, 255]);

    // 384 resets every basic property, retaining the element header:
    // visibility, reference basis and inherited provider stay live.
    run(&mut vm, 384, &[0, 64]);
    run(&mut vm, 390, &[0, 64, 0]);
    run(&mut vm, 388, &[0, 64, u32::MAX]);
    run(&mut vm, 389, &[0, 64, 20, 20]);
    run(&mut vm, 395, &[0, 64, 4.0_f32.to_bits(), 4.0_f32.to_bits()]);
    run(&mut vm, 384, &[0, 64]);
    run(&mut vm, 386, &[0, 64, 0, 0, 1, 1]);
    let mut commands = Vec::new();
    scene
        .append_channel_images(&vm, 0, "fixture:window-rebuild", &atlas, &mut commands)
        .unwrap();
    let frame = scene.draw(8, 5, commands).unwrap();
    assert_eq!(&frame.rgba8[36..40], &[128, 64, 192, 255]);

    run(&mut vm, 377, &[0, u32::MAX]);
    run(&mut vm, 376, &[0, 64]);
    run(&mut vm, 379, &[0, 64]);
    assert_eq!(vm.interpreter_words[&81220], 0);
    let mut commands = Vec::new();
    scene
        .append_channel_images(&vm, 0, "fixture:window-rebuild", &atlas, &mut commands)
        .unwrap();
    assert!(
        scene
            .draw(8, 5, commands)
            .unwrap()
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [0, 0, 0, 255])
    );

    // Recreating the owner explicitly starts hidden and has no old tiles.
    run(&mut vm, 376, &[0, u32::MAX]);
    run(&mut vm, 384, &[0, u32::MAX]);
    run(&mut vm, 386, &[0, u32::MAX, 0, 0, 1, 1]);
    run(&mut vm, 378, &[0, u32::MAX, 1]);
    let mut commands = Vec::new();
    scene
        .append_channel_images(&vm, 0, "fixture:window-rebuild", &atlas, &mut commands)
        .unwrap();
    let frame = scene.draw(8, 5, commands).unwrap();
    assert_eq!(&frame.rgba8[..4], &[128, 64, 192, 255]);
    assert!(
        frame.rgba8[4..]
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [0, 0, 0, 255])
    );
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_channel382_detaches_parent_basic_context_and_uses_child_basis_and_pivot() {
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    scene.clear(32, 24).unwrap();
    let mut vm = CmvsPs2aVmState::new(0);
    run(&mut vm, 320, &[0, 0, 0, 0, 32, 24]);
    run(&mut vm, 384, &[0, u32::MAX]);
    run(&mut vm, 380, &[0, u32::MAX, 32, 24]);
    run(&mut vm, 387, &[0, u32::MAX, 8, 8]);
    run(&mut vm, 389, &[0, u32::MAX, 5, 4]);
    run(&mut vm, 390, &[0, u32::MAX, 128]);
    run(
        &mut vm,
        395,
        &[0, u32::MAX, 2.0f32.to_bits(), 3.0f32.to_bits()],
    );
    run(&mut vm, 376, &[0, 0]);
    run(&mut vm, 378, &[0, 0, 1]);
    run(&mut vm, 380, &[0, 0, 16, 24]);
    run(&mut vm, 386, &[0, 0, 1, 0, 2, 1]);
    run(&mut vm, 387, &[0, 0, 3, 2]);
    run(&mut vm, 389, &[0, 0, 1, 1]);
    run(&mut vm, 390, &[0, 0, 192]);
    run(&mut vm, 395, &[0, 0, 1.0f32.to_bits(), 2.0f32.to_bits()]);
    // Sprite quads use the shared linear sampler. A uniform provider keeps
    // this test's exact edge/alpha oracle independent of magnified UV blends.
    let atlas = TextureFrame::from_vec(3, 1, [0, 255, 0, 255].repeat(3)).unwrap();
    for phase in 0..3 {
        if phase == 1 {
            run(&mut vm, 382, &[0, 0]);
        } else if phase == 2 {
            // Root visibility still gates the subtree, but its basic alpha
            // no longer suppresses a detached child (445130/4454D0).
            run(&mut vm, 390, &[0, u32::MAX, 0]);
        }
        let mut commands = Vec::new();
        scene
            .append_channel_images(&vm, 0, "fixture:detached-context", &atlas, &mut commands)
            .unwrap();
        let frame = scene.draw(32, 24, commands).unwrap();
        for y in 0..24 {
            for x in 0..32 {
                let expected = if phase == 0 && (6..12).contains(&y) {
                    match x {
                        12..16 => [0, 96, 0, 255],
                        _ => [0, 0, 0, 255],
                    }
                } else if phase != 0 && (0..4).contains(&y) {
                    match x {
                        4..8 => [0, 192, 0, 255],
                        _ => [0, 0, 0, 255],
                    }
                } else {
                    [0, 0, 0, 255]
                };
                let offset = (y * 32 + x) * 4;
                assert_eq!(
                    &frame.rgba8[offset..offset + 4],
                    &expected,
                    "phase {phase} pixel {x},{y}"
                );
            }
        }
    }
}
