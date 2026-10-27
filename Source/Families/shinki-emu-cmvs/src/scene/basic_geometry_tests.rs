use super::parent_load_tests::{command, fixture, write_image};
use super::*;
use crate::{CmvsPs2aVmAction, CmvsResourceSearchBank};

const ROOT: u32 = u32::MAX;
const COLOR: [u8; 4] = [240, 80, 24, 255];
// Native 42EA90/42FAD0 corners for the flat fixture. Row seven deliberately
// excludes (12,7): an unquantized affine quad covers that pixel.
const FLAT_SPANS: &[(u32, u32, u32)] = &[
    (7, 11, 12),
    (8, 10, 14),
    (9, 10, 15),
    (10, 9, 15),
    (11, 8, 15),
    (12, 9, 14),
    (13, 11, 13),
];
const SOLID_SPANS: &[(u32, u32, u32)] = &[
    (7, 11, 13),
    (8, 10, 14),
    (9, 10, 15),
    (10, 9, 15),
    (11, 8, 15),
    (12, 9, 14),
    (13, 11, 13),
];
const CAMERA_SPANS: &[(u32, u32, u32)] = &[
    (10, 15, 20),
    (11, 15, 21),
    (12, 15, 21),
    (13, 16, 22),
    (14, 17, 23),
    (15, 19, 23),
];

fn initialize(vm: &mut CmvsPs2aVmState) {
    command(vm, 32, &[0]);
    command(vm, 64, &[0]);
    command(vm, 68, &[0, ROOT, 2, 1, 7, 5]);
}

fn flat(vm: &mut CmvsPs2aVmState) {
    command(vm, 42, &[0, ROOT, 24, 20]);
    command(vm, 69, &[0, ROOT, 12, 10]);
    command(vm, 70, &[0, ROOT, 3, 2]);
    command(vm, 73, &[0, ROOT, 0.73f32.to_bits(), 1.17f32.to_bits()]);
    command(vm, 74, &[0, ROOT, 37]);
    command(vm, 85, &[0, ROOT, 2]);
    command(vm, 84, &[0, ROOT, 12.0f32.to_bits(), 10.05f32.to_bits()]);
}

fn bind(scene: &mut CmvsScene, vm: &mut CmvsPs2aVmState, archive: &CmvsArchive) {
    vm.process_string_slots.insert(0, b"plane.pb3".to_vec());
    command(vm, 48, &[0, 0x8000_0000]);
    let uri = archive
        .resolve_native_resource(vm, CmvsResourceSearchBank::Texture, "plane.pb3")
        .unwrap();
    scene.bind_parent_image(vm, 0, &uri, archive).unwrap();
    command(vm, 64, &[0]);
    command(vm, 68, &[0, ROOT, 2, 1, 7, 5]);
}

fn camera(vm: &mut CmvsPs2aVmState) -> CmvsTextureSlot {
    command(vm, 42, &[0, ROOT, 32, 24]);
    command(vm, 66, &[0, ROOT]);
    command(vm, 68, &[0, ROOT, 0, 0, 0, 0]);
    command(vm, 70, &[0, ROOT, 1, 2]);
    command(vm, 73, &[0, ROOT, 1.4f32.to_bits(), 0.8f32.to_bits()]);
    command(vm, 74, &[0, ROOT, 23]);
    command(vm, 115, &[0, ROOT, 3.0f32.to_bits()]);
    command(vm, 116, &[0, ROOT, 10.0f32.to_bits()]);
    command(vm, 112, &[0, ROOT, 10.0f32.to_bits()]);
    command(vm, 97, &[32.0f32.to_bits(), 24.0f32.to_bits()]);
    command(
        vm,
        96,
        &[10.0f32.to_bits(), 6.0f32.to_bits(), 10.0f32.to_bits()],
    );
    command(vm, 99, &[13.27f32.to_bits()]);
    command(vm, 34, &[0, 1]);
    command(vm, 80, &[0, 1]);
    command(vm, 66, &[0, 1]);
    command(vm, 68, &[0, 1, 2, 1, 7, 5]);
    command(vm, 69, &[0, 1, 3, (-1i32) as u32]);
    command(vm, 70, &[0, 1, 2, 1]);
    command(vm, 73, &[0, 1, 0.73f32.to_bits(), 1.17f32.to_bits()]);
    command(vm, 74, &[0, 1, 37]);
    CmvsTextureSlot::Child {
        parent: 0,
        child: 1,
    }
}

