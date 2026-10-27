use super::*;

fn parameters(count: u32) -> [u32; 4] {
    [0, 800, 100, count]
}

fn moving(y: u32, speed: u32) -> CmvsNativeParticleKind9 {
    CmvsNativeParticleKind9 {
        resource: b"particle".to_vec(),
        parameter_words: parameters(1),
        extent: [800, 600],
        particles: vec![CmvsNativeParticleKind9Particle {
            fixed_position_words: [41 << 10, y],
            speed_word: speed,
            atlas_variant: 1,
        }],
        clock_ms: 0,
    }
}

#[test]
fn births_use_four_shared_crt_draws_and_unsigned_speed_jitter() {
    let mut rng = 1;
    let value =
        CmvsNativeParticleKind9::new(b"particle".to_vec(), parameters(2), [800, 600], &mut rng)
            .unwrap();
    //CRT41,18467,6334,26500;19169,15724,11478,29358.
    assert_eq!(rng, 0x72ae7869);
    assert_eq!(
        value.particles[0].fixed_position_words,
        [41 << 10, 467 << 10]
    );
    assert_eq!(value.particles[0].atlas_variant, 1);
    assert_eq!(value.particles[0].speed_word, 800);
    assert_eq!(
        value.particles[1].fixed_position_words,
        [769 << 10, 124 << 10]
    );
    assert_eq!(value.particles[1].atlas_variant, 0);
    assert_eq!(value.particles[1].speed_word, 2736);
}

#[test]
fn birth_jitter_addition_and_variant_products_wrap_before_unsigned_division() {
    let added =
        CmvsNativeParticleKind9::new(Vec::new(), [0, u32::MAX, 0, 1], [800, 600], &mut 1).unwrap();
    //Variant1: FFFFFFFF + (26500 << 7), wrapped before use.
    assert_eq!(added.particles[0].speed_word, 3_391_999);
    let multiplied =
        CmvsNativeParticleKind9::new(Vec::new(), [0, 0x80000000, 0, 3], [800, 600], &mut 1)
            .unwrap();
    assert_eq!(multiplied.particles[0].speed_word, 2_150_875_648);
    //Variant0: wrapped*3 then SHR1; variant2: wrapped*2 then unsigned DIV3.
    assert_eq!(multiplied.particles[1].speed_word, 1_079_378_560);
    assert_eq!(multiplied.particles[2].speed_word, 2_401_706);
}

#[test]
fn all_three_variants_have_native_crop_pivot_speed_and_wrapping_order() {
    let mut params = parameters(3);
    params[2] = u32::MAX - 1;
    let value = CmvsNativeParticleKind9::new(Vec::new(), params, [800, 600], &mut 1).unwrap();
    //Third particle CRT26962,24464,5705,28145:variant2,jitter5.
    for (index, variant, rect, pivot, speed, order) in [
        (0, 1, [24, 0, 16, 16], 8, 800, 0),
        (1, 0, [0, 0, 24, 24], 12, 2736, u32::MAX - 1),
        (2, 2, [48, 0, 8, 8], 4, 960, 2),
    ] {
        let basic = value.draw_basic(index).unwrap();
        assert_eq!(value.particles[index].atlas_variant, variant);
        assert_eq!(value.particles[index].speed_word, speed);
        assert_eq!(basic.rect_words, Some(rect));
        assert_eq!(basic.position_words, Some([pivot; 2]));
        assert_eq!(basic.auxiliary_word, Some(order));
        assert_eq!(value.draw_order(index), Some(order));
        assert_eq!(basic.basic_kind, 0);
        assert_eq!(basic.blend_mode, Some(0));
        assert_eq!(basic.alpha, Some(255));
        assert_eq!(basic.scale_words, Some([1.0_f32.to_bits(); 2]));
        assert_eq!(basic.rotation_hundredths, 0);
        assert_eq!(basic.float_position_words, [0; 3]);
    }
    assert_eq!(
        value.draw_basic(0).unwrap().auxiliary_pair_words,
        Some([41, 467])
    );
    assert_eq!(value.draw_basic(3), None);
    assert_eq!(value.draw_order(3), None);
}

#[test]
fn flags_do_not_select_plane_projection_blending_or_birth_behavior() {
    let base = CmvsNativeParticleKind9::new(Vec::new(), parameters(3), [800, 600], &mut 1).unwrap();
    let mut params = parameters(3);
    params[0] = u32::MAX;
    let flagged = CmvsNativeParticleKind9::new(Vec::new(), params, [800, 600], &mut 1).unwrap();
    assert_eq!(base.particles, flagged.particles);
    for index in 0..3 {
        assert_eq!(base.draw_basic(index), flagged.draw_basic(index));
    }
    assert_eq!(flagged.capture().unwrap().parameter_words[0], u32::MAX);
}

