use super::*;
use crate::{
    CmvsPs2aControlInstruction, CmvsPs2aInstructionFrame, CmvsPs2aSourceSpan, CmvsPs2aVmAction,
    CmvsResourceSearchBank, execute_cmvs390_frame,
};

pub(crate) fn fixture() -> (tempfile::TempDir, CmvsArchive, CmvsPs2aVmState) {
    let (root, mut archive) = crate::archive::cache_tests::fixture();
    archive
        .bind_game_root(root.path().canonicalize().unwrap())
        .unwrap();
    let mut vm = CmvsPs2aVmState::new(0);
    vm.resource_search_paths.insert(
        CmvsResourceSearchBank::Texture,
        BTreeMap::from([(0, String::new())]),
    );
    vm.resource_search_paths.insert(
        CmvsResourceSearchBank::Native0d00,
        BTreeMap::from([(0, String::new())]),
    );
    (root, archive, vm)
}

/// Constant PB3 blocks, through the real header/plane decoder.
pub(crate) fn write_image(
    root: &std::path::Path,
    name: &str,
    width: u16,
    height: u16,
    rgba: [u8; 4],
) {
    write_image_with_bpp(root, name, width, height, rgba, 32);
}

fn write_image_with_bpp(
    root: &std::path::Path,
    name: &str,
    width: u16,
    height: u16,
    rgba: [u8; 4],
    bits: u16,
) {
    assert!(matches!(bits, 24 | 32));
    let channels = usize::from(bits / 8);
    assert!(width != 0 && height != 0);
    let blocks = usize::from(width).div_ceil(16) * usize::from(height).div_ceil(16);
    let flags = blocks.div_ceil(8);
    let block_bytes = 12 + flags + blocks;
    let data1 = 84;
    let data2 = data1 + channels * 4 + channels * block_bytes;
    let source_len = (data2 + channels * 4 + 64).max(256);
    let mut source = vec![0u8; source_len];
    source[..4].copy_from_slice(b"PB3B");
    source[4..8].copy_from_slice(&(source_len as u32).to_le_bytes());
    source[0x1c..0x1e].copy_from_slice(&1u16.to_le_bytes());
    source[0x1e..0x20].copy_from_slice(&width.to_le_bytes());
    source[0x20..0x22].copy_from_slice(&height.to_le_bytes());
    source[0x22..0x24].copy_from_slice(&bits.to_le_bytes());
    source[0x2c..0x30].copy_from_slice(&(data1 as u32).to_le_bytes());
    source[0x30..0x34].copy_from_slice(&(data2 as u32).to_le_bytes());
    for (channel, value) in [rgba[2], rgba[1], rgba[0], rgba[3]]
        .into_iter()
        .take(channels)
        .enumerate()
    {
        let table = data1 + channel * 4;
        source[table..table + 4].copy_from_slice(&(block_bytes as u32).to_le_bytes());
        let header = data1 + channels * 4 + channel * block_bytes;
        source[header..header + 4].copy_from_slice(&(flags as u32).to_le_bytes());
        source[header + 12..header + 12 + flags].fill(0xff);
        source[header + 12 + flags..header + block_bytes].fill(value);
    }
    // The zero tail key leaves this synthetic metadata prefix unchanged.
    std::fs::write(root.join(name), source).unwrap();
}

/// `words` are in native top-of-stack argument order.
pub(crate) fn command(
    vm: &mut CmvsPs2aVmState,
    id: u16,
    words: &[u32],
) -> Option<CmvsPs2aVmAction> {
    for value in words.iter().rev() {
        let span = CmvsPs2aSourceSpan {
            offset: vm.program_counter,
            byte_length: 8,
        };
        execute_cmvs390_frame(
            vm,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span,
                stack_bytes: 4,
                value: *value,
            }),
        )
        .unwrap();
    }
    let span = CmvsPs2aSourceSpan {
        offset: vm.program_counter,
        byte_length: 2,
    };
    execute_cmvs390_frame(
        vm,
        &CmvsPs2aInstructionFrame::Command {
            span,
            command_id: id,
        },
    )
    .unwrap()
}

