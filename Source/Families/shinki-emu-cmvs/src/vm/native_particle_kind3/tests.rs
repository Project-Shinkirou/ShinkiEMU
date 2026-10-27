use super::*;

fn parameters(count: u32) -> [u32; 4] {
    [0, 7.0_f32.to_bits(), 1000.0_f32.to_bits(), count]
}

fn owner(y: f32, time: f32) -> CmvsNativeParticleKind3 {
    let mut value =
        CmvsNativeParticleKind3::new(b"particle".to_vec(), parameters(1), &mut 1).unwrap();
    value.particles[0].position_words = [1.25, y, 9.75].map(f32::to_bits);
    value.particles[0].fall_time_word = time.to_bits();
    value
}

#[test]
fn constructor_matches_four_draw_crt_sse_reference() {
    let mut rng = 1;
    let value =
        CmvsNativeParticleKind3::new(b"particle".to_vec(), parameters(1), &mut rng).unwrap();
    // Native CRT outputs41,18467,6334,26500; SSE rounds after every operation.
    assert_eq!(rng, 3_884_216_597);
    assert_eq!(
        value.particles[0].position_words,
        [0xc09f997f, 0x3ff76def, 0x414a2c74]
    );
    assert_eq!(value.particles[0].fall_time_word, 0x4484b7f0);
}

#[test]
fn constructor_consumes_four_draws_for_each_particle_in_order() {
    let mut rng = 1;
    let combined = CmvsNativeParticleKind3::new(Vec::new(), parameters(2), &mut rng).unwrap();
    let mut separate_rng = 1;
    let first = CmvsNativeParticleKind3::new(Vec::new(), parameters(1), &mut separate_rng).unwrap();
    let second =
        CmvsNativeParticleKind3::new(Vec::new(), parameters(1), &mut separate_rng).unwrap();
    assert_eq!(
        combined.particles,
        [first.particles[0].clone(), second.particles[0].clone()]
    );
    assert_eq!(rng, separate_rng);
    assert_eq!(rng, 1_924_036_713);
}

#[test]
fn zero_count_keeps_rng_and_produces_valid_empty_configuration() {
    let mut rng = 0x12345678;
    let value = CmvsNativeParticleKind3::new(Vec::new(), parameters(0), &mut rng).unwrap();
    assert_eq!(rng, 0x12345678);
    assert!(value.particles.is_empty());
    assert_eq!(value.capture().unwrap().encode().unwrap().len(), 52);
}

#[test]
fn missing_resource_does_not_suppress_births_or_rng_consumption() {
    let mut first_rng = 1;
    let missing =
        CmvsNativeParticleKind3::new(b"missing".to_vec(), parameters(2), &mut first_rng).unwrap();
    let mut second_rng = 1;
    let other =
        CmvsNativeParticleKind3::new(b"other".to_vec(), parameters(2), &mut second_rng).unwrap();
    assert_eq!(missing.particles, other.particles);
    assert_eq!(first_rng, second_rng);
}

#[test]
fn visible_motion_preserves_x_z_time_and_draw_attributes() {
    let mut value = owner(5.0, 1000.0);
    let before = value.draw_basic(0).unwrap();
    value.advance(100).unwrap();
    let after = value.draw_basic(0).unwrap();
    assert_eq!(
        after.float_position_words,
        [1.25, 4.0, 9.75].map(f32::to_bits)
    );
    assert_eq!(before.alpha, after.alpha);
    assert_eq!(before.rotation_hundredths, after.rotation_hundredths);
    assert_eq!(before.resource_frame, after.resource_frame);
    assert_eq!(value.particles[0].fall_time_word, 1000.0_f32.to_bits());
}

#[test]
fn clock_caps_unsigned_elapsed_at_five_hundred_milliseconds() {
    let mut capped = owner(8.0, 1000.0);
    let mut long = capped.clone();
    capped.advance(500).unwrap();
    long.advance(u32::MAX).unwrap();
    assert_eq!(capped, long);
    assert_eq!(f32::from_bits(capped.particles[0].position_words[1]), 3.0);
}

