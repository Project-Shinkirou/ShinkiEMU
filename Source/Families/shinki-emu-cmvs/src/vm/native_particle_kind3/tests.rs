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
