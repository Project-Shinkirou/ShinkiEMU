use super::*;

fn owner() -> CmvsNativeSceneKind55 {
    CmvsNativeSceneKind55::new().unwrap()
}
fn tick(owner: &mut CmvsNativeSceneKind55, count: usize, delta: u32) {
    for _ in 0..count {
        owner.advance(delta, 73).unwrap();
    }
}
fn complete(owner: &mut CmvsNativeSceneKind55, selector: u32) {
    owner.request(selector);
    for _ in 0..40 {
        owner.advance(200, 73).unwrap();
        if owner.status() == 0 {
            return;
        }
    }
    panic!("native motion did not finish");
}
fn initialize() -> CmvsNativeSceneKind55 {
    let mut owner = owner();
    for selector in 1..=3 {
        complete(&mut owner, selector);
    }
    owner
}
fn sounds(frame: NativeSceneFrame) -> Vec<(u8, u32)> {
    frame
        .audio
        .into_iter()
        .flatten()
        .map(|sound| match sound {
            NativeSceneSound::PlayStatic { slot, volume } => (slot, volume),
            other => panic!("unexpected native sound: {other:?}"),
        })
        .collect()
}

#[test]
fn constructor_leaves_planes_uninitialized_and_pending_zero() {
    let mut owner = owner();
    assert_eq!(owner.status(), u32::MAX);
    assert_eq!(owner.initial_frame(), NativeSceneFrame::default());
    assert_eq!(owner.particle_capacity(), 3);
    for index in 0..3 {
        assert!(owner.draw_basic(index).is_none());
    }
    assert_eq!(owner.resource(0), Some(b"ci001a.pb3".as_slice()));
    assert_eq!(owner.resource(2), Some(b"ci001c.pb3".as_slice()));
    assert_eq!(owner.resource(3), None);
    assert_eq!(
        owner.advance(1000, 17).unwrap(),
        NativeSceneFrame::default()
    );
    assert_eq!(owner.status(), u32::MAX);
    owner.request(0);
    owner.request(9);
    owner.request(99);
    assert_eq!(owner.status(), u32::MAX);
}

#[test]
fn opening_uses_quantized_motion_and_one_callback_completion_lag() {
    let mut owner = owner();
    owner.request(1);
    owner.advance(200, 0).unwrap();
    assert_eq!(owner.status(), 90);
    let p = floats(owner.plane_position(0).unwrap());
    assert!((p[0] - -1.88).abs() < 0.011);
    assert!((p[1] - 2.59).abs() < 0.011);
    tick(&mut owner, 2, 200);
    assert_eq!(owner.plane_position(0), Some(words([-0.7, 3.2, 5.0])));
    assert_eq!(owner.status(), 90);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 0);
    assert_eq!(
        owner.draw_basic(0).unwrap().rect_words,
        Some([0, 384, 256, 192])
    );
}

#[test]
fn plane_descriptor_retains_native_projection_and_distinct_providers() {
    let owner = initialize();
    for index in 0..3 {
        let basic = owner.draw_basic(index).unwrap();
        assert_eq!(owner.basic_resource_index(index), index);
        assert_eq!(basic.basic_kind, 2);
        assert_eq!(basic.position_words, Some([128, 96]));
        assert_eq!(basic.auxiliary_pair_words, Some([0, 0]));
        assert_eq!(basic.projection_depth_word, 5.0_f32.to_bits());
        assert_eq!(basic.projection_height_word, 0);
        assert_eq!(basic.blend_mode, Some(1));
        assert_eq!(basic.alpha, Some(255));
        assert!(!basic.screen_shake_disabled);
        assert_eq!(
            basic.float_position_words,
            owner.plane_position(index).unwrap()
        );
    }
    assert!(owner.draw_basic(3).is_none());
}

