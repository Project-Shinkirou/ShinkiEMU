use super::parent_load_tests::{command, fixture, write_image};
use super::*;
use crate::CmvsResourceSearchBank;
use crate::system_save::slot_projection::{
    SavedCamera, SavedCameraTransition, SavedPlane, SavedProjection,
};
use crate::vm::{camera_transition, native_slot_projection};

const RED: [u8; 4] = [240, 48, 24, 255];
const CYAN: [u8; 4] = [24, 192, 224, 255];
type Rectangle = (u32, u32, u32, u32);

// Explicit plane geometry makes each world unit one pixel at depth ten.
// This is the native 456E80/457CB0 descriptor, not an extra script opcode.
fn cameras(kind: u32) -> SavedProjection {
    let mut saved = SavedProjection::default();
    for camera in &mut saved.cameras {
        *camera = SavedCamera {
            enabled: 1,
            reserved: 0,
            kind,
            plane: Some(SavedPlane {
                mode: 1,
                position: [0.0; 3],
                auxiliary: [0.0; 2],
                depth: 10.0,
                extent: [32.0, 24.0],
                output: [32.0, 24.0],
                slope: 0.0,
                pitch: 0.0,
                yaw: 0.0,
                roll: 0.0,
            }),
            transition: (kind == 1).then(SavedCameraTransition::default),
            center: [16.0, 12.0],
            scale: [1.0; 2],
        };
    }
    saved
}

fn bind_tile(
    scene: &mut CmvsScene,
    vm: &mut CmvsPs2aVmState,
    archive: &CmvsArchive,
    parent: u32,
    camera: u32,
    name: &str,
    y: f32,
) {
    command(vm, 32, &[parent]);
    vm.process_string_slots.insert(0, name.as_bytes().to_vec());
    command(vm, 48, &[parent, 0x8000_0000]);
    let uri = archive
        .resolve_native_resource(vm, CmvsResourceSearchBank::Texture, name)
        .unwrap();
    scene
        .bind_parent_image(vm, parent as u8, &uri, archive)
        .unwrap();
    command(vm, 64, &[parent]);
    command(vm, 68, &[parent, u32::MAX, 0, 0, 4, 2]);
    command(vm, 70, &[parent, u32::MAX, 2, 1]);
    command(vm, 66, &[parent, u32::MAX]);
    if camera == 1 {
        command(vm, 76, &[parent, u32::MAX, 1]);
    }
    command(vm, 114, &[parent, u32::MAX, 0.0f32.to_bits()]);
    command(vm, 115, &[parent, u32::MAX, y.to_bits()]);
    command(vm, 116, &[parent, u32::MAX, 10.0f32.to_bits()]);
    command(vm, 112, &[parent, u32::MAX, 10.0f32.to_bits()]);
}

fn render(scene: &mut CmvsScene, vm: &mut CmvsPs2aVmState, archive: &CmvsArchive) -> TextureFrame {
    scene
        .render(vm, CmvsTextureSlot::Parent(0), archive, (32, 24), None)
        .unwrap()
        .unwrap()
}

fn assert_pixels(frame: &TextureFrame, rectangles: &[(Rectangle, [u8; 4])]) {
    assert_eq!((frame.width, frame.height), (32, 24));
    let mut coverage = vec![0usize; rectangles.len()];
    for y in 0..frame.height {
        for x in 0..frame.width {
            let mut expected = [0, 0, 0, 255];
            for (index, &((left, top, right, bottom), color)) in rectangles.iter().enumerate() {
                if x >= left && x < right && y >= top && y < bottom {
                    expected = color;
                    coverage[index] += 1;
                }
            }
            let offset = ((y * frame.width + x) * 4) as usize;
            assert_eq!(&frame.rgba8[offset..offset + 4], &expected, "pixel {x},{y}");
        }
    }
    for (covered, &((left, top, right, bottom), _)) in coverage.iter().zip(rectangles) {
        assert_eq!(*covered, ((right - left) * (bottom - top)) as usize);
        assert!(*covered > 0);
    }
}

