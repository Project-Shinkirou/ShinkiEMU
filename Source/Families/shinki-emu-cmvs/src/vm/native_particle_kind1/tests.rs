use super::*;

fn parameters(speed: f32, interval: u32) -> [u32; 6] {
    [0, 0, 0, 0.0_f32.to_bits(), speed.to_bits(), interval]
}
fn worker(speed: f32, interval: u32) -> CmvsNativeParticleKind1 {
    CmvsNativeParticleKind1::new(b"rain.pb3".to_vec(), parameters(speed, interval), &mut 1).unwrap()
}

#[test]
fn strict_spawn_boundary_and_two_shared_crt_draws() {
    let mut rng = 1;
    let mut value =
        CmvsNativeParticleKind1::new(b"rain.pb3".to_vec(), parameters(2.0, 100), &mut rng).unwrap();
    assert_eq!(rng, 1);
    assert!(value.plane_position(0).is_none());
    value.advance(100, &mut rng).unwrap();
    assert_eq!(value.draw_count, 0);
    assert_eq!(rng, 1);
    value.advance(1, &mut rng).unwrap();
    assert_eq!(rng, 3_357_800_067);
    let first = &value.particles[0];
    assert_eq!(first.object_id, 0);
    assert_eq!(first.last_tick_ms, 101);
    assert_eq!(
        first.position_words,
        [
            ((41.0_f32 / 32767.0) * 10.0 - 5.0).to_bits(),
            (6.0_f32 - 0.002).to_bits(),
            ((18467.0_f32 / 32767.0) * 10.0).to_bits(),
        ]
    );
    value.advance(100, &mut rng).unwrap();
    assert_eq!(rng, 3_884_216_597);
    assert_eq!(
        value.particles[1].position_words[0],
        ((6334.0_f32 / 32767.0) * 10.0 - 5.0).to_bits()
    );
    assert_eq!(
        value.particles[1].position_words[2],
        ((26500.0_f32 / 32767.0) * 10.0).to_bits()
    );
    assert_eq!(value.draw_indices(), &[0, 1]);
}

#[test]
fn zero_angle_keeps_x_exact_and_applies_each_frame_elapsed() {
    let mut value = worker(10.0, 100);
    let mut rng = 1;
    value.advance(101, &mut rng).unwrap();
    let x = value.particles[0].position_words[0];
    let old_y = f32::from_bits(value.particles[0].position_words[1]);
    value.advance(23, &mut rng).unwrap();
    assert_eq!(value.particles[0].position_words[0], x);
    assert_eq!(
        value.particles[0].position_words[1],
        (old_y - 0.23_f32).to_bits()
    );
    assert_eq!(value.particles[0].last_tick_ms, 124);
}

#[test]
fn negative_right_angle_uses_double_trig_after_single_degree_conversion() {
    let mut params = parameters(10.0, 100);
    params[3] = (-90.0_f32).to_bits();
    let mut rng = 1;
    let mut value = CmvsNativeParticleKind1::new(Vec::new(), params, &mut rng).unwrap();
    value.advance(201, &mut rng).unwrap();
    let initial_x = (41.0_f32 / 32767.0) * 10.0 - 5.0;
    assert_eq!(
        value.particles[0].position_words[0],
        (initial_x + 1.01_f32).to_bits()
    );
    assert_eq!(value.particles[0].position_words[1], 6.0_f32.to_bits());
    assert_eq!(value.draw_basic(0).unwrap().rotation_hundredths, 27000);
    value.advance(1000, &mut rng).unwrap();
    assert!(value.particles[0].active);
    assert_eq!(value.particles[0].position_words[1], 6.0_f32.to_bits());
}

#[test]
fn non_cardinal_motion_retains_native_single_precision_radian_rounding() {
    let mut params = parameters(7.0, 100);
    params[3] = 17.25_f32.to_bits();
    let mut rng = 1;
    let mut value = CmvsNativeParticleKind1::new(Vec::new(), params, &mut rng).unwrap();
    value.advance(121, &mut rng).unwrap();
    let radians = ((107.25_f32 / 180.0) * std::f32::consts::PI) as f64;
    let distance = (21.0_f32 * 7.0) / 1000.0;
    let initial_x = (41.0_f32 / 32767.0) * 10.0 - 5.0;
    assert_eq!(
        value.particles[0].position_words,
        [
            (initial_x + (radians.cos() * f64::from(distance)) as f32).to_bits(),
            (6.0_f32 - (radians.sin() * f64::from(distance)) as f32).to_bits(),
            ((18467.0_f32 / 32767.0) * 10.0).to_bits(),
        ]
    );
    assert_eq!(value.draw_basic(0).unwrap().rotation_hundredths, 1700);
}

