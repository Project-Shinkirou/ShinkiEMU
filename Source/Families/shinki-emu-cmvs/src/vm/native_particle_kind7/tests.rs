use super::*;

fn parameters(flags: u32, count: u32) -> [u32; 4] {
    [flags, 7.0_f32.to_bits(), 1000.0_f32.to_bits(), count]
}

fn moving(flags: u32, y: f32, time: f32) -> CmvsNativeParticleKind7 {
    let mut value = CmvsNativeParticleKind7::new(Vec::new(), parameters(flags, 1), &mut 1).unwrap();
    value.particles[0].position_words = [1.25, y, 9.75].map(f32::to_bits);
    value.particles[0].fall_time_word = time.to_bits();
    value
}

#[test]
fn births_follow_five_draw_crt_sse_reference_including_unused_fourth_draw() {
    let mut rng = 1;
    let value =
        CmvsNativeParticleKind7::new(b"particle".to_vec(), parameters(0, 1), &mut rng).unwrap();
    // CRT41,18467,6334,26500(unused),19169; each operation rounds to SSE f32.
    assert_eq!(rng, 0xcae1df84);
    assert_eq!(
        value.particles[0].position_words,
        [0xc233851f, 0x4167d70a, 0x419228f6]
    );
    assert_eq!(value.particles[0].fall_time_word, 0x447f5026);
    let mut rng = 1;
    let two =
        CmvsNativeParticleKind7::new(b"missing".to_vec(), parameters(0, 2), &mut rng).unwrap();
    assert_eq!(rng, 0xdf90722b);
    assert_eq!(two.particle_capacity(), 2);
    assert_eq!(two.plane_position(2), None);
    assert!(two.draw_basic(2).is_none());
}

#[test]
fn spread_nibble_is_independent_of_projection_nibble() {
    let expected = [
        0x419228f6, 0x41773333, 0x414a147b, 0x411cf5c3, 0x41066666, 0x40f1eb85,
    ];
    for (nibble, z) in expected.into_iter().enumerate() {
        let value = CmvsNativeParticleKind7::new(
            Vec::new(),
            parameters(0x40000000 | (nibble as u32) << 24, 1),
            &mut 1,
        )
        .unwrap();
        assert_eq!(value.particles[0].position_words[2], z);
        assert_eq!(
            value.draw_basic(0).unwrap().projection_depth_word,
            60.0_f32.to_bits()
        );
    }
}

#[test]
fn basic_setters_preserve_native_plane_crop_pivot_rotation_scale_and_shake() {
    for (nibble, depth) in [
        (0, 12.0_f32),
        (4, 60.0),
        (5, 50.0),
        (6, 40.0),
        (7, 30.0),
        (8, 20.0),
        (15, 12.0),
    ] {
        let value = moving(nibble << 28, 5.0, 1000.0);
        let basic = value.draw_basic(0).unwrap();
        assert_eq!(basic.basic_kind, 2);
        assert_eq!(basic.rect_words, Some([0, 0, 32, 32]));
        assert_eq!(basic.position_words, Some([16, 16]));
        assert_eq!(basic.projection_depth_word, depth.to_bits());
        assert_eq!(basic.projection_height_word, 0);
        assert_eq!(basic.rotation_hundredths, 0);
        assert_eq!(basic.scale_words, Some([1.0_f32.to_bits(); 2]));
        assert_eq!(basic.blend_mode, Some(1));
        assert!(basic.screen_shake_disabled);
        assert!(!basic.hidden);
        assert!(basic.collision_geometry.is_none());
    }
}

#[test]
fn constructor_owner_alpha_and_unupdated_basic_alpha_are_distinct() {
    let mut value = moving(1 << 16 | 1 << 8 | 2, 5.0, 1000.0);
    assert_eq!(value.flash_mode, 1);
    assert_eq!(value.alpha, 0);
    assert_eq!(value.draw_basic(0).unwrap().alpha, Some(255));
    let before = value.particles[0].position_words;
    value.advance(0, &mut 1).unwrap();
    assert_eq!(value.draw_basic(0).unwrap().alpha, Some(0));
    assert_eq!(value.particles[0].position_words, before);
}

#[test]
fn fade_boundaries_use_unsigned_integer_division_and_inclusive_delay() {
    for mode in [1, 2] {
        for (elapsed, fade_in_alpha) in [
            (0, 0),
            (99, 0),
            (100, 0),
            (101, 1),
            (199, 126),
            (200, 127),
            (299, 253),
            (300, 255),
            (301, 255),
        ] {
            let mut value = moving(mode << 16 | 1 << 8 | 2, 50.0, 1000.0);
            value.advance(elapsed, &mut 1).unwrap();
            let expected = if mode == 1 {
                fade_in_alpha
            } else {
                255 - fade_in_alpha
            };
            assert_eq!(value.alpha, expected, "mode{mode} elapsed{elapsed}");
            assert_eq!(value.draw_basic(0).unwrap().alpha, Some(expected as i32));
        }
    }
}

