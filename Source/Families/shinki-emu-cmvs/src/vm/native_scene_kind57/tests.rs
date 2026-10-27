use super::*;

fn scene() -> CmvsNativeSceneKind57 {
    CmvsNativeSceneKind57::new(&mut 1).unwrap()
}

#[test]
fn constructor_preserves_native_provider_major_geometry() {
    let owner = scene();
    assert_eq!(owner.mesh_count(), 200);
    assert_eq!(owner.resource_count(), 9);
    assert_eq!(owner.mesh(200), None);
    assert_eq!(owner.resource(9), None);
    for i in 0..200 {
        let mesh = owner.mesh(i).unwrap();
        assert_eq!(owner.mesh_resource_index(i), i.min(8));
        assert_eq!((mesh.kind, mesh.blend, mesh.billboard), (3, 0, false));
        assert_eq!(mesh.scale_words, words([1.0; 3]));
        assert_eq!(mesh.force_blend, i != 0);
        assert_eq!(mesh.late_pass, i >= 2);
        assert_eq!(mesh.alpha, if matches!(i, 1 | 4..=7) { 0 } else { 255 });
    }
    assert_eq!(
        owner.mesh(0).unwrap().position_words,
        words([0.0, 0.0, 32.0])
    );
    assert_eq!(owner.mesh(1).unwrap().position_words, [0, 0, 0x41ff_3333]);
    assert_eq!(
        owner.mesh(2).unwrap().position_words,
        words([-0.17, 0.17, 1.2])
    );
    assert_eq!(
        owner.mesh(3).unwrap().position_words,
        words([2.6, -2.5, 10.0])
    );
    let expected = [
        [-15.0, 9.0, 0.0],
        [15.0, 9.0, 0.0],
        [-15.0, -9.0, -0.0],
        [15.0, -9.0, -0.0],
    ]
    .map(words);
    assert_eq!(
        owner
            .mesh(0)
            .unwrap()
            .vertices
            .each_ref()
            .map(|v| v.position_words),
        expected
    );
    assert_eq!(
        owner
            .mesh(1)
            .unwrap()
            .vertices
            .each_ref()
            .map(|v| v.position_words),
        expected
    );
    assert_eq!(
        owner
            .mesh(3)
            .unwrap()
            .vertices
            .each_ref()
            .map(|v| v.position_words),
        [
            [-2.3, 3.8, 0.0],
            [1.7, 3.8, 0.0],
            [-2.3, -0.2, -0.0],
            [1.7, -0.2, -0.0]
        ]
        .map(words)
    );
    for i in 4..8 {
        let mesh = owner.mesh(i).unwrap();
        assert_eq!(mesh.position_words, words([2.6, -2.5, 10.1]));
        assert_eq!(
            mesh.vertices.each_ref().map(|v| v.position_words),
            [
                [-12.0, 0.0, 12.0],
                [12.0, 0.0, 12.0],
                [-12.0, 0.0, -12.0],
                [12.0, 0.0, -12.0]
            ]
            .map(words)
        );
        assert!(
            mesh.vertices
                .iter()
                .all(|v| v.rgba_words == words([1.0, 1.0, 1.0, 0.0]))
        );
    }
}

#[test]
fn constructor_consumes_one_draw_per_atlas_quad_and_keeps_sse_height_accumulation() {
    let mut rng = 1;
    let owner = CmvsNativeSceneKind57::new(&mut rng).unwrap();
    assert_eq!(rng, 0x3a2d_7dc1);
    assert_eq!(
        owner.height_words,
        [
            0xbf19_999a,
            0xbe99_999a,
            0,
            0x3e99_999a,
            0x3f19_999a,
            0x3f66_6667
        ]
    );
    for (column, atlas) in [1, 3, 6, 4, 1, 4, 6, 6].into_iter().enumerate() {
        let u = (atlas & 3) as f32 / 4.0;
        let v = (atlas >> 2) as f32 / 4.0;
        let mesh = owner.mesh(8 + column).unwrap();
        assert_eq!(
            mesh.vertices.each_ref().map(|v| v.uv_words),
            [[u, v], [u + 0.25, v], [u, v + 0.25], [u + 0.25, v + 0.25]].map(words)
        );
        assert_eq!(
            mesh.angle_words,
            words([0.0, ((450 - column * 11) % 360) as f32, 0.0])
        );
    }
    assert_eq!(
        owner.mesh(199).unwrap().vertices[0].uv_words,
        words([0.25, 0.25])
    );
    assert_eq!(owner.mesh(199).unwrap().angle_words[1], 109.0_f32.to_bits());
}

