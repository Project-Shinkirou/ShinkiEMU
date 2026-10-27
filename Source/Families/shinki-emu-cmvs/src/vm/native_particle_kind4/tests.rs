use super::*;

fn parameters(speed: f32, interval: u32) -> [u32; 6] {
    [0, 0, 0, 0.0_f32.to_bits(), speed.to_bits(), interval]
}

fn worker(speed: f32, interval: u32) -> CmvsNativeParticleKind4 {
    CmvsNativeParticleKind4::new(b"rain.pb3".to_vec(), parameters(speed, interval), &mut 1).unwrap()
}

#[test]
fn strict_spawn_boundary_uses_native_crt_x_z_order_and_quarter_second_speed() {
    let mut rng = 1;
    let mut value =
        CmvsNativeParticleKind4::new(Vec::new(), parameters(100.0, 100), &mut rng).unwrap();
    assert_eq!(rng, 1);
    value.advance(100, &mut rng).unwrap();
    assert!(value.draw_indices().is_empty());
    assert_eq!(rng, 1);
    value.advance(1, &mut rng).unwrap();
    assert_eq!(rng, 3_357_800_067);
    assert_eq!(
        value.particles[0].position_words,
        [
            ((41.0_f32 / 32767.0) * 100.0 - 50.0).to_bits(),
            (150.0_f32 - 0.4).to_bits(),
            ((18467.0_f32 / 32767.0) * 100.0).to_bits(),
        ]
    );
    assert_eq!(value.particles[0].last_tick_ms, 101);
    value.advance(100, &mut rng).unwrap();
    assert_eq!(rng, 3_884_216_597);
    assert_eq!(
        value.particles[1].position_words[0],
        ((6334.0_f32 / 32767.0) * 100.0 - 50.0).to_bits()
    );
    assert_eq!(
        value.particles[1].position_words[2],
        ((26500.0_f32 / 32767.0) * 100.0).to_bits()
    );
    assert_eq!(value.draw_indices(), &[0, 1]);
}

#[test]
fn each_x_distribution_has_its_proven_offset_and_one_draw() {
    for (flag, offset) in [
        (0x00, -50.0_f32),
        (0x10, 0.0),
        (0x20, -100.0),
        (0x30, 50.0),
        (0x40, -150.0),
        (0x50, -15.0),
        (0x60, -85.0),
        (0x70, -25.0),
        (0x80, -75.0),
        (0xa0, -78.0),
    ] {
        let mut value = worker(0.0, 100);
        value.parameter_words[0] = flag | 0x700; // Z copies X without another draw.
        let mut rng = 1;
        value.advance(101, &mut rng).unwrap();
        assert_eq!(rng, 2_745_024);
        let expected = ((41.0_f32 / 32767.0) * 100.0 + offset).to_bits();
        assert_eq!(
            value.particles[0].position_words[0], expected,
            "flag {flag:x}"
        );
        assert_eq!(value.particles[0].position_words[2], expected);
    }
}

#[test]
fn each_z_distribution_uses_its_native_width_and_offset() {
    for (flag, width, offset) in [
        (0x000, 100.0_f32, 0.0_f32),
        (0x100, 50.0, 0.0),
        (0x200, 50.0, 10.0),
        (0x300, 50.0, 20.0),
        (0x400, 50.0, 30.0),
        (0x500, 50.0, 40.0),
        (0x600, 50.0, 50.0),
    ] {
        let mut value = worker(0.0, 100);
        value.parameter_words[0] = flag | 0x90; // X takes raw timestamp; Z gets draw1.
        let mut rng = 1;
        value.advance(101, &mut rng).unwrap();
        assert_eq!(rng, 2_745_024);
        assert_eq!(value.particles[0].position_words[0], 100);
        assert_eq!(
            value.particles[0].position_words[2],
            ((41.0_f32 / 32767.0) * width + offset).to_bits()
        );
    }
}

#[test]
fn double_spawn_has_independent_slots_and_random_draws_at_the_same_birth_time() {
    let mut value = worker(100.0, 100);
    value.parameter_words[0] = 0x1000;
    let mut rng = 1;
    value.advance(101, &mut rng).unwrap();
    assert_eq!(value.draw_indices(), &[0, 1]);
    assert_eq!(rng, 3_884_216_597);
    assert_eq!(
        value.particles[0].position_words[1],
        (150.0_f32 - 0.4).to_bits()
    );
    assert_eq!(
        value.particles[0].position_words[1],
        value.particles[1].position_words[1]
    );
    assert_eq!(
        value.particles[1].position_words[0],
        ((6334.0_f32 / 32767.0) * 100.0 - 50.0).to_bits()
    );
    assert_eq!(value.particles[1].object_id, 1);
}

