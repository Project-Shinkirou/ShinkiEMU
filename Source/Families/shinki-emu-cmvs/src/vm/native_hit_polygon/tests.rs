use super::*;
use crate::vm::native_scene_worker::tick;

fn collision_owner(vm: &CmvsPs2aVmState) -> &CmvsNativeCollisionParticles {
    let CmvsNativeSceneWorker::CollisionParticles(owner) = &vm.native_scene_workers[&0] else {
        panic!("expected collision particles")
    };
    owner
}

fn vertex(value: [f32; 3]) -> [u32; 3] {
    value.map(f32::to_bits)
}
fn floor(y: f32) -> [[f32; 4]; 3] {
    [[0.0, y, 0.0, 1.0], [4.0, y, 0.0, 1.0], [0.0, y, 4.0, 1.0]]
}
fn command(
    vm: &mut CmvsPs2aVmState,
    id: u16,
    words: &[u32],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    for word in words.iter().rev() {
        push_word(vm, *word)?;
    }
    execute_cmvs390_frame(
        vm,
        &CmvsPs2aInstructionFrame::Command {
            span: crate::CmvsPs2aSourceSpan {
                offset: 0,
                byte_length: 2,
            },
            command_id: id,
        },
    )
}
fn worker() -> CmvsNativeCollisionParticles {
    CmvsNativeCollisionParticles::new(
        b"snow.pb3".to_vec(),
        [0, 4.0_f32.to_bits(), 1000.0_f32.to_bits(), 1],
        &mut 1,
    )
    .unwrap()
}
fn set_position(worker: &mut CmvsNativeCollisionParticles, value: [f32; 3]) {
    worker.particles[0].position_words = vertex(value);
    worker.particles[0].fall_time_word = 1000.0_f32.to_bits();
}

#[test]
fn ordinary_geometry_create_append_capacity_reset_delete_and_guards() {
    let mut vm = CmvsPs2aVmState::new(0);
    vm.texture_parents
        .insert(20, CmvsTextureParentState::default());
    vm.slot_objects.insert((1924 << 8) | 20, 1);
    vm.write_interpreter_word(81220, 123);
    command(&mut vm, 818, &[20, 1, 2, 3]).unwrap();
    assert!(vm.texture_parents[&20].collision_geometry.is_none());
    command(&mut vm, 816, &[20]).unwrap();
    for index in 0..129 {
        command(&mut vm, 818, &[20, index, 0x80000000, 0x7fc00001]).unwrap();
    }
    let geometry = vm.texture_parents[&20].collision_geometry.as_ref().unwrap();
    assert_eq!(geometry.vertices.len(), 128);
    assert_eq!(geometry.vertices[127], [127, 0x80000000, 0x7fc00001]);
    command(&mut vm, 816, &[20]).unwrap();
    assert_eq!(
        vm.texture_parents[&20].collision_geometry,
        Some(Default::default())
    );
    command(&mut vm, 817, &[20]).unwrap();
    assert!(vm.texture_parents[&20].collision_geometry.is_none());
    for id in [816, 817] {
        command(&mut vm, id, &[256]).unwrap();
        command(&mut vm, id, &[21]).unwrap();
    }
    assert_eq!(vm.interpreter_flag_words[&10532] & 0x10, 0x10);
    assert_eq!(vm.interpreter_words[&81220], 123);
    assert_eq!(vm.stack_cursor_bytes, 0);
}

#[test]
fn translation_requires_basic_and_uses_xyz_float_addition_only() {
    let geometry = CmvsNativeCollisionGeometry {
        kind: 777,
        vertices: vec![vertex([1.5, -2.0, 3.0])],
    };
    let mut output = [[9.0; 4]; 1];
    assert_eq!(geometry.translated(None, &mut output).unwrap(), 0);
    assert_eq!(output, [[9.0; 4]; 1]);
    assert_eq!(
        geometry
            .translated(Some(vertex([10.0, 20.0, -30.0])), &mut output)
            .unwrap(),
        1
    );
    assert_eq!(output, [[11.5, 18.0, -27.0, 1.0]]);
    assert!(geometry.translated(Some([0; 3]), &mut []).is_err());
}