#[test]
fn constructor_xyz_uses_bottom_atlas_v_until_the_first_even_zero_delta_callback() {
    let mut owner = scene();
    assert_eq!(
        owner.mesh(8).unwrap().position_words,
        [0x3fa8_f5c3, 0xbf19_999a, 0x3f99_999a]
    );
    assert_eq!(
        owner.mesh(10).unwrap().position_words,
        [0x3fa8_f5c3, 0xbf19_999a, 0x3f9a_e148]
    );
    let uv = owner
        .mesh(10)
        .unwrap()
        .vertices
        .each_ref()
        .map(|v| v.uv_words);
    assert_eq!(owner.advance(0, 999).unwrap(), NativeSceneFrame::default());
    assert_eq!(
        owner.mesh(8).unwrap().position_words,
        [0x3faa_3d71, 0xbf19_999a, 0x3f99_999a]
    );
    assert_eq!(
        owner.mesh(9).unwrap().position_words,
        [0x3fa6_6667, 0xbf19_999a, 0x3f68_f5c4]
    );
    assert_eq!(
        owner.mesh(41).unwrap().position_words,
        [0x3fa6_6667, 0xbe99_999a, 0x3fbe_b852]
    );
    assert_eq!(owner.mesh(9).unwrap().angle_words[1], 101.0_f32.to_bits());
    assert_eq!(owner.mesh(41).unwrap().angle_words[1], 78.0_f32.to_bits());
    assert_eq!(
        owner
            .mesh(10)
            .unwrap()
            .vertices
            .each_ref()
            .map(|v| v.uv_words),
        uv
    );
}

#[test]
fn camera_only_resets_xyz_pitch_and_yaw_and_scene_never_emits_audio() {
    let mut owner = scene();
    let frame = owner.initial_frame();
    assert_eq!(frame.position_words, Some([0; 3]));
    assert_eq!(frame.pitch_word, Some(0));
    assert_eq!(frame.yaw_word, Some(0));
    assert_eq!(frame.angle_words, None);
    assert_eq!(frame.field_of_view_word, None);
    assert_eq!(frame.audio, [None; 4]);
    owner.request(10);
    for delta in [0, 1, 200, u32::MAX] {
        assert_eq!(
            owner.advance(delta, u32::MAX).unwrap(),
            NativeSceneFrame::default()
        );
    }
}

#[test]
fn request10_reloads_exact_providers_and_reveals_without_reset_or_pending_gate() {
    let mut owner = scene();
    owner.advance(123, 0).unwrap();
    let before = owner.clone();
    for selector in [0, 9, 11, 20, 50, u32::MAX] {
        owner.request(selector);
    }
    assert_eq!(owner, before);
    owner.request(10);
    assert_eq!(owner.status(), 0);
    assert_eq!(owner.rotation_clock, before.rotation_clock);
    assert_eq!(owner.orbit_clock, before.orbit_clock);
    assert_eq!(owner.height_words, before.height_words);
    assert_eq!(owner.resource_reload_generation(), 1);
    for i in 0..9 {
        let expected: &[u8] = match i {
            2 => b"__ev-xx004e.pb3",
            3 => b"__ev-xx004b2.pb3",
            _ => CmvsNativeSceneKind57::RESOURCES[i],
        };
        assert_eq!(owner.resource(i), Some(expected));
    }
    for i in 0..200 {
        let old = &before.meshes[i];
        let new = &owner.meshes[i];
        assert_eq!(new.alpha, 255);
        assert_eq!(new.position_words, old.position_words);
        assert_eq!(new.angle_words, old.angle_words);
        assert_eq!(
            new.vertices.each_ref().map(|v| v.uv_words),
            old.vertices.each_ref().map(|v| v.uv_words)
        );
    }
    owner.request(10);
    assert_eq!(owner.resource_reload_generation(), 2);
    assert_eq!(owner.status(), 0);
}

