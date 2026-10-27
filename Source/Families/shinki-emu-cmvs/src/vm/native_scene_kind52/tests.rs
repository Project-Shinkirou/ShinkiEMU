use super::*;

fn scene() -> CmvsNativeSceneKind52 {
    CmvsNativeSceneKind52::new().unwrap()
}
fn upper_index(child: usize) -> usize {
    3 + child % 6 * 4 + child / 6
}
fn alpha(owner: &CmvsNativeSceneKind52, child: usize) -> i32 {
    owner.mesh(upper_index(child)).unwrap().alpha
}
fn enter(owner: &mut CmvsNativeSceneKind52, selector: u32) {
    owner.request(selector);
    assert_eq!(owner.status(), u32::MAX);
    let frame = owner.advance(0, 0).unwrap();
    assert_eq!(frame.audio, [None; 4]);
    assert_eq!(owner.status(), selector);
}

#[test]
fn all_fixed_nodes_keep_native_provider_child_order_and_geometry() {
    let owner = scene();
    let expected_y = [
        [1.25, 2.5, 5.5, 8.5],
        [1.5, 3.25, 6.25, 9.25],
        [1.5, 3.25, 6.25, 9.25],
        [1.75, 4.0, 7.0, 10.0],
        [1.75, 4.0, 7.0, 10.0],
        [2.0, 4.75, 7.75, 10.75],
    ];
    assert_eq!(owner.mesh_count(), 27);
    assert_eq!(owner.resource_count(), 9);
    assert_eq!(owner.resource(9), None);
    assert_eq!(owner.mesh(27), None);
    for index in 0..27 {
        let mesh = owner.mesh(index).unwrap();
        let (provider, positions, position) = match index {
            0 => (
                0,
                [
                    [-30.0, 36.0, 0.0],
                    [30.0, 36.0, 0.0],
                    [-30.0, 0.0, 0.0],
                    [30.0, 0.0, 0.0],
                ],
                [0.0, -12.0, 39.0],
            ),
            1 => (
                1,
                [
                    [-5.0, 6.0, 0.0],
                    [5.0, 6.0, 0.0],
                    [-5.0, 0.0, 0.0],
                    [5.0, 0.0, 0.0],
                ],
                [0.0, 2.0, 7.15],
            ),
            26 => (
                8,
                [
                    [-6.0, 20.0, 0.0],
                    [6.0, 20.0, 0.0],
                    [-6.0, 0.0, 0.0],
                    [6.0, 0.0, 0.0],
                ],
                [2.0, -20.0, 12.0],
            ),
            _ => {
                let provider = (index - 2) / 4;
                (
                    provider + 2,
                    [
                        [-3.0, 0.0, 3.0],
                        [3.0, 0.0, 3.0],
                        [-3.0, 0.0, -3.0],
                        [3.0, 0.0, -3.0],
                    ],
                    [2.0, expected_y[provider][(index - 2) % 4], 13.0],
                )
            }
        };
        assert_eq!(owner.mesh_resource_index(index), provider);
        assert_eq!(mesh.position_words, words(position));
        assert_eq!(
            mesh.vertices.each_ref().map(|v| v.position_words),
            positions.map(words)
        );
        assert_eq!(
            mesh.vertices.each_ref().map(|v| v.uv_words),
            [[0, 0], [0x3f80_0000, 0], [0, 0x3f80_0000], [0x3f80_0000; 2]]
        );
        assert_eq!(mesh.kind, 3);
        assert_eq!(mesh.alpha, 255);
        assert_eq!(mesh.blend, 0);
        assert_eq!(mesh.angle_words, [0; 3]);
        assert_eq!(mesh.scale_words, [0x3f80_0000; 3]);
        assert!(!mesh.billboard);
        assert_eq!(mesh.force_blend, index != 0);
        assert_eq!(mesh.late_pass, index != 0);
    }
}

