use super::*;

fn scene() -> CmvsNativeSceneKind53 {
    CmvsNativeSceneKind53::new([512, 512]).unwrap()
}
fn yaws(owner: &CmvsNativeSceneKind53) -> [f32; 2] {
    owner
        .meshes
        .each_ref()
        .map(|mesh| f32::from_bits(mesh.angle_words[1]))
}
fn select(owner: &mut CmvsNativeSceneKind53, selector: u32) {
    owner.request(selector);
    assert_eq!(owner.status(), u32::MAX);
    assert_eq!(
        owner.advance(0, u32::MAX).unwrap(),
        NativeSceneFrame::default()
    );
}

#[test]
fn fixed_geometry_is_two_independently_hinged_late_pass_quads() {
    let owner = scene();
    assert_eq!(owner.mesh_count(), 2);
    assert_eq!(owner.mesh(2), None);
    assert!(!owner.mesh_visible(2));
    for index in 0..2 {
        let mesh = owner.mesh(index).unwrap();
        let (left, right, x) = if index == 0 {
            (-20.0, 0.0, -0.3)
        } else {
            (0.0, 20.0, 0.1)
        };
        assert_eq!(mesh.position_words, words([x, 0.0, 28.0]));
        assert_eq!(
            mesh.vertices.each_ref().map(|vertex| vertex.position_words),
            [
                [left, 12.5, 0.0],
                [right, 12.5, 0.0],
                [left, -7.5, 0.0],
                [right, -7.5, 0.0],
            ]
            .map(words)
        );
        assert_eq!(
            mesh.vertices.each_ref().map(|vertex| vertex.uv_words),
            [[0, 0], [0x3f80_0000, 0], [0, 0x3f80_0000], [0x3f80_0000; 2]]
        );
        assert!(
            mesh.vertices
                .iter()
                .all(|vertex| vertex.rgba_words == [0x3f80_0000; 4])
        );
        assert_eq!(mesh.kind, 3);
        assert_eq!(mesh.alpha, 255);
        assert_eq!(mesh.blend, 0);
        assert_eq!(mesh.angle_words, [0; 3]);
        assert_eq!(mesh.scale_words, [0x3f80_0000; 3]);
        assert!(mesh.force_blend && mesh.late_pass);
        assert!(!mesh.billboard);
        assert!(!owner.mesh_visible(index));
        assert_eq!(owner.mesh_resource_index(index), index);
    }
}

#[test]
fn camera_reset_preserves_roll_and_fov_and_has_no_audio() {
    let frame = scene().initial_frame();
    assert_eq!(frame.position_words, Some(words([0.0, 3.2, 0.0])));
    assert_eq!(frame.pitch_word, Some(0));
    assert_eq!(frame.yaw_word, Some(0));
    assert_eq!(frame.angle_words, None);
    assert_eq!(frame.field_of_view_word, None);
    assert_eq!(frame.audio, [None; 4]);
}

#[test]
fn native_width_alternates_are_independent_and_base_providers_survive_transition() {
    for (widths, expected) in [
        ([512, 512], [b"try007c3.pb3", b"try007c4.pb3"]),
        ([0, 512], [b"try007c1.pb3", b"try007c4.pb3"]),
        ([512, 1024], [b"try007c3.pb3", b"try007c2.pb3"]),
        ([u32::MAX, 511], [b"try007c1.pb3", b"try007c2.pb3"]),
    ] {
        let mut owner = CmvsNativeSceneKind53::new(widths).unwrap();
        assert_eq!(owner.resource(0), Some(expected[0].as_slice()));
        assert_eq!(owner.resource(1), Some(expected[1].as_slice()));
        assert_eq!(owner.resource(2), Some(b"__try007a.pb3".as_slice()));
        assert_eq!(owner.resource(3), None);
        select(&mut owner, 1);
        owner.prepare_draw(5000).unwrap();
        assert_eq!(owner.resource(0), Some(expected[0].as_slice()));
        assert_eq!(owner.resource(3), None);
    }
}