fn start_xyz(vm: &mut CmvsPs2aVmState, ease: u32, x: f32, y: f32, z: f32, ms: u32) {
    command(vm, 108, &[ease, x.to_bits(), y.to_bits(), z.to_bits(), ms]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_camera_linear_quadratic_cosine_and_zero_duration_have_distinct_visible_progress() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "camera.pb3", 4, 2, RED);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    bind_tile(&mut scene, &mut vm, &archive, 0, 0, "camera.pb3", 0.0);
    // Native ease 0 is linear, 1 is quadratic ease-out, 2 is cosine.
    // Target 8.009 truncates to 8.00; half-time displacements are 4, 6, 2.35.
    for (ease, middle_left) in [(0, 10), (1, 8), (2, 12)] {
        native_slot_projection::apply(&mut vm, &cameras(1), [0, 0]).unwrap();
        start_xyz(&mut vm, ease, 8.009, 0.0, 0.0, 800);
        assert_pixels(
            &render(&mut scene, &mut vm, &archive),
            &[((14, 11, 18, 13), RED)],
        );
        assert!(camera_transition::pending(&vm));
        camera_transition::advance(&mut vm, 400).unwrap();
        assert_pixels(
            &render(&mut scene, &mut vm, &archive),
            &[((middle_left, 11, middle_left + 4, 13), RED)],
        );
        assert!(camera_transition::pending(&vm));
        // The native camera clock does not inherit the image worker's 200ms cap.
        camera_transition::advance(&mut vm, 400).unwrap();
        assert_pixels(
            &render(&mut scene, &mut vm, &archive),
            &[((6, 11, 10, 13), RED)],
        );
        assert!(!camera_transition::pending(&vm));
    }
    native_slot_projection::apply(&mut vm, &cameras(1), [0, 0]).unwrap();
    start_xyz(&mut vm, 0, 8.0, 0.0, 0.0, 0);
    camera_transition::advance(&mut vm, 0).unwrap();
    assert_pixels(
        &render(&mut scene, &mut vm, &archive),
        &[((6, 11, 10, 13), RED)],
    );
    assert!(!camera_transition::pending(&vm));
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_camera_axis_setters_reach_the_projected_geometry_consumer() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "camera.pb3", 4, 2, RED);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    bind_tile(&mut scene, &mut vm, &archive, 0, 0, "camera.pb3", 0.0);
    for (id, words, middle, target) in [
        (
            107,
            vec![0, 8.0f32.to_bits(), 0.0f32.to_bits(), 800],
            (10, 11, 14, 13),
            (6, 11, 10, 13),
        ),
        (
            109,
            vec![0, 4.0f32.to_bits(), 800],
            (14, 13, 18, 15),
            (14, 15, 18, 17),
        ),
        (
            110,
            vec![0, 8.0f32.to_bits(), 800],
            (10, 11, 14, 13),
            (6, 11, 10, 13),
        ),
    ] {
        native_slot_projection::apply(&mut vm, &cameras(1), [0, 0]).unwrap();
        command(&mut vm, id, &words);
        assert_pixels(
            &render(&mut scene, &mut vm, &archive),
            &[((14, 11, 18, 13), RED)],
        );
        camera_transition::advance(&mut vm, 400).unwrap();
        assert_pixels(&render(&mut scene, &mut vm, &archive), &[(middle, RED)]);
        camera_transition::advance(&mut vm, 400).unwrap();
        assert_pixels(&render(&mut scene, &mut vm, &archive), &[(target, RED)]);
        assert!(!camera_transition::pending(&vm), "command {id}");
    }
    native_slot_projection::apply(&mut vm, &cameras(1), [0, 0]).unwrap();
    command(&mut vm, 111, &[0, 2.0f32.to_bits(), 800]);
    camera_transition::advance(&mut vm, 400).unwrap();
    let middle = render(&mut scene, &mut vm, &archive);
    let colored = middle
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| **pixel == RED)
        .count();
    assert!(colored > 8 && colored < 32, "mid-depth coverage {colored}");
    for pixel in middle.rgba8.as_chunks::<4>().0 {
        assert!(*pixel == RED || *pixel == [0, 0, 0, 255]);
    }
    camera_transition::advance(&mut vm, 400).unwrap();
    assert_pixels(
        &render(&mut scene, &mut vm, &archive),
        &[((12, 10, 20, 14), RED)],
    );
    assert!(!camera_transition::pending(&vm));
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_camera_combined_roll_accepts_native_raw_bool_and_rotates_visible_coverage() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "camera.pb3", 4, 2, RED);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    bind_tile(&mut scene, &mut vm, &archive, 0, 0, "camera.pb3", 0.0);
    native_slot_projection::apply(&mut vm, &cameras(1), [0, 0]).unwrap();
    command(
        &mut vm,
        125,
        &[
            0,
            7,
            0.0f32.to_bits(),
            0.0f32.to_bits(),
            0.0f32.to_bits(),
            180.0f32.to_bits(),
            800,
        ],
    );
    assert_pixels(
        &render(&mut scene, &mut vm, &archive),
        &[((14, 11, 18, 13), RED)],
    );
    camera_transition::advance(&mut vm, 400).unwrap();
    assert_pixels(
        &render(&mut scene, &mut vm, &archive),
        &[((15, 10, 17, 14), RED)],
    );
    camera_transition::advance(&mut vm, 400).unwrap();
    assert_pixels(
        &render(&mut scene, &mut vm, &archive),
        &[((14, 11, 18, 13), RED)],
    );
    assert!(!camera_transition::pending(&vm));
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_camera_frame_clock_advances_both_native_owners_once() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "red.pb3", 4, 2, RED);
    write_image(root.path(), "cyan.pb3", 4, 2, CYAN);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    bind_tile(&mut scene, &mut vm, &archive, 0, 0, "red.pb3", 2.0);
    bind_tile(&mut scene, &mut vm, &archive, 1, 1, "cyan.pb3", -2.0);
    let mut saved = cameras(1);
    saved.cameras[1].transition = Some(SavedCameraTransition {
        active: 1,
        flags: 0,
        target_position: [-8.0, 0.0, 0.0],
        target_angles: [0.0; 3],
        duration: 800,
        origin_position: [0.0; 3],
        origin_angles: [0.0; 3],
        elapsed: 0,
    });
    native_slot_projection::apply(&mut vm, &saved, [0, 0]).unwrap();
    start_xyz(&mut vm, 0, 8.0, 0.0, 0.0, 800);
    for (delta, red_left, cyan_left, active) in
        [(0, 14, 14, true), (400, 10, 18, true), (400, 6, 22, false)]
    {
        camera_transition::advance(&mut vm, delta).unwrap();
        assert_pixels(
            &render(&mut scene, &mut vm, &archive),
            &[
                ((red_left, 9, red_left + 4, 11), RED),
                ((cyan_left, 13, cyan_left + 4, 15), CYAN),
            ],
        );
        assert_eq!(camera_transition::pending(&vm), active);
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_camera_commit_false_freezes_visible_instant_and_true_snaps_endpoint() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "camera.pb3", 4, 2, RED);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    bind_tile(&mut scene, &mut vm, &archive, 0, 0, "camera.pb3", 0.0);
    for (snap, left) in [(0, 10), (1, 6)] {
        native_slot_projection::apply(&mut vm, &cameras(1), [0, 0]).unwrap();
        start_xyz(&mut vm, 0, 8.0, 0.0, 0.0, 800);
        camera_transition::advance(&mut vm, 400).unwrap();
        assert_pixels(
            &render(&mut scene, &mut vm, &archive),
            &[((10, 11, 14, 13), RED)],
        );
        command(&mut vm, 104, &[snap]);
        command(&mut vm, 106, &[]);
        assert_eq!(vm.interpreter_words[&81220], 0);
        assert!(!camera_transition::pending(&vm));
        camera_transition::advance(&mut vm, 1600).unwrap();
        assert_pixels(
            &render(&mut scene, &mut vm, &archive),
            &[((left, 11, left + 4, 13), RED)],
        );
    }
}