#[test]
fn unsafe_zero_divisors_fail_without_consuming_rng_but_count_zero_skips_division() {
    let mut rng = 0x12345678;
    for (speed, extent) in [
        (79, [800, 600]),
        (0, [800, 600]),
        (80, [0, 600]),
        (80, [800, 0]),
    ] {
        assert!(
            CmvsNativeParticleKind9::new(Vec::new(), [0, speed, 0, 1], extent, &mut rng).is_err()
        );
        assert_eq!(rng, 0x12345678);
    }
    let empty = CmvsNativeParticleKind9::new(Vec::new(), [0; 4], [0; 2], &mut rng).unwrap();
    empty.validate().unwrap();
    assert_eq!(empty.particle_capacity(), 0);
    assert_eq!(rng, 0x12345678);
    let boundary =
        CmvsNativeParticleKind9::new(Vec::new(), [0, 80, 0, 1], [1, 1], &mut rng).unwrap();
    assert_eq!(boundary.particles[0].fixed_position_words, [0; 2]);
}

#[test]
fn missing_or_empty_resource_does_not_gate_native_births() {
    let mut missing_rng = 1;
    let mut empty_rng = 1;
    let missing = CmvsNativeParticleKind9::new(
        b"missing".to_vec(),
        parameters(2),
        [800, 600],
        &mut missing_rng,
    )
    .unwrap();
    let empty = CmvsNativeParticleKind9::new(Vec::new(), parameters(2), [800, 600], &mut empty_rng)
        .unwrap();
    assert_eq!(missing.particles, empty.particles);
    assert_eq!(missing_rng, empty_rng);
    assert!(missing.draw_basic(0).unwrap().surface_initialized);
    for resource in [vec![b'a'; 32], b"a\0b".to_vec()] {
        assert!(
            CmvsNativeParticleKind9::new(resource, parameters(1), [800, 600], &mut missing_rng)
                .is_err()
        );
        assert_eq!(missing_rng, 0x72ae7869);
    }
}

#[test]
fn draw_queue_limit_is_not_a_particle_allocation_cap() {
    let value =
        CmvsNativeParticleKind9::new(Vec::new(), parameters(8193), [800, 600], &mut 1).unwrap();
    assert_eq!(value.particle_capacity(), 8193);
    assert!(value.draw_basic(8192).is_some());
}

#[test]
fn fixed_point_fraction_survives_frames_and_uses_signed_pixel_shift() {
    let mut value = moving((-1025_i32) as u32, 16);
    let mut rng = 77;
    assert_eq!(
        value.draw_basic(0).unwrap().auxiliary_pair_words,
        Some([41, (-2_i32) as u32])
    );
    value.advance(1, &mut rng).unwrap();
    assert_eq!(
        value.particles[0].fixed_position_words[1],
        (-1024_i32) as u32
    );
    assert_eq!(
        value.draw_basic(0).unwrap().auxiliary_pair_words,
        Some([41, (-1_i32) as u32])
    );
    value.advance(1, &mut rng).unwrap();
    assert_eq!(
        value.particles[0].fixed_position_words[1],
        (-1023_i32) as u32
    );
    assert_eq!(rng, 77);
}

#[test]
fn movement_wraps_product_before_logical_shift_and_wraps_fixed_addition() {
    let mut product = moving(0, 0x80000010);
    product.advance(2, &mut 0).unwrap();
    assert_eq!(product.particles[0].fixed_position_words[1], 2);
    let mut addition = moving(u32::MAX - 15, 256);
    addition.advance(1, &mut 0).unwrap();
    assert_eq!(addition.particles[0].fixed_position_words[1], 0);
}

#[test]
fn clock_advances_full_delta_while_motion_caps_at_five_hundred() {
    let mut short = moving(0, 80);
    let mut long = short.clone();
    long.clock_ms = u32::MAX - 99;
    short.advance(500, &mut 8).unwrap();
    long.advance(1000, &mut 8).unwrap();
    assert_eq!(short.particles, long.particles);
    assert_eq!(short.particles[0].fixed_position_words[1], 2500);
    assert_eq!(long.clock_ms, 900);
    long.advance(1, &mut 8).unwrap();
    assert_eq!(long.particles[0].fixed_position_words[1], 2505);
    assert_eq!(long.clock_ms, 901);
}

#[test]
fn bottom_boundary_resets_to_minus_sixteen_without_modulo_or_rng() {
    let mut value = moving((632 << 10) - 1, 16);
    let mut rng = 99;
    value.advance(0, &mut rng).unwrap();
    assert_eq!(value.particles[0].fixed_position_words[1], (632 << 10) - 1);
    value.advance(1, &mut rng).unwrap();
    assert_eq!(
        value.particles[0].fixed_position_words[1],
        (-16384_i32) as u32
    );
    assert_eq!(
        value.draw_basic(0).unwrap().auxiliary_pair_words,
        Some([41, (-16_i32) as u32])
    );
    value.advance(1, &mut rng).unwrap();
    assert_eq!(
        value.particles[0].fixed_position_words[1],
        (-16383_i32) as u32
    );
    value.particles[0].fixed_position_words[1] = 1000 << 10;
    value.advance(0, &mut rng).unwrap();
    assert_eq!(
        value.particles[0].fixed_position_words[1],
        (-16384_i32) as u32
    );
    assert_eq!(rng, 99);
}

