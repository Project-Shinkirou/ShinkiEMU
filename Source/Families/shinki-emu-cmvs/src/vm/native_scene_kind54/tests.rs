use super::*;

fn scene() -> CmvsNativeSceneKind54 {
    CmvsNativeSceneKind54::new(&mut 1).unwrap()
}
fn main_sound(volume: u32) -> Option<NativeSceneSound> {
    Some(NativeSceneSound::PlayStaticMain {
        slot: 16,
        volume,
        looping: false,
    })
}
fn active_particle(owner: &mut CmvsNativeSceneKind54, index: usize, clock: u32) {
    owner.particles[index].delay = 0;
    owner.particles[index].pulse_clock = clock;
}

#[test]
fn constructor_keeps_all_geometry_flags_and_root_major_provider_order() {
    let owner = scene();
    assert_eq!(owner.mesh_count(), 517);
    assert_eq!(owner.resource_count(), 6);
    assert_eq!(owner.mesh(517), None);
    assert_eq!(owner.resource(6), None);
    for (index, height) in [0.0, 0.1, 0.3, 0.4, 0.5].into_iter().enumerate() {
        let mesh = owner.mesh(index).unwrap();
        assert_eq!(mesh.position_words, [0xbe23_d70a, 0x3f78_51ec, 0x4000_0000]);
        assert_eq!(mesh.angle_words, words([-90.0, 0.0, 0.0]));
        assert_eq!(mesh.scale_words, words([1.0; 3]));
        assert_eq!(
            mesh.vertices.each_ref().map(|v| v.position_words),
            [
                [-1.0, height, 1.0],
                [1.0, height, 1.0],
                [-1.0, height, -1.0],
                [1.0, height, -1.0]
            ]
            .map(words)
        );
        assert_eq!(mesh.kind, 3);
        assert_eq!(mesh.blend, 0);
        assert!(!mesh.billboard);
        assert!(mesh.force_blend && mesh.late_pass);
        assert_eq!(mesh.alpha, 0);
        assert_eq!(owner.mesh_resource_index(index), index);
    }
    for index in 5..517 {
        let mesh = owner.mesh(index).unwrap();
        assert_eq!((mesh.kind, mesh.blend, mesh.alpha), (2, 1, 0));
        assert!(mesh.billboard && mesh.force_blend && mesh.late_pass);
        assert_eq!(mesh.angle_words, [0; 3]);
        assert_eq!(mesh.scale_words, words([1.0; 3]));
        assert_eq!(owner.mesh_resource_index(index), 5);
        let extent = 0.1_f32 - 0.05_f32;
        assert_eq!(
            mesh.vertices.each_ref().map(|v| v.position_words),
            [
                [-0.05, extent, 0.0],
                [extent, extent, 0.0],
                [-0.05, -0.05, 0.0],
                [extent, -0.05, 0.0]
            ]
            .map(words)
        );
    }
    for index in 0..517 {
        let mesh = owner.mesh(index).unwrap();
        assert_eq!(
            mesh.vertices.each_ref().map(|v| v.uv_words),
            [[0, 0], [0x3f80_0000, 0], [0, 0x3f80_0000], [0x3f80_0000; 2]]
        );
        assert!(
            mesh.vertices
                .iter()
                .all(|v| v.rgba_words == words([1.0, 1.0, 1.0, 0.0]))
        );
    }
}

#[test]
fn constructor_consumes_2560_draws_in_native_xyz_period_delay_order() {
    let mut rng = 1;
    let owner = CmvsNativeSceneKind54::new(&mut rng).unwrap();
    assert_eq!(rng, 0xa1f4_e201);
    assert_eq!(
        owner.particles[0].mesh.position_words,
        [0xbfff_5bff, 0xc044_4908, 0x4008_2390]
    );
    assert_eq!(owner.particles[0].rise_time_word, 0x4606_a5ec);
    assert_eq!(owner.particles[0].delay, 169);
    assert_eq!(
        owner.particles[511].mesh.position_words,
        [0xbfb1_e764, 0xc034_1ce8, 0x3fcd_629b]
    );
    assert_eq!(owner.particles[511].rise_time_word, 0x45de_0668);
    assert_eq!(owner.particles[511].delay, 692);
    assert!(owner.particles.iter().all(|p| p.pulse_clock == 0));
}