#[test]
fn zero_elapsed_keeps_ordinary_particle_phase() {
    let mut value = owner(5.5, 1000.0);
    let before = value.clone();
    value.advance(0).unwrap();
    assert_eq!(value, before);
}

#[test]
fn lifecycle_uses_strict_negative_single_wrap_without_new_random_birth() {
    let mut value = owner(1.0, 1000.0);
    value.advance(100).unwrap();
    assert_eq!(f32::from_bits(value.particles[0].position_words[1]), 0.0);
    value.advance(100).unwrap();
    assert_eq!(
        value.particles[0].position_words,
        [1.25, 9.0, 9.75].map(f32::to_bits)
    );
    let mut fast = owner(1.0, 100.0);
    fast.advance(500).unwrap();
    assert_eq!(f32::from_bits(fast.particles[0].position_words[1]), -39.0);
}

#[test]
fn negative_time_moves_upward_without_upper_boundary_wrap() {
    let mut value = owner(9.0, -1000.0);
    value.advance(500).unwrap();
    assert_eq!(f32::from_bits(value.particles[0].position_words[1]), 14.0);
}

#[test]
fn zero_time_and_nonfinite_native_words_are_not_artificially_rejected() {
    let mut value = owner(1.0, 0.0);
    value.advance(1).unwrap();
    assert_eq!(
        value.particles[0].position_words[1],
        f32::NEG_INFINITY.to_bits()
    );
    let mut config = parameters(1);
    config[1] = 0x7fc01234;
    config[2] = 0;
    let mut rng = 1;
    let value = CmvsNativeParticleKind3::new(Vec::new(), config, &mut rng).unwrap();
    value.validate().unwrap();
    assert!(f32::from_bits(value.particles[0].position_words[2]).is_nan());
    assert_eq!(value.capture().unwrap().parameter_words, config);
}

#[test]
fn basic_surface_matches_native_crop_pivot_projection_and_defaults() {
    for low_flags in 0..4 {
        let mut value = owner(4.0, 1000.0);
        value.parameter_words[0] = low_flags;
        let basic = value.draw_basic(0).unwrap();
        assert!(basic.surface_initialized);
        assert!(!basic.hidden);
        assert_eq!(basic.basic_kind, 2);
        assert_eq!(basic.blend_mode, Some(1));
        assert_eq!(basic.rect_words, Some([0, 0, 32, 32]));
        assert_eq!(basic.position_words, Some([16, 16]));
        assert_eq!(
            basic.projection_depth_word,
            (3.0 + low_flags as f32).to_bits()
        );
        assert_eq!(basic.projection_height_word, 3.0_f32.to_bits());
        assert_eq!(basic.alpha, Some(255));
        assert_eq!(basic.scale_words, Some([1.0_f32.to_bits(); 2]));
        assert_eq!(basic.rotation_hundredths, 0);
        assert_eq!(basic.resource_frame, Some(0));
        assert!(basic.resource.is_none());
        assert!(basic.reference_size.is_none());
        assert_eq!(
            basic.float_position_words,
            value.particles[0].position_words
        );
    }
}

#[test]
fn only_low_two_flag_bits_affect_kind3_planes() {
    let mut low = owner(4.0, 1000.0);
    low.parameter_words[0] = 2;
    let mut high = low.clone();
    high.parameter_words[0] = 0xffff_fffe;
    assert_eq!(low.draw_basic(0), high.draw_basic(0));
    low.advance(17).unwrap();
    high.advance(17).unwrap();
    assert_eq!(low.particles, high.particles);
}