#[test]
fn rotations_use_yaw_only_and_reverse_nodes_retain_360_at_zero() {
    let mut owner = scene();
    owner.advance(0, 0).unwrap();
    for provider in 0..6 {
        for ring in 0..4 {
            let angle = if matches!(provider, 0 | 1 | 4) {
                0.0_f32
            } else {
                360.0_f32
            };
            assert_eq!(
                owner.mesh(2 + provider * 4 + ring).unwrap().angle_words,
                [0, angle.to_bits(), 0]
            );
        }
    }
    owner.advance(13, 0).unwrap();
    assert_eq!(owner.mesh(2).unwrap().angle_words[1], 0);
    owner.advance(1, 0).unwrap();
    assert_eq!(owner.mesh(2).unwrap().angle_words[1], 1.0_f32.to_bits());
    assert_eq!(owner.mesh(10).unwrap().angle_words[1], 359.0_f32.to_bits());
    owner.advance(4986, 0).unwrap();
    assert_eq!(owner.rotation_clock, 5000);
    assert_eq!(owner.mesh(2).unwrap().angle_words[1], 0);
    assert_eq!(owner.mesh(10).unwrap().angle_words[1], 360.0_f32.to_bits());
    for index in [0, 1, 26] {
        assert_eq!(owner.mesh(index).unwrap().angle_words, [0; 3]);
    }
}

#[test]
fn clock_add_and_multiply_wrap_before_unsigned_rotation_division() {
    let mut owner = scene();
    owner.rotation_clock = 11_930_464;
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.mesh(2).unwrap().angle_words[1], 33.0_f32.to_bits());
    owner.advance(1, 0).unwrap();
    assert_eq!(owner.mesh(2).unwrap().angle_words[1], 0);
    owner.rotation_clock = u32::MAX - 100;
    owner.advance(300, 0).unwrap();
    assert_eq!(owner.rotation_clock, 199);
    assert_eq!(owner.mesh(2).unwrap().angle_words[1], 14.0_f32.to_bits());
    owner.advance(u32::MAX, 0).unwrap();
    assert_eq!(owner.rotation_clock, 198);
}

#[test]
fn yaw_world_transform_uses_native_single_precision_radians_and_double_trig() {
    let mut owner = scene();
    owner.advance(1250, 0).unwrap();
    let mesh = owner.mesh(2).unwrap();
    let matrix = mesh
        .world_matrix(super::super::native_mesh::identity())
        .unwrap();
    // 433620 MULSS degrees*0.017453292, then 413330 precise double cos/sin.
    assert_eq!(mesh.angle_words, [0, 90.0_f32.to_bits(), 0]);
    assert_eq!(matrix[0].to_bits(), 0xb33b_bd2e);
    assert_eq!(matrix[2], -1.0);
    assert_eq!(matrix[8], 1.0);
    assert_eq!(matrix[10].to_bits(), 0xb33b_bd2e);
    assert_eq!(&matrix[12..15], &[2.0, 1.25, 13.0]);
    let local = mesh.vertices[0].position_words.map(f32::from_bits);
    let world_x = local[0] * matrix[0] + local[2] * matrix[8] + matrix[12];
    let world_z = local[0] * matrix[2] + local[2] * matrix[10] + matrix[14];
    assert_eq!(world_x, 5.0);
    assert_eq!(world_z, 16.0);
    let rotated_view = [
        0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 4.0, 5.0, 6.0, 1.0,
    ];
    assert_eq!(mesh.world_matrix(rotated_view).unwrap(), matrix);
}

#[test]
fn selector_queue_overwrites_only_valid_requests_and_waits_for_idle_callback() {
    let mut owner = scene();
    owner.request(1);
    owner.request(99);
    assert_eq!(owner.pending, 1);
    owner.request(2);
    owner.advance(250, 123).unwrap();
    assert_eq!(
        (owner.status(), owner.state_timer, owner.event_gate),
        (2, 0, false)
    );
    owner.request(1);
    owner.state_timer = 6999;
    let frame = owner.advance(1, 345).unwrap();
    assert_eq!(
        frame.audio[0],
        Some(NativeSceneSound::PlayStaticMain {
            slot: 17,
            volume: 345,
            looping: false
        })
    );
    assert_eq!(
        frame.audio[1],
        Some(NativeSceneSound::StopStatic { slot: 17 })
    );
    assert_eq!(owner.status(), u32::MAX);
    assert_eq!(owner.state, 0);
    let next = owner.advance(200, 456).unwrap();
    assert_eq!(owner.status(), 1);
    assert_eq!(owner.state_timer, 0);
    assert_eq!(next.audio, [None; 4]);
    let active = owner.advance(0, 789).unwrap();
    assert_eq!(
        active.audio[0],
        Some(NativeSceneSound::PlayStaticMain {
            slot: 16,
            volume: 789,
            looping: false
        })
    );
    assert_eq!(owner.advance(0, 999).unwrap().audio, [None; 4]);
}