#[test]
fn initial_camera_changes_only_position_pitch_and_yaw() {
    let frame = scene().initial_frame();
    assert_eq!(frame.position_words, Some(words([0.0, 1.0, 0.0])));
    assert_eq!(frame.pitch_word, Some(0));
    assert_eq!(frame.yaw_word, Some(0));
    assert_eq!(frame.angle_words, None);
    assert_eq!(frame.field_of_view_word, None);
    assert_eq!(frame.audio, [None; 4]);
}

#[test]
fn rotation_uses_common_200ms_cap_and_wrapped_multiply_before_unsigned_division() {
    let mut owner = scene();
    owner.request(10);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.rings[0].angle_words, words([-90.0, 0.0, 0.0]));
    assert_eq!(owner.rings[1].angle_words, words([-90.0, 360.0, 0.0]));
    owner.advance(u32::MAX, 0).unwrap();
    assert_eq!(owner.rotation_clock, 200);
    assert_eq!(owner.growth_clock, 0);
    assert_eq!(owner.rings[0].angle_words[1], 12.0_f32.to_bits());
    assert_eq!(owner.rings[2].angle_words[1], 12.0_f32.to_bits());
    for index in [1, 3, 4] {
        assert_eq!(owner.rings[index].angle_words[1], 348.0_f32.to_bits());
    }
    owner.rotation_clock = u32::MAX - 20;
    owner.advance(50, 0).unwrap();
    assert_eq!(owner.rotation_clock, 29);
    assert_eq!(owner.rings[0].angle_words[1], 1.7_f32.to_bits());
    owner.rotation_clock = 1_193_047;
    let expected = (owner.rotation_clock.wrapping_mul(3600) / 6000 % 3600) as f32 / 10.0;
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.rings[0].angle_words[1], expected.to_bits());
}

#[test]
fn automatic_growth_waits_1500ms_then_uses_strict_staggered_boundaries() {
    for index in 0..5 {
        let start = 1500 + 800 - index as u32 * 200;
        let duration = 500 + index as u32 * 50;
        for (progress, alpha) in [
            (0, 0),
            (1, 256 / duration),
            (duration - 1, 256 * (duration - 1) / duration),
            (duration, 255),
        ] {
            let mut owner = scene();
            owner.growth_clock = start + progress;
            owner.advance(0, 99).unwrap();
            assert_eq!(
                owner.rings[index].alpha, alpha as i32,
                "ring {index}, progress {progress}"
            );
            let scale = if progress == 0 || progress >= duration {
                1.0
            } else {
                progress as f32 / duration as f32
            };
            assert_eq!(owner.rings[index].scale_words, words([scale; 3]));
            assert_eq!(owner.status(), 0);
        }
    }
    let mut owner = scene();
    owner.growth_clock = 1500;
    assert_eq!(owner.advance(0, 123).unwrap().audio, [None; 4]);
    assert_eq!(
        owner.advance(1, u32::MAX).unwrap().audio[0],
        main_sound(u32::MAX)
    );
    assert!(owner.sound_started);
    assert_eq!(owner.advance(200, 999).unwrap().audio, [None; 4]);
    owner.growth_clock = 100_000;
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 0);
    assert!(
        owner
            .rings
            .iter()
            .all(|m| m.alpha == 255 && m.scale_words == words([1.0; 3]))
    );
}

