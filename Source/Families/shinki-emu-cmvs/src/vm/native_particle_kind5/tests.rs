use super::*;

fn parameters(count: u32) -> [u32; 6] {
    [
        0,
        2.0_f32.to_bits(),
        7.0_f32.to_bits(),
        95.25_f32.to_bits(),
        1000.0_f32.to_bits(),
        count,
    ]
}

fn moving(y: f32, heading: f32) -> CmvsNativeParticleKind5 {
    CmvsNativeParticleKind5 {
        resource: b"particle".to_vec(),
        parameter_words: parameters(1),
        clock_ms: 0,
        particles: vec![CmvsNativeParticleKind5Particle {
            initial_position_words: [2.0, 100.0, 7.0].map(f32::to_bits),
            position_words: [1.25, y, 9.75].map(f32::to_bits),
            initial_heading_word: heading.to_bits(),
            heading_word: heading.to_bits(),
            travel_time_word: 1000.0_f32.to_bits(),
            atlas_variant: 2,
            animation_frame: 0,
            animation_elapsed_ms: 0,
        }],
    }
}

#[test]
fn births_match_eight_draw_crt_sse_instruction_reference() {
    let mut rng = 1;
    let value =
        CmvsNativeParticleKind5::new(b"particle".to_vec(), parameters(1), &mut rng).unwrap();
    //CRT:41,18467,6334,26500,19169,15724,11478,29358.
    let particle = &value.particles[0];
    assert_eq!(rng, 0x72ae7869);
    assert_eq!(
        particle.initial_position_words,
        [0xc267663f, 0x42c80000, 0x417742af]
    );
    assert_eq!(
        particle.position_words,
        [0xc20b3333, 0x426a0000, 0x41b95c29]
    );
    assert_eq!(particle.heading_word, 0x40299980);
    assert_eq!(particle.initial_heading_word, 0x40299980);
    assert_eq!(particle.travel_time_word, 0x447283bb);
    assert_eq!(particle.atlas_variant, 0);
    assert_eq!(particle.animation_frame, 0);
    assert_eq!(particle.animation_elapsed_ms, 0);
}

#[test]
fn draw_queue_capacity_is_not_a_particle_allocation_limit() {
    let value = CmvsNativeParticleKind5::new(Vec::new(), parameters(8193), &mut 1).unwrap();
    assert_eq!(value.particle_capacity(), 8193);
    assert!(value.draw_basic(8192).is_some());
}

#[test]
fn spread_uses_bits_twenty_four_through_twenty_seven() {
    let expected = [
        0x41b95c29, 0x41990a3d, 0x417147ae, 0x4130a3d7, 0x411051ec, 0x40f9999a,
    ];
    for (nibble, word) in expected.into_iter().enumerate() {
        let mut config = parameters(1);
        config[0] = ((nibble as u32) << 24) | 0x40000000;
        let value = CmvsNativeParticleKind5::new(Vec::new(), config, &mut 1).unwrap();
        assert_eq!(value.particles[0].position_words[2], word);
        assert_eq!(
            value.draw_basic(0).unwrap().projection_depth_word,
            60.0_f32.to_bits()
        );
        assert_eq!(value.particles[0].initial_position_words[2], 0x417742af);
    }
}

#[test]
fn motion_uses_heading_and_hundredth_truncation_not_a_downward_only_model() {
    let mut horizontal = moving(5.0, 0.0);
    let mut vertical = moving(5.0, 90.0);
    let mut rng = 7;
    horizontal.advance(127, 0, &mut rng).unwrap();
    vertical.advance(127, 0, &mut rng).unwrap();
    assert_eq!(
        horizontal.particles[0].position_words,
        [2.52, 5.0, 9.75].map(f32::to_bits)
    );
    assert_eq!(
        vertical.particles[0].position_words,
        [1.25, 6.27, 9.75].map(f32::to_bits)
    );
    assert_eq!(rng, 7);
}

#[test]
fn zero_y_is_not_a_respawn_and_negative_y_draws_only_x_then_z() {
    let mut value = moving(1.0, 270.0);
    let mut rng = 1;
    let original = value.particles[0].clone();
    value.advance(100, 0, &mut rng).unwrap();
    assert_eq!(value.particles[0].position_words[1], 0.0_f32.to_bits());
    assert_eq!(rng, 1);
    value.advance(100, 0, &mut rng).unwrap();
    assert_eq!(rng, 0xc823f683);
    assert_eq!(
        value.particles[0].position_words,
        [0xc2675c29, 0x42c80000, 0x419228f6]
    );
    assert_eq!(value.particles[0].heading_word, original.heading_word);
    assert_eq!(
        value.particles[0].travel_time_word,
        original.travel_time_word
    );
    assert_eq!(value.particles[0].atlas_variant, original.atlas_variant);
    assert_eq!(
        value.particles[0].initial_position_words,
        original.initial_position_words
    );
}

#[test]
fn motion_and_sprite_clocks_have_independent_caps() {
    let mut value = moving(8.0, 270.0);
    value.advance(u32::MAX, 0, &mut 1).unwrap();
    assert_eq!(value.particles[0].position_words[1], 3.0_f32.to_bits());
    assert_eq!(value.particles[0].animation_frame, 0);
    assert_eq!(value.clock_ms, u32::MAX);
    value.advance(0, u32::MAX, &mut 1).unwrap();
    assert_eq!(value.particles[0].position_words[1], 3.0_f32.to_bits());
    assert_eq!(value.particles[0].animation_frame, 2);
    assert_eq!(value.particles[0].animation_elapsed_ms, 0);
}