#[test]
fn callbacks_cap_delta_and_use_wrapped_unsigned_clock_products() {
    let mut capped = scene();
    let mut exact = capped.clone();
    capped.advance(u32::MAX, 0).unwrap();
    exact.advance(200, 256).unwrap();
    assert_eq!(capped, exact);
    assert_eq!(capped.rotation_clock, 200);
    assert_eq!(capped.orbit_clock, 200);
    assert_eq!(capped.mesh(4).unwrap().angle_words[1], 2.4_f32.to_bits());
    assert_eq!(
        capped.mesh(5).unwrap().angle_words[1],
        (360.0_f32 - 2.4_f32).to_bits()
    );
    assert_eq!(capped.mesh(8).unwrap().position_words[0], 0x3fa0_0000);
    assert_eq!(capped.mesh(8).unwrap().position_words[2], 0x3f3d_70a4);
    assert_eq!(capped.mesh(40).unwrap().position_words[2], 0x3fd4_7ae2);
    capped.rotation_clock = u32::MAX - 20;
    capped.orbit_clock = u32::MAX - 20;
    capped.advance(50, 0).unwrap();
    assert_eq!((capped.rotation_clock, capped.orbit_clock), (29, 29));
    for clock in [119_304, 119_305, 1_193_047, u32::MAX] {
        capped.rotation_clock = clock;
        capped.orbit_clock = clock;
        capped.advance(0, 0).unwrap();
        let yaw = (clock.wrapping_mul(3600) / 30000 % 3600) as f32 / 10.0;
        assert_eq!(capped.mesh(4).unwrap().angle_words[1], yaw.to_bits());
        let phase = clock.wrapping_mul(36000) / 4000 % 36000;
        let reversed = (36000 - phase) % 36000;
        assert_eq!(
            capped.mesh(8).unwrap().angle_words[1],
            (((45000 - reversed) % 36000 / 100) as f32).to_bits()
        );
    }
}

#[test]
fn descent_quantizes_each_delta_and_wraps_at_inclusive_negative_boundary_once() {
    let mut single = scene();
    let mut split = single.clone();
    single.advance(2, 0).unwrap();
    split.advance(1, 0).unwrap();
    split.advance(1, 0).unwrap();
    assert_eq!(single.height_words[2], (-0.001_f32).to_bits());
    assert_eq!(split.height_words[2], 0);
    single.height_words[0] = (-0.9_f32).to_bits();
    single.height_words[1] = (-2.0_f32).to_bits();
    single.advance(0, 0).unwrap();
    assert_eq!(single.height_words[0], 0.9_f32.to_bits());
    assert_eq!(single.height_words[1], (-2.0_f32 + 1.8_f32).to_bits());
    assert_eq!(
        single.mesh(8).unwrap().position_words[1],
        single.height_words[0]
    );
    assert_eq!(
        single.mesh(40).unwrap().position_words[1],
        single.height_words[1]
    );
}

#[test]
fn private_snapshot_preserves_atlas_geometry_reload_and_full_phase() {
    let mut owner = scene();
    owner.request(10);
    owner.advance(199, 42).unwrap();
    let saved = serde_json::to_vec(&owner).unwrap();
    let mut restored: CmvsNativeSceneKind57 = serde_json::from_slice(&saved).unwrap();
    assert_eq!(restored, owner);
    for delta in [1, 17, 200, 0, u32::MAX] {
        assert_eq!(
            restored.advance(delta, 7).unwrap(),
            owner.advance(delta, 8).unwrap()
        );
        assert_eq!(restored, owner);
    }
}

#[test]
fn invalid_private_geometry_is_rejected_before_callback_mutation() {
    let mut owner = scene();
    owner.meshes.pop();
    assert!(owner.validate().is_err());
    assert!(owner.advance(10, 0).is_err());
    assert_eq!(owner.rotation_clock, 0);
    let mut owner = scene();
    owner.height_words[3] = f32::NAN.to_bits();
    assert!(owner.validate().is_err());
    let mut owner = scene();
    owner.meshes[80].vertices[0].uv_words[1] = f32::INFINITY.to_bits();
    assert!(owner.validate().is_err());
}