#[test]
fn backdrop_is_a_basic_queue3_surface_not_a_third_world_mesh() {
    let owner = scene();
    let basic = owner.backdrop_basic();
    assert!(basic.surface_initialized && !basic.hidden);
    assert_eq!(basic.basic_kind, 0);
    assert_eq!(basic.blend_mode, Some(0));
    assert_eq!(basic.alpha, Some(255));
    assert_eq!(basic.scale_words, Some([0x3f80_0000; 2]));
    assert_eq!(basic.rect_words, Some([0, 0, 1024, 614]));
    assert_eq!(basic.position_words, Some([0, 0]));
    assert_eq!(basic.auxiliary_pair_words, Some([0, 0]));
    assert_eq!(basic.auxiliary_word, Some(64));
    assert_eq!(basic.float_position_words, [0; 3]);
    assert_eq!(owner.mesh_count(), 2);
}

#[test]
fn selectors_execute_immediately_but_invalid_requests_do_not_replace_the_queue() {
    let mut owner = scene();
    owner.request(3);
    for selector in [0, 6, 10, 11, 12, u32::MAX] {
        owner.request(selector);
    }
    assert_eq!(owner.pending, 3);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 0);
    assert_eq!(owner.visible, [true; 2]);
    select(&mut owner, 2);
    assert_eq!(owner.status(), 0);
    assert_eq!(owner.visible, [false; 2]);
    owner.request(1);
    owner.request(4);
    owner.advance(1234, 0).unwrap();
    assert_eq!(owner.status(), 11);
    assert_eq!(owner.effect_timer, 0);
    assert!(owner.transitions.iter().all(Option::is_none));
}

#[test]
fn transition_advances_only_in_render_phase_and_status_clears_one_callback_later() {
    let mut owner = scene();
    select(&mut owner, 1);
    assert_eq!(owner.status(), 10);
    assert_eq!(owner.visible, [true; 2]);
    owner.advance(9000, 0).unwrap();
    assert_eq!(owner.status(), 10);
    owner.prepare_draw(2500).unwrap();
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 10);
    owner.prepare_draw(2499).unwrap();
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 10);
    owner.prepare_draw(1).unwrap();
    assert_eq!(owner.status(), 10);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 0);
    assert_eq!(owner.visible, [true; 2]);
}

#[test]
fn pending_request_waits_through_transition_completion_then_runs_next_callback() {
    let mut owner = scene();
    select(&mut owner, 1);
    owner.request(2);
    owner.request(3);
    owner.request(2);
    owner.prepare_draw(u32::MAX).unwrap();
    assert_eq!(owner.status(), u32::MAX);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.state, 0);
    assert_eq!(owner.pending, 2);
    assert_eq!(owner.visible, [true; 2]);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 0);
    assert_eq!(owner.visible, [false; 2]);
}

#[test]
fn wobble_has_strict_hold_boundaries_and_discards_excess_delta() {
    let mut owner = scene();
    owner.advance(1000, 0).unwrap();
    assert_eq!(yaws(&owner), [0.5, -0.5]);
    owner.advance(1000, 0).unwrap();
    assert_eq!((owner.wobble_phase, owner.wobble_timer), (1, 0));
    assert_eq!(yaws(&owner), [1.0, -1.0]);
    owner.advance(200, 0).unwrap();
    assert_eq!((owner.wobble_phase, owner.wobble_timer), (1, 200));
    owner.advance(1, 0).unwrap();
    assert_eq!((owner.wobble_phase, owner.wobble_timer), (2, 0));
    assert_eq!(yaws(&owner), [1.0, -1.0]);
    owner.advance(1000, 0).unwrap();
    assert_eq!(yaws(&owner), [0.5, -0.5]);
    owner.advance(1000, 0).unwrap();
    assert_eq!((owner.wobble_phase, owner.wobble_timer), (3, 0));
    assert_eq!(yaws(&owner), [0.0, 0.0]);
    owner.advance(200, 0).unwrap();
    assert_eq!(owner.wobble_phase, 3);
    owner.advance(1, 0).unwrap();
    assert_eq!((owner.wobble_phase, owner.wobble_timer), (0, 0));
    owner.advance(50_000, 0).unwrap();
    assert_eq!((owner.wobble_phase, owner.wobble_timer), (1, 0));
    assert_eq!(yaws(&owner), [1.0, -1.0]);
}