#[test]
fn explicit_growth_has_no_initial_pause_and_retains_existing_clocks_and_sound_gate() {
    let mut owner = scene();
    owner.request(11);
    assert_eq!(owner.status(), u32::MAX);
    let frame = owner.advance(1, 55).unwrap();
    assert_eq!(frame.audio[0], main_sound(55));
    assert_eq!((owner.state, owner.growth_clock), (11, 1));
    owner.growth_clock = 1299;
    assert_eq!(owner.advance(1, 99).unwrap().audio, [None; 4]);
    assert_eq!(owner.status(), 12);
    owner.request(10);
    owner.advance(200, 0).unwrap();
    assert_eq!(owner.growth_clock, 1300);
    owner.request(11);
    assert_eq!(owner.advance(0, 101).unwrap().audio, [None; 4]);
    assert_eq!(owner.status(), 12);
}

#[test]
fn pending_requests_overwrite_and_same_state_request_keeps_status_busy() {
    let mut owner = scene();
    owner.request(11);
    owner.request(99);
    assert_eq!(owner.pending, 11);
    owner.request(10);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 10);
    owner.request(10);
    for _ in 0..3 {
        owner.advance(200, 0).unwrap();
    }
    assert_eq!(owner.state, 10);
    assert_eq!(owner.status(), u32::MAX);
    owner.request(22);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 22);
}

#[test]
fn selector50_immediately_swaps_particle_provider_without_touching_pending_or_clocks() {
    let mut owner = scene();
    owner.request(20);
    let before = owner.clone();
    owner.request(50);
    assert_eq!(owner.resource(5), Some(&b"try008z2.pb3"[..]));
    assert_eq!(owner.pending, 20);
    assert_eq!(owner.status(), u32::MAX);
    assert_eq!(owner.rotation_clock, before.rotation_clock);
    assert_eq!(owner.particles, before.particles);
    assert_eq!(owner.rings, before.rings);
    owner.request(50);
    assert_eq!(
        owner.resource(5),
        Some(CmvsNativeSceneKind54::ALTERNATE_RESOURCE)
    );
}

#[test]
fn rise_is_float32_single_subtraction_wrap_and_runs_while_particles_are_hidden() {
    let mut owner = scene();
    owner.request(10);
    let before = owner.particles[0].mesh.position_words;
    owner.particles[0].rise_time_word = 8000.0_f32.to_bits();
    owner.particles[0].mesh.position_words[1] = 4.875_f32.to_bits();
    owner.advance(200, 0).unwrap();
    assert_eq!(
        owner.particles[0].mesh.position_words,
        [before[0], (-4.875_f32).to_bits(), before[2]]
    );
    assert_eq!(owner.particles[0].mesh.alpha, 0);
    owner.particles[0].mesh.position_words[1] = 25.0_f32.to_bits();
    owner.advance(0, 0).unwrap();
    assert_eq!(
        owner.particles[0].mesh.position_words[1],
        15.0_f32.to_bits()
    );
}

#[test]
fn delay_discards_overshoot_and_equal_delta_does_not_begin_pulse_until_next_callback() {
    for delay in [1, 100, 200] {
        let mut owner = scene();
        owner.request(10);
        owner.particles[0].delay = delay;
        owner.advance(200, 0).unwrap();
        assert_eq!(owner.particles[0].delay, 0);
        assert_eq!(owner.particles[0].pulse_clock, 0);
        assert_eq!(owner.particles[0].mesh.alpha, 0);
        owner.advance(100, 0).unwrap();
        assert_eq!(owner.particles[0].pulse_clock, 100);
        assert_eq!(owner.particles[0].mesh.alpha, 25);
    }
}

#[test]
fn pulse_uses_native_integer_triangle_boundaries_and_upper_alpha_clamp() {
    for (clock, expected) in [
        (0, 0),
        (1, 0),
        (499, 127),
        (500, 128),
        (999, 255),
        (1000, 255),
        (1001, 255),
        (1499, 129),
        (1500, 128),
        (2000, 255),
    ] {
        let mut owner = scene();
        owner.request(10);
        active_particle(&mut owner, 0, clock);
        owner.advance(0, 0).unwrap();
        assert_eq!(owner.particles[0].mesh.alpha, expected, "clock {clock}");
        assert!(
            owner.particles[0]
                .mesh
                .vertices
                .iter()
                .all(|v| v.rgba_words[3] == (expected as f32 / 255.0_f32).to_bits())
        );
    }
}