#[test]
fn bottom_comparison_is_signed_and_height_addition_wraps() {
    let mut value = moving(0, 16);
    value.extent[1] = 0x7fffffe0;
    value.advance(0, &mut 0).unwrap();
    assert_eq!(
        value.particles[0].fixed_position_words[1],
        (-16384_i32) as u32
    );
    value.extent[1] = u32::MAX;
    value.particles[0].fixed_position_words[1] = (31 << 10) - 1;
    value.advance(0, &mut 0).unwrap();
    assert_eq!(value.particles[0].fixed_position_words[1], (31 << 10) - 1);
    value.advance(1, &mut 0).unwrap();
    assert_eq!(
        value.particles[0].fixed_position_words[1],
        (-16384_i32) as u32
    );
}

#[test]
fn native_wire_retains_extent_and_raw_resource_tail() {
    let value = CmvsNativeParticleKind9::new(
        b"particle".to_vec(),
        [0xdeadbeef, 800, 0xfffffff0, 1],
        [1234, 567],
        &mut 1,
    )
    .unwrap();
    let saved = value.capture().unwrap();
    assert_eq!(
        saved.parameter_words,
        [0xdeadbeef, 1234, 567, 800, 0xfffffff0, 1]
    );
    let mut bytes = saved.encode().unwrap();
    assert_eq!(bytes.len(), 60);
    assert_eq!(&bytes[..4], &[1, 0, 9, 0]);
    assert_eq!(&bytes[8..12], &1234_u32.to_le_bytes());
    assert_eq!(&bytes[12..16], &567_u32.to_le_bytes());
    bytes[59] = 0x81;
    let decoded = SavedNativeParticleKind9::decode(&bytes).unwrap();
    assert_eq!(decoded.encode().unwrap(), bytes);
}

#[test]
fn native_rebuild_uses_current_owner_extent_and_restarts_births() {
    let mut value =
        CmvsNativeParticleKind9::new(b"particle".to_vec(), parameters(2), [800, 600], &mut 1)
            .unwrap();
    value.clock_ms = 12345;
    value.advance(250, &mut 9).unwrap();
    let mut saved = value.capture().unwrap();
    //449710 case8 never passes the saved extent words to416010.
    saved.parameter_words[1] = 0;
    saved.parameter_words[2] = 0;
    let mut rng = 1;
    let rebuilt = saved.rebuild([100, 90], &mut rng).unwrap();
    assert_eq!(rebuilt.extent, [100, 90]);
    assert_eq!(rebuilt.clock_ms, 0);
    assert_eq!(
        rebuilt.particles[0].fixed_position_words,
        [41 << 10, 17 << 10]
    );
    assert_eq!(
        rebuilt.particles[1].fixed_position_words,
        [69 << 10, 64 << 10]
    );
    assert_eq!(rng, 0x72ae7869);
    assert!(saved.rebuild([0, 90], &mut rng).is_err());
    assert_eq!(rng, 0x72ae7869);
}

#[test]
fn private_restore_continues_fractional_phase_extent_clock_and_rng() {
    let mut value = moving((-1000_i32) as u32, 83);
    value.clock_ms = u32::MAX - 20;
    value.advance(7, &mut 9).unwrap();
    let bytes = serde_json::to_vec(&value).unwrap();
    let mut private: CmvsNativeParticleKind9 = serde_json::from_slice(&bytes).unwrap();
    private.validate().unwrap();
    assert_eq!(private.extent, [800, 600]);
    assert_eq!(private.clock_ms, u32::MAX - 13);
    assert_eq!(
        private.particles[0].fixed_position_words[1],
        (-964_i32) as u32
    );
    let mut rng = 9;
    private.advance(9, &mut rng).unwrap();
    value.advance(9, &mut 9).unwrap();
    assert_eq!(private, value);
    assert_eq!(
        private.particles[0].fixed_position_words[1],
        (-918_i32) as u32
    );
    assert_eq!(rng, 9);
}

#[test]
fn malformed_wire_and_private_invariants_fail_explicitly() {
    let saved = moving(0, 80).capture().unwrap();
    let bytes = saved.encode().unwrap();
    for length in [0, 3, 59] {
        assert!(SavedNativeParticleKind9::decode(&bytes[..length]).is_err());
    }
    let mut wrong_kind = bytes.clone();
    wrong_kind[2] = 7;
    assert!(SavedNativeParticleKind9::decode(&wrong_kind).is_err());
    let mut unterminated = bytes;
    unterminated[28..].fill(b'a');
    assert!(SavedNativeParticleKind9::decode(&unterminated).is_err());
    let mut value = moving(0, 80);
    value.particles[0].atlas_variant = 3;
    assert!(value.validate().is_err());
    value.particles[0].atlas_variant = 1;
    value.parameter_words[3] = 2;
    assert!(value.validate().is_err());
}