#[test]
fn state_clock_caps_at_200_but_rotation_uses_the_entire_delta_and_wraps() {
    let mut owner = scene();
    enter(&mut owner, 1);
    let frame = owner.advance(1250, u32::MAX).unwrap();
    assert_eq!(owner.state_timer, 200);
    assert_eq!(owner.rotation_clock, 1250);
    assert_eq!(owner.mesh(2).unwrap().angle_words[1], 90.0_f32.to_bits());
    assert_eq!(
        frame.audio[0],
        Some(NativeSceneSound::PlayStaticMain {
            slot: 16,
            volume: u32::MAX,
            looping: false
        })
    );
    owner.state_timer = u32::MAX - 50;
    owner.advance(1000, 0).unwrap();
    assert_eq!(owner.state_timer, 149);
    assert_eq!(owner.state, 1);
}

#[test]
fn all_twelve_fade_in_groups_follow_integer_boundaries_without_touching_lower_rings() {
    // These are native upper-ring references, not root-major render indices.
    let groups: [&[usize]; 12] = [
        &[0],
        &[1, 2],
        &[3, 4],
        &[5],
        &[6],
        &[7, 8],
        &[9, 10],
        &[11],
        &[12],
        &[13, 14],
        &[15, 16],
        &[17],
    ];
    for (group, children) in groups.iter().enumerate() {
        for (elapsed, expected) in [(0, 0), (1, 0), (2999, 254), (3000, 255)] {
            let mut owner = scene();
            enter(&mut owner, 1);
            owner.state_timer = group as u32 * 300 + elapsed;
            owner.advance(0, 0).unwrap();
            for child in *children {
                assert_eq!(
                    alpha(&owner, *child),
                    expected,
                    "group {group}, time {elapsed}"
                );
                for vertex in &owner.mesh(upper_index(*child)).unwrap().vertices {
                    assert_eq!(
                        vertex.rgba_words[3],
                        (expected as f32 / 255.0_f32).to_bits()
                    );
                }
            }
            for provider in 0..6 {
                assert_eq!(owner.mesh(2 + provider * 4).unwrap().alpha, 255);
            }
        }
    }
    let mut owner = scene();
    enter(&mut owner, 1);
    owner.advance(200, 0).unwrap();
    assert_eq!(alpha(&owner, 0), 17);
    assert_eq!(alpha(&owner, 1), 255); // Not pre-cleared before its delayed start.
    assert_eq!(alpha(&owner, 2), 255);
    owner.advance(100, 0).unwrap();
    assert_eq!(alpha(&owner, 1), 0);
    assert_eq!(alpha(&owner, 2), 0);
}

#[test]
fn fade_in_finishes_at_6500_and_does_not_stop_sound16() {
    let mut owner = scene();
    enter(&mut owner, 1);
    owner.advance(1, 11).unwrap();
    owner.state_timer = 6498;
    assert_eq!(owner.advance(1, 11).unwrap().audio, [None; 4]);
    assert_eq!(owner.status(), 1);
    let done = owner.advance(1, 11).unwrap();
    assert_eq!(done.audio, [None; 4]);
    assert_eq!(
        (owner.status(), owner.state_timer, owner.event_gate),
        (0, 0, false)
    );
    for child in 0..18 {
        assert_eq!(alpha(&owner, child), 255);
    }
}

