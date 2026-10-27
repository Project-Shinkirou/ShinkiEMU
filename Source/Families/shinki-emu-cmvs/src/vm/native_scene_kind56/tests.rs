use super::*;

fn seeded(clock: u32, state: u32, pending: u32) -> CmvsNativeSceneKind56 {
    CmvsNativeSceneKind56::new_with_native_heap_phase([state, clock, pending]).unwrap()
}

#[test]
fn constructor_preserves_native_geometry_and_resource_owner_order() {
    let owner = CmvsNativeSceneKind56::new().unwrap();
    assert_eq!(owner.mesh_count(), 8);
    assert_eq!(owner.resource_count(), 2);
    assert_eq!(owner.resource(0), Some(b"po_bg510c.pb3".as_slice()));
    assert_eq!(owner.resource(1), Some(b"it008d2.pb3".as_slice()));
    assert_eq!(owner.resource(2), None);
    assert_eq!(owner.mesh(8), None);
    let positions = [
        [0, 0xc040_0000, 0x410b_3333],
        [0xbf5c_28f6, 0x3f19_999a, 0x4040_0000],
        [0x3f4c_cccd, 0x3fb3_3333, 0x4080_0000],
        [0xbf19_999a, 0x400c_cccd, 0x40a0_0000],
        [0x3f19_999a, 0x4040_0000, 0x40c0_0000],
        [0xbecc_cccd, 0x4073_3333, 0x40e0_0000],
        [0x3ecc_cccd, 0x4093_3333, 0x4100_0000],
        [0xbe4c_cccd, 0x40ac_cccd, 0x4110_0000],
    ];
    let pitch = [
        0,
        0x40a0_0000,
        0xc170_0000,
        0xc170_0000,
        0xc190_0000,
        0xc190_0000,
        0xc190_0000,
        0xc190_0000,
    ];
    for index in 0..8 {
        let mesh = owner.mesh(index).unwrap();
        assert_eq!(mesh.position_words, positions[index]);
        assert_eq!(
            mesh.angle_words,
            [
                pitch[index],
                0,
                match index {
                    0 => 0,
                    1 | 3 | 5 | 7 => 0xc120_0000,
                    _ => 0x4120_0000,
                }
            ]
        );
        assert_eq!((mesh.kind, mesh.alpha, mesh.blend), (3, 255, 0));
        assert_eq!(mesh.scale_words, words([1.0; 3]));
        assert!(!mesh.billboard);
        assert_eq!(mesh.force_blend, index != 0);
        assert_eq!(mesh.late_pass, index != 0);
        assert_eq!(owner.mesh_resource_index(index), usize::from(index != 0));
        assert_eq!(
            mesh.vertices.each_ref().map(|v| v.rgba_words),
            [words([1.0; 4]); 4]
        );
        assert_eq!(
            mesh.vertices.each_ref().map(|v| v.uv_words),
            [[0, 0], [0x3f80_0000, 0], [0, 0x3f80_0000], [0x3f80_0000; 2]]
        );
        let vertices = if index == 0 {
            [
                [-7.5, 9.0, 0.0],
                [7.5, 9.0, 0.0],
                [-7.5, 0.0, 0.0],
                [7.5, 0.0, 0.0],
            ]
        } else {
            [
                [-1.0, 0.0, 1.0],
                [1.0, 0.0, 1.0],
                [-1.0, 0.0, -1.0],
                [1.0, 0.0, -1.0],
            ]
        };
        assert_eq!(
            mesh.vertices.each_ref().map(|v| v.position_words),
            vertices.map(words)
        );
    }
}

#[test]
fn production_constructor_leaves_heap_phase_indeterminate_without_losing_geometry() {
    let mut owner = CmvsNativeSceneKind56::new().unwrap();
    owner.validate().unwrap();
    assert_eq!(owner.initial_frame(), NativeSceneFrame::default());
    assert!(owner.status().is_err());
    let before = owner.clone();
    for delta in [0, 1, 200, u32::MAX] {
        assert!(owner.advance(delta, u32::MAX).is_err());
        assert_eq!(owner, before);
        assert_eq!(owner.mesh_count(), 8);
    }
}

#[test]
fn every_request_is_a_noop_including_busy_and_unknown_phase() {
    for mut owner in [
        CmvsNativeSceneKind56::new().unwrap(),
        seeded(31, 19, 0),
        seeded(31, 19, u32::MAX),
    ] {
        let before = owner.clone();
        for selector in [0, 1, 2, 3, 10, 11, 20, 50, 100, u32::MAX] {
            owner.request(selector);
            assert_eq!(owner, before);
        }
    }
}

#[test]
fn status_short_circuits_the_native_pending_gate_without_transitions() {
    for state in [0, 1, 86, 0xdead_beef, u32::MAX] {
        let mut idle = seeded(1000, state, u32::MAX);
        assert_eq!(idle.status().unwrap(), state);
        idle.request(1);
        idle.advance(200, 0).unwrap();
        assert_eq!(idle.status().unwrap(), state);
        for pending in [0, 1, 86, u32::MAX - 1] {
            let mut busy = seeded(1000, state, pending);
            assert_eq!(busy.status().unwrap(), u32::MAX);
            busy.advance(200, 0).unwrap();
            busy.request(u32::MAX);
            assert_eq!(busy.status().unwrap(), u32::MAX);
        }
    }
    let mut partially_known = CmvsNativeSceneKind56::new().unwrap();
    partially_known.pending_word = Some(0);
    assert_eq!(partially_known.status().unwrap(), u32::MAX);
    partially_known.pending_word = Some(u32::MAX);
    assert!(partially_known.status().is_err());
}