#[test]
fn native_triangle_edges_vertices_and_both_windings_are_inclusive() {
    for triangle in [floor(7.0), [floor(7.0)[2], floor(7.0)[1], floor(7.0)[0]]] {
        for [x, z] in [[0.0, 0.0], [4.0, 0.0], [0.0, 4.0], [2.0, 2.0], [1.0, 1.0]] {
            assert_eq!(
                vertical_hit([x, -1000.0, z], &triangle).unwrap(),
                Some([x, 7.0, z])
            );
        }
        assert!(
            vertical_hit([2.01, 0.0, 2.01], &triangle)
                .unwrap()
                .is_none()
        );
        assert!(
            vertical_hit([-0.01, 0.0, 0.0], &triangle)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn triangle_soup_preserves_concave_gap_and_first_hit_order_not_nearest() {
    let mut triangles = floor(9.0).to_vec();
    triangles.extend(floor(2.0));
    assert_eq!(
        vertical_hit([1.0, 2.01, 1.0], &triangles).unwrap(),
        Some([1.0, 9.0, 1.0])
    );
    triangles[3..].copy_from_slice(&[
        [4.0, 2.0, 0.0, 1.0],
        [4.0, 2.0, 4.0, 1.0],
        [3.0, 2.0, 4.0, 1.0],
    ]);
    assert!(vertical_hit([2.5, 0.0, 3.0], &triangles).unwrap().is_none());
}

#[test]
fn sloped_height_uses_world_xz_and_ignores_query_y() {
    let triangle = [
        [0.0, 1.0, 0.0, 1.0],
        [4.0, 5.0, 0.0, 1.0],
        [0.0, 1.0, 4.0, 1.0],
    ];
    for y in [-100.0, 1000.0] {
        let hit = vertical_hit([1.0, y, 1.0], &triangle).unwrap().unwrap();
        assert!((hit[1] - 2.0).abs() < 0.00001);
    }
    assert_eq!(vertical_hit([0.0; 3], &[]).unwrap(), None);
    assert!(vertical_hit([0.0; 3], &triangle[..2]).is_err());
}

#[test]
fn constructor_uses_five_crt_draws_and_native_hundredth_truncation() {
    let mut rng = 1;
    let worker = CmvsNativeCollisionParticles::new(
        b"snow.pb3".to_vec(),
        [0, 4.0_f32.to_bits(), 1000.0_f32.to_bits(), 1],
        &mut rng,
    )
    .unwrap();
    let mut expected_rng = 1;
    let x = random(&mut expected_rng) * 90.0 - 45.0;
    let z = 4.0 + random(&mut expected_rng) * 20.0;
    let y = random(&mut expected_rng) * 75.0;
    random(&mut expected_rng);
    let fall_time = 1000.0 + (random(&mut expected_rng) * 250.0 - 125.0);
    assert_eq!(
        worker.particles[0].position_words,
        [hundredths(x), hundredths(y), hundredths(z)].map(f32::to_bits)
    );
    assert_eq!(worker.particles[0].fall_time_word, fall_time.to_bits());
    assert_eq!(rng, expected_rng);
}

#[test]
fn particle_fade_latches_height_and_respawns_after_crossing_half_unit() {
    let mut worker = worker();
    set_position(&mut worker, [1.0, 5.25, 1.0]);
    let mut rng = 1;
    worker.advance(0, &floor(5.0), &mut rng).unwrap();
    assert_eq!(worker.particles[0].alpha, 191);
    assert_eq!(
        f32::from_bits(worker.particles[0].collision_height_word),
        5.0
    );
    worker.advance(100, &floor(99.0), &mut rng).unwrap();
    assert_eq!(f32::from_bits(worker.particles[0].position_words[1]), 79.25);
    assert_eq!(worker.particles[0].alpha, 255);
    assert_eq!(worker.particles[0].collision_height_word, 0);
    let mut expected_rng = 1;
    random(&mut expected_rng);
    random(&mut expected_rng);
    assert_eq!(rng, expected_rng);
}

#[test]
fn particle_motion_caps_at120_and_zero_height_does_not_respawn() {
    let mut worker = worker();
    set_position(&mut worker, [1.0, 1.2, 1.0]);
    let mut rng = 1;
    worker.advance(1000, &[], &mut rng).unwrap();
    assert_eq!(f32::from_bits(worker.particles[0].position_words[1]), 0.0);
    assert_eq!(rng, 1);
    worker.advance(1, &[], &mut rng).unwrap();
    assert_eq!(f32::from_bits(worker.particles[0].position_words[1]), 74.99);
}

#[test]
fn native_save_rebuilds_particle_phase_and_retains_exact52byte_config() {
    let mut original = worker();
    set_position(&mut original, [1.0, 5.0, 1.0]);
    original.particles[0].alpha = 123;
    let saved = original.capture().unwrap();
    let bytes = saved.encode().unwrap();
    assert_eq!(bytes.len(), 52);
    assert_eq!(&bytes[..4], &[1, 0, 6, 0]);
    assert_eq!(
        SavedNativeCollisionParticles::decode(&bytes).unwrap(),
        saved
    );
    let rebuilt = saved.rebuild(&mut 1).unwrap();
    assert_ne!(
        rebuilt.particles[0].position_words,
        original.particles[0].position_words
    );
    assert_eq!(rebuilt.particles[0].alpha, 255);
    assert_eq!(rebuilt.particles[0].collision_height_word, 0);
}

#[test]
fn malformed_worker_construction_and_triangle_frame_fail_atomically() {
    let mut rng = 123;
    assert!(
        CmvsNativeCollisionParticles::new(b"snow.pb3".to_vec(), [0, 0, 0, 1], &mut rng).is_err()
    );
    assert_eq!(rng, 123);
    let mut worker = worker();
    let before = worker.clone();
    assert!(worker.advance(20, &floor(2.0)[..2], &mut rng).is_err());
    assert_eq!(worker, before);
    assert_eq!(rng, 123);
    let mut bytes = worker.capture().unwrap().encode().unwrap();
    bytes[16..20].copy_from_slice(&8193_u32.to_le_bytes());
    assert!(SavedNativeCollisionParticles::decode(&bytes).is_err());
    bytes.truncate(51);
    assert!(SavedNativeCollisionParticles::decode(&bytes).is_err());
}

#[test]
fn frame_collects_only_parent20_through27_with_basic_even_when_hidden() {
    let mut vm = CmvsPs2aVmState::new(0);
    let geometry = CmvsNativeCollisionGeometry {
        kind: 0,
        vertices: floor(0.0).map(|p| vertex([p[0], p[1], p[2]])).to_vec(),
    };
    vm.texture_parents.insert(
        20,
        CmvsTextureParentState {
            surface_initialized: true,
            hidden: true,
            float_position_words: vertex([0.0, 5.0, 0.0]),
            collision_geometry: Some(geometry.clone()),
            ..Default::default()
        },
    );
    vm.texture_parents.insert(
        19,
        CmvsTextureParentState {
            collision_geometry: Some(CmvsNativeCollisionGeometry {
                kind: 0,
                vertices: vec![[0; 3]],
            }),
            ..Default::default()
        },
    );
    vm.texture_parents.insert(
        21,
        CmvsTextureParentState {
            collision_geometry: Some(CmvsNativeCollisionGeometry {
                kind: 0,
                vertices: vec![[0; 3]],
            }),
            ..Default::default()
        },
    );
    let mut worker = worker();
    set_position(&mut worker, [1.0, 5.25, 1.0]);
    vm.native_scene_workers
        .insert(0, CmvsNativeSceneWorker::CollisionParticles(worker));
    tick(&mut vm, 0).unwrap();
    assert_eq!(collision_owner(&vm).particles[0].alpha, 191);
    assert_eq!(
        f32::from_bits(collision_owner(&vm).particles[0].collision_height_word),
        5.0
    );
}

#[test]
fn ordinary678_creates_native_kind6_and_mutually_replaces_other737_worker() {
    let mut vm = CmvsPs2aVmState::new(0);
    vm.frame_string_pool_bytes.insert(0, b"snow.pb3\0".to_vec());
    vm.frame_string_values
        .insert(0, BTreeMap::from([(0, b"snow.pb3".to_vec())]));
    command(
        &mut vm,
        674,
        &[
            0,
            0,
            2.0_f32.to_bits(),
            3.0_f32.to_bits(),
            1000.0_f32.to_bits(),
            20.0_f32.to_bits(),
            1,
            0,
        ],
    )
    .unwrap();
    assert!(matches!(
        vm.native_scene_workers.get(&0),
        Some(CmvsNativeSceneWorker::MeshSnow(_))
    ));
    command(
        &mut vm,
        678,
        &[0, 0, 0, 10.0_f32.to_bits(), 1000.0_f32.to_bits(), 2],
    )
    .unwrap();
    assert!(matches!(
        vm.native_scene_workers.get(&0),
        Some(CmvsNativeSceneWorker::CollisionParticles(_))
    ));
    assert_eq!(collision_owner(&vm).resource, b"snow.pb3");
    assert_eq!(collision_owner(&vm).particles.len(), 2);
    assert_eq!(
        f32::from_bits(collision_owner(&vm).projection_height_word()),
        15.0
    );
    command(&mut vm, 672, &[0]).unwrap();
    assert!(vm.native_scene_workers.is_empty());
}