#[test]
fn camera_wait_preserves_pc_and_operand_until_uncapped_native_clock_completes() {
    let mut vm = CmvsPs2aVmState::new(0);
    native_slot_projection::apply(&mut vm, &cameras(1), [0, 0]).unwrap();
    start_xyz(&mut vm, 0, 8.0, 0.0, 0.0, 1000);
    command(&mut vm, 106, &[]);
    assert_eq!(vm.interpreter_words[&81220], 1);
    command(&mut vm, 105, &[0]);
    let pc = vm.program_counter;
    let cursor = vm.stack_cursor_bytes;
    let operands = vm.stack_bytes.clone();
    let initialized = vm.stack_initialized.clone();
    assert!(vm.dispatch_stopped);
    assert_eq!(vm.interpreter_words[&81220], 0);
    let poll = |vm: &mut CmvsPs2aVmState| {
        vm.dispatch_stopped = false;
        crate::execute_cmvs390_frame(
            vm,
            &crate::CmvsPs2aInstructionFrame::Command {
                span: crate::CmvsPs2aSourceSpan {
                    offset: vm.program_counter,
                    byte_length: 2,
                },
                command_id: 105,
            },
        )
        .unwrap();
    };
    camera_transition::advance(&mut vm, 333).unwrap();
    poll(&mut vm);
    assert_eq!((vm.program_counter, vm.stack_cursor_bytes), (pc, cursor));
    assert_eq!(vm.stack_bytes, operands);
    assert_eq!(vm.stack_initialized, initialized);
    assert!(vm.dispatch_stopped);
    assert!(camera_transition::pending(&vm));
    camera_transition::advance(&mut vm, 667).unwrap();
    poll(&mut vm);
    assert_eq!(
        (vm.program_counter, vm.stack_cursor_bytes),
        (pc + 2, cursor - 4)
    );
    assert!(!vm.dispatch_stopped);
    assert!(!camera_transition::pending(&vm));
    assert_eq!(vm.interpreter_words[&81220], 0);
    command(&mut vm, 106, &[]);
    assert_eq!(vm.interpreter_words[&81220], 0);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_csv2_kind_one_restarts_origin_while_kind_zero_fast_forwards_target() {
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "camera.pb3", 4, 2, RED);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    bind_tile(&mut scene, &mut vm, &archive, 0, 0, "camera.pb3", 0.0);
    for kind in [1, 0] {
        native_slot_projection::apply(&mut vm, &cameras(kind), [7, 1]).unwrap();
        start_xyz(&mut vm, 0, 8.0, 0.0, 0.0, 800);
        camera_transition::advance(&mut vm, 400).unwrap();
        assert_pixels(
            &render(&mut scene, &mut vm, &archive),
            &[((10, 11, 14, 13), RED)],
        );
        let before_capture = vm.screen_words.clone();
        let (saved, scalars) = native_slot_projection::capture(&vm).unwrap();
        assert_eq!(
            vm.screen_words, before_capture,
            "capture must not commit the live owner"
        );
        assert_eq!(scalars, [7, 1]);
        let camera = &saved.cameras[0];
        assert_eq!(
            camera.plane.as_ref().unwrap().position,
            if kind == 1 { [0.0; 3] } else { [8.0, 0.0, 0.0] }
        );
        if kind == 1 {
            let transition = camera.transition.as_ref().unwrap();
            assert_eq!(
                (transition.active, transition.elapsed, transition.duration),
                (1, 0, 800)
            );
            assert_eq!(transition.origin_position, [0.0; 3]);
            assert_eq!(transition.target_position, [8.0, 0.0, 0.0]);
        } else {
            assert!(camera.transition.is_none());
        }
        let mut restored = vm.clone();
        native_slot_projection::apply(&mut restored, &saved, scalars).unwrap();
        assert_pixels(
            &render(&mut scene, &mut restored, &archive),
            &[(
                (
                    if kind == 1 { 14 } else { 6 },
                    11,
                    if kind == 1 { 18 } else { 10 },
                    13,
                ),
                RED,
            )],
        );
        assert_eq!(camera_transition::pending(&restored), kind == 1);
        camera_transition::advance(&mut restored, 400).unwrap();
        assert_pixels(
            &render(&mut scene, &mut restored, &archive),
            &[(
                (
                    if kind == 1 { 10 } else { 6 },
                    11,
                    if kind == 1 { 14 } else { 10 },
                    13,
                ),
                RED,
            )],
        );
        assert_eq!(camera_transition::pending(&restored), kind == 1);
        camera_transition::advance(&mut restored, 400).unwrap();
        assert_pixels(
            &render(&mut scene, &mut restored, &archive),
            &[((6, 11, 10, 13), RED)],
        );
        assert!(!camera_transition::pending(&restored));
        let (inactive, _) = native_slot_projection::capture(&restored).unwrap();
        if kind == 1 {
            assert_eq!(
                inactive.cameras[0].transition,
                Some(SavedCameraTransition::default())
            );
        }
    }
}