pub(super) fn surface_action(scene: &mut CmvsScene, action: Option<CmvsPs2aVmAction>) {
    match action {
        Some(CmvsPs2aVmAction::RemoveTextureParentSurface { parent_slot }) => {
            scene.release_parent_surface(parent_slot);
        }
        Some(CmvsPs2aVmAction::RemoveTextureChildSurface {
            parent_slot,
            child_id,
        }) => {
            scene.release_child_surface(parent_slot, child_id);
        }
        Some(CmvsPs2aVmAction::ClearTextureParentSurfaces) => scene.clear_parent_surfaces(),
        other => panic!("surface lifetime operation did not reach its consumer: {other:?}"),
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_parent_load_command_replaces_pixels_without_pointer_hit_or_old_filter() {
    let (root, archive, mut vm) = fixture();
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    surface_action(&mut scene, command(&mut vm, 32, &[0]));
    for (name, color, width, height, bits) in [
        ("first.pb3", [230, 20, 10, 255], 2, 2, 32),
        ("next.pb3", [10, 180, 220, 255], 3, 1, 24),
    ] {
        write_image_with_bpp(root.path(), name, width, height, color, bits);
        vm.process_string_slots.insert(0, name.as_bytes().to_vec());
        vm.texture_parents.get_mut(&0).unwrap().color_filter = Some([2, 0, 0]);
        let action = command(&mut vm, 48, &[0, 0x8000_0000]).unwrap();
        let CmvsPs2aVmAction::LoadTextureParentResource {
            parent_slot,
            resource,
        } = action
        else {
            panic!("image load did not reach the resource consumer")
        };
        let uri = archive
            .resolve_native_resource(&vm, CmvsResourceSearchBank::Texture, &resource)
            .unwrap();
        scene
            .bind_parent_image(&mut vm, parent_slot, &uri, &archive)
            .unwrap();
        if !vm.texture_parents[&parent_slot].surface_initialized {
            let pending = scene
                .render(
                    &mut vm,
                    CmvsTextureSlot::Parent(parent_slot),
                    &archive,
                    (2, 2),
                    None,
                )
                .unwrap()
                .unwrap();
            for pixel in pending.rgba8.as_chunks::<4>().0 {
                assert_eq!(*pixel, [0, 0, 0, 255]);
            }
        }
        command(&mut vm, 51, &[u32::from(parent_slot)]);
        assert_eq!(vm.interpreter_words[&81232], u32::from(bits == 32));
        let queried_width = vm.interpreter_words[&81224];
        let queried_height = vm.interpreter_words[&81228];
        let parent = vm.texture_parents.get_mut(&parent_slot).unwrap();
        parent.surface_initialized = true;
        parent.rect_words = Some([0, 0, queried_width, queried_height]);
        parent.position_words = Some([0, 0]);
        let frame = scene
            .render(
                &mut vm,
                CmvsTextureSlot::Parent(0),
                &archive,
                (queried_width, queried_height),
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            (frame.width, frame.height),
            (u32::from(width), u32::from(height))
        );
        for pixel in frame.rgba8.as_chunks::<4>().0 {
            assert_eq!(*pixel, color);
        }
    }
}

#[test]
fn image_query_without_provider_preserves_auxiliary_results_and_missing_slot_result() {
    let mut vm = CmvsPs2aVmState::new(0);
    command(&mut vm, 32, &[0]);
    for (offset, value) in [(81220, 7), (81224, 11), (81228, 13), (81232, 17)] {
        vm.write_interpreter_word(offset, value);
    }
    command(&mut vm, 51, &[0]);
    assert_eq!(vm.interpreter_words[&81220], 0);
    assert_eq!(
        [
            vm.interpreter_words[&81224],
            vm.interpreter_words[&81228],
            vm.interpreter_words[&81232]
        ],
        [11, 13, 17],
    );
    vm.write_interpreter_word(81220, 23);
    command(&mut vm, 51, &[256]);
    assert_eq!(vm.interpreter_words[&81220], 23);
    assert_eq!(vm.interpreter_flag_words[&10532] & 0x10, 0x10);
}

#[test]
fn parent_occupancy_and_activity_queries_share_the_real_parent_table() {
    let mut vm = CmvsPs2aVmState::new(0);
    command(&mut vm, 39, &[20, u32::MAX]);
    assert_eq!(vm.interpreter_words[&81220], 0);
    command(&mut vm, 32, &[20]);
    command(&mut vm, 39, &[20, u32::MAX]);
    assert_eq!(vm.interpreter_words[&81220], 1);
    command(&mut vm, 39, &[20, 3]);
    assert_eq!(vm.interpreter_words[&81220], 0);
    vm.texture_children
        .get_mut(&20)
        .unwrap()
        .insert(3, Default::default());
    command(&mut vm, 39, &[20, 3]);
    assert_eq!(vm.interpreter_words[&81220], 1);
    vm.write_interpreter_word(81220, 29);
    command(&mut vm, 509, &[20, u32::MAX]);
    assert_eq!(vm.interpreter_words[&81220], u32::MAX);
    vm.write_interpreter_word(81220, 31);
    command(&mut vm, 52, &[20, u32::MAX, 0, 0]);
    assert_eq!(vm.interpreter_words[&81220], 0);
    assert!(vm.interpreter_flag_words.is_empty());
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_parent_destroy_clear_and_rebuild_release_the_rendered_family() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "parent.pb3", 2, 2, [255, 0, 0, 255]);
    write_image(root.path(), "child.pb3", 2, 2, [0, 255, 0, 255]);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    surface_action(&mut scene, command(&mut vm, 32, &[0]));
    load_parent_image(&mut scene, &mut vm, &archive, 0, "parent.pb3");
    let parent = vm.texture_parents.get_mut(&0).unwrap();
    parent.surface_initialized = true;
    parent.rect_words = Some([0, 0, 2, 2]);
    parent.position_words = Some([0, 0]);
    parent.auxiliary_word = Some(0);
    let child_uri = archive
        .resolve_native_resource(&vm, CmvsResourceSearchBank::Texture, "child.pb3")
        .unwrap();
    scene
        .bind(
            CmvsTextureSlot::Child {
                parent: 0,
                child: 1,
            },
            &child_uri,
            &archive,
        )
        .unwrap();
    vm.texture_children.get_mut(&0).unwrap().insert(
        1,
        crate::CmvsTextureChildState {
            surface_initialized: true,
            rect_words: Some([0, 0, 2, 2]),
            position_words: Some([0, 0]),
            auxiliary_word: Some(1),
            ..Default::default()
        },
    );
    let before = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (2, 2), None)
        .unwrap()
        .unwrap();
    for pixel in before.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [0, 255, 0, 255]);
    }
    command(&mut vm, 38, &[0, u32::MAX, 0]);
    let hidden_root = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (2, 2), None)
        .unwrap()
        .unwrap();
    for pixel in hidden_root.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [0, 0, 0, 255]);
    }
    command(&mut vm, 38, &[0, u32::MAX, 7]);
    let shown_root = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (2, 2), None)
        .unwrap()
        .unwrap();
    for pixel in shown_root.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [0, 255, 0, 255]);
    }
    command(&mut vm, 38, &[0, 1, 0]);
    let hidden_child = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (2, 2), None)
        .unwrap()
        .unwrap();
    for pixel in hidden_child.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [255, 0, 0, 255]);
    }
    command(&mut vm, 38, &[0, 1, 1]);
    let shown_child = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (2, 2), None)
        .unwrap()
        .unwrap();
    for pixel in shown_child.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [0, 255, 0, 255]);
    }
    surface_action(&mut scene, command(&mut vm, 35, &[0, 1]));
    let no_child = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (2, 2), None)
        .unwrap()
        .unwrap();
    for pixel in no_child.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [255, 0, 0, 255]);
    }
    command(&mut vm, 65, &[0]);
    let no_surface = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (2, 2), None)
        .unwrap()
        .unwrap();
    for pixel in no_surface.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [0, 0, 0, 255]);
    }
    command(&mut vm, 51, &[0]);
    assert_eq!(
        [
            vm.interpreter_words[&81220],
            vm.interpreter_words[&81224],
            vm.interpreter_words[&81228]
        ],
        [1, 2, 2],
    );
    command(&mut vm, 64, &[0]);
    let recreated = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (2, 2), None)
        .unwrap()
        .unwrap();
    for pixel in recreated.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [0, 0, 0, 255]);
    }
    command(&mut vm, 68, &[0, u32::MAX, 0, 0, 2, 2]);
    let configured = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (2, 2), None)
        .unwrap()
        .unwrap();
    for pixel in configured.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [255, 0, 0, 255]);
    }
    surface_action(&mut scene, command(&mut vm, 33, &[0]));
    let after = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (2, 2), None)
        .unwrap()
        .unwrap();
    for pixel in after.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [0, 0, 0, 255]);
    }
    let bind_parent = |scene: &mut CmvsScene, vm: &mut CmvsPs2aVmState| {
        surface_action(scene, command(vm, 32, &[0]));
        load_parent_image(scene, vm, &archive, 0, "parent.pb3");
        let parent = vm.texture_parents.get_mut(&0).unwrap();
        parent.surface_initialized = true;
        parent.rect_words = Some([0, 0, 2, 2]);
        parent.position_words = Some([0, 0]);
        parent.auxiliary_word = Some(0);
    };
    bind_parent(&mut scene, &mut vm);
    surface_action(&mut scene, command(&mut vm, 47, &[]));
    let cleared = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (2, 2), None)
        .unwrap()
        .unwrap();
    for pixel in cleared.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [0, 0, 0, 255]);
    }
    bind_parent(&mut scene, &mut vm);
    let restored = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (2, 2), None)
        .unwrap()
        .unwrap();
    for pixel in restored.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [255, 0, 0, 255]);
    }
    surface_action(&mut scene, command(&mut vm, 32, &[0]));
    let rebuilt = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (2, 2), None)
        .unwrap()
        .unwrap();
    for pixel in rebuilt.rgba8.as_chunks::<4>().0 {
        assert_eq!(*pixel, [0, 0, 0, 255]);
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_basic_surface_xywh_crop_pivot_and_alpha_select_only_sheet_pixels() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "background.pb3", 8, 6, [255, 0, 0, 255]);
    write_image(root.path(), "sheet.pb3", 8, 6, [255, 0, 255, 255]);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    let root_selector = (-7_i32) as u32;
    for (parent, name) in [(0_u8, "background.pb3"), (1, "sheet.pb3")] {
        surface_action(&mut scene, command(&mut vm, 32, &[u32::from(parent)]));
        load_parent_image(&mut scene, &mut vm, &archive, parent, name);
        command(&mut vm, 64, &[u32::from(parent)]);
    }
    command(&mut vm, 68, &[0, root_selector, 0, 0, 8, 6]);
    command(&mut vm, 69, &[0, root_selector, 0, 0]);
    command(&mut vm, 70, &[0, root_selector, 0, 0]);
    command(&mut vm, 71, &[0, root_selector, 9]);

    // Keep the real PB header/resource binding, then supply a generated atlas
    // frame through the same decoded-image cache used by the renderer.
    let selected: [[u8; 4]; 6] = [
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
        [0, 255, 255, 255],
        [255, 255, 255, 255],
        [64, 128, 192, 255],
    ];
    let mut sheet = [255_u8, 0, 255, 255].repeat(8 * 6);
    for (index, color) in selected.iter().enumerate() {
        let offset = ((1 + index / 3) * 8 + 2 + index % 3) * 4;
        sheet[offset..offset + 4].copy_from_slice(color);
    }
    let uri = scene.bindings[&CmvsTextureSlot::Parent(1)].clone();
    scene
        .textures
        .insert(
            uri,
            TextureFrame {
                width: 8,
                height: 6,
                rgba8: sheet.into(),
            },
        )
        .unwrap();
    // XYWH selects (2,1)..(4,2); command70 is a geometry pivot, not UV offset.
    // Destination (1,2) minus pivot (1,1) places the 3x2 crop at (0,1).
    command(&mut vm, 68, &[1, root_selector, 2, 1, 3, 2]);
    command(&mut vm, 69, &[1, root_selector, 1, 2]);
    command(&mut vm, 70, &[1, root_selector, 1, 1]);
    command(&mut vm, 71, &[1, root_selector, 7]);
    for alpha in [255_i32, 0, 128, -1, 256] {
        command(&mut vm, 72, &[1, root_selector, alpha as u32]);
        let frame = scene
            .render(&mut vm, CmvsTextureSlot::Parent(1), &archive, (8, 6), None)
            .unwrap()
            .unwrap();
        assert_eq!((frame.width, frame.height), (8, 6));
        for (index, pixel) in frame.rgba8.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (index % 8, index / 8);
            let mut expected = [255_u8, 0, 0, 255];
            if x < 3 && (1..3).contains(&y) {
                let color = selected[(y - 1) * 3 + x];
                let coverage = alpha.clamp(0, 255) as u32;
                for channel in 0..3 {
                    expected[channel] = ((u32::from(color[channel]) * coverage
                        + u32::from(expected[channel]) * (255 - coverage)
                        + 127)
                        / 255) as u8;
                }
            }
            for (actual, expected) in pixel.iter().zip(expected) {
                assert!(
                    (i16::from(*actual) - i16::from(expected)).abs() <= 1,
                    "alpha {alpha}, pixel ({x},{y}): {pixel:?}, expected {expected}"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_child_reset_discards_its_provider_before_basic_recreation() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "parent-red.pb3", 8, 6, [255, 0, 0, 255]);
    write_image(root.path(), "child-blue.pb3", 8, 6, [0, 0, 255, 255]);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    surface_action(&mut scene, command(&mut vm, 32, &[0]));
    load_parent_image(&mut scene, &mut vm, &archive, 0, "parent-red.pb3");
    command(&mut vm, 64, &[0]);
    command(&mut vm, 68, &[0, u32::MAX, 0, 0, 8, 6]);
    command(&mut vm, 69, &[0, u32::MAX, 0, 0]);
    if let Some(action) = command(&mut vm, 34, &[0, 1]) {
        surface_action(&mut scene, Some(action));
    }
    command(&mut vm, 80, &[0, 1]);
    let uri = archive
        .resolve_native_resource(&vm, CmvsResourceSearchBank::Texture, "child-blue.pb3")
        .unwrap();
    scene
        .bind_child_image(&mut vm, 0, 1, &uri, &archive)
        .unwrap();
    for (reset, expected) in [(false, [0, 0, 255, 255]), (true, [255, 0, 0, 255])] {
        if reset {
            if let Some(action) = command(&mut vm, 34, &[0, 1]) {
                surface_action(&mut scene, Some(action));
            }
            command(&mut vm, 80, &[0, 1]);
        }
        command(&mut vm, 68, &[0, 1, 0, 0, 8, 6]);
        command(&mut vm, 69, &[0, 1, 0, 0]);
        let frame = scene
            .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (8, 6), None)
            .unwrap()
            .unwrap();
        for pixel in frame.rgba8.as_chunks::<4>().0 {
            assert_eq!(*pixel, expected, "reset {reset}");
        }
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_child_opacity_inherits_parent_and_preserves_signed_alpha_clamp() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "background.pb3", 8, 6, [255, 0, 0, 255]);
    write_image(root.path(), "parent.pb3", 8, 6, [0, 255, 0, 255]);
    write_image(root.path(), "child.pb3", 8, 6, [0, 0, 255, 255]);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    let root_selector = (-7_i32) as u32;
    for (parent, name, order) in [(0_u8, "background.pb3", 9), (1, "parent.pb3", 2)] {
        let parent = u32::from(parent);
        surface_action(&mut scene, command(&mut vm, 32, &[parent]));
        load_parent_image(&mut scene, &mut vm, &archive, parent as u8, name);
        command(&mut vm, 64, &[parent]);
        command(&mut vm, 68, &[parent, root_selector, 0, 0, 8, 6]);
        command(&mut vm, 69, &[parent, root_selector, 0, 0]);
        command(&mut vm, 70, &[parent, root_selector, 0, 0]);
        command(&mut vm, 71, &[parent, root_selector, order]);
    }
    command(&mut vm, 34, &[1, 1]);
    command(&mut vm, 80, &[1, 1]);
    let uri = archive
        .resolve_native_resource(&vm, CmvsResourceSearchBank::Texture, "child.pb3")
        .unwrap();
    scene
        .bind(
            CmvsTextureSlot::Child {
                parent: 1,
                child: 1,
            },
            &uri,
            &archive,
        )
        .unwrap();
    command(&mut vm, 68, &[1, 1, 0, 0, 8, 6]);
    command(&mut vm, 69, &[1, 1, 0, 0]);
    command(&mut vm, 70, &[1, 1, 0, 0]);
    command(&mut vm, 71, &[1, 1, 3]);

    // Parent green and child blue each blend over the preceding red/green
    // layer. Child coverage is multiplied by its parent's coverage.
    for (parent_alpha, child_alpha, expected) in [
        (128_i32, 255_i32, [63_u8, 64, 128, 255]),
        (128, 128, [95, 96, 64, 255]),
        (0, 255, [255, 0, 0, 255]),
        (128, -4, [127, 128, 0, 255]),
        (256, 128, [0, 127, 128, 255]),
        (128, 0, [127, 128, 0, 255]),
        (-4, 255, [255, 0, 0, 255]),
    ] {
        command(&mut vm, 72, &[1, root_selector, parent_alpha as u32]);
        command(&mut vm, 72, &[1, 1, child_alpha as u32]);
        let frame = scene
            .render(&mut vm, CmvsTextureSlot::Parent(1), &archive, (8, 6), None)
            .unwrap()
            .unwrap();
        assert_eq!((frame.width, frame.height), (8, 6));
        for pixel in frame.rgba8.as_chunks::<4>().0 {
            for (actual, expected) in pixel.iter().zip(expected) {
                assert!(
                    (i16::from(*actual) - i16::from(expected)).abs() <= 1,
                    "parent alpha {parent_alpha}, child alpha {child_alpha}: {pixel:?}, expected channel {expected}"
                );
            }
        }
    }
}