#[test]
fn zero_duration_switches_at_delay_without_division() {
    for mode in [1, 2] {
        for elapsed in [0, 99, 100, 101] {
            let mut value = moving(mode << 16 | 1 << 8, 50.0, 1000.0);
            value.advance(elapsed, &mut 1).unwrap();
            let visible = if mode == 1 {
                elapsed >= 100
            } else {
                elapsed < 100
            };
            assert_eq!(value.alpha, if visible { 255 } else { 0 });
        }
        let mut immediate = moving(mode << 16, 5.0, 1000.0);
        immediate.advance(0, &mut 1).unwrap();
        assert_eq!(immediate.alpha, if mode == 1 { 255 } else { 0 });
    }
}

#[test]
fn mode_uses_full_byte_modulo_three_not_low_bits_or_nonzero_test() {
    for byte in [0_u32, 1, 2, 3, 4, 5, 127, 128, 255] {
        let mut value = moving(byte << 16 | 0xff00 | 255, 5.0, 1000.0);
        assert_eq!(value.flash_mode, byte % 3);
        assert_eq!(value.flash_delay_ms, 25500);
        assert_eq!(value.flash_duration_ms, 25500);
        value.advance(50000, &mut 1).unwrap();
        if byte % 3 == 0 {
            assert_eq!(value.alpha, 255);
        }
    }
    let mut unknown = moving(0, 5.0, 1000.0);
    unknown.flash_mode = 99;
    unknown.alpha = 73;
    unknown.advance(0, &mut 1).unwrap();
    assert_eq!(unknown.draw_basic(0).unwrap().alpha, Some(73));
}

#[test]
fn full_flash_delta_and_wrapping_clock_do_not_follow_motion_cap() {
    let mut value = moving(1 << 16 | 10, 50.0, 1000.0);
    value.clock_ms = u32::MAX - 99;
    value.advance(500, &mut 1).unwrap();
    assert_eq!(value.clock_ms, 400);
    assert_eq!(value.flash_elapsed_ms, 500);
    assert_eq!(value.alpha, 127);
    assert_eq!(value.particles[0].position_words[1], 48.8_f32.to_bits());
    value.flash_elapsed_ms = u32::MAX - 49;
    value.advance(100, &mut 1).unwrap();
    assert_eq!(value.flash_elapsed_ms, 50);
    assert_eq!(value.alpha, 12);
}

#[test]
fn empty_particles_still_advance_both_clocks_and_alpha() {
    let mut rng = 0x12345678;
    let mut value =
        CmvsNativeParticleKind7::new(Vec::new(), parameters(2 << 16 | 1, 0), &mut rng).unwrap();
    value.advance(100, &mut rng).unwrap();
    assert_eq!(value.clock_ms, 100);
    assert_eq!(value.flash_elapsed_ms, 100);
    assert_eq!(value.alpha, 0);
    assert_eq!(rng, 0x12345678);
}

#[test]
fn zero_y_does_not_respawn_but_negative_y_redraws_x_then_z_only() {
    let mut value = moving(0, 1.0, 1000.0);
    let mut rng = 1;
    value.advance(100, &mut rng).unwrap();
    assert_eq!(value.particles[0].position_words[1], 0);
    assert_eq!(rng, 1);
    value.advance(100, &mut rng).unwrap();
    assert_eq!(rng, 0xc823f683);
    assert_eq!(
        value.particles[0].position_words,
        [0xc233851f, 74.0_f32.to_bits(), 0x419228f6]
    );
    assert_eq!(value.particles[0].fall_time_word, 1000.0_f32.to_bits());
}

#[test]
fn motion_has_no_hundredth_quantization_and_adds_seventy_five_only_once() {
    let mut value = moving(0, 0.0, 1.0);
    let mut rng = 1;
    value.advance(120, &mut rng).unwrap();
    assert_eq!(
        value.particles[0].position_words[1],
        (-1125.0_f32).to_bits()
    );
    assert_eq!(rng, 0xc823f683);
    let mut fractional = moving(0, 5.0, 333.0);
    fractional.advance(1, &mut 1).unwrap();
    assert_eq!(
        fractional.particles[0].position_words[1],
        (5.0_f32 - 10.0_f32 / 333.0).to_bits()
    );
}

