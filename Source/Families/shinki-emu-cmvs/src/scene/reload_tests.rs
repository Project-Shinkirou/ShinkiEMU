use super::parent_load_tests::{command, fixture, surface_action, write_image};
use super::*;
use crate::{CmvsPs2aVmAction, CmvsResourceSearchBank, CmvsTextureChildState};

fn seed(scene: &mut CmvsScene, vm: &mut CmvsPs2aVmState, archive: &CmvsArchive, name: &str) {
    surface_action(scene, command(vm, 32, &[0]));
    vm.script_frames.insert(
        0,
        crate::vm::CmvsScriptFrameIdentity {
            script_uri: "cmvs:/fixture/scene.ps3".into(),
            script_hash: roukaku_core::Hash256::from_sha256(b"scene"),
        },
    );
    let uri = archive
        .resolve_native_resource(vm, CmvsResourceSearchBank::Texture, name)
        .unwrap();
    scene.bind_parent_image(vm, 0, &uri, archive).unwrap();
    let image = vm.texture_parents.get_mut(&0).unwrap();
    image.resource = Some(name.into());
    image.resource_frame = Some(0);
    image.surface_initialized = true;
    image.rect_words = Some([0, 0, 2, 2]);
    image.position_words = Some([0, 0]);
    image.auxiliary_word = Some(0);
}

fn reload(
    scene: &mut CmvsScene,
    vm: &mut CmvsPs2aVmState,
    archive: &CmvsArchive,
    name: &str,
    duration: u32,
    secondary: bool,
) -> bool {
    vm.process_string_slots.insert(0, name.as_bytes().to_vec());
    let action = command(vm, 708, &[0, 0x8000_0000, duration, u32::from(secondary)]).unwrap();
    let CmvsPs2aVmAction::ReloadTextureParentImage {
        parent_slot,
        resource,
        bank,
        result_field_offset,
    } = action
    else {
        panic!("reload did not reach the image consumer")
    };
    scene
        .load_image_reload(
            vm,
            parent_slot,
            resource,
            bank,
            result_field_offset,
            archive,
        )
        .unwrap()
}

fn pixels(scene: &mut CmvsScene, vm: &mut CmvsPs2aVmState, archive: &CmvsArchive) -> TextureFrame {
    scene
        .render(vm, CmvsTextureSlot::Parent(0), archive, (2, 2), None)
        .unwrap()
        .unwrap()
}

