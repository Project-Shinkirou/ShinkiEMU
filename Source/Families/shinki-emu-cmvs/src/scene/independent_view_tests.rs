use super::*;
use crate::scene::parent_load_tests::{command, fixture, write_image};
use crate::vm::independent_view;

#[test]
fn independent_view_native_bridge_round_trips_both_owners_without_changing_planes() {
    let mut vm = crate::CmvsPs2aVmState::new(0);
    command(
        &mut vm,
        608,
        &[2.0_f32.to_bits(), 4.0_f32.to_bits(), (-3.0_f32).to_bits()],
    );
    command(&mut vm, 609, &[12.0_f32.to_bits()]);
    command(&mut vm, 610, &[24.0_f32.to_bits()]);
    command(&mut vm, 611, &[36.0_f32.to_bits()]);
    command(&mut vm, 612, &[72.0_f32.to_bits()]);
    vm.independent_views[1].position_words[0] = 99.0_f32.to_bits();
    let (saved, scalars) = crate::vm::native_slot_projection::capture(&vm).unwrap();
    let mut cold = crate::CmvsPs2aVmState::new(0);
    crate::vm::native_slot_projection::apply(&mut cold, &saved, scalars).unwrap();
    assert_eq!(cold.independent_views, vm.independent_views);
    let (recaptured, recaptured_scalars) =
        crate::vm::native_slot_projection::capture(&cold).unwrap();
    assert_eq!(recaptured, saved);
    assert_eq!(recaptured_scalars, scalars);
    let before = cold.clone();
    let mut bad = saved;
    bad.cameras[0].center[0] = 42.0;
    bad.views[0].position[0] = 100.0;
    bad.views[1].field_of_view = f32::NAN;
    assert!(crate::vm::native_slot_projection::apply(&mut cold, &bad, scalars).is_err());
    assert_eq!(cold, before);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_independent_views_leave_flat_and_plane_pixels_unchanged_and_reject_mesh_candidates_atomically()
 {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "view.pb3", 4, 2, [220, 70, 30, 255]);
    let mut scene = CmvsScene::new(crate::scene::tests::gpu_instance()).unwrap();
    command(&mut vm, 32, &[0]);
    vm.process_string_slots.insert(0, b"view.pb3".to_vec());
    command(&mut vm, 48, &[0, 0x8000_0000]);
    let uri = archive
        .resolve_native_resource(&vm, crate::CmvsResourceSearchBank::Texture, "view.pb3")
        .unwrap();
    scene.bind_parent_image(&mut vm, 0, &uri, &archive).unwrap();
    command(&mut vm, 64, &[0]);
    command(&mut vm, 68, &[0, u32::MAX, 0, 0, 4, 2]);
    command(&mut vm, 69, &[0, u32::MAX, 6, 3]);
    let flat = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (32, 24),
            None,
        )
        .unwrap()
        .unwrap();
    assert!(flat.rgba8.as_chunks::<4>().0.contains(&[220, 70, 30, 255]));
    // Current flat/plane owners are not D3D meshes. FOV, rotation and
    // near/far-side positions must never introduce perspective clipping.
    for (position, angles, fov) in [
        ([0.0, 1.0, 0.05], [0.0_f32, 90.0, 0.0], 45.0_f32),
        ([0.0, 1.0, 101.0], [30.0, 45.0, 60.0], 120.0),
        ([50.0, -20.0, 80.0], [0.0, 0.0, 90.0], 270.0),
    ] {
        command(&mut vm, 608, &position.map(f32::to_bits));
        command(&mut vm, 609, &[angles[0].to_bits()]);
        command(&mut vm, 610, &[angles[1].to_bits()]);
        command(&mut vm, 611, &[angles[2].to_bits()]);
        command(&mut vm, 612, &[fov.to_bits()]);
        let mut views = independent_view::capture_pair(&vm).unwrap();
        views[1].position = [-10.0, -20.0, -30.0];
        views[1].angles = [60.0, 70.0, 80.0];
        views[1].field_of_view = 100.0;
        independent_view::restore_pair(&mut vm, &views).unwrap();
        let encoded = serde_json::to_vec(&vm).unwrap();
        vm = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(independent_view::capture_pair(&vm).unwrap(), views);
        let changed = scene
            .render(
                &mut vm,
                CmvsTextureSlot::Parent(0),
                &archive,
                (32, 24),
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(changed.rgba8, flat.rgba8);
    }
    command(&mut vm, 70, &[0, u32::MAX, 2, 1]);
    command(&mut vm, 66, &[0, u32::MAX]);
    command(&mut vm, 97, &[32.0_f32.to_bits(), 24.0_f32.to_bits()]);
    command(
        &mut vm,
        96,
        &[8.0_f32.to_bits(), 6.0_f32.to_bits(), 10.0_f32.to_bits()],
    );
    command(&mut vm, 115, &[0, u32::MAX, 3.0_f32.to_bits()]);
    command(&mut vm, 116, &[0, u32::MAX, 10.0_f32.to_bits()]);
    command(&mut vm, 112, &[0, u32::MAX, 10.0_f32.to_bits()]);
    let plane = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (32, 24),
            None,
        )
        .unwrap()
        .unwrap();
    assert!(plane.rgba8.as_chunks::<4>().0.contains(&[220, 70, 30, 255]));
    vm.independent_views = independent_view::default_pair();
    let changed = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (32, 24),
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(changed.rgba8, plane.rgba8);
    let mut unsupported = scene.capture_native_image(&vm, 0, &archive).unwrap();
    unsupported.header[0] = 2; // native433620 mesh, not CBasic kind2.
    let before = vm.clone();
    assert!(
        scene
            .restore_native_image(&mut vm, 0, &unsupported, &archive)
            .is_err()
    );
    assert_eq!(vm, before);
    let after = scene
        .render(
            &mut vm,
            CmvsTextureSlot::Parent(0),
            &archive,
            (32, 24),
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(after.rgba8, plane.rgba8);
}