#[test]
fn immediate_atlas_requests_do_not_change_status_and_restart_cell_clock() {
    let mut owner = initialize();
    let status = owner.status();
    owner.request(5);
    assert_eq!(owner.status(), status);
    assert_eq!(
        owner.draw_basic(1).unwrap().rect_words,
        Some([256, 0, 256, 192])
    );
    owner.advance(99, 1).unwrap();
    assert_eq!(
        owner.draw_basic(1).unwrap().rect_words,
        Some([256, 0, 256, 192])
    );
    owner.advance(1, 1).unwrap();
    assert_eq!(
        owner.draw_basic(1).unwrap().rect_words,
        Some([256, 192, 256, 192])
    );
    owner.request(15);
    owner.request(16);
    assert_eq!(
        owner.draw_basic(1).unwrap().rect_words,
        Some([512, 0, 256, 192])
    );
    assert_eq!(
        owner.draw_basic(0).unwrap().rect_words,
        Some([1280, 0, 256, 192])
    );
    owner.advance(4000, 1).unwrap();
    assert_eq!(
        owner.draw_basic(0).unwrap().rect_words,
        Some([1280, 384, 256, 192])
    );
}

#[test]
fn shared_move_waits_only_for_actor_zero_and_preserves_background_motion() {
    let mut owner = initialize();
    owner.request(4);
    tick(&mut owner, 4, 200);
    assert_eq!(owner.status(), 0);
    assert_eq!(owner.plane_position(0), Some(words([1.0, 2.8, -1.0])));
    assert_ne!(owner.plane_position(1), Some(words([-1.0, 2.7, 5.0])));
    tick(&mut owner, 23, 200);
    assert_eq!(owner.plane_position(1), Some(words([-1.0, 2.7, 5.0])));
}

#[test]
fn recoil_sound_is_duplicate_static_and_uses_supplied_volume() {
    let mut owner = initialize();
    owner.request(12);
    assert_eq!(
        sounds(owner.advance(0, 123).unwrap()),
        [(16, 123), (17, 123)]
    );
    assert_eq!(owner.status(), 95);
    assert!(sounds(owner.advance(200, 4).unwrap()).is_empty());
    tick(&mut owner, 6, 200);
    assert_eq!(owner.status(), 0);
    owner.request(6);
    assert_eq!(sounds(owner.advance(0, 8).unwrap()), [(17, 8)]);
}

#[test]
fn request_queue_overwrites_and_defers_until_owner_idle() {
    let mut owner = owner();
    owner.request(1);
    owner.request(2);
    owner.advance(0, 0).unwrap();
    assert!(owner.draw_basic(0).is_none());
    assert!(owner.draw_basic(1).is_some());
    owner.request(3);
    assert_eq!(owner.status(), u32::MAX);
    tick(&mut owner, 16, 200);
    assert_eq!(owner.status(), u32::MAX);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 92);
    assert!(owner.draw_basic(2).is_some());
}

#[test]
fn walking_accepts_ten_only_after_entry_motion_and_attacks_accept_eleven() {
    let mut owner = initialize();
    owner.request(8);
    owner.advance(200, 30).unwrap();
    assert_eq!(owner.status(), 9);
    owner.request(10);
    tick(&mut owner, 4, 200);
    assert_eq!(owner.status(), 10);
    owner.request(11);
    tick(&mut owner, 2, 200);
    assert_eq!(owner.status(), 95);
    tick(&mut owner, 6, 200);
    assert_eq!(owner.status(), 0);
}

#[test]
fn raw_owner_delta_and_capped_basic_delta_are_separate() {
    let mut owner = initialize();
    owner.request(14);
    let frame = owner.advance(1000, 19).unwrap();
    assert_eq!(owner.status(), 20);
    assert_eq!(owner.plane_position(0), Some(words([-6.0, 0.5, 11.0])));
    assert_eq!(sounds(frame), [(17, 19)]);
    assert_eq!(owner.draw_basic(0).unwrap().rect_words.unwrap()[1] % 192, 0);
}