fn assert_spans(frame: &TextureFrame, spans: &[(u32, u32, u32)], color: [u8; 4]) {
    for y in 0..frame.height {
        for x in 0..frame.width {
            let inside = spans
                .iter()
                .any(|&(row, left, right)| y == row && x >= left && x < right);
            let offset = ((y * frame.width + x) * 4) as usize;
            assert_eq!(
                &frame.rgba8[offset..offset + 4],
                if inside { &color } else { &[0, 0, 0, 255] },
                "pixel {x},{y}"
            );
        }
    }
}

#[test]
fn native_flat_corners_preserve_independent_hundredth_truncation() {
    let (_root, _archive, mut vm) = fixture();
    initialize(&mut vm);
    flat(&mut vm);
    let corners = surface::geometry(&vm, CmvsTextureSlot::Parent(0), 24, true)
        .unwrap()
        .corners([7, 5]);
    assert_eq!(
        corners.map(|point| point.map(f32::to_bits)),
        [
            [0x413a_8f5c, 0x40db_d70a],
            [0x417b_d70a, 0x411f_0a3e],
            [0x4102_3d71, 0x4138_7ae2],
            [0x4143_5c29, 0x4169_c290],
        ]
    );
    assert_ne!(corners[0][0] + corners[3][0], corners[1][0] + corners[2][0]);
    assert_ne!(corners[0][1] + corners[3][1], corners[1][1] + corners[2][1]);
    // Crop coordinates never enter the geometric corner calculation.
    command(&mut vm, 68, &[0, ROOT, 4, 2, 7, 5]);
    assert_eq!(
        surface::geometry(&vm, CmvsTextureSlot::Parent(0), 24, true)
            .unwrap()
            .corners([7, 5]),
        corners
    );
}