#[test]
fn sprite_atlas_visits_all_ten_cells_and_preserves_residual_time() {
    let mut value = moving(5.0, 0.0);
    let cells = [
        (80, 0),
        (80, 20),
        (80, 40),
        (80, 60),
        (80, 80),
        (80, 100),
        (100, 0),
        (100, 20),
        (100, 40),
        (100, 60),
    ];
    for (x, y) in cells {
        let basic = value.draw_basic(0).unwrap();
        assert_eq!(basic.rect_words, Some([x, y, 20, 20]));
        value.advance(0, 100, &mut 1).unwrap();
    }
    assert_eq!(value.particles[0].animation_frame, 0);
    value.advance(0, 99, &mut 1).unwrap();
    assert_eq!(value.particles[0].animation_frame, 0);
    assert_eq!(value.particles[0].animation_elapsed_ms, 99);
    value.advance(0, 2, &mut 1).unwrap();
    assert_eq!(value.particles[0].animation_frame, 1);
    assert_eq!(value.particles[0].animation_elapsed_ms, 1);
}

#[test]
fn surface_matches_native_basic_defaults_without_allocating_resource_copies() {
    let mut value = moving(5.0, 270.0);
    let expected_depths = [
        (0, 12.0_f32),
        (4, 60.0),
        (5, 50.0),
        (6, 40.0),
        (7, 30.0),
        (8, 20.0),
        (15, 12.0),
    ];
    for (flags, depth) in expected_depths {
        value.parameter_words[0] = flags << 28;
        let basic = value.draw_basic(0).unwrap();
        assert_eq!(basic.projection_depth_word, depth.to_bits());
        assert_eq!(basic.projection_height_word, 0);
        assert_eq!(basic.basic_kind, 2);
        assert_eq!(basic.blend_mode, Some(1));
        assert_eq!(basic.position_words, Some([10, 10]));
        assert_eq!(basic.alpha, Some(255));
        assert_eq!(basic.scale_words, Some([1.0_f32.to_bits(); 2]));
        assert_eq!(basic.rotation_hundredths, 0);
        assert_eq!(basic.resource_frame, Some(0));
        assert!(basic.surface_initialized);
        assert!(basic.resource.is_none());
        assert_eq!(basic.float_position_words, value.plane_position(0).unwrap());
    }
}

#[test]
fn native_wire_is_sixty_bytes_of_configuration_not_live_particle_phase() {
    let mut value = moving(5.0, 270.0);
    let before = value.capture().unwrap();
    value.advance(123, 135, &mut 1).unwrap();
    assert_eq!(value.capture().unwrap(), before);
    let bytes = before.encode().unwrap();
    assert_eq!(bytes.len(), 60);
    assert_eq!(&bytes[..4], &[1, 0, 5, 0]);
    assert_eq!(
        &bytes[4..28],
        &[
            0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 224, 64, 0, 128, 190, 66, 0, 0, 122, 68, 1, 0, 0, 0
        ]
    );
    assert_eq!(&bytes[28..37], b"particle\0");
    let mut tail = bytes;
    tail[59] = 0xa5;
    let saved = SavedNativeParticleKind5::decode(&tail).unwrap();
    assert_eq!(saved.encode().unwrap(), tail);
}

#[test]
fn native_cold_rebuild_has_fresh_births_and_restarts_sprite_phase() {
    let mut value = moving(5.0, 270.0);
    value.advance(123, 135, &mut 1).unwrap();
    let saved = value.capture().unwrap();
    let mut rng = 1;
    let rebuilt = saved.rebuild(&mut rng).unwrap();
    assert_eq!(
        rebuilt.particles[0].position_words,
        [0xc20b3333, 0x426a0000, 0x41b95c29]
    );
    assert_eq!(rebuilt.particles[0].animation_frame, 0);
    assert_eq!(rebuilt.particles[0].animation_elapsed_ms, 0);
    assert_eq!(rng, 0x72ae7869);
    assert_ne!(rebuilt.particles, value.particles);
}

#[test]
fn private_json_retains_raw_float_payloads_and_continuable_sprite_phase() {
    let mut value = moving(5.0, 270.0);
    value.particles[0].initial_position_words[0] = 0x7fc01234;
    value.particles[0].initial_heading_word = 0x80000000;
    value.advance(123, 135, &mut 7).unwrap();
    let bytes = serde_json::to_vec(&value).unwrap();
    let mut private: CmvsNativeParticleKind5 = serde_json::from_slice(&bytes).unwrap();
    private.validate().unwrap();
    assert_eq!(private, value);
    private.advance(77, 65, &mut 7).unwrap();
    value.advance(77, 65, &mut 7).unwrap();
    assert_eq!(private, value);
    assert_eq!(private.particles[0].animation_frame, 2);
    assert_eq!(private.particles[0].animation_elapsed_ms, 0);
    assert_eq!(private.particles[0].initial_position_words[0], 0x7fc01234);
    assert_eq!(private.particles[0].initial_heading_word, 0x80000000);
}

#[test]
fn codec_rejects_truncation_wrong_headers_and_unterminated_fields() {
    let bytes = moving(5.0, 270.0).capture().unwrap().encode().unwrap();
    for length in 0..60 {
        assert!(SavedNativeParticleKind5::decode(&bytes[..length]).is_err());
    }
    let mut invalid = bytes.clone();
    invalid.push(0);
    assert!(SavedNativeParticleKind5::decode(&invalid).is_err());
    for (offset, byte) in [(0, 2), (1, 1), (2, 4), (3, 1)] {
        let mut invalid = bytes.clone();
        invalid[offset] = byte;
        assert!(SavedNativeParticleKind5::decode(&invalid).is_err());
    }
    let mut invalid = bytes;
    invalid[28..60].fill(b'x');
    assert!(SavedNativeParticleKind5::decode(&invalid).is_err());
}