#[test]
fn zero_delta_still_applies_all_seven_native_phase_offsets() {
    let mut owner = seeded(0, 0, u32::MAX);
    let background = owner.mesh(0).unwrap().clone();
    let frame = owner.advance(0, 99).unwrap();
    assert_eq!(frame, NativeSceneFrame::default());
    let expected = [
        0x4290_0000,
        0,
        0x432c_0000,
        0x4240_0000,
        0x4316_0000,
        0x4280_0000,
        0x4300_0000,
    ];
    for (index, yaw) in expected.into_iter().enumerate() {
        let mesh = owner.mesh(index + 1).unwrap();
        assert_eq!(mesh.angle_words[1], yaw);
        assert_eq!(mesh.alpha, 255);
    }
    assert_eq!(owner.mesh(0), Some(&background));
}

#[test]
fn pulse_keeps_integer_truncation_on_both_sides_of_the_period_boundary() {
    for (clock, alpha) in [
        (0, 255),
        (1, 255),
        (15, 255),
        (16, 254),
        (999, 193),
        (1000, 192),
        (1001, 192),
        (1015, 192),
        (1016, 193),
        (1999, 254),
        (2000, 255),
    ] {
        let mut owner = seeded(clock, 0, u32::MAX);
        let background = owner.mesh(0).unwrap().clone();
        owner.advance(0, 0).unwrap();
        for index in 1..8 {
            let mesh = owner.mesh(index).unwrap();
            assert_eq!(mesh.alpha, alpha);
            assert!(
                mesh.vertices
                    .iter()
                    .all(|v| v.rgba_words[3] == (alpha as f32 / 255.0_f32).to_bits())
            );
        }
        assert_eq!(owner.mesh(0), Some(&background));
    }
}

#[test]
fn common_delta_caps_before_wrapping_addition_and_volume_has_no_effect() {
    for delta in [200, 201, 100_000, u32::MAX] {
        let mut actual = seeded(u32::MAX - 20, 71, u32::MAX);
        let mut expected = actual.clone();
        actual.advance(delta, u32::MAX).unwrap();
        expected.advance(200, 0).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(actual.clock_ms, Some(179));
    }
    let mut owner = seeded(u32::MAX - 20, 0, 0);
    owner.advance(50, 0).unwrap();
    assert_eq!(owner.clock_ms, Some(29));
    assert_eq!(owner.mesh(2).unwrap().angle_words[1], 0);
    assert_eq!(owner.mesh(2).unwrap().alpha, 254);
}

#[test]
fn rotation_multiplies_in_u32_before_division_and_wraps_each_offset() {
    for (clock, yaw_words) in [
        (
            1_193_046,
            [
                0x424c_6666,
                0x43a9_8ccd,
                0x4317_199a,
                0x41d8_cccd,
                0x4301_199a,
                0x422c_6666,
                0x42d6_3333,
            ],
        ),
        (
            1_193_047,
            [
                0x4290_0000,
                0,
                0x432c_0000,
                0x4240_0000,
                0x4316_0000,
                0x4280_0000,
                0x4300_0000,
            ],
        ),
        (
            u32::MAX,
            [
                0x424c_6666,
                0x43a9_8ccd,
                0x4317_199a,
                0x41d8_cccd,
                0x4301_199a,
                0x422c_6666,
                0x42d6_3333,
            ],
        ),
    ] {
        let mut owner = seeded(clock, 0, u32::MAX);
        let before = owner.clone();
        owner.advance(0, 0).unwrap();
        for (index, yaw) in yaw_words.into_iter().enumerate() {
            let mesh = owner.mesh(index + 1).unwrap();
            let initial = before.mesh(index + 1).unwrap();
            assert_eq!(mesh.angle_words[1], yaw);
            assert_eq!(mesh.angle_words[0], initial.angle_words[0]);
            assert_eq!(mesh.angle_words[2], initial.angle_words[2]);
            assert_eq!(mesh.position_words, initial.position_words);
            assert_eq!(mesh.scale_words, initial.scale_words);
        }
    }
}

#[test]
fn private_snapshot_preserves_both_unknown_words_and_exact_seeded_phase() {
    for mut owner in [
        CmvsNativeSceneKind56::new().unwrap(),
        seeded(u32::MAX - 150, 0xfedc_ba98, u32::MAX),
    ] {
        let encoded = serde_json::to_vec(&owner).unwrap();
        let mut restored: CmvsNativeSceneKind56 = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(owner, restored);
        restored.validate().unwrap();
        if owner.clock_ms.is_some() {
            for delta in [200, 0, 199, 1] {
                assert_eq!(
                    owner.advance(delta, 0).unwrap(),
                    restored.advance(delta, 999).unwrap()
                );
                assert_eq!(owner, restored);
            }
        } else {
            assert!(restored.advance(0, 0).is_err());
            assert!(restored.status().is_err());
        }
    }
}

#[test]
fn validation_rejects_nonfinite_drawable_geometry_without_mutation() {
    let mut owner = seeded(0, 0, u32::MAX);
    owner.meshes[7].position_words[0] = f32::NAN.to_bits();
    let before = owner.clone();
    assert!(owner.validate().is_err());
    assert!(owner.advance(200, 0).is_err());
    assert_eq!(owner, before);
}