fn load_parent_image(
    scene: &mut CmvsScene,
    vm: &mut CmvsPs2aVmState,
    archive: &CmvsArchive,
    parent: u8,
    name: &str,
) {
    vm.process_string_slots.insert(0, name.as_bytes().to_vec());
    let action = command(vm, 48, &[u32::from(parent), 0x8000_0000]);
    let Some(CmvsPs2aVmAction::LoadTextureParentResource {
        parent_slot,
        resource,
    }) = action
    else {
        panic!("image creation did not reach its resource consumer");
    };
    let uri = archive
        .resolve_native_resource(vm, CmvsResourceSearchBank::Texture, &resource)
        .unwrap();
    scene
        .bind_parent_image(vm, parent_slot, &uri, archive)
        .unwrap();
}

#[test]
fn native_display_query_uses_bound_stage_dimensions() {
    for size in [[1920, 1080], [640, 480]] {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.native_stage_size = [2048, 1536];
        vm.native_render_size = size;
        command(&mut vm, 294, &[]);
        assert_eq!(
            [vm.interpreter_words[&81224], vm.interpreter_words[&81228]],
            size
        );
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_solid_surface_draws_its_rectangle_without_an_image_provider() {
    let (_, archive, mut vm) = fixture();
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    surface_action(&mut scene, command(&mut vm, 32, &[0]));
    command(&mut vm, 64, &[0]);
    command(&mut vm, 67, &[0, u32::MAX, 3, 2, 0x00ff_0000]);
    command(&mut vm, 69, &[0, u32::MAX, 1, 2]);
    let frame = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (8, 6), None)
        .unwrap()
        .unwrap();
    for (index, pixel) in frame.rgba8.as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (index % 8, index / 8);
        let expected = if (1..4).contains(&x) && (2..4).contains(&y) {
            [255, 0, 0, 255]
        } else {
            [0, 0, 0, 255]
        };
        assert_eq!(*pixel, expected);
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_scale_inherits_parent_and_child_rebuild_resets_its_transform() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "red.pb3", 8, 6, [255, 0, 0, 255]);
    write_image(root.path(), "green.pb3", 2, 2, [0, 255, 0, 255]);
    write_image(root.path(), "blue.pb3", 1, 1, [0, 0, 255, 255]);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    for (parent, name, width, height) in [(0, "red.pb3", 8, 6), (1, "green.pb3", 2, 2)] {
        surface_action(&mut scene, command(&mut vm, 32, &[parent]));
        load_parent_image(&mut scene, &mut vm, &archive, parent as u8, name);
        command(&mut vm, 64, &[parent]);
        command(&mut vm, 68, &[parent, u32::MAX, 0, 0, width, height]);
        command(
            &mut vm,
            71,
            &[parent, u32::MAX, if parent == 0 { 9 } else { 2 }],
        );
    }
    command(&mut vm, 69, &[1, u32::MAX, 1, 1]);
    command(
        &mut vm,
        73,
        &[1, u32::MAX, 2.0_f32.to_bits(), 1.0_f32.to_bits()],
    );
    command(&mut vm, 34, &[1, 1]);
    command(&mut vm, 80, &[1, 1]);
    let uri = archive
        .resolve_native_resource(&vm, CmvsResourceSearchBank::Texture, "blue.pb3")
        .unwrap();
    scene
        .bind(
            CmvsTextureSlot::Child {
                parent: 1,
                child: 1,
            },
            &uri,
            &archive,
        )
        .unwrap();
    command(&mut vm, 68, &[1, 1, 0, 0, 1, 1]);
    command(&mut vm, 69, &[1, 1, 1, 1]);
    command(&mut vm, 71, &[1, 1, 3]);
    command(&mut vm, 73, &[1, 1, 0.5_f32.to_bits(), 1.0_f32.to_bits()]);
    for rebuilt in [false, true] {
        if rebuilt {
            command(&mut vm, 80, &[1, 1]);
            command(&mut vm, 68, &[1, 1, 0, 0, 1, 1]);
        }
        let frame = scene
            .render(&mut vm, CmvsTextureSlot::Parent(1), &archive, (8, 6), None)
            .unwrap()
            .unwrap();
        for (index, pixel) in frame.rgba8.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (index % 8, index / 8);
            let blue = if rebuilt {
                (1..3).contains(&x) && y == 1
            } else {
                x == 3 && y == 3
            };
            let expected = if blue {
                [0, 0, 255, 255]
            } else if (1..5).contains(&x) && (1..3).contains(&y) {
                [0, 255, 0, 255]
            } else {
                [255, 0, 0, 255]
            };
            assert_eq!(*pixel, expected, "rebuilt={rebuilt} pixel=({x},{y})");
        }
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_reference_width_scales_both_axes_and_destination_without_host_raster_guess() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "red.pb3", 8, 6, [255, 0, 0, 255]);
    write_image(root.path(), "green.pb3", 2, 2, [0, 255, 0, 255]);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    for (parent, name, width, height) in [(0, "red.pb3", 8, 6), (1, "green.pb3", 2, 2)] {
        surface_action(&mut scene, command(&mut vm, 32, &[parent]));
        load_parent_image(&mut scene, &mut vm, &archive, parent as u8, name);
        command(&mut vm, 64, &[parent]);
        command(&mut vm, 68, &[parent, u32::MAX, 0, 0, width, height]);
        command(
            &mut vm,
            71,
            &[parent, u32::MAX, if parent == 0 { 9 } else { 2 }],
        );
    }
    command(&mut vm, 69, &[1, u32::MAX, 1, 1]);
    command(&mut vm, 42, &[1, u32::MAX, 4, 99]);
    let frame = scene
        .render(&mut vm, CmvsTextureSlot::Parent(1), &archive, (8, 6), None)
        .unwrap()
        .unwrap();
    for (index, pixel) in frame.rgba8.as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (index % 8, index / 8);
        let expected = if (2..6).contains(&x) && (2..6).contains(&y) {
            [0, 255, 0, 255]
        } else {
            [255, 0, 0, 255]
        };
        assert_eq!(*pixel, expected, "pixel=({x},{y})");
    }
}