#[test]
fn death_reuses_first_slot_but_preserves_child_creation_order() {
    let mut value = worker(100.0, 10);
    let mut rng = 1;
    value.advance(31, &mut rng).unwrap();
    assert_eq!(value.draw_indices(), &[0, 1, 2]);
    value.advance(40, &mut rng).unwrap();
    assert!(!value.particles[0].active);
    assert_eq!(value.draw_indices(), &[1, 2, 3, 4, 5, 6]);
    value.advance(10, &mut rng).unwrap();
    assert_eq!(value.particles[0].object_id, 7);
    assert!(!value.particles[1].active);
    assert_eq!(value.draw_indices(), &[2, 3, 4, 5, 6, 0]);
    assert!(value.draw_basic(1).is_none());
    value.validate().unwrap();
}

#[test]
fn native_capacity_is_1024_and_full_spawn_attempts_do_not_consume_rng() {
    let mut value = worker(0.0, 1);
    let mut rng = 1;
    value.advance(1000, &mut rng).unwrap();
    assert_eq!(value.draw_count, 999);
    assert_eq!(rng, 572_715_807);
    value.advance(50, &mut rng).unwrap();
    assert_eq!(value.draw_count, 1024);
    assert_eq!(value.next_object_id, 1024);
    assert_eq!(rng, 463_726_593);
    let particle_storage = value.particles.as_ptr();
    let order_storage = value.draw_order.as_ptr();
    value.advance(1000, &mut rng).unwrap();
    assert_eq!(rng, 463_726_593);
    assert_eq!(value.particles.as_ptr(), particle_storage);
    assert_eq!(value.draw_order.as_ptr(), order_storage);
    // A full-frame spawn pass precedes all deaths, so it cannot reuse this slot yet.
    value.particles[0].speed_word = 10000.0_f32.to_bits();
    value.advance(1, &mut rng).unwrap();
    assert_eq!(rng, 463_726_593);
    assert!(!value.particles[0].active);
    value.advance(1, &mut rng).unwrap();
    assert!(value.particles[0].active);
    assert_eq!(value.particles[0].object_id, 1024);
    assert_ne!(rng, 463_726_593);
}

#[test]
fn spawn_clock_cap_does_not_cap_existing_particle_motion() {
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
        (previous_y - 5.0).to_bits()
    );
    assert_eq!(value.particles[1].last_tick_ms, 5101);
    assert_eq!(
        value.particles[1].position_words[1],
        (6.0_f32 - 0.9).to_bits()
    );
}

#[test]
fn clock_and_object_counter_wrap_without_changing_native_age() {
    let mut value = worker(2.0, 100);
    value.clock_ms = u32::MAX - 50;
    value.next_object_id = u32::MAX;
    let mut rng = 1;
    value.advance(101, &mut rng).unwrap();
    assert_eq!(value.clock_ms, 50);
    assert_eq!(value.particles[0].object_id, u32::MAX);
    assert_eq!(value.next_object_id, 0);
    assert_eq!(value.particles[0].last_tick_ms, 50);
    assert_eq!(
        value.particles[0].position_words[1],
        (6.0_f32 - 0.002).to_bits()
    );
    value.advance(100, &mut rng).unwrap();
    assert_eq!(value.particles[1].object_id, 0);
}

