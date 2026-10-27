use super::parent_load_tests::{command, fixture, write_image};
use super::*;
use crate::CmvsResourceSearchBank;

const COLOR: [u8; 4] = [240, 80, 24, 255];

fn bind_plane(scene: &mut CmvsScene, vm: &mut CmvsPs2aVmState, archive: &CmvsArchive) {
    command(vm, 32, &[0]);
    vm.process_string_slots.insert(0, b"plane.pb3".to_vec());
    command(vm, 48, &[0, 0x8000_0000]);
    let uri = archive
        .resolve_native_resource(vm, CmvsResourceSearchBank::Texture, "plane.pb3")
        .unwrap();
    scene.bind_parent_image(vm, 0, &uri, archive).unwrap();
    command(vm, 64, &[0]);
    command(vm, 68, &[0, u32::MAX, 0, 0, 4, 2]);
}

fn assert_rectangle(frame: &TextureFrame, bounds: Option<(u32, u32, u32, u32)>) {
    for y in 0..frame.height {
        for x in 0..frame.width {
            let inside = bounds.is_some_and(|(left, top, right, bottom)| {
                x >= left && x < right && y >= top && y < bottom
            });
            let offset = ((y * frame.width + x) * 4) as usize;
            assert_eq!(
                &frame.rgba8[offset..offset + 4],
                if inside { &COLOR } else { &[0, 0, 0, 255] },
                "pixel {x},{y}"
            );
        }
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_basic_pivot_keeps_source_uv_and_float_destination_independent() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "plane.pb3", 4, 2, COLOR);
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    bind_plane(&mut scene, &mut vm, &archive);
    command(&mut vm, 69, &[0, u32::MAX, 6, 3]);
    command(&mut vm, 70, &[0, u32::MAX, 2, 1]);
    let frame = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (16, 16),
            None,
        )
        .unwrap()
        .unwrap();
    assert_rectangle(&frame, Some((4, 2, 8, 4)));
    // 85 selects kind 5; 84 writes float XY without changing the source crop.
    command(&mut vm, 85, &[0, u32::MAX, 2]);
    command(
        &mut vm,
        84,
        &[0, u32::MAX, 3.0f32.to_bits(), 5.0f32.to_bits()],
    );
    command(&mut vm, 42, &[0, u32::MAX, 8, 8]);
    let frame = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (16, 16),
            None,
        )
        .unwrap()
        .unwrap();
    assert_rectangle(&frame, Some((2, 8, 10, 12)));
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_basic_camera_projection_obeys_depth_scale_and_near_plane() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "plane.pb3", 4, 2, COLOR);
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    bind_plane(&mut scene, &mut vm, &archive);
    command(&mut vm, 70, &[0, u32::MAX, 2, 1]);
    command(&mut vm, 66, &[0, u32::MAX]);
    command(&mut vm, 97, &[32.0f32.to_bits(), 24.0f32.to_bits()]);
    command(
        &mut vm,
        96,
        &[8.0f32.to_bits(), 6.0f32.to_bits(), 10.0f32.to_bits()],
    );
    command(&mut vm, 115, &[0, u32::MAX, 3.0f32.to_bits()]);
    command(&mut vm, 116, &[0, u32::MAX, 10.0f32.to_bits()]);
    command(&mut vm, 112, &[0, u32::MAX, 10.0f32.to_bits()]);
    let frame = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (32, 24),
            None,
        )
        .unwrap()
        .unwrap();
    assert_rectangle(&frame, Some((14, 11, 18, 13)));
    command(&mut vm, 95, &[0, 2]);
    command(&mut vm, 100, &[0.0f32.to_bits()]);
    command(&mut vm, 112, &[0, u32::MAX, 20.0f32.to_bits()]);
    let frame = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (32, 24),
            None,
        )
        .unwrap()
        .unwrap();
    assert_rectangle(&frame, Some((8, 8, 24, 16)));
    command(&mut vm, 116, &[0, u32::MAX, 9.0f32.to_bits()]);
    let frame = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (32, 24),
            None,
        )
        .unwrap()
        .unwrap();
    assert_rectangle(&frame, None);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_basic_kind_three_selects_its_own_camera_bank() {
    let (root, archive, mut vm) = fixture();
    // This fixture selects a 32x24 native display mode and fullscreen target;
    // command 356 must exercise the real viewport consumer with bound extents.
    vm.native_stage_size = [32, 24];
    vm.native_render_size = [32, 24];
    vm.native_view.fullscreen_size = Some([32, 24]);
    write_image(root.path(), "plane.pb3", 4, 2, COLOR);
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    bind_plane(&mut scene, &mut vm, &archive);
    command(&mut vm, 94, &[1, 1]);
    command(&mut vm, 70, &[0, u32::MAX, 2, 1]);
    command(&mut vm, 76, &[0, u32::MAX, 1]);
    command(&mut vm, 89, &[1, 32.0f32.to_bits(), 24.0f32.to_bits()]);
    command(
        &mut vm,
        88,
        &[1, 8.0f32.to_bits(), 6.0f32.to_bits(), 10.0f32.to_bits()],
    );
    command(&mut vm, 95, &[1, 3]);
    command(&mut vm, 92, &[1, 0]);
    command(&mut vm, 93, &[1, 8.0f32.to_bits(), 6.0f32.to_bits()]);
    command(&mut vm, 103, &[1, 2.0f32.to_bits(), 2.0f32.to_bits()]);
    command(&mut vm, 114, &[0, u32::MAX, 1.0f32.to_bits()]);
    command(&mut vm, 115, &[0, u32::MAX, 2.0f32.to_bits()]);
    command(&mut vm, 116, &[0, u32::MAX, 10.0f32.to_bits()]);
    command(&mut vm, 112, &[0, u32::MAX, 10.0f32.to_bits()]);
    let frame = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (32, 24),
            None,
        )
        .unwrap()
        .unwrap();
    assert_rectangle(&frame, Some((8, 7, 12, 9)));
    command(&mut vm, 94, &[1, 0]);
    let frame = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (32, 24),
            None,
        )
        .unwrap()
        .unwrap();
    assert_rectangle(&frame, None);
    command(&mut vm, 94, &[1, 1]);
    // The same real global view predicate gates only camera-one's queue.
    // Construct the playback record before 279, which validates that slot.
    command(&mut vm, 320, &[0, 0, 0, 0, 32, 24]);
    command(&mut vm, 356, &[1]);
    // 356 yields the current dispatch; the next command runs next frame.
    vm.dispatch_stopped = false;
    command(&mut vm, 748, &[1, 1]);
    command(&mut vm, 279, &[0, 0]);
    let frame = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (32, 24),
            None,
        )
        .unwrap()
        .unwrap();
    assert_rectangle(&frame, None);
    command(&mut vm, 279, &[0, 1]);
    let frame = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (32, 24),
            None,
        )
        .unwrap()
        .unwrap();
    assert_rectangle(&frame, Some((8, 7, 12, 9)));
    command(&mut vm, 279, &[0, 0]);
    command(&mut vm, 356, &[0]);
    vm.dispatch_stopped = false;
    let frame = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (32, 24),
            None,
        )
        .unwrap()
        .unwrap();
    assert_rectangle(&frame, Some((8, 7, 12, 9)));
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_basic_child_kind_must_match_native_parent_queue() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "plane.pb3", 4, 2, COLOR);
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    bind_plane(&mut scene, &mut vm, &archive);
    let uri = archive
        .resolve_native_resource(&vm, CmvsResourceSearchBank::Texture, "plane.pb3")
        .unwrap();
    let child = CmvsTextureSlot::Child {
        parent: 0,
        child: 1,
    };
    scene.bind(child, &uri, &archive).unwrap();
    vm.texture_children.get_mut(&0).unwrap().insert(
        1,
        crate::CmvsTextureChildState {
            surface_initialized: true,
            basic_kind: 5,
            rect_words: Some([0, 0, 4, 2]),
            float_position_words: [6.0f32.to_bits(), 4.0f32.to_bits(), 0],
            ..Default::default()
        },
    );
    command(&mut vm, 68, &[0, u32::MAX, 0, 0, 0, 0]);
    let frame = scene
        .render(&mut vm, child, &archive, (16, 16), None)
        .unwrap()
        .unwrap();
    assert_rectangle(&frame, None);
    command(&mut vm, 85, &[0, u32::MAX, 2]);
    let frame = scene
        .render(&mut vm, child, &archive, (16, 16), None)
        .unwrap()
        .unwrap();
    assert_rectangle(&frame, Some((6, 4, 10, 6)));
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_basic_kind_five_solid_keeps_native_integer_destination() {
    let (_root, archive, mut vm) = fixture();
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    command(&mut vm, 32, &[0]);
    command(&mut vm, 64, &[0]);
    command(&mut vm, 67, &[0, u32::MAX, 4, 2, 0x00f0_5018]);
    command(&mut vm, 69, &[0, u32::MAX, 6, 3]);
    command(&mut vm, 70, &[0, u32::MAX, 2, 1]);
    command(&mut vm, 85, &[0, u32::MAX, 2]);
    command(
        &mut vm,
        84,
        &[0, u32::MAX, 12.0f32.to_bits(), 12.0f32.to_bits()],
    );
    let frame = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (16, 16),
            None,
        )
        .unwrap()
        .unwrap();
    assert_rectangle(&frame, Some((4, 2, 8, 4)));
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_basic_blend_zero_ignores_source_alpha_and_screen_uses_local_tint_once() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "plane.pb3", 4, 2, [240, 80, 24, 128]);
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    bind_plane(&mut scene, &mut vm, &archive);
    command(&mut vm, 72, &[0, u32::MAX, 64]);
    command(&mut vm, 75, &[0, u32::MAX, 0]);
    let frame = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (8, 6), None)
        .unwrap()
        .unwrap();
    for y in 0..frame.height {
        for x in 0..frame.width {
            let offset = ((y * frame.width + x) * 4) as usize;
            let expected = if x < 4 && y < 2 {
                [240, 80, 24, 128]
            } else {
                [0, 0, 0, 255]
            };
            assert_eq!(&frame.rgba8[offset..offset + 4], &expected);
        }
    }
    command(&mut vm, 75, &[0, u32::MAX, 3]);
    let frame = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (8, 6), None)
        .unwrap()
        .unwrap();
    for y in 0..frame.height {
        for x in 0..frame.width {
            let offset = ((y * frame.width + x) * 4) as usize;
            let pixel = &frame.rgba8[offset..offset + 4];
            let expected = if x < 4 && y < 2 {
                [60u8, 20, 6]
            } else {
                [0; 3]
            };
            for channel in 0..3 {
                assert!(
                    pixel[channel].abs_diff(expected[channel]) <= 1,
                    "pixel {x},{y} channel {channel}: {pixel:?}"
                );
            }
            assert_eq!(pixel[3], 255);
        }
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_basic_projected_queue_orders_float_depth_not_command_71() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "plane.pb3", 4, 2, COLOR);
    write_image(root.path(), "far.pb3", 4, 2, [10, 30, 230, 255]);
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    bind_plane(&mut scene, &mut vm, &archive);
    command(&mut vm, 32, &[1]);
    vm.process_string_slots.insert(1, b"far.pb3".to_vec());
    command(&mut vm, 48, &[1, 0x8000_0001]);
    let uri = archive
        .resolve_native_resource(&vm, CmvsResourceSearchBank::Texture, "far.pb3")
        .unwrap();
    scene.bind_parent_image(&mut vm, 1, &uri, &archive).unwrap();
    command(&mut vm, 64, &[1]);
    command(&mut vm, 68, &[1, u32::MAX, 0, 0, 4, 2]);
    command(&mut vm, 97, &[8.0f32.to_bits(), 6.0f32.to_bits()]);
    for (parent, depth, integer_order) in [(0, 10.0f32, 0), (1, 20.0f32, 100)] {
        command(&mut vm, 66, &[parent, u32::MAX]);
        command(&mut vm, 71, &[parent, u32::MAX, integer_order]);
        command(&mut vm, 115, &[parent, u32::MAX, 3.0f32.to_bits()]);
        command(&mut vm, 116, &[parent, u32::MAX, depth.to_bits()]);
        command(&mut vm, 112, &[parent, u32::MAX, depth.to_bits()]);
    }
    let frame = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (8, 6), None)
        .unwrap()
        .unwrap();
    assert_rectangle(&frame, Some((4, 3, 8, 5)));
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_texture44_detaches_flat_parent_position_scale_alpha_and_reference_basis() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "plane.pb3", 4, 2, [255, 0, 0, 255]);
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    bind_plane(&mut scene, &mut vm, &archive);
    command(&mut vm, 68, &[0, u32::MAX, 0, 0, 0, 0]);
    command(&mut vm, 42, &[0, u32::MAX, 32, 24]);
    command(&mut vm, 69, &[0, u32::MAX, 8, 8]);
    command(&mut vm, 70, &[0, u32::MAX, 5, 4]);
    command(&mut vm, 72, &[0, u32::MAX, 128]);
    command(
        &mut vm,
        73,
        &[0, u32::MAX, 2.0f32.to_bits(), 3.0f32.to_bits()],
    );
    command(&mut vm, 34, &[0, 1]);
    command(&mut vm, 80, &[0, 1]);
    command(&mut vm, 42, &[0, 1, 16, 24]);
    command(&mut vm, 68, &[0, 1, 1, 0, 2, 1]);
    command(&mut vm, 69, &[0, 1, 3, 2]);
    command(&mut vm, 70, &[0, 1, 1, 1]);
    command(&mut vm, 72, &[0, 1, 192]);
    command(&mut vm, 73, &[0, 1, 1.0f32.to_bits(), 2.0f32.to_bits()]);
    let child = CmvsTextureSlot::Child {
        parent: 0,
        child: 1,
    };
    for phase in 0..4 {
        if phase == 1 {
            command(&mut vm, 44, &[0, 1]);
        } else if phase == 2 {
            command(&mut vm, 72, &[0, u32::MAX, 0]);
        } else if phase == 3 {
            // Replacing only the basic must retain the header detach bit.
            command(&mut vm, 80, &[0, 1]);
            command(&mut vm, 68, &[0, 1, 1, 0, 2, 1]);
            command(&mut vm, 69, &[0, 1, 3, 2]);
            command(&mut vm, 70, &[0, 1, 1, 1]);
            command(&mut vm, 72, &[0, 1, 192]);
            command(&mut vm, 73, &[0, 1, 1.0f32.to_bits(), 2.0f32.to_bits()]);
        }
        let frame = scene
            .render(&mut vm, child, &archive, (32, 24), None)
            .unwrap()
            .unwrap();
        for y in 0..24 {
            for x in 0..32 {
                let inside = if phase == 0 {
                    (12..16).contains(&x) && (6..12).contains(&y)
                } else {
                    (4..8).contains(&x) && (0..4).contains(&y)
                };
                let expected = if inside {
                    [if phase == 0 { 96 } else { 192 }, 0, 0, 255]
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

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_texture44_selects_child_camera_context_without_parent_pivot_or_rotation() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "plane.pb3", 4, 2, [255, 0, 0, 255]);
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    bind_plane(&mut scene, &mut vm, &archive);
    command(&mut vm, 68, &[0, u32::MAX, 0, 0, 0, 0]);
    command(&mut vm, 42, &[0, u32::MAX, 32, 24]);
    command(&mut vm, 66, &[0, u32::MAX]);
    command(&mut vm, 70, &[0, u32::MAX, 2, 1]);
    command(&mut vm, 72, &[0, u32::MAX, 128]);
    command(
        &mut vm,
        73,
        &[0, u32::MAX, 2.0f32.to_bits(), 3.0f32.to_bits()],
    );
    command(&mut vm, 74, &[0, u32::MAX, 180]);
    command(&mut vm, 115, &[0, u32::MAX, 3.0f32.to_bits()]);
    command(&mut vm, 116, &[0, u32::MAX, 10.0f32.to_bits()]);
    command(&mut vm, 112, &[0, u32::MAX, 10.0f32.to_bits()]);
    command(&mut vm, 97, &[32.0f32.to_bits(), 24.0f32.to_bits()]);
    command(
        &mut vm,
        96,
        // 96 configures world width/height/far, not camera XYZ.
        &[10.0f32.to_bits(), 6.0f32.to_bits(), 10.0f32.to_bits()],
    );
    command(&mut vm, 34, &[0, 1]);
    command(&mut vm, 80, &[0, 1]);
    command(&mut vm, 66, &[0, 1]);
    command(&mut vm, 42, &[0, 1, 16, 24]);
    command(&mut vm, 68, &[0, 1, 0, 0, 2, 1]);
    command(&mut vm, 69, &[0, 1, 3, 2]);
    command(&mut vm, 70, &[0, 1, 1, 1]);
    command(&mut vm, 72, &[0, 1, 192]);
    command(&mut vm, 115, &[0, 1, 3.0f32.to_bits()]);
    command(&mut vm, 116, &[0, 1, 20.0f32.to_bits()]);
    command(&mut vm, 112, &[0, 1, 20.0f32.to_bits()]);
    let child = CmvsTextureSlot::Child {
        parent: 0,
        child: 1,
    };
    for phase in 0..5 {
        match phase {
            1 => {
                command(&mut vm, 44, &[0, 1]);
            }
            2 => {
                command(&mut vm, 72, &[0, u32::MAX, 0]);
            }
            3 => {
                command(&mut vm, 116, &[0, 1, 0.25f32.to_bits()]);
                command(&mut vm, 112, &[0, 1, 0.25f32.to_bits()]);
            }
            4 => {
                command(&mut vm, 116, &[0, 1, (-1.0f32).to_bits()]);
            }
            _ => {}
        }
        let frame = scene
            .render(&mut vm, child, &archive, (32, 24), None)
            .unwrap()
            .unwrap();
        for y in 0..24 {
            for x in 0..32 {
                let inside = match phase {
                    0 => (12..16).contains(&x) && (9..12).contains(&y),
                    1..=3 => (30..32).contains(&x) && (22..24).contains(&y),
                    _ => false,
                };
                let expected = if inside {
                    [if matches!(phase, 0 | 3) { 96 } else { 192 }, 0, 0, 255]
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

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_quantized_corner_mesh_keeps_atlas_crop_independent_of_pivot() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "plane.pb3", 16, 16, COLOR);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    bind_plane(&mut scene, &mut vm, &archive);
    // Retain the decoded PB/resource binding and install a generated atlas in
    // its existing image cache. The pivot lies in the atlas's blue area.
    let mut atlas = [0, 0, 255, 255].repeat(16 * 16);
    for y in 8..16 {
        for x in 8..16 {
            let offset = (y * 16 + x) * 4;
            atlas[offset..offset + 4].copy_from_slice(&COLOR);
        }
    }
    let uri = scene.bindings[&CmvsTextureSlot::Parent(0)].clone();
    scene
        .textures
        .insert(
            uri,
            TextureFrame {
                width: 16,
                height: 16,
                rgba8: atlas.into(),
            },
        )
        .unwrap();
    command(&mut vm, 42, &[0, u32::MAX, 24, 20]);
    command(&mut vm, 68, &[0, u32::MAX, 8, 8, 7, 5]);
    command(&mut vm, 70, &[0, u32::MAX, 3, 2]);
    command(
        &mut vm,
        73,
        &[0, u32::MAX, 0.73f32.to_bits(), 1.17f32.to_bits()],
    );
    command(&mut vm, 74, &[0, u32::MAX, 37]);
    command(&mut vm, 85, &[0, u32::MAX, 2]);
    command(
        &mut vm,
        84,
        &[0, u32::MAX, 12.0f32.to_bits(), 10.05f32.to_bits()],
    );
    let frame = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (24, 20),
            None,
        )
        .unwrap()
        .unwrap();
    for (x, y) in [(11, 9), (12, 10), (10, 11), (12, 12)] {
        let offset = (y * 24 + x) * 4;
        assert_eq!(&frame.rgba8[offset..offset + 4], &COLOR, "pixel {x},{y}");
    }
    let offset = (7 * 24 + 12) * 4;
    assert_eq!(&frame.rgba8[offset..offset + 4], &[0, 0, 0, 255]);
}

// Independent 456E80 v3 layout: prefix14, plane DWORD0..14, tail DWORD15..18.
fn native_slot_camera_fixture(enabled: u32, mode: u32, roll: f32) -> Vec<u8> {
    let mut bytes = 3u16.to_le_bytes().to_vec();
    for word in [enabled, 0, 0, mode] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for value in [
        1.0f32, 2.0, 4.0, 0.0, 0.0, 10.0, 8.0, 6.0, 32.0, 24.0, 0.0, 0.0, 0.0, roll, 16.0, 12.0,
        2.0, 3.0,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn restored_native_slot_camera_drives_projection_roll_and_near_plane() {
    use crate::system_save::slot_projection::SavedProjection;
    use crate::vm::native_slot_projection;
    let mut saved = SavedProjection::default();
    saved
        .decode_record(2176, &native_slot_camera_fixture(1, 3, 90.0))
        .unwrap();
    saved
        .decode_record(2177, &native_slot_camera_fixture(1, 3, 0.0))
        .unwrap();
    let mut vm = CmvsPs2aVmState::new(0);
    // Restore replaces poisoned live camera geometry, not just a slot byte blob.
    command(&mut vm, 99, &[180.0f32.to_bits()]);
    command(
        &mut vm,
        98,
        &[99.0f32.to_bits(), 99.0f32.to_bits(), 99.0f32.to_bits()],
    );
    native_slot_projection::apply(&mut vm, &saved, [7, 1]).unwrap();
    let geometry = |vm: &CmvsPs2aVmState, bank| {
        let projected = projection::project(vm, bank, [3.0, 2.0, 14.0], 10.0, 0.0).unwrap();
        assert_eq!(projected.alpha, 255);
        assert_eq!(projected.scale, 1.0);
        basic_geometry::BasicGeometry::camera(
            [0.0; 2],
            [projected.scale; 2],
            0.0,
            [0.0; 2],
            [1.0; 2],
            0.0,
            projected,
            1.0,
        )
        .corners([4, 2])
    };
    // 42EA90 truncates the f32-rotated coordinates to hundredths; its
    // near-zero cosine can move an axis-aligned ideal corner by one step.
    let ideal = [[16.0, 16.0], [16.0, 20.0], [14.0, 16.0], [14.0, 20.0]];
    for (actual, ideal) in geometry(&vm, 0)
        .into_iter()
        .flatten()
        .zip(ideal.into_iter().flatten())
    {
        assert!((actual - ideal).abs() <= 0.01 + f32::EPSILON * ideal.abs());
    }
    assert_eq!(
        geometry(&vm, 1),
        [[20.0, 12.0], [24.0, 12.0], [20.0, 14.0], [24.0, 14.0]]
    );
    assert!(projection::project(&vm, 0, [3.0, 2.0, 13.0], 10.0, 0.0).is_none());
    assert_eq!(
        projection::project(&vm, 0, [3.0, 2.0, 13.5], 10.0, 0.0)
            .unwrap()
            .alpha,
        128
    );
    // Existing script setters must operate on the restored camera owners.
    command(&mut vm, 94, &[1, 0]);
    assert!(projection::project(&vm, 1, [3.0, 2.0, 14.0], 10.0, 0.0).is_none());
    command(&mut vm, 91, &[0, 0.0f32.to_bits()]);
    assert_eq!(
        geometry(&vm, 0),
        [[20.0, 12.0], [24.0, 12.0], [20.0, 14.0], [24.0, 14.0]]
    );
    let (recaptured, scalars) = native_slot_projection::capture(&vm).unwrap();
    assert_eq!(recaptured.cameras[0].plane.as_ref().unwrap().roll, 0.0);
    assert_eq!(recaptured.cameras[1].enabled, 0);
    assert_eq!(scalars, [7, 1]);
}

#[test]
fn restored_native_slot_legacy_plane_uses_its_own_depth_and_alpha_boundary() {
    use crate::system_save::slot_projection::SavedProjection;
    let mut saved = SavedProjection::default();
    saved
        .decode_record(2176, &native_slot_camera_fixture(1, 1, 0.0))
        .unwrap();
    let mut vm = CmvsPs2aVmState::new(0);
    crate::vm::native_slot_projection::apply(&mut vm, &saved, [0, 0]).unwrap();
    let projected = projection::project(&vm, 0, [3.0, 2.0, 14.0], 10.0, 0.0).unwrap();
    assert_eq!(projected.position, [24.0, 12.0]);
    assert_eq!(projected.scale, 1.0);
    assert!(projection::project(&vm, 0, [3.0, 2.0, 4.0], 10.0, 0.0).is_none());
    assert_eq!(
        projection::project(&vm, 0, [3.0, 2.0, 4.25], 10.0, 0.0)
            .unwrap()
            .alpha,
        128
    );
}

#[test]
fn restored_short_camera_retains_plane_and_inactive_transition_uses_real_kind_owner() {
    use crate::system_save::slot_projection::SavedProjection;
    use crate::vm::native_slot_projection;
    let mut saved = SavedProjection::default();
    saved
        .decode_record(2176, &native_slot_camera_fixture(1, 3, 0.0))
        .unwrap();
    let mut vm = CmvsPs2aVmState::new(0);
    native_slot_projection::apply(&mut vm, &saved, [0, 0]).unwrap();
    let mut short = 3u16.to_le_bytes().to_vec();
    for word in [1u32, 0, 7] {
        short.extend_from_slice(&word.to_le_bytes());
    }
    for value in [20.0f32, 18.0, 1.0, 1.0] {
        short.extend_from_slice(&value.to_le_bytes());
    }
    saved.decode_record(2176, &short).unwrap();
    native_slot_projection::apply(&mut vm, &saved, [0, 0]).unwrap();
    let projected = projection::project(&vm, 0, [3.0, 2.0, 14.0], 10.0, 0.0).unwrap();
    assert_eq!(projected.position, [28.0, 18.0]);
    assert_eq!(
        native_slot_projection::capture(&vm)
            .unwrap()
            .0
            .encode_record(2176)
            .unwrap(),
        short
    );
    // Command102 controls native DWORD38, distinct from projection mode0.
    command(&mut vm, 102, &[0, 1]);
    let (saved, scalars) = native_slot_projection::capture(&vm).unwrap();
    assert_eq!(saved.encode_record(2176).unwrap().len(), 154);
    let mut candidate = CmvsPs2aVmState::new(0);
    native_slot_projection::apply(&mut candidate, &saved, scalars).unwrap();
    let projected = projection::project(&candidate, 0, [3.0, 2.0, 14.0], 10.0, 0.0).unwrap();
    assert_eq!(projected.position, [28.0, 18.0]);
    assert_eq!(candidate.screen_words[&38], 1);
    assert_eq!(candidate.screen_words[&0], 3);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_plane_auxiliary_storage_and_pitch_worker_preserve_flat_projection_pixels() {
    use crate::system_save::slot_projection::{SavedCameraTransition, SavedPlane, SavedProjection};
    use crate::vm::{camera_transition, native_slot_projection};
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "plane.pb3", 4, 2, COLOR);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    bind_plane(&mut scene, &mut vm, &archive);
    command(&mut vm, 70, &[0, u32::MAX, 2, 1]);
    command(&mut vm, 66, &[0, u32::MAX]);
    command(&mut vm, 116, &[0, u32::MAX, 10.0f32.to_bits()]);
    command(&mut vm, 112, &[0, u32::MAX, 10.0f32.to_bits()]);
    let mut saved = SavedProjection::default();
    saved.cameras[0].kind = 1;
    saved.cameras[0].plane = Some(SavedPlane {
        mode: 3,
        position: [0.0; 3],
        auxiliary: [2.5, -7.75],
        depth: 10.0,
        extent: [32.0, 24.0],
        output: [32.0, 24.0],
        slope: 0.0,
        pitch: 17.0,
        yaw: 0.0,
        roll: 0.0,
    });
    saved.cameras[0].center = [16.0, 12.0];
    saved.cameras[0].transition = Some(SavedCameraTransition {
        active: 1,
        flags: 0x8000_0000,
        target_position: [0.0; 3],
        target_angles: [45.0, 0.0, 0.0],
        duration: 400,
        origin_position: [0.0; 3],
        origin_angles: [17.0, 0.0, 0.0],
        elapsed: 0,
    });
    // 4566B0 emits pitch, but 42C330/42F270 do not consume it or DWORD4/5.
    native_slot_projection::apply(&mut vm, &saved, [0, 0]).unwrap();
    for delta in [0, 200, 200] {
        camera_transition::advance(&mut vm, delta).unwrap();
        let frame = scene
            .render(
                &mut vm,
                CmvsTextureSlot::Parent(0),
                &archive,
                (32, 24),
                None,
            )
            .unwrap()
            .unwrap();
        assert_rectangle(&frame, Some((14, 11, 18, 13)));
    }
    assert_eq!(f32::from_bits(vm.screen_words[&12]), 45.0);
    let (captured, _) = native_slot_projection::capture(&vm).unwrap();
    let wire = captured.encode_record(2176).unwrap();
    let mut restored = SavedProjection::default();
    restored.decode_record(2176, &wire).unwrap();
    assert_eq!(
        restored.cameras[0].plane.as_ref().unwrap().auxiliary,
        [2.5, -7.75]
    );
    assert_eq!(restored.cameras[0].plane.as_ref().unwrap().pitch, 45.0);
    let before = vm.clone();
    restored.cameras[0].plane.as_mut().unwrap().auxiliary[1] = f32::NAN;
    assert!(native_slot_projection::apply(&mut vm, &restored, [3, 4]).is_err());
    assert_eq!(vm, before);
}