#[test]
fn finish_request_latches_a_segment_and_retains_native_pending_gate() {
    let mut owner = initialize();
    owner.request(14);
    owner.advance(0, 5).unwrap();
    owner.request(17);
    owner.advance(100, 5).unwrap();
    assert_eq!(owner.status(), u32::MAX);
    owner.advance(500, 5).unwrap();
    assert_eq!(owner.state, 17);
    assert_eq!(owner.status(), u32::MAX);
    let frame = owner.advance(200, 42).unwrap();
    assert_eq!(sounds(frame), [(17, 42), (16, 42)]);
    tick(&mut owner, 3, 200);
    assert_eq!(owner.plane_position(0), Some(words([0.0, 3.1, 0.5])));
    //44CA46 reads DWORD6..8, actor0 baseline, not actor1's saved DWORD16..18.
    assert_eq!(owner.plane_position(1), Some(words([4.0, 1.0, -6.0])));
    assert_eq!(owner.status(), u32::MAX);
}

#[test]
fn private_snapshot_preserves_all_controller_and_audio_phases() {
    let mut owner = initialize();
    owner.request(8);
    tick(&mut owner, 9, 137);
    let bytes = serde_json::to_vec(&owner).unwrap();
    let mut restored: CmvsNativeSceneKind55 = serde_json::from_slice(&bytes).unwrap();
    restored.validate().unwrap();
    for delta in [0, 99, 200, 17, 800, 23, 500, 137] {
        assert_eq!(
            owner.advance(delta, 67).unwrap(),
            restored.advance(delta, 67).unwrap()
        );
        assert_eq!(owner, restored);
        for index in 0..3 {
            assert_eq!(owner.draw_basic(index), restored.draw_basic(index));
        }
    }
}

#[test]
fn opening_rotation_preserves_negative_hundredths_with_motion_atlas_and_snapshot() {
    let mut owner = owner();
    owner.request(2);
    owner.advance(100, 73).unwrap();
    assert_eq!(owner.draw_basic(1).unwrap().rotation_hundredths, 154);
    tick(&mut owner, 2, 200);
    assert_eq!(owner.draw_basic(1).unwrap().rotation_hundredths, 500);
    tick(&mut owner, 5, 200);
    assert_eq!(owner.sprites[1].rotation, (-500_i32) as u32);
    assert_eq!(owner.draw_basic(1).unwrap().rotation_hundredths, -500);
    owner.validate().unwrap();
    let position = owner.plane_position(1);
    owner.advance(100, 73).unwrap();
    assert_eq!(owner.sprites[1].rotation, (-344_i32) as u32);
    assert_eq!(owner.draw_basic(1).unwrap().rotation_hundredths, -344);
    assert_ne!(owner.plane_position(1), position);
    assert_eq!(owner.status(), 91);
    assert_eq!(owner.sprites[1].motion.as_ref().unwrap().elapsed, 1600);
    assert_eq!(owner.sprites[1].atlas_index, 0);
    assert_eq!(owner.sprites[1].atlas_clock, 0);
    let rotation = owner.sprites[1].rotation_controller.as_ref().unwrap();
    assert_eq!(
        (rotation.index, rotation.elapsed, rotation.start),
        (2, 100, -500)
    );
    let bytes = serde_json::to_vec(&owner).unwrap();
    let mut restored: CmvsNativeSceneKind55 = serde_json::from_slice(&bytes).unwrap();
    restored.validate().unwrap();
    assert_eq!(owner, restored);
    for delta in [0, 99, 200, 17, 800, 23, 500, 137] {
        assert_eq!(
            owner.advance(delta, 67).unwrap(),
            restored.advance(delta, 67).unwrap()
        );
        owner.validate().unwrap();
        restored.validate().unwrap();
        assert_eq!(owner, restored);
        assert_eq!(owner.draw_basic(1), restored.draw_basic(1));
    }
}