#[test]
fn corrupt_active_csv2_camera_is_rejected_before_either_candidate_owner_changes() {
    let mut candidate = CmvsPs2aVmState::new(0);
    native_slot_projection::apply(&mut candidate, &cameras(1), [11, 13]).unwrap();
    start_xyz(&mut candidate, 0, 4.0, 0.0, 0.0, 800);
    camera_transition::advance(&mut candidate, 200).unwrap();
    for malformed in 0..3 {
        let mut saved = cameras(1);
        saved.cameras[0].plane.as_mut().unwrap().position[0] = 12.0;
        let mut transition = SavedCameraTransition {
            active: 1,
            flags: 0x2000_0000,
            target_position: [8.0, 0.0, 0.0],
            target_angles: [0.0, 0.0, 90.0],
            duration: 800,
            origin_position: [0.0; 3],
            origin_angles: [0.0; 3],
            elapsed: 0,
        };
        match malformed {
            0 => transition.target_position[0] = f32::NAN,
            1 => transition.origin_position[1] = f32::INFINITY,
            2 => transition.target_angles[2] = f32::NAN,
            _ => unreachable!(),
        }
        saved.cameras[1].transition = Some(transition);
        let before = (
            candidate.screen_words.clone(),
            candidate.screen_camera_words.clone(),
            candidate.component_words.clone(),
            candidate.program_counter,
            candidate.stack_cursor_bytes,
            candidate.stack_bytes.clone(),
            candidate.dispatch_stopped,
        );
        assert!(native_slot_projection::apply(&mut candidate, &saved, [17, 19]).is_err());
        assert_eq!(
            (
                candidate.screen_words.clone(),
                candidate.screen_camera_words.clone(),
                candidate.component_words.clone(),
                candidate.program_counter,
                candidate.stack_cursor_bytes,
                candidate.stack_bytes.clone(),
                candidate.dispatch_stopped,
            ),
            before,
            "corrupt field {malformed} changed a candidate before validation",
        );
    }
}