#[test]
fn lift_starts_on_the_next_callback_and_truncates_sine_to_hundredths() {
    let mut owner = scene();
    owner.request(4);
    owner.advance(9000, 0).unwrap();
    assert_eq!(owner.state, 11);
    assert_eq!(owner.effect_timer, 0);
    assert_eq!(owner.base_yaw_word, 0);
    owner.advance(400, 0).unwrap();
    assert_eq!(owner.effect_timer, 400);
    assert_eq!(owner.base_yaw_word, 3.53_f32.to_bits());
    assert_eq!(yaws(&owner), [3.53_f32 + 1.0_f32, 3.53_f32 - 1.0_f32]);
    owner.advance(399, 0).unwrap();
    assert_eq!(owner.state, 11);
    assert_eq!(owner.base_yaw_word, 4.99_f32.to_bits());
    owner.advance(1, 0).unwrap();
    assert_eq!(owner.status(), 0);
    assert_eq!(owner.base_yaw_word, 5.0_f32.to_bits());
    assert_eq!(owner.visible, [false; 2]);
}

#[test]
fn fade_hides_without_restoring_alpha_and_show_does_not_reinitialize_it() {
    let mut owner = scene();
    select(&mut owner, 3);
    select(&mut owner, 5);
    assert_eq!(owner.status(), 12);
    assert_eq!(owner.meshes[0].alpha, 255);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.meshes[0].alpha, 255);
    owner.advance(250, 0).unwrap();
    assert_eq!(owner.meshes[0].alpha, 128);
    assert_eq!(
        owner.meshes[0].vertices[0].rgba_words[3],
        (128.0_f32 / 255.0_f32).to_bits()
    );
    owner.advance(249, 0).unwrap();
    assert_eq!(owner.meshes[0].alpha, 1);
    assert_eq!(owner.visible, [true; 2]);
    owner.advance(1, 0).unwrap();
    assert_eq!(owner.status(), 0);
    assert_eq!(owner.visible, [false; 2]);
    assert_eq!(owner.meshes[0].alpha, 1);
    select(&mut owner, 3);
    assert_eq!(owner.visible, [true; 2]);
    assert_eq!(owner.meshes[0].alpha, 1);
    select(&mut owner, 1);
    assert_eq!(owner.meshes[0].alpha, 1);
}

#[test]
fn large_fade_step_does_not_write_a_completion_alpha() {
    let mut owner = scene();
    select(&mut owner, 3);
    select(&mut owner, 5);
    owner.advance(500, 0).unwrap();
    assert_eq!(owner.meshes[0].alpha, 255);
    assert_eq!(owner.visible, [false; 2]);
    assert_eq!(owner.status(), 0);
}

#[test]
fn private_snapshot_retains_pending_requests_and_both_callback_phases() {
    let mut owner = CmvsNativeSceneKind53::new([512, 256]).unwrap();
    select(&mut owner, 1);
    owner.prepare_draw(4321).unwrap();
    owner.request(4);
    owner.advance(1789, 17).unwrap();
    let bytes = postcard::to_allocvec(&owner).unwrap();
    let mut restored: CmvsNativeSceneKind53 = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(restored, owner);
    for delta in [679, 0, 201, 400, 399, 1, u32::MAX, 0] {
        assert_eq!(
            owner.advance(delta, 123).unwrap(),
            restored.advance(delta, 123).unwrap()
        );
        owner.prepare_draw(delta).unwrap();
        restored.prepare_draw(delta).unwrap();
        assert_eq!(restored, owner);
        owner.validate().unwrap();
    }
}