#[test]
fn all_fade_out_groups_start_after_1500_and_finish_2000ms_later() {
    let groups: [&[usize]; 12] = [
        &[0],
        &[1, 2],
        &[3, 4],
        &[5],
        &[6],
        &[7, 8],
        &[9, 10],
        &[11],
        &[12],
        &[13, 14],
        &[15, 16],
        &[17],
    ];
    for (group, children) in groups.iter().enumerate() {
        for (elapsed, expected) in [(0, 255), (1, 255), (1999, 1), (2000, 0)] {
            let mut owner = scene();
            enter(&mut owner, 2);
            owner.state_timer = 1500 + group as u32 * 300 + elapsed;
            owner.advance(0, 0).unwrap();
            for child in *children {
                assert_eq!(
                    alpha(&owner, *child),
                    expected,
                    "group {group}, time {elapsed}"
                );
            }
        }
    }
    let mut owner = scene();
    enter(&mut owner, 2);
    owner.state_timer = 1499;
    owner.advance(0, 0).unwrap();
    for child in 0..18 {
        assert_eq!(alpha(&owner, child), 255);
    }
}

#[test]
fn moving_d_quad_uses_mulss_divss_subss_then_resets_at_7000() {
    let mut owner = scene();
    enter(&mut owner, 2);
    for time in [0_u32, 1, 199, 999, 1000, 1001, 6999] {
        owner.state_timer = time;
        owner.advance(0, 0).unwrap();
        let lift = if time >= 1000 {
            32.0_f32
        } else {
            time as f32 * 32.0_f32 / 1000.0_f32
        };
        assert_eq!(
            owner.mesh(26).unwrap().position_words,
            words([2.0, lift - 20.0_f32, 12.0])
        );
        assert_eq!(owner.status(), 2);
    }
    let frame = owner.advance(1, 0).unwrap();
    assert_eq!(frame.audio[0], None);
    assert_eq!(
        frame.audio[1],
        Some(NativeSceneSound::StopStatic { slot: 17 })
    );
    assert_eq!(
        owner.mesh(26).unwrap().position_words,
        words([2.0, -20.0, 12.0])
    );
    assert_eq!(
        (owner.state, owner.state_timer, owner.event_gate),
        (0, 0, false)
    );
    for child in 0..18 {
        assert_eq!(alpha(&owner, child), 0);
    }
    for index in [0, 1, 26] {
        assert_eq!(owner.mesh(index).unwrap().alpha, 255);
    }
}

#[test]
fn effects_never_emit_camera_updates_or_repeat_their_audio_gate() {
    let mut owner = scene();
    for selector in [1, 2] {
        enter(&mut owner, selector);
        for step in 0..40 {
            let frame = owner.advance(200, 0x00ff_ffff).unwrap();
            assert_eq!(frame.position_words, None);
            assert_eq!(frame.angle_words, None);
            assert_eq!(frame.field_of_view_word, None);
            if step != 0 {
                assert_eq!(frame.audio[0], None);
            }
        }
        assert_eq!(owner.status(), 0);
    }
}

#[test]
fn private_snapshot_resumes_upper_ring_alpha_clocks_gate_and_pending_request() {
    let mut owner = scene();
    enter(&mut owner, 2);
    for _ in 0..13 {
        owner.advance(173, 713).unwrap();
    }
    owner.request(1);
    let encoded = serde_json::to_vec(&owner).unwrap();
    let mut restored: CmvsNativeSceneKind52 = serde_json::from_slice(&encoded).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.state_timer, 2249);
    assert_eq!(restored.pending, 1);
    assert!(restored.event_gate);
    let frame = restored.advance(137, 999).unwrap();
    assert_eq!(frame.audio[0], None); // A resumed effect must not replay its opening sound.
    assert_eq!(frame, owner.advance(137, 999).unwrap());
    for delta in [200_u32; 70] {
        assert_eq!(
            restored.advance(delta, 31).unwrap(),
            owner.advance(delta, 31).unwrap()
        );
        assert_eq!(restored, owner);
    }
}

#[test]
fn malformed_private_mesh_errors_before_advancing_the_live_phase() {
    let mut owner = scene();
    owner.meshes[3].position_words[0] = f32::NAN.to_bits();
    owner.request(2);
    assert!(owner.validate().is_err());
    assert!(owner.advance(17, 255).is_err());
    assert_eq!(owner.rotation_clock, 0);
    assert_eq!(owner.state, 0);
    assert_eq!(owner.pending, 2);
}
