use super::*;
use crate::vm::native_scene_worker::tick;

fn mesh_owner(vm: &CmvsPs2aVmState) -> &CmvsNativeSnow {
    let CmvsNativeSceneWorker::MeshSnow(owner) = &vm.native_scene_workers[&0] else {
        panic!("expected mesh snow")
    };
    owner
}
fn mesh_owner_mut(vm: &mut CmvsPs2aVmState) -> &mut CmvsNativeSnow {
    let CmvsNativeSceneWorker::MeshSnow(owner) = vm.native_scene_workers.get_mut(&0).unwrap()
    else {
        panic!("expected mesh snow")
    };
    owner
}

fn parameters(count: u32) -> [u32; 6] {
    [
        2.0_f32.to_bits(),
        3.0_f32.to_bits(),
        1000.0_f32.to_bits(),
        20.0_f32.to_bits(),
        count,
        0,
    ]
}
fn command(
    vm: &mut CmvsPs2aVmState,
    id: u16,
    top_first: &[u32],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    for value in top_first.iter().rev() {
        push_word(vm, *value)?;
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
fn command_fixture() -> CmvsPs2aVmState {
    let mut vm = CmvsPs2aVmState::new(100);
    vm.frame_string_pool_bytes
        .insert(0, b"red.pb3\0blue.pb3\0".to_vec());
    vm.frame_string_values.insert(
        0,
        BTreeMap::from([(0, b"red.pb3".to_vec()), (8, b"blue.pb3".to_vec())]),
    );
    vm
}

#[test]
fn ordinary_674_creates_replaces_and_destroys_real_billboards() {
    let mut vm = command_fixture();
    let params = parameters(3);
    let mut words = vec![0, 0];
    words.extend_from_slice(&params);
    command(&mut vm, 674, &words).unwrap();
    let snow = mesh_owner(&vm);
    assert_eq!(snow.resource, b"red.pb3");
    assert_eq!(snow.particles.len(), 3);
    assert!(snow.particles.iter().all(|particle| particle.mesh.billboard
        && particle.mesh.force_blend
        && particle.mesh.late_pass));
    let initial = snow.particles[0].mesh.position_words;
    tick(&mut vm, 20).unwrap();
    assert_ne!(
        mesh_owner(&vm).particles[0].mesh.position_words[1],
        initial[1]
    );
    words[1] = 8;
    words[6] = 1;
    command(&mut vm, 674, &words).unwrap();
    assert_eq!(mesh_owner(&vm).resource, b"blue.pb3");
    assert_eq!(mesh_owner(&vm).particles.len(), 1);
    command(&mut vm, 686, &[0]).unwrap();
    assert_eq!(vm.interpreter_words[&81220], u32::MAX);
    command(&mut vm, 686, &[4]).unwrap();
    assert_eq!(vm.interpreter_words[&81220], u32::MAX);
    command(&mut vm, 41, &[1]).unwrap();
    assert!(vm.native_mesh_equal_prepend);
    command(&mut vm, 672, &[0]).unwrap();
    assert!(vm.native_scene_workers.is_empty());
    assert!(command(&mut vm, 686, &[0]).is_err());
}

#[test]
fn ordinary_random_and_mesh_creation_share_msvc_sequence() {
    let mut vm = command_fixture();
    command(&mut vm, 212, &[32768]).unwrap();
    let mut words = vec![0, 0];
    words.extend_from_slice(&parameters(1));
    command(&mut vm, 674, &words).unwrap();
    // Native MSVC rand draws after case212: 18467,6334,26500,19169.
    let particle = &mesh_owner(&vm).particles[0];
    assert_eq!(
        particle.mesh.position_words,
        [0x3e023908, 0x401b4736, 0xbf1d073a]
    );
    assert_eq!(particle.fall_time_word, 0x447e401f);
    assert_eq!(vm.prng_state, 3403800452);
}

#[test]
fn generated_strip_keeps_xyz_float_color_uv_and_world_orientation() {
    let mut mesh = CmvsNativeMesh::billboard(4.0, 6.0, [1.0, 2.0]).unwrap();
    assert_eq!(
        mesh.vertices[0].position_words.map(f32::from_bits),
        [-1.0, 4.0, 0.0]
    );
    assert_eq!(
        mesh.vertices[3].position_words.map(f32::from_bits),
        [3.0, -2.0, 0.0]
    );
    assert_eq!(mesh.vertices[2].uv_words.map(f32::from_bits), [0.0, 1.0]);
    mesh.set_alpha(128);
    assert_eq!(
        f32::from_bits(mesh.vertices[0].rgba_words[3]),
        128.0 / 255.0
    );
    mesh.billboard = false;
    mesh.scale_words = bits([2.0, 3.0, 4.0]);
    mesh.angle_words[2] = 90.0_f32.to_bits();
    mesh.position_words = bits([5.0, 6.0, 7.0]);
    let world = mesh.world_matrix(identity()).unwrap();
    assert!((world[1] - 2.0).abs() < 0.00001);
    assert!((world[4] + 3.0).abs() < 0.00001);
    assert_eq!(&world[12..15], &[5.0, 6.0, 7.0]);
    mesh.billboard = true;
    let view = [
        0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 9.0, 8.0, 7.0, 1.0,
    ];
    let world = mesh.world_matrix(view).unwrap();
    assert_eq!([world[2], world[8]], [2.0, -4.0]);
    assert_eq!(&world[12..15], &[5.0, 6.0, 7.0]);
}

#[test]
fn native_particle_clock_caps_each_step_and_wraps_only_once() {
    let mut rng = 1;
    let snow = CmvsNativeSnow::new(b"red.pb3".to_vec(), parameters(2), &mut rng).unwrap();
    let mut capped = snow.clone();
    let mut exact = snow;
    capped.advance(5000, [0.0, 1.0, 0.0]).unwrap();
    exact.advance(500, [0.0, 1.0, 0.0]).unwrap();
    assert_eq!(capped, exact);
    let particle = &mut exact.particles[0];
    particle.mesh.position_words = bits([0.0, 0.1, 0.0]);
    particle.fall_time_word = 100.0_f32.to_bits();
    exact.advance(500, [0.0, 1.0, 0.0]).unwrap();
    assert!((f32::from_bits(exact.particles[0].mesh.position_words[1]) + 6.9).abs() < 0.00001);
}

#[test]
fn native_follow_camera_and_radial_alpha_modify_real_mesh_data() {
    let mut rng = 1;
    let mut params = parameters(1);
    params[5] = 1;
    params[3] = 1.0_f32.to_bits();
    let mut snow = CmvsNativeSnow::new(b"red.pb3".to_vec(), params, &mut rng).unwrap();
    snow.particles[0].mesh.position_words = bits([3.5, 1.0, 0.0]);
    snow.advance(0, [0.0, 1.0, 0.0]).unwrap();
    assert_eq!(
        snow.particles[0].mesh.position_words.map(f32::from_bits),
        [1.5, 1.0, 0.0]
    );
    assert_eq!(snow.particles[0].mesh.alpha, 127);
    assert_eq!(
        f32::from_bits(snow.particles[0].mesh.vertices[0].rgba_words[3]),
        127.0 / 255.0
    );
}

#[test]
fn independently_encoded_2048_configuration_rebuilds_native_geometry() {
    let mut wire = vec![1, 0, 2, 0];
    for word in parameters(2) {
        wire.extend_from_slice(&word.to_le_bytes());
    }
    let mut name = [0xa5; 32];
    name[..8].copy_from_slice(b"red.pb3\0");
    wire.extend_from_slice(&name);
    let saved = SavedNativeSnow::decode(&wire).unwrap();
    assert_eq!(saved.encode().unwrap(), wire);
    let mut rng = 1;
    let mut snow = saved.rebuild(&mut rng).unwrap();
    let born = snow.particles.clone();
    snow.advance(100, [0.0, 1.0, 0.0]).unwrap();
    let capture = snow.capture().unwrap();
    assert_eq!(capture.parameter_words, saved.parameter_words);
    assert_ne!(snow.particles, born);
    let mut cold_rng = 1;
    let cold = capture.rebuild(&mut cold_rng).unwrap();
    assert_eq!(cold.particles, born);
}

#[test]
fn malformed_native_restore_is_atomic_across_managers_and_random_state() {
    let mut vm = command_fixture();
    super::super::native_scene_worker::start(
        &mut vm,
        674,
        &[
            0,
            0,
            parameters(1)[0],
            parameters(1)[1],
            parameters(1)[2],
            parameters(1)[3],
            1,
            0,
        ],
        b"red.pb3".to_vec(),
    )
    .unwrap();
    let before = vm.native_scene_workers.clone();
    let before_rng = vm.prng_state;
    let super::super::native_scene_worker::SavedNativeSceneWorker::MeshSnow(good) =
        super::super::native_scene_worker::capture(&vm)
            .unwrap()
            .remove(&0)
            .unwrap()
    else {
        unreachable!()
    };
    let mut bad = good.clone();
    bad.parameter_words[4] = 8193;
    let records = BTreeMap::from([
        (
            0,
            super::super::native_scene_worker::SavedNativeSceneWorker::MeshSnow(good),
        ),
        (
            1,
            super::super::native_scene_worker::SavedNativeSceneWorker::MeshSnow(bad),
        ),
    ]);
    assert!(super::super::native_scene_worker::restore(&mut vm, &records).is_err());
    assert_eq!(vm.native_scene_workers, before);
    assert_eq!(vm.prng_state, before_rng);
    let mut invalid_params = parameters(1);
    invalid_params[2] = 0;
    assert!(CmvsNativeSnow::new(b"red.pb3".to_vec(), invalid_params, &mut vm.prng_state).is_err());
    assert_eq!(vm.prng_state, before_rng);
}

#[test]
fn malformed_geometry_cannot_partially_advance_one_manager() {
    let mut rng = 1;
    let mut snow = CmvsNativeSnow::new(b"red.pb3".to_vec(), parameters(2), &mut rng).unwrap();
    snow.particles[1].mesh.vertices[0].position_words[0] = f32::NAN.to_bits();
    let before = snow.clone();
    assert!(snow.advance(50, [0.0, 1.0, 0.0]).is_err());
    assert_eq!(snow, before);
}

#[test]
fn private_snapshot_continues_particle_phase_and_shared_random_sequence() {
    let mut vm = command_fixture();
    let mut words = vec![0, 0];
    words.extend_from_slice(&parameters(1));
    command(&mut vm, 674, &words).unwrap();
    let particle = &mut mesh_owner_mut(&mut vm).particles[0];
    particle.mesh.position_words = bits([0.25, 0.1, -0.5]);
    particle.fall_time_word = 100.0_f32.to_bits();
    tick(&mut vm, 20).unwrap();
    let encoded = postcard::to_allocvec(&vm).unwrap();
    let mut restored: CmvsPs2aVmState = postcard::from_bytes(&encoded).unwrap();
    validate_cmvs390_vm_state(&restored).unwrap();
    tick(&mut restored, 100).unwrap();
    let particle = &mesh_owner(&restored).particles[0];
    assert_eq!(particle.mesh.position_words[0], 0.25_f32.to_bits());
    assert!((f32::from_bits(particle.mesh.position_words[1]) - 0.7).abs() < 0.00001);
    assert_eq!(particle.mesh.position_words[2], (-0.5_f32).to_bits());
    assert_eq!(particle.mesh.alpha, 255);
    command(&mut restored, 212, &[32768]).unwrap();
    assert_eq!(restored.interpreter_words[&81220], 19169);
    assert_eq!(restored.prng_state, 3403800452);
}