#[test]
fn native_wire_is_exact_configuration_only_and_preserves_resource_tail() {
    let mut value = owner(1.0, 1000.0);
    let before = value.capture().unwrap();
    value.advance(200).unwrap();
    assert_eq!(value.capture().unwrap(), before);
    let bytes = before.encode().unwrap();
    assert_eq!(&bytes[..4], &[1, 0, 3, 0]);
    assert_eq!(&bytes[4..8], &0_u32.to_le_bytes());
    assert_eq!(&bytes[20..29], b"particle\0");
    let mut tail = bytes;
    tail[51] = 0xa5;
    let decoded = SavedNativeParticleKind3::decode(&tail).unwrap();
    assert_eq!(decoded.encode().unwrap(), tail);
}

#[test]
fn cold_rebuild_restarts_phase_using_current_shared_rng() {
    let mut value = owner(1.0, 1000.0);
    value.advance(500).unwrap();
    let saved = value.capture().unwrap();
    let mut first_rng = 123;
    let mut second_rng = first_rng;
    let rebuilt = saved.rebuild(&mut first_rng).unwrap();
    let expected =
        CmvsNativeParticleKind3::new(b"particle".to_vec(), parameters(1), &mut second_rng).unwrap();
    assert_eq!(rebuilt, expected);
    assert_eq!(first_rng, second_rng);
    assert_ne!(rebuilt.particles, value.particles);
}

#[test]
fn private_snapshot_retains_complete_phase_and_continues_identically() {
    let mut original = owner(0.25, 1000.0);
    original.advance(79).unwrap();
    let bytes = postcard::to_allocvec(&original).unwrap();
    let mut restored: CmvsNativeParticleKind3 = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(restored, original);
    original.advance(43).unwrap();
    restored.advance(43).unwrap();
    assert_eq!(restored, original);
}

#[test]
fn resource_and_count_guards_do_not_consume_rng() {
    for resource in [vec![b'a'; 32], b"a\0b".to_vec()] {
        let mut rng = 77;
        assert!(CmvsNativeParticleKind3::new(resource, parameters(1), &mut rng).is_err());
        assert_eq!(rng, 77);
    }
    let mut rng = 77;
    assert!(CmvsNativeParticleKind3::new(Vec::new(), parameters(8193), &mut rng).is_err());
    assert_eq!(rng, 77);
    let value = CmvsNativeParticleKind3::new(vec![b'a'; 31], parameters(8192), &mut rng).unwrap();
    assert_eq!(value.particles.len(), 8192);
}

#[test]
fn malformed_native_wire_and_inconsistent_private_counts_fail() {
    let value = owner(1.0, 1000.0);
    let valid = value.capture().unwrap().encode().unwrap();
    for length in [0, 3, 4, 51, 53] {
        assert!(SavedNativeParticleKind3::decode(&vec![0; length]).is_err());
    }
    let mut wrong_kind = valid.clone();
    wrong_kind[2] = 6;
    assert!(SavedNativeParticleKind3::decode(&wrong_kind).is_err());
    let mut unterminated = valid;
    unterminated[20..].fill(1);
    assert!(SavedNativeParticleKind3::decode(&unterminated).is_err());
    let mut malformed = value;
    malformed.particles.clear();
    assert!(malformed.validate().is_err());
    assert!(malformed.capture().is_err());
    assert!(malformed.advance(1).is_err());
}

#[test]
fn plane_adapter_exposes_all_live_particles_and_bounds_indices() {
    let value = owner(1.0, 1000.0);
    assert_eq!(value.particle_capacity(), 1);
    assert_eq!(
        value.plane_position(0),
        Some(value.particles[0].position_words)
    );
    assert!(value.plane_position(1).is_none());
    assert!(value.draw_basic(1).is_none());
    let empty = CmvsNativeParticleKind3::new(Vec::new(), parameters(0), &mut 1).unwrap();
    assert_eq!(empty.particle_capacity(), 0);
    assert!(empty.plane_position(0).is_none());
    assert!(empty.draw_basic(0).is_none());
}