#[test]
fn cbasic_projection_crop_pivot_and_flags_are_independent_of_scale() {
    for flags in 0..4 {
        let mut params = parameters(0.0, 1);
        params[0] = 0xffff_fffc | flags;
        params[1] = 0x7fc0_1234;
        params[2] = 0xff80_0000;
        params[3] = (-721.75_f32).to_bits();
        let mut rng = 1;
        let mut value = CmvsNativeParticleKind1::new(Vec::new(), params, &mut rng).unwrap();
        value.advance(2, &mut rng).unwrap();
        value.validate().unwrap();
        assert_eq!(value.particles[0].unused_words, [0x7fc0_1234, 0xff80_0000]);
        let basic = value.draw_basic(0).unwrap();
        assert_eq!(basic.basic_kind, 2);
        assert_eq!(basic.rect_words, Some([0, 0, 6, 112]));
        assert_eq!(basic.position_words, Some([3, 112]));
        assert_eq!(basic.float_position_words, value.plane_position(0).unwrap());
        assert_eq!(
            basic.projection_depth_word,
            (3.0_f32 + flags as f32).to_bits()
        );
        assert_eq!(basic.projection_height_word, 3.0_f32.to_bits());
        assert_eq!(basic.scale_words, Some([1.0_f32.to_bits(); 2]));
        assert_eq!(basic.rotation_hundredths, 100);
        assert_eq!(basic.alpha, Some(255));
        assert_eq!(basic.blend_mode, None);
        assert_eq!(basic.reference_size, None);
        assert_eq!(basic.resource, None);
    }
}

#[test]
fn native_config_rebuild_resets_phase_without_random_draws_but_private_phase_continues() {
    let mut value = worker(3.0, 70);
    let mut rng = 1;
    value.advance(311, &mut rng).unwrap();
    let saved = value.capture().unwrap();
    let bytes = saved.encode().unwrap();
    assert_eq!(bytes.len(), 60);
    assert_eq!(&bytes[..4], &[1, 0, 1, 0]);
    assert_eq!(&bytes[28..37], b"rain.pb3\0");
    let mut cold_rng = rng;
    let rebuilt = SavedNativeParticleKind1::decode(&bytes)
        .unwrap()
        .rebuild(&mut cold_rng)
        .unwrap();
    assert_eq!(cold_rng, rng);
    assert_eq!(rebuilt.clock_ms, 0);
    assert_eq!(rebuilt.accumulator_ms, 0);
    assert_eq!(rebuilt.draw_count, 0);
    assert_eq!(rebuilt.capture().unwrap(), saved);
    let encoded = serde_json::to_vec(&value).unwrap();
    let mut private: CmvsNativeParticleKind1 = serde_json::from_slice(&encoded).unwrap();
    private.validate().unwrap();
    assert_eq!(private, value);
    let mut private_rng = rng;
    value.advance(127, &mut rng).unwrap();
    private.advance(127, &mut private_rng).unwrap();
    assert_eq!(private_rng, rng);
    assert_eq!(private, value);
}

#[test]
fn config_preserves_unused_resource_tail_and_rejects_wrong_layouts() {
    let saved = worker(0.0, 100).capture().unwrap();
    let mut bytes = saved.encode().unwrap();
    bytes[59] = 0xa5;
    assert_eq!(
        SavedNativeParticleKind1::decode(&bytes)
            .unwrap()
            .encode()
            .unwrap(),
        bytes
    );
    for length in [0, 3, 59, 61, 2048] {
        assert!(SavedNativeParticleKind1::decode(&vec![0; length]).is_err());
    }
    bytes[2] = 2;
    assert!(SavedNativeParticleKind1::decode(&bytes).is_err());
    bytes[2] = 1;
    bytes[28..].fill(b'x');
    assert!(SavedNativeParticleKind1::decode(&bytes).is_err());
}

#[test]
fn invalid_execution_parameters_and_private_layout_are_bounded_errors() {
    for params in [parameters(0.0, 0), parameters(f32::INFINITY, 1)] {
        assert!(CmvsNativeParticleKind1::new(Vec::new(), params, &mut 1).is_err());
    }
    let mut params = parameters(0.0, 1);
    params[3] = f32::NAN.to_bits();
    assert!(CmvsNativeParticleKind1::new(Vec::new(), params, &mut 1).is_err());
    for resource in [vec![b'x'; 32], b"bad\0name".to_vec()] {
        assert!(CmvsNativeParticleKind1::new(resource, parameters(0.0, 1), &mut 1).is_err());
    }
    let mut value = worker(0.0, 1);
    value.particles.pop();
    assert!(value.advance(1, &mut 1).is_err());
    let mut value = worker(0.0, 1);
    value.advance(3, &mut 1).unwrap();
    value.draw_order[1] = value.draw_order[0];
    assert!(value.validate().is_err());
    let mut value = worker(f32::MAX, 100);
    value.advance(101, &mut 1).unwrap();
    assert!(value.advance(1000, &mut 1).is_err());
}