#[test]
fn camera_wait_skip_priority_preserves_forced_edges_and_consumes_only_confirmation_release() {
    // Native484D40 tests forced conditions before the consumable release edge.
    for condition in 0..6 {
        let mut vm = CmvsPs2aVmState::new(0);
        native_slot_projection::apply(&mut vm, &cameras(1), [0, 0]).unwrap();
        start_xyz(&mut vm, 0, 8.0, 0.0, 0.0, 1000);
        vm.write_interpreter_word(1452, u32::from(condition != 0));
        vm.write_interpreter_word(1464, u32::from(condition == 3));
        vm.input_shift_held = condition == 1;
        vm.input_control_held = condition == 2;
        vm.input_advance_press = true;
        vm.input_advance_release = condition != 5;
        command(&mut vm, 105, &[1]);
        assert!(vm.dispatch_stopped);
        if condition == 5 {
            // Held/press alone is not the native confirmation-release condition.
            assert!(camera_transition::pending(&vm));
            assert_eq!(vm.interpreter_words[&81220], 0);
            assert!(vm.input_advance_press);
        } else {
            assert!(!camera_transition::pending(&vm));
            assert_eq!(f32::from_bits(vm.screen_words[&1]), 8.0);
            assert_eq!(vm.interpreter_words[&81220], 1);
            assert_eq!(vm.input_advance_release, condition < 4);
            assert_eq!(vm.input_advance_press, condition < 4);
        }
    }
    let mut vm = CmvsPs2aVmState::new(0);
    native_slot_projection::apply(&mut vm, &cameras(1), [0, 0]).unwrap();
    start_xyz(&mut vm, 0, 8.0, 0.0, 0.0, 1000);
    vm.input_advance_release = true;
    command(&mut vm, 105, &[0]);
    assert!(camera_transition::pending(&vm));
    assert!(vm.input_advance_release);
    assert_eq!(vm.interpreter_words[&81220], 0);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_timeline_camera_delay_native_restore_and_camera_one_isolation() {
    use crate::system_save::slot_globals::NativeTimeline;
    use crate::vm::native_timeline;
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "camera.pb3", 4, 2, RED);
    write_image(root.path(), "second.pb3", 4, 2, CYAN);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    bind_tile(&mut scene, &mut vm, &archive, 0, 0, "camera.pb3", 4.0);
    bind_tile(&mut scene, &mut vm, &archive, 1, 1, "second.pb3", -4.0);
    native_slot_projection::apply(&mut vm, &cameras(1), [0, 0]).unwrap();
    command(&mut vm, 646, &[50]);
    command(&mut vm, 647, &[0, 8.0f32.to_bits(), 0, 0, 800]);
    command(&mut vm, 642, &[]);
    let saved = native_timeline::capture(&vm).unwrap();
    let saved = NativeTimeline::decode(&saved.encode().unwrap()).unwrap();
    vm.session_clock_millis = 1000;
    native_timeline::apply(&mut vm, &saved).unwrap();
    vm.session_clock_millis = 1049;
    native_timeline::advance(&mut vm, false).unwrap();
    assert!(!camera_transition::pending(&vm));
    vm.session_clock_millis = 1050;
    native_timeline::advance(&mut vm, false).unwrap();
    assert!(camera_transition::pending(&vm));
    command(&mut vm, 105, &[0]);
    assert!(vm.dispatch_stopped);
    camera_transition::advance(&mut vm, 400).unwrap();
    assert_pixels(
        &render(&mut scene, &mut vm, &archive),
        &[((10, 7, 14, 9), RED), ((14, 15, 18, 17), CYAN)],
    );
    // The active camera worker follows its native kind1 origin/replay policy.
    let (projection, scalars) = native_slot_projection::capture(&vm).unwrap();
    let mut decoded = SavedProjection::default();
    for tag in 2176..=2179 {
        decoded
            .decode_record(tag, &projection.encode_record(tag).unwrap())
            .unwrap();
    }
    native_slot_projection::apply(&mut vm, &decoded, scalars).unwrap();
    assert_pixels(
        &render(&mut scene, &mut vm, &archive),
        &[((14, 7, 18, 9), RED), ((14, 15, 18, 17), CYAN)],
    );
    camera_transition::advance(&mut vm, 800).unwrap();
    assert_pixels(
        &render(&mut scene, &mut vm, &archive),
        &[((6, 7, 10, 9), RED), ((14, 15, 18, 17), CYAN)],
    );
    assert!(!camera_transition::pending(&vm));
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_timeline_transform_native_image_restore_continues_running_geometry() {
    use crate::system_save::slot_image::SavedImage;
    use crate::vm::native_timeline;
    let (root, archive, mut vm) = fixture();
    write_image(root.path(), "camera.pb3", 4, 2, RED);
    let mut scene = CmvsScene::new(super::tests::gpu_instance()).unwrap();
    for id in [649, 650] {
        bind_tile(&mut scene, &mut vm, &archive, 0, 0, "camera.pb3", 0.0);
        native_slot_projection::apply(&mut vm, &cameras(1), [0, 0]).unwrap();
        native_timeline::reset(&mut vm);
        if id == 649 {
            // Native mode1 requires an existing DWORD33 owner; 650 allocates it.
            command(&mut vm, 650, &[0, 0, 0, 10.0f32.to_bits(), 0, 800]);
        }
        command(
            &mut vm,
            id,
            &[0, 8.0f32.to_bits(), 0, 10.0f32.to_bits(), 0, 800],
        );
        command(&mut vm, 642, &[]);
        native_timeline::advance(&mut vm, false).unwrap();
        scene.advance_native_image_effects(&mut vm, 999).unwrap();
        assert_pixels(
            &render(&mut scene, &mut vm, &archive),
            &[((16, 11, 20, 13), RED)],
        );
        let saved = scene.capture_native_image(&vm, 0, &archive).unwrap();
        let saved = SavedImage::decode(&saved.encode().unwrap()).unwrap();
        scene
            .restore_native_image(&mut vm, 0, &saved, &archive)
            .unwrap();
        scene.advance_native_image_effects(&mut vm, 200).unwrap();
        assert_pixels(
            &render(&mut scene, &mut vm, &archive),
            &[((18, 11, 22, 13), RED)],
        );
        scene.advance_native_image_effects(&mut vm, 200).unwrap();
        scene.advance_native_image_effects(&mut vm, 200).unwrap();
        assert_pixels(
            &render(&mut scene, &mut vm, &archive),
            &[((22, 11, 26, 13), RED)],
        );
        assert!(vm.texture_parents[&0].basic_effects.motion.is_none());
    }
}