#[test]
fn ignored_flags_and_nonfinite_configuration_keep_native_sse_semantics() {
    let mut config = parameters(0x0f000000, 1);
    config[1] = 0x7fc01234;
    config[2] = 0;
    let mut value = CmvsNativeParticleKind7::new(Vec::new(), config, &mut 1).unwrap();
    assert_eq!(value.parameter_words, config);
    // CVTTSS2SI(NaN) -> INT_MIN, converted back to f32 and divided by100.
    assert_eq!(
        value.particles[0].position_words[2],
        ((i32::MIN as f32) / 100.0).to_bits()
    );
    let mut rng = 1;
    value.advance(0, &mut rng).unwrap();
    assert!(f32::from_bits(value.particles[0].position_words[1]).is_nan());
    assert_eq!(rng, 1);
    value.validate().unwrap();
    let mut infinite = moving(0, 5.0, 0.0);
    infinite.advance(1, &mut rng).unwrap();
    assert_eq!(
        infinite.particles[0].position_words[1],
        f32::NEG_INFINITY.to_bits()
    );
    assert_eq!(rng, 0xc823f683);
}

#[test]
fn native_record_has_exact_fifty_two_byte_layout_and_retains_name_padding() {
    let mut field = [0xa5; 32];
    field[..4].copy_from_slice(b"fog\0");
    let saved = SavedNativeParticleKind7 {
        parameter_words: [0x12345678, 0x7fc01234, 0x80000000, 0],
        resource_field: field,
    };
    let bytes = saved.encode().unwrap();
    assert_eq!(bytes.len(), 52);
    assert_eq!(&bytes[..8], &[1, 0, 7, 0, 0x78, 0x56, 0x34, 0x12]);
    assert_eq!(&bytes[8..12], &[0x34, 0x12, 0xc0, 0x7f]);
    assert_eq!(&bytes[20..], &field);
    assert_eq!(SavedNativeParticleKind7::decode(&bytes).unwrap(), saved);
    let mut rng = 1;
    let value = saved.rebuild(&mut rng).unwrap();
    assert_eq!(value.resource, b"fog");
    assert_eq!(value.parameter_words, saved.parameter_words);
    assert_eq!(rng, 1);
}

#[test]
fn codec_rejects_wrong_shape_version_kind_and_unterminated_name() {
    let valid = moving(0, 5.0, 1000.0).capture().unwrap().encode().unwrap();
    for length in [0, 3, 51, 53, 60] {
        assert!(SavedNativeParticleKind7::decode(&vec![0; length]).is_err());
    }
    for index in [0, 1, 2, 3] {
        let mut bytes = valid.clone();
        bytes[index] ^= 1;
        assert!(SavedNativeParticleKind7::decode(&bytes).is_err());
    }
    let mut bytes = valid;
    bytes[20..].fill(1);
    assert!(SavedNativeParticleKind7::decode(&bytes).is_err());
}

#[test]
fn native_rebuild_discards_phase_while_private_roundtrip_continues_exactly() {
    let mut value = moving(1 << 16 | 10, 1.0, 1000.0);
    value.clock_ms = 0xfffffff0;
    let mut rng = 1;
    value.advance(200, &mut rng).unwrap();
    let bytes = serde_json::to_vec(&value).unwrap();
    let mut private: CmvsNativeParticleKind7 = serde_json::from_slice(&bytes).unwrap();
    let mut private_rng = rng;
    value.advance(117, &mut rng).unwrap();
    private.advance(117, &mut private_rng).unwrap();
    assert_eq!(private, value);
    assert_eq!(private_rng, rng);
    let saved = value.capture().unwrap();
    let mut rebuild_rng = 1;
    let rebuilt = saved.rebuild(&mut rebuild_rng).unwrap();
    assert_eq!(rebuild_rng, 0xcae1df84);
    assert_eq!(rebuilt.clock_ms, 0);
    assert_eq!(rebuilt.flash_elapsed_ms, 0);
    assert_eq!(rebuilt.alpha, 0);
    assert_eq!(rebuilt.draw_basic(0).unwrap().alpha, Some(255));
    assert_eq!(
        rebuilt.particles[0].position_words,
        [0xc233851f, 0x4167d70a, 0x419228f6]
    );
}

#[test]
fn draw_queue_limit_does_not_cap_native_particle_allocation() {
    let value = CmvsNativeParticleKind7::new(Vec::new(), parameters(0, 8193), &mut 1).unwrap();
    assert_eq!(value.particle_capacity(), 8193);
    assert!(value.draw_basic(8192).is_some());
}

#[test]
fn invalid_resource_and_count_fail_without_advancing_rng() {
    let mut rng = 1;
    for resource in [vec![b'a'; 32], b"a\0b".to_vec()] {
        assert!(CmvsNativeParticleKind7::new(resource, parameters(0, 1), &mut rng).is_err());
    }
    assert_eq!(rng, 1);
    let mut value = moving(0, 5.0, 1000.0);
    value.parameter_words[3] = 2;
    assert!(value.advance(100, &mut rng).is_err());
    assert_eq!(rng, 1);
    assert_eq!(value.clock_ms, 0);
}