#[test]
fn native_pool_has_2048_records_and_full_attempts_do_not_draw_or_reuse_current_deaths() {
    let mut value = worker(0.0, 1);
    value.parameter_words[0] = 0x1000;
    let mut rng = 1;
    value.advance(1000, &mut rng).unwrap();
    assert_eq!(value.draw_count, 1998);
    value.advance(25, &mut rng).unwrap();
    assert_eq!(value.draw_count, 2048);
    assert_eq!(value.next_object_id, 2048);
    let full_rng = rng;
    let particle_storage = value.particles.as_ptr();
    let order_storage = value.draw_order.as_ptr();
    value.advance(1000, &mut rng).unwrap();
    assert_eq!(rng, full_rng);
    assert_eq!(value.particles.as_ptr(), particle_storage);
    assert_eq!(value.draw_order.as_ptr(), order_storage);
    value.particles[0].speed_word = 50_000.0_f32.to_bits();
    value.advance(1, &mut rng).unwrap();
    assert_eq!(rng, full_rng);
    assert!(!value.particles[0].active);
    assert_eq!(value.draw_count, 2047);
    value.advance(1, &mut rng).unwrap();
    assert_eq!(value.particles[0].object_id, 2048);
    assert_eq!(value.draw_count, 2048);
    assert_eq!(value.draw_indices().last(), Some(&0));
    assert_ne!(rng, full_rng);
    value.validate().unwrap();
}

#[test]
fn death_at_minus_fifty_and_recycling_preserve_child_insertion_order() {
    let mut value = worker(1000.0, 10);
    let mut rng = 1;
    value.advance(31, &mut rng).unwrap();
    assert_eq!(value.draw_indices(), &[0, 1, 2]);
    value.advance(29, &mut rng).unwrap(); // First birth at10 is exactly50ms old.
    assert_eq!(value.particles[0].position_words[1], (-50.0_f32).to_bits());
    assert!(!value.particles[0].active);
    assert_eq!(value.particles[0].last_tick_ms, 31); // Death does not update the tick.
    assert_eq!(value.draw_indices(), &[1, 2, 3, 4]);
    value.advance(11, &mut rng).unwrap();
    assert_eq!(value.particles[0].object_id, 5);
    assert_eq!(value.particles[5].object_id, 6);
    assert!(!value.particles[1].active);
    assert_eq!(value.draw_indices(), &[2, 3, 4, 0, 5]);
    assert!(value.draw_basic(1).is_none());
    value.validate().unwrap();
}

#[test]
fn spawn_debt_cap_does_not_cap_existing_motion_or_replay_the_entire_gap() {
    let mut value = worker(1.0, 100);
    let mut rng = 1;
    value.advance(101, &mut rng).unwrap();
    let previous_y = f32::from_bits(value.particles[0].position_words[1]);
    value.advance(5000, &mut rng).unwrap();
    assert_eq!(value.clock_ms, 5101);
    assert_eq!(value.accumulator_ms, 100);
    assert_eq!(value.draw_count, 10);
    assert_eq!(
        value.particles[0].position_words[1],
        (previous_y - 20.0).to_bits()
    );
    assert_eq!(
        value.particles[1].position_words[1],
        (150.0_f32 - 3.6).to_bits()
    );
}

#[test]
fn non_cardinal_trajectory_keeps_f32_radian_conversion_and_fractional_angle() {
    let mut value = worker(7.0, 100);
    value.parameter_words[3] = 17.25_f32.to_bits();
    let mut rng = 1;
    value.advance(121, &mut rng).unwrap();
    let radians = ((107.25_f32 / 180.0_f32) * std::f32::consts::PI) as f64;
    let distance = (21.0_f32 * 7.0_f32) / 250.0_f32;
    assert_eq!(
        value.particles[0].position_words,
        [
            (((41.0_f32 / 32767.0) * 100.0 - 50.0) + (radians.cos() * f64::from(distance)) as f32)
                .to_bits(),
            (150.0_f32 - (radians.sin() * f64::from(distance)) as f32).to_bits(),
            ((18467.0_f32 / 32767.0) * 100.0).to_bits(),
        ]
    );
    assert_eq!(value.draw_basic(0).unwrap().rotation_hundredths, 1700);
    assert_eq!(value.particles[0].angle_word, 17.25_f32.to_bits());
    value.parameter_words[3] = (-90.0_f32).to_bits();
    value.advance(100, &mut rng).unwrap();
    assert_eq!(value.particles[1].position_words[1], 150.0_f32.to_bits());
    assert_eq!(value.draw_basic(1).unwrap().rotation_hundredths, 27000);
}