#[test]
fn state20_fades_all_geometry_and_finishes_at21_without_stopping_the_static_sound() {
    let mut owner = scene();
    owner.sound_started = true;
    owner.request(20);
    active_particle(&mut owner, 0, 1000);
    let frame = owner.advance(200, 33).unwrap();
    assert_eq!(owner.fade_clock, 200);
    assert_eq!(owner.rings[0].alpha, 205);
    let base = 256 - ((700 * 128 - 64000) / 500);
    assert_eq!(owner.particles[0].mesh.alpha, base - base * 200 / 1000);
    assert_eq!(frame.audio, [None; 4]);
    owner.fade_clock = 999;
    let frame = owner.advance(1, 0).unwrap();
    assert_eq!(owner.status(), 21);
    assert!(owner.rings.iter().all(|m| m.alpha == 0));
    assert_eq!(owner.particles[0].mesh.alpha, 0);
    assert_eq!(frame.audio, [None; 4]);
    owner.request(20);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 21);
    assert_eq!(owner.fade_clock, 1000);
}

#[test]
fn state22_fades_only_particles_and_waits_for_nonzero_pulse_before_finishing23() {
    let mut owner = scene();
    for mesh in &mut owner.rings {
        mesh.set_alpha(123);
    }
    for particle in &mut owner.particles {
        particle.delay = 0;
        particle.pulse_clock = 0;
    }
    owner.request(22);
    owner.fade_clock = 1000;
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 22);
    assert!(owner.rings.iter().all(|m| m.alpha == 123));
    active_particle(&mut owner, 511, 1);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 23);
    assert_eq!(owner.particles[511].mesh.alpha, 0);
    assert!(owner.rings.iter().all(|m| m.alpha == 123));
    owner.request(20);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 21);
    assert_eq!(owner.fade_clock, 1000);
}

#[test]
fn zero_pulse_skips_alpha_even_in_hidden_states_and_all_timers_wrap_as_dwords() {
    let mut owner = scene();
    owner.state = 23;
    owner.particles[0].mesh.set_alpha(77);
    active_particle(&mut owner, 0, u32::MAX - 99);
    owner.advance(100, 0).unwrap();
    assert_eq!(owner.particles[0].pulse_clock, 0);
    assert_eq!(owner.particles[0].mesh.alpha, 77);
    owner.state = 0;
    owner.growth_clock = u32::MAX - 49;
    owner.advance(100, 0).unwrap();
    assert_eq!(owner.growth_clock, 50);
    owner.request(22);
    owner.fade_clock = u32::MAX - 49;
    owner.advance(100, 0).unwrap();
    assert_eq!(owner.fade_clock, 50);
    assert_eq!(owner.status(), 22);
}

#[test]
fn private_snapshot_preserves_live_resources_pending_gates_and_all_particle_clocks() {
    let mut owner = scene();
    owner.request(11);
    owner.advance(200, 7).unwrap();
    owner.request(50);
    owner.request(22);
    let encoded = serde_json::to_vec(&owner).unwrap();
    let mut restored: CmvsNativeSceneKind54 = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(owner, restored);
    assert_eq!(
        owner.advance(200, 123).unwrap(),
        restored.advance(200, 123).unwrap()
    );
    assert_eq!(owner, restored);
}

#[test]
fn validation_rejects_invalid_mesh_consumers() {
    let mut owner = scene();
    owner.validate().unwrap();
    for invalid in [
        0,
        (-1.0_f32).to_bits(),
        f32::INFINITY.to_bits(),
        f32::NAN.to_bits(),
        1,
    ] {
        let mut malformed = owner.clone();
        malformed.particles[0].rise_time_word = invalid;
        assert!(malformed.validate().is_err());
        let before = malformed.clone();
        assert!(malformed.advance(200, 0).is_err());
        assert_eq!(malformed, before);
    }
    owner.particles.pop();
    assert!(owner.validate().is_err());
}