#[test]
fn native_camera_corners_quantize_local_parent_and_positive_roll_separately() {
    let (_root, _archive, mut vm) = fixture();
    initialize(&mut vm);
    let child = camera(&mut vm);
    let positive = surface::geometry(&vm, child, 32, true)
        .unwrap()
        .corners([7, 5]);
    assert_eq!(
        positive.map(|p| p.map(f32::to_bits)),
        [
            [0x419c_cccd, 0x411c_51ec],
            [0x41ba_8f5c, 0x417c_7ae1],
            [0x4164_a3d7, 0x4129_1eb8],
            [0x418a_0000, 0x4175_eb85],
        ]
    );
    // Native 456C00 stores abs(trunc(input * 100)) % 36000 / 100:
    // negative script input is normalized before 42F270 sees the roll.
    command(&mut vm, 99, &[(-13.27f32).to_bits()]);
    assert_eq!(
        surface::geometry(&vm, child, 32, true)
            .unwrap()
            .corners([7, 5]),
        positive
    );
    // A zero roll skips BOTH the per-coordinate floor and the final rotation.
    command(&mut vm, 99, &[0]);
    assert_eq!(
        surface::geometry(&vm, child, 32, true)
            .unwrap()
            .corners([7, 5])
            .map(|p| p.map(f32::to_bits)),
        [
            [0x419b_d70a, 0x4114_f5c2],
            [0x41be_28f6, 0x415c_7ae1],
            [0x4158_0000, 0x412c_f5c2],
            [0x418e_28f6, 0x4174_cccd],
        ]
    );
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_scale_gates_keep_signed_camera_local_and_combined_flat_scales() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "plane.pb3", 11, 7, COLOR);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    initialize(&mut vm);
    bind(&mut scene, &mut vm, &archive);
    command(&mut vm, 42, &[0, ROOT, 24, 20]);
    command(&mut vm, 68, &[0, ROOT, 0, 0, 0, 0]);
    command(&mut vm, 69, &[0, ROOT, 8, 8]);
    command(
        &mut vm,
        73,
        &[0, ROOT, (-2.0f32).to_bits(), (-3.0f32).to_bits()],
    );
    command(&mut vm, 34, &[0, 1]);
    command(&mut vm, 80, &[0, 1]);
    command(&mut vm, 68, &[0, 1, 0, 0, 2, 2]);
    command(&mut vm, 69, &[0, 1, (-3i32) as u32, (-2i32) as u32]);
    command(
        &mut vm,
        73,
        &[0, 1, (-1.0f32).to_bits(), (-1.0f32).to_bits()],
    );
    let child = CmvsTextureSlot::Child {
        parent: 0,
        child: 1,
    };
    let frame = scene
        .render(&mut vm, child, &archive, (24, 20), None)
        .unwrap()
        .unwrap();
    assert_spans(
        &frame,
        &[
            (12, 14, 18),
            (13, 14, 18),
            (14, 14, 18),
            (15, 14, 18),
            (16, 14, 18),
            (17, 14, 18),
        ],
        COLOR,
    );
    command(&mut vm, 73, &[0, 1, 0, 1.0f32.to_bits()]);
    let frame = scene
        .render(&mut vm, child, &archive, (24, 20), None)
        .unwrap()
        .unwrap();
    assert_spans(&frame, &[], COLOR);

    // A root camera basic's local negative X survives: the positive gate
    // applies to 4566B0's projected context, not to 4306A0's local scale.
    command(&mut vm, 66, &[0, ROOT]);
    command(&mut vm, 42, &[0, ROOT, 32, 24]);
    command(&mut vm, 68, &[0, ROOT, 0, 0, 7, 5]);
    command(
        &mut vm,
        73,
        &[0, ROOT, (-1.0f32).to_bits(), 1.0f32.to_bits()],
    );
    command(&mut vm, 115, &[0, ROOT, 3.0f32.to_bits()]);
    command(&mut vm, 116, &[0, ROOT, 10.0f32.to_bits()]);
    command(&mut vm, 112, &[0, ROOT, 10.0f32.to_bits()]);
    command(&mut vm, 97, &[32.0f32.to_bits(), 24.0f32.to_bits()]);
    command(
        &mut vm,
        96,
        &[10.0f32.to_bits(), 6.0f32.to_bits(), 10.0f32.to_bits()],
    );
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
    assert_spans(
        &frame,
        &[
            (12, 9, 16),
            (13, 9, 16),
            (14, 9, 16),
            (15, 9, 16),
            (16, 9, 16),
        ],
        COLOR,
    );
    command(&mut vm, 73, &[0, ROOT, 0, 1.0f32.to_bits()]);
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
    assert_spans(&frame, &[], COLOR);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_quantized_non_affine_texture_and_solid_cover_exact_pixels() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "plane.pb3", 11, 7, COLOR);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    initialize(&mut vm);
    bind(&mut scene, &mut vm, &archive);
    flat(&mut vm);
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
    assert_spans(&frame, FLAT_SPANS, COLOR);

    let (_solid_root, solid_archive, mut solid_vm) = fixture();
    let mut solid_scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    initialize(&mut solid_vm);
    command(&mut solid_vm, 67, &[0, ROOT, 7, 5, 0x00f0_5018]);
    flat(&mut solid_vm);
    let frame = solid_scene
        .render(
            &mut solid_vm,
            CmvsTextureSlot::Parent(0),
            &solid_archive,
            (24, 20),
            None,
        )
        .unwrap()
        .unwrap();
    assert_spans(&frame, SOLID_SPANS, COLOR);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_quantized_camera_parent_child_and_roll_cover_exact_pixels() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "plane.pb3", 11, 7, COLOR);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    initialize(&mut vm);
    bind(&mut scene, &mut vm, &archive);
    let child = camera(&mut vm);
    let frame = scene
        .render(&mut vm, child, &archive, (32, 24), None)
        .unwrap()
        .unwrap();
    assert_spans(&frame, CAMERA_SPANS, COLOR);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_quantized_fast_reload_preserves_midpoint_and_corner_coverage() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "plane.pb3", 11, 7, COLOR);
    write_image(root.path(), "next.pb3", 11, 7, [24, 80, 240, 255]);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    initialize(&mut vm);
    bind(&mut scene, &mut vm, &archive);
    flat(&mut vm);
    vm.script_frames.insert(
        0,
        crate::vm::CmvsScriptFrameIdentity {
            script_uri: "cmvs:/fixture/geometry.ps3".into(),
            script_hash: roukaku_core::Hash256::from_sha256(b"geometry"),
        },
    );
    vm.process_string_slots.insert(0, b"next.pb3".to_vec());
    let Some(CmvsPs2aVmAction::ReloadTextureParentImage {
        parent_slot,
        resource,
        bank,
        result_field_offset,
    }) = command(&mut vm, 708, &[0, 0x8000_0000, 1000, 0])
    else {
        panic!("708 did not reach the reload image consumer");
    };
    assert!(
        scene
            .load_image_reload(
                &mut vm,
                parent_slot,
                resource,
                bank,
                result_field_offset,
                &archive
            )
            .unwrap()
    );
    assert_eq!(
        vm.texture_reloads[&0].mode,
        crate::vm::texture_reload::ReloadMode::Fast
    );
    crate::vm::texture_reload::advance(&mut vm, 500);
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
    assert_spans(&frame, FLAT_SPANS, [132, 80, 132, 255]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_composite_reload_syncs_position_without_replacing_retained_shape() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "plane.pb3", 11, 7, COLOR);
    write_image(root.path(), "next.pb3", 16, 7, [24, 80, 240, 255]);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    initialize(&mut vm);
    bind(&mut scene, &mut vm, &archive);
    flat(&mut vm);
    vm.script_frames.insert(
        0,
        crate::vm::CmvsScriptFrameIdentity {
            script_uri: "cmvs:/fixture/geometry.ps3".into(),
            script_hash: roukaku_core::Hash256::from_sha256(b"geometry"),
        },
    );
    vm.process_string_slots.insert(0, b"next.pb3".to_vec());
    let Some(CmvsPs2aVmAction::ReloadTextureParentImage {
        parent_slot,
        resource,
        bank,
        result_field_offset,
    }) = command(&mut vm, 708, &[0, 0x8000_0000, 1000, 0])
    else {
        panic!("708 did not reach the reload image consumer");
    };
    assert!(
        scene
            .load_image_reload(
                &mut vm,
                parent_slot,
                resource,
                bank,
                result_field_offset,
                &archive
            )
            .unwrap()
    );
    assert_eq!(
        vm.texture_reloads[&0].mode,
        crate::vm::texture_reload::ReloadMode::Composite
    );
    command(&mut vm, 69, &[0, ROOT, 16, 12]);
    command(&mut vm, 71, &[0, ROOT, 9]);
    command(
        &mut vm,
        84,
        &[0, ROOT, 16.0f32.to_bits(), 12.05f32.to_bits()],
    );
    command(&mut vm, 70, &[0, ROOT, 1, 1]);
    command(&mut vm, 73, &[0, ROOT, 3.0f32.to_bits(), 3.0f32.to_bits()]);
    command(&mut vm, 74, &[0, ROOT, 79]);
    // A zero-time native advance still synchronizes DWORD8..13. At phase zero
    // only the retained old/overlap pixels are visible, with the original
    // 37-degree rotation, anisotropic scale, pivot and cropped dimensions.
    crate::vm::texture_reload::advance(&mut vm, 0);
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
    assert_spans(
        &frame,
        &[
            (9, 15, 16),
            (10, 14, 18),
            (11, 14, 19),
            (12, 13, 19),
            (13, 12, 19),
            (14, 13, 18),
            (15, 15, 17),
        ],
        COLOR,
    );
}