#[test]
fn cbasic_projection_is_not_scale_and_birth_config_is_not_live_config() {
    for flags in 0..4 {
        let mut value = worker(0.0, 10);
        value.parameter_words[0] = flags;
        value.parameter_words[1] = 0x7fc0_1234;
        value.parameter_words[2] = 0xff80_0000;
        let mut rng = 1;
        value.advance(11, &mut rng).unwrap();
        let basic = value.draw_basic(0).unwrap();
        assert_eq!(
            basic.projection_depth_word,
            (20.5_f32 + 10.0 * flags as f32).to_bits()
        );
        assert_eq!(basic.projection_height_word, 20.0_f32.to_bits());
        assert_eq!(basic.scale_words, Some([1.0_f32.to_bits(); 2]));
        assert_eq!(basic.rect_words, Some([0, 0, 6, 112]));
        assert_eq!(basic.position_words, Some([3, 112]));
        assert_eq!(basic.alpha, Some(255));
        assert_eq!(value.particles[0].unused_words, [0x7fc0_1234, 0xff80_0000]);
        value.parameter_words[0] = (flags + 1) & 3;
        value.parameter_words[3] = 90.5_f32.to_bits();
        value.parameter_words[4] = 1.0_f32.to_bits();
        value.advance(10, &mut rng).unwrap();
        assert_eq!(
            value.draw_basic(0).unwrap().projection_depth_word,
            basic.projection_depth_word
        );
        assert_eq!(value.particles[0].angle_word, 0.0_f32.to_bits());
        assert_eq!(value.particles[0].speed_word, 0.0_f32.to_bits());
        assert_eq!(value.particles[1].angle_word, 90.5_f32.to_bits());
    }
}

#[test]
fn native_wire_cold_restore_resets_pool_but_private_snapshot_continues_every_phase_word() {
    let mut value = worker(75.0, 70);
    value.clock_ms = 0x1234_0000;
    let mut rng = 1;
    value.advance(311, &mut rng).unwrap();
    let saved = value.capture().unwrap();
    let bytes = saved.encode().unwrap();
    assert_eq!(bytes.len(), 60);
    assert_eq!(&bytes[..4], &[1, 0, 4, 0]);
    for (index, word) in value.parameter_words.iter().enumerate() {
        assert_eq!(&bytes[4 + index * 4..8 + index * 4], &word.to_le_bytes());
    }
    assert_eq!(&bytes[28..37], b"rain.pb3\0");
    let mut cold_rng = rng;
    let cold = SavedNativeParticleKind4::decode(&bytes)
        .unwrap()
        .rebuild(&mut cold_rng)
        .unwrap();
    assert_eq!(cold_rng, rng);
    assert!(cold.draw_indices().is_empty());
    assert_eq!(cold.clock_ms, 0);
    assert_eq!(cold.accumulator_ms, 0);
    assert_eq!(cold.next_object_id, 0);
    assert_eq!(cold.capture().unwrap(), saved);
    let encoded = serde_json::to_vec(&value).unwrap();
    let mut private: CmvsNativeParticleKind4 = serde_json::from_slice(&encoded).unwrap();
    private.validate().unwrap();
    assert_eq!(private, value);
    let mut private_rng = rng;
    value.advance(127, &mut rng).unwrap();
    private.advance(127, &mut private_rng).unwrap();
    assert_eq!(private_rng, rng);
    assert_eq!(private, value);
    assert_ne!(private, cold);
}

#[test]
fn native_wire_preserves_unused_name_tail_and_raw_unused_parameter_words() {
    let mut value = worker(0.0, 100);
    value.parameter_words[1] = 0x7fc0_1234;
    value.parameter_words[2] = 0xffff_ffff;
    let mut bytes = value.capture().unwrap().encode().unwrap();
    bytes[59] = 0xa5;
    let saved = SavedNativeParticleKind4::decode(&bytes).unwrap();
    assert_eq!(saved.encode().unwrap(), bytes);
    assert_eq!(saved.parameter_words[1..3], [0x7fc0_1234, 0xffff_ffff]);
    assert_eq!(
        saved.rebuild(&mut 1).unwrap().resource.as_slice(),
        b"rain.pb3"
    );
    for length in [0, 3, 59, 61, 2048] {
        assert!(SavedNativeParticleKind4::decode(&vec![0; length]).is_err());
    }
    bytes[2] = 1;
    assert!(SavedNativeParticleKind4::decode(&bytes).is_err());
    bytes[2] = 4;
    bytes[28..].fill(b'x');
    assert!(SavedNativeParticleKind4::decode(&bytes).is_err());
}

#[test]
fn invalid_layout_and_nonterminating_interval_are_rejected() {
    assert!(CmvsNativeParticleKind4::new(Vec::new(), parameters(1.0, 0), &mut 1).is_err());
    for resource in [vec![b'x'; 32], b"bad\0name".to_vec()] {
        assert!(CmvsNativeParticleKind4::new(resource, parameters(0.0, 1), &mut 1).is_err());
    }
    let mut value = worker(0.0, 1);
    value.advance(3, &mut 1).unwrap();
    value.draw_order[1] = value.draw_order[0];
    assert!(value.validate().is_err());
    let mut value = worker(0.0, 1);
    value.particles.pop();
    assert!(value.advance(1, &mut 1).is_err());
}