#[test]
fn linear_and_sine_rotation_complete_to_exact_negative_target_in_both_directions() {
    for (mode, halfway) in [(0, 17750), (1, 25102), (0x8000, 35750), (0x8001, 35647)] {
        let mut owner = owner();
        owner.setup(1);
        owner.move_to(1, [1.0, 4.0, 5.0], false, 1000);
        owner.rotate(1, &[(-1, mode, 100, -5)]);
        owner.advance(50, 0).unwrap();
        assert_eq!(owner.draw_basic(1).unwrap().rotation_hundredths, halfway);
        owner.advance(50, 0).unwrap();
        owner.validate().unwrap();
        assert_eq!(owner.sprites[1].rotation, (-500_i32) as u32);
        assert_eq!(owner.draw_basic(1).unwrap().rotation_hundredths, -500);
        assert!(owner.sprites[1].rotation_controller.is_none());
        assert_eq!(owner.sprites[1].atlas_index, 1);
        assert_eq!(owner.sprites[1].motion.as_ref().unwrap().elapsed, 100);
    }
}

#[test]
fn linear_rotation_keeps_signed_remainder_and_fractional_degrees_after_step_change() {
    let mut owner = owner();
    owner.setup(1);
    owner.rotate(1, &[(1, 0, 100, -5), (-1, 0, 1000, 5)]);
    owner.advance(100, 0).unwrap();
    assert_eq!(owner.draw_basic(1).unwrap().rotation_hundredths, -500);
    owner.advance(17, 0).unwrap();
    owner.validate().unwrap();
    assert_eq!(owner.draw_basic(1).unwrap().rotation_hundredths, -483);
    tick(&mut owner, 4, 200);
    owner.advance(183, 0).unwrap();
    owner.validate().unwrap();
    assert_eq!(owner.draw_basic(1).unwrap().rotation_hundredths, 500);
    assert!(owner.sprites[1].rotation_controller.is_none());
}

#[test]
fn recoil_controller_starts_at_native_zero_without_copying_current_basic_angle() {
    for (reverse, angle) in [(false, 9000), (true, 27000)] {
        let mut owner = owner();
        owner.setup(1);
        owner.sprites[1].rotation = (-344_i32) as u32;
        owner.recoil_rotation(reverse);
        let rotation = owner.sprites[1].rotation_controller.as_ref().unwrap();
        assert_eq!(rotation.start, 0);
        assert_eq!(owner.draw_basic(1).unwrap().rotation_hundredths, -344);
        owner.advance(100, 0).unwrap();
        assert_eq!(owner.draw_basic(1).unwrap().rotation_hundredths, angle);
        owner.validate().unwrap();
    }
}

#[test]
fn validator_bounds_signed_angles_and_rejects_corrupt_controller_targets() {
    let mut owner = owner();
    owner.setup(1);
    for angle in [-35999_i32, -500, -1, 0, 1, 500, 35999] {
        owner.sprites[1].rotation = angle as u32;
        owner.validate().unwrap();
        assert_eq!(owner.draw_basic(1).unwrap().rotation_hundredths, angle);
    }
    for angle in [i32::MIN, -36000, 36000, i32::MAX] {
        owner.sprites[1].rotation = angle as u32;
        assert!(owner.validate().is_err());
    }
    owner.sprites[1].rotation = 0;
    owner.sprites[1].rotation_controller.as_mut().unwrap().start = i32::MIN;
    assert!(owner.validate().is_err());
    owner.sprites[1].rotation_controller.as_mut().unwrap().start = 0;
    for target in [i32::MIN, -36000, -499, 36000, i32::MAX] {
        owner.sprites[1].rotation_controller.as_mut().unwrap().steps[0]
            .as_mut()
            .unwrap()
            .target = target;
        assert!(owner.validate().is_err());
    }
}

#[test]
fn invalid_snapshot_geometry_and_controller_links_are_rejected() {
    let mut owner = initialize();
    owner.sprites[0].position[0] = f32::NAN.to_bits();
    assert!(owner.validate().is_err());
    owner.sprites[0].position[0] = 0;
    owner.sprites[1].rotation_controller.as_mut().unwrap().steps[0]
        .as_mut()
        .unwrap()
        .next = 4;
    assert!(owner.validate().is_err());
}