fn near(frame: &TextureFrame, expected: [u8; 4]) {
    for actual in frame.rgba8.as_chunks::<4>().0 {
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(
                (i16::from(*actual) - i16::from(expected)).abs() <= 2,
                "pixel {actual:?}, expected {expected:?}"
            );
        }
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_reload_composite_uses_native_overlap_order_size_and_old_subtree() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "old.pb3", 2, 2, [255, 0, 0, 255]);
    write_image(root.path(), "next.pb3", 4, 2, [0, 0, 255, 255]);
    write_image(root.path(), "child.pb3", 2, 2, [0, 255, 0, 255]);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    seed(&mut scene, &mut vm, &archive, "old.pb3");
    let uri = archive
        .resolve_native_resource(&vm, CmvsResourceSearchBank::Texture, "child.pb3")
        .unwrap();
    scene
        .bind(
            CmvsTextureSlot::Child {
                parent: 0,
                child: 1,
            },
            &uri,
            &archive,
        )
        .unwrap();
    vm.texture_children.entry(0).or_default().insert(
        1,
        CmvsTextureChildState {
            surface_initialized: true,
            rect_words: Some([0, 0, 2, 2]),
            position_words: Some([0, 0]),
            auxiliary_word: Some(1),
            ..Default::default()
        },
    );
    assert!(reload(
        &mut scene, &mut vm, &archive, "next.pb3", 1000, false
    ));
    assert_eq!(
        vm.texture_reloads[&0].mode,
        crate::vm::texture_reload::ReloadMode::Composite
    );
    near(&pixels(&mut scene, &mut vm, &archive), [0, 255, 0, 255]);
    crate::vm::texture_reload::advance(&mut vm, 500);
    near(&pixels(&mut scene, &mut vm, &archive), [32, 64, 160, 255]);
    assert_eq!(vm.texture_parents[&0].rect_words, Some([0, 0, 4, 2]));
    crate::vm::texture_reload::advance(&mut vm, 500);
    scene.synchronize_image_reloads(&vm);
    near(&pixels(&mut scene, &mut vm, &archive), [0, 0, 255, 255]);
    assert!(scene.reload_surfaces.is_empty());
    assert!(vm.texture_children[&0].is_empty());
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_reload_fast_midpoint_transparent_endpoint_cancel_and_reentry() {
    let (root, archive, mut vm) = fixture();
    for (name, color) in [
        ("old.pb3", [255, 0, 0, 255]),
        ("next.pb3", [0, 0, 255, 255]),
        ("third.pb3", [0, 255, 0, 255]),
        ("empty.pb3", [200, 20, 40, 0]),
    ] {
        write_image(root.path(), name, 2, 2, color);
    }
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    seed(&mut scene, &mut vm, &archive, "old.pb3");
    assert!(reload(
        &mut scene, &mut vm, &archive, "next.pb3", 1000, false
    ));
    near(&pixels(&mut scene, &mut vm, &archive), [255, 0, 0, 255]);
    crate::vm::texture_reload::advance(&mut vm, 500);
    near(&pixels(&mut scene, &mut vm, &archive), [128, 0, 128, 255]);
    assert!(reload(
        &mut scene,
        &mut vm,
        &archive,
        "third.pb3",
        1000,
        false
    ));
    near(&pixels(&mut scene, &mut vm, &archive), [0, 0, 255, 255]);
    let Some(CmvsPs2aVmAction::ReleaseTextureReload { parent_slot }) = command(&mut vm, 709, &[0])
    else {
        panic!("transition cancellation did not reach the image consumer")
    };
    scene.release_image_reload(parent_slot);
    near(&pixels(&mut scene, &mut vm, &archive), [0, 255, 0, 255]);
    assert!(reload(
        &mut scene,
        &mut vm,
        &archive,
        "empty.pb3",
        1000,
        false
    ));
    crate::vm::texture_reload::advance(&mut vm, 500);
    near(&pixels(&mut scene, &mut vm, &archive), [0, 128, 0, 255]);
    crate::vm::texture_reload::advance(&mut vm, 500);
    near(&pixels(&mut scene, &mut vm, &archive), [0, 0, 0, 255]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_reload_real_failure_secondary_bank_snapshot_restore_and_unsigned_duration() {
    let (root, archive, mut vm) = fixture();
    std::fs::create_dir(root.path().join("secondary")).unwrap();
    write_image(root.path(), "old.pb3", 2, 2, [240, 0, 0, 255]);
    write_image(root.path(), "next.pb3", 2, 2, [0, 240, 0, 255]);
    write_image(
        &root.path().join("secondary"),
        "next.pb3",
        2,
        2,
        [0, 0, 240, 255],
    );
    vm.resource_search_paths.insert(
        CmvsResourceSearchBank::Native0d00,
        BTreeMap::from([(9, "secondary/".into())]),
    );
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    seed(&mut scene, &mut vm, &archive, "old.pb3");
    let old_identity = vm.slot_objects[&(1924 << 8)];
    assert!(!reload(
        &mut scene,
        &mut vm,
        &archive,
        "absent.pb3",
        1000,
        false
    ));
    assert_eq!(vm.interpreter_words[&81220], 0);
    assert_ne!(vm.slot_objects[&(1924 << 8)], old_identity);
    assert!(!vm.texture_reloads[&0].busy());
    assert_eq!(
        vm.texture_reloads[&0].previous_parent.resource.as_deref(),
        Some("old.pb3")
    );
    assert!(!scene.bound_p(CmvsTextureSlot::Parent(0)));
    command(&mut vm, 709, &[0]);
    scene.release_image_reload(0);
    seed(&mut scene, &mut vm, &archive, "old.pb3");
    std::fs::write(root.path().join("invalid.pb3"), b"invalid decoder input").unwrap();
    assert!(!reload(
        &mut scene,
        &mut vm,
        &archive,
        "invalid.pb3",
        1000,
        false
    ));
    assert_eq!(vm.interpreter_words[&81220], 0);
    assert!(!scene.bound_p(CmvsTextureSlot::Parent(0)));
    command(&mut vm, 709, &[0]);
    scene.release_image_reload(0);
    seed(&mut scene, &mut vm, &archive, "old.pb3");
    assert!(reload(
        &mut scene,
        &mut vm,
        &archive,
        "next.pb3",
        u32::MAX,
        true
    ));
    crate::vm::texture_reload::advance(&mut vm, 100);
    let encoded = postcard::to_allocvec(&vm).unwrap();
    let mut restored: CmvsPs2aVmState = postcard::from_bytes(&encoded).unwrap();
    crate::validate_cmvs390_vm_state(&restored).unwrap();
    drop(scene);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    near(
        &pixels(&mut scene, &mut restored, &archive),
        [240, 0, 0, 255],
    );
    assert_eq!(restored.texture_reloads[&0].elapsed_ms, 100);
    crate::vm::texture_reload::advance(&mut restored, u32::MAX - 100);
    scene.synchronize_image_reloads(&restored);
    near(
        &pixels(&mut scene, &mut restored, &archive),
        [0, 0, 240, 255],
    );
    assert!(scene.reload_surfaces.is_empty());
    assert!(reload(
        &mut scene,
        &mut restored,
        &archive,
        "old.pb3",
        0,
        false
    ));
    assert!(!restored.texture_reloads.contains_key(&0));
    near(
        &pixels(&mut scene, &mut restored, &archive),
        [240, 0, 0, 255],
    );
    assert!(reload(
        &mut scene,
        &mut restored,
        &archive,
        "next.pb3",
        0,
        false
    ));
    near(
        &pixels(&mut scene, &mut restored, &archive),
        [0, 240, 0, 255],
    );
    // Destroying an owner drops both detached source frames and overlap state.
    assert!(reload(
        &mut scene,
        &mut restored,
        &archive,
        "next.pb3",
        1000,
        true
    ));
    surface_action(&mut scene, command(&mut restored, 33, &[0]));
    scene.synchronize_image_reloads(&restored);
    assert!(restored.texture_reloads.is_empty());
    assert!(scene.reload_surfaces.is_empty());
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_reload_inherits_the_old_filter_for_both_images() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "old.pb3", 2, 2, [240, 0, 0, 255]);
    write_image(root.path(), "next.pb3", 2, 2, [0, 0, 240, 255]);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    seed(&mut scene, &mut vm, &archive, "old.pb3");
    // Native mode 2 multiplies by factor/128: 0x40 halves each channel.
    vm.texture_parents.get_mut(&0).unwrap().color_filter = Some([2, 0x404040, 0]);
    assert!(reload(
        &mut scene, &mut vm, &archive, "next.pb3", 1000, false
    ));
    crate::vm::texture_reload::advance(&mut vm, 500);
    near(&pixels(&mut scene, &mut vm, &archive), [60, 0, 60, 255]);
    crate::vm::texture_reload::advance(&mut vm, 500);
    near(&pixels(&mut scene, &mut vm, &archive), [0, 0, 120, 255]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_reload_composite_preserves_zero_and_partial_source_alpha() {
    let (root, archive, mut vm) = fixture();
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    for (old_alpha, new_alpha, midpoint, endpoint) in [
        (255, 0, [128, 0, 0, 255], [0, 0, 0, 255]),
        (0, 255, [0, 0, 128, 255], [0, 0, 255, 255]),
        (255, 128, [96, 0, 64, 255], [0, 0, 128, 255]),
    ] {
        write_image(root.path(), "old.pb3", 2, 2, [255, 0, 0, old_alpha]);
        write_image(root.path(), "next.pb3", 4, 2, [0, 0, 255, new_alpha]);
        seed(&mut scene, &mut vm, &archive, "old.pb3");
        assert!(reload(
            &mut scene, &mut vm, &archive, "next.pb3", 1000, false
        ));
        assert_eq!(
            vm.texture_reloads[&0].mode,
            crate::vm::texture_reload::ReloadMode::Composite
        );
        crate::vm::texture_reload::advance(&mut vm, 500);
        near(&pixels(&mut scene, &mut vm, &archive), midpoint);
        crate::vm::texture_reload::advance(&mut vm, 500);
        near(&pixels(&mut scene, &mut vm, &archive), endpoint);
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_reload_uploads_once_moves_owner_and_releases_resources_on_table_clear() {
    let (root, archive, mut vm) = fixture();
    for width in [2, 4] {
        write_image(root.path(), "old.pb3", 2, 2, [255, 0, 0, 255]);
        write_image(root.path(), "next.pb3", width, 2, [0, 0, 255, 255]);
        let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
        seed(&mut scene, &mut vm, &archive, "old.pb3");
        assert!(reload(
            &mut scene, &mut vm, &archive, "next.pb3", 1000, false
        ));
        pixels(&mut scene, &mut vm, &archive);
        let source = if width == 2 {
            [0, 0, 2, 2]
        } else {
            [0, 0, 4, 2]
        };
        let commands = scene
            .reload_commands(
                &vm,
                CmvsTextureSlot::Parent(0),
                &vm.texture_reloads[&0],
                RectI {
                    x: source[0],
                    y: source[1],
                    width: source[2] as u32,
                    height: source[3] as u32,
                },
                &archive,
                (2, 2),
            )
            .unwrap();
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, SceneCommand::UploadTexture { .. }))
        );
        let Some(CmvsPs2aVmAction::MoveTextureParentSurface {
            source,
            destination,
        }) = command(&mut vm, 40, &[1, 0])
        else {
            panic!("surface movement did not reach the image consumer")
        };
        scene.move_parent_surface(source, CmvsTextureSlot::Parent(destination));
        assert!(!scene.bound_p(CmvsTextureSlot::Parent(0)));
        assert!(scene.bound_p(CmvsTextureSlot::Parent(1)));
        crate::vm::texture_reload::advance(&mut vm, 500);
        let frame = scene
            .render(&mut vm, CmvsTextureSlot::Parent(1), &archive, (2, 2), None)
            .unwrap()
            .unwrap();
        near(
            &frame,
            if width == 2 {
                [128, 0, 128, 255]
            } else {
                [64, 0, 191, 255]
            },
        );
        command(&mut vm, 710, &[1]);
        assert_eq!(vm.interpreter_words[&81220], 1);
        surface_action(&mut scene, command(&mut vm, 47, &[]));
        assert!(vm.texture_reloads.is_empty());
        assert!(!vm.slot_objects.keys().any(|key| *key >> 8 == 1924));
        scene.synchronize_image_reloads(&vm);
        assert!(scene.reload_surfaces.is_empty());
        assert!(!scene.reload_releases.is_empty());
        scene.draw(2, 2, Vec::new()).unwrap();
        assert!(scene.reload_releases.is_empty());
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_same_descriptor_reload_releases_old_children_before_fast_transition() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "old.pb3", 2, 2, [255, 0, 0, 255]);
    write_image(root.path(), "next.pb3", 2, 2, [0, 0, 255, 255]);
    write_image(root.path(), "child.pb3", 2, 2, [0, 255, 0, 255]);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    seed(&mut scene, &mut vm, &archive, "old.pb3");
    let uri = archive
        .resolve_native_resource(&vm, CmvsResourceSearchBank::Texture, "child.pb3")
        .unwrap();
    scene
        .bind(
            CmvsTextureSlot::Child {
                parent: 0,
                child: 1,
            },
            &uri,
            &archive,
        )
        .unwrap();
    vm.texture_children.entry(0).or_default().insert(
        1,
        CmvsTextureChildState {
            surface_initialized: true,
            rect_words: Some([0, 0, 2, 2]),
            position_words: Some([0, 0]),
            auxiliary_word: Some(1),
            ..Default::default()
        },
    );
    near(&pixels(&mut scene, &mut vm, &archive), [0, 255, 0, 255]);
    assert!(reload(
        &mut scene, &mut vm, &archive, "next.pb3", 1000, false
    ));
    assert!(vm.texture_reloads[&0].previous_children.is_empty());
    assert!(vm.texture_reloads[&0].previous_child_uris.is_empty());
    assert!(!scene.bound_p(CmvsTextureSlot::Child {
        parent: 0,
        child: 1
    }));
    near(&pixels(&mut scene, &mut vm, &archive), [255, 0, 0, 255]);
    crate::vm::texture_reload::advance(&mut vm, 500);
    near(&pixels(&mut scene, &mut vm, &archive), [128, 0, 128, 255]);
    crate::vm::texture_reload::advance(&mut vm, 500);
    near(&pixels(&mut scene, &mut vm, &archive), [0, 0, 255, 255]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_composite_detached_old_child_keeps_its_own_alpha_until_tree_retirement() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "old-small.pb3", 1, 1, [255, 0, 0, 255]);
    write_image(root.path(), "new-wide.pb3", 2, 1, [0, 255, 0, 255]);
    write_image(root.path(), "detached.pb3", 1, 1, [0, 0, 255, 255]);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    seed(&mut scene, &mut vm, &archive, "old-small.pb3");
    command(&mut vm, 68, &[0, u32::MAX, 0, 0, 1, 1]);
    command(&mut vm, 34, &[0, 1]);
    command(&mut vm, 80, &[0, 1]);
    command(&mut vm, 68, &[0, 1, 0, 0, 1, 1]);
    command(&mut vm, 69, &[0, 1, 4, 0]);
    command(&mut vm, 44, &[0, 1]);
    let uri = archive
        .resolve_native_resource(&vm, CmvsResourceSearchBank::Texture, "detached.pb3")
        .unwrap();
    scene
        .bind_child_image(&mut vm, 0, 1, &uri, &archive)
        .unwrap();
    assert!(reload(
        &mut scene,
        &mut vm,
        &archive,
        "new-wide.pb3",
        1000,
        false
    ));
    crate::vm::texture_reload::advance(&mut vm, 500);
    let frame = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (6, 1), None)
        .unwrap()
        .unwrap();
    assert_eq!(&frame.rgba8[16..20], &[0, 0, 255, 255]);
    crate::vm::texture_reload::advance(&mut vm, 500);
    let frame = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (6, 1), None)
        .unwrap()
        .unwrap();
    assert_eq!(&frame.rgba8[16..20], &[0, 0, 0, 255]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_composite_cmv_update_keeps_live_provider_and_original_opaque_overlap() {
    let (root, archive, mut vm) = fixture();
    std::fs::write(
        root.path().join("old.cmv"),
        crate::cmv::tests::container(
            true,
            &[
                crate::cmv::tests::frame(Some(2040), 0, false),
                crate::cmv::tests::frame(Some(0), 1024, false),
            ],
        ),
    )
    .unwrap();
    write_image(root.path(), "next.pb3", 8, 16, [0, 0, 240, 255]);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    vm.frame_string_pool_bytes.insert(0, b"old.cmv\0".to_vec());
    assert!(matches!(
        command(&mut vm, 776, &[0, 0, 0, 0, 0]),
        Some(CmvsPs2aVmAction::OpenCmvResource { .. })
    ));
    let uri = archive.resolve_native_file_resource("old.cmv").unwrap();
    let mut decoder = archive.load_cmv(&uri).unwrap();
    crate::vm::cmv::opened(&mut vm, 0, decoder.metadata()).unwrap();
    let image = vm.texture_parents.get_mut(&0).unwrap();
    image.surface_initialized = true;
    image.rect_words = Some([0, 0, 16, 16]);
    image.image_info = [16, 16, 1];
    scene.bind_cmv_frame(0, &uri, decoder.decode_frame(0).unwrap());
    command(&mut vm, 777, &[0, 0, 0, 1, 0]);
    assert!(reload(
        &mut scene, &mut vm, &archive, "next.pb3", 1000, false
    ));
    crate::vm::texture_reload::advance(&mut vm, 500);
    let render = |scene: &mut CmvsScene, vm: &mut CmvsPs2aVmState| {
        scene
            .render(vm, CmvsTextureSlot::Parent(0), &archive, (16, 16), None)
            .unwrap()
            .unwrap()
    };
    let first = render(&mut scene, &mut vm);
    let at = |frame: &TextureFrame, x: usize| -> [u8; 4] {
        frame.rgba8[(16 + x) * 4..(16 + x + 1) * 4]
            .try_into()
            .unwrap()
    };
    for (actual, expected) in at(&first, 2).into_iter().zip([32, 32, 212, 255]) {
        assert!(actual.abs_diff(expected) <= 2);
    }
    let actions = crate::vm::cmv::advance(&mut vm, 42);
    assert!(matches!(
        actions.as_slice(),
        [CmvsPs2aVmAction::PresentCmvFrame {
            controller_slot: 0,
            frame: 1
        }]
    ));
    assert_eq!(
        crate::vm::cmv::bound_target(&vm, 0),
        Some(crate::vm::cmv::CmvTarget::ReloadPrevious(0))
    );
    scene
        .bind_retained_cmv_frame(
            &vm,
            CmvsTextureSlot::Parent(0),
            decoder.decode_frame(1).unwrap(),
        )
        .unwrap();
    let updated = render(&mut scene, &mut vm);
    // The composite alpha-over passes may quantize by one UNORM unit.
    for (actual, expected) in at(&updated, 2).into_iter().zip([0, 0, 240, 255]) {
        assert!(actual.abs_diff(expected) <= 1);
    }
    assert_eq!(at(&updated, 12), [0, 0, 0, 255]);
    command(&mut vm, 709, &[0]);
    scene.synchronize_image_reloads(&vm);
    assert_eq!(crate::vm::cmv::bound_target(&vm, 0), None);
    let complete = render(&mut scene, &mut vm);
    assert_eq!(at(&complete, 2), [0, 0, 240, 255]);
    assert_eq!(at(&complete, 12), [0, 0, 0, 255]);
}
