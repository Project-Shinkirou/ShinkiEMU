use super::*;

fn command(
    vm: &mut CmvsPs2aVmState,
    id: u16,
    words: &[u32],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    for &word in words.iter().rev() {
        push_word(vm, word)?;
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

#[test]
fn controller_constructor_and_request_preserve_unwritten_camera_axes_and_slot_gates() {
    let mut vm = CmvsPs2aVmState::new(0);
    vm.independent_views[0].angle_words[2] = 12.0_f32.to_bits();
    vm.independent_views[0].field_of_view_word = 55.0_f32.to_bits();
    command(&mut vm, 657, &[0]).unwrap();
    assert_eq!(
        vm.independent_views[0].position_words.map(f32::from_bits),
        [0.0, 1.5, 0.0]
    );
    assert_eq!(
        vm.independent_views[0].angle_words.map(f32::from_bits),
        [35.0, 0.0, 12.0]
    );
    assert_eq!(
        f32::from_bits(vm.independent_views[0].field_of_view_word),
        55.0
    );
    command(&mut vm, 687, &[0, 2, 0x7fc0_0000, u32::MAX]).unwrap();
    tick(&mut vm, 200).unwrap();
    command(&mut vm, 686, &[0]).unwrap();
    assert_eq!(vm.interpreter_words[&81220], 2);
    command(&mut vm, 658, &[4]).unwrap();
    command(&mut vm, 687, &[4, 1, 0, 0]).unwrap();
    command(&mut vm, 686, &[4]).unwrap();
    assert_eq!(vm.interpreter_words[&81220], 2);
    assert_eq!(vm.stack_cursor_bytes, 0);
}

#[test]
fn scene_camera_changes_are_visible_to_later_slots_in_the_same_callback() {
    for controller_slot in [0, 1] {
        let mut vm = CmvsPs2aVmState::new(0);
        let floor_slot = 1 - controller_slot;
        start_scene50(&mut vm, floor_slot, [b"a.pb3".to_vec(), b"b.pb3".to_vec()]).unwrap();
        command(&mut vm, 657, &[controller_slot]).unwrap();
        command(&mut vm, 687, &[controller_slot, 1, 0, 0]).unwrap();
        tick(&mut vm, 200).unwrap();
        let previous_z = vm.independent_views[0].position_words[2];
        tick(&mut vm, 200).unwrap();
        let current_z = vm.independent_views[0].position_words[2];
        assert_ne!(previous_z, current_z);
        let floor_z = vm.native_scene_workers[&(floor_slot as u8)]
            .mesh(3)
            .unwrap()
            .position_words[2];
        assert_eq!(
            floor_z,
            if controller_slot == 0 {
                current_z
            } else {
                previous_z
            }
        );
    }
}

#[test]
fn private_continuation_preserves_pending_effect_and_sound_gate_but_native_load_clears_controllers()
{
    let mut vm = CmvsPs2aVmState::new(0);
    command(&mut vm, 657, &[0]).unwrap();
    command(&mut vm, 658, &[1]).unwrap();
    command(&mut vm, 687, &[0, 3, 0, 0]).unwrap();
    command(&mut vm, 687, &[1, 1, 0, 0]).unwrap();
    for _ in 0..7 {
        tick(&mut vm, 200).unwrap();
    }
    let bytes = postcard::to_allocvec(&vm).unwrap();
    let mut restored: CmvsPs2aVmState = postcard::from_bytes(&bytes).unwrap();
    for _ in 0..35 {
        assert_eq!(
            tick(&mut vm, 200).unwrap(),
            tick(&mut restored, 200).unwrap()
        );
        assert_eq!(vm.independent_views, restored.independent_views);
        assert_eq!(vm.native_scene_workers, restored.native_scene_workers);
    }
    let records = capture(&vm).unwrap();
    assert_eq!(records[&0].encode().unwrap(), [1, 0, 0x51, 0]);
    assert_eq!(records[&1].encode().unwrap(), [1, 0, 0x52, 0]);
    restore(&mut restored, &records).unwrap();
    for slot in [0, 1] {
        assert!(matches!(
            restored.native_scene_workers[&slot],
            CmvsNativeSceneWorker::Empty
        ));
        assert_eq!(
            restored.native_scene_workers[&slot].status().unwrap(),
            u32::MAX
        );
    }
}

#[test]
fn collision_request_changes_future_respawn_span_but_not_existing_basic_projection() {
    let mut vm = CmvsPs2aVmState::new(0);
    let mut rng = vm.prng_state;
    let owner = CmvsNativeCollisionParticles::new(
        b"p.pb3".to_vec(),
        [0x4000_0000, 4.0_f32.to_bits(), 1000.0_f32.to_bits(), 1],
        &mut rng,
    )
    .unwrap();
    vm.native_scene_workers
        .insert(0, CmvsNativeSceneWorker::CollisionParticles(owner));
    command(&mut vm, 687, &[0, 1, 0x8500_0000, 0]).unwrap();
    let CmvsNativeSceneWorker::CollisionParticles(owner) =
        vm.native_scene_workers.get_mut(&0).unwrap()
    else {
        unreachable!()
    };
    assert_eq!(f32::from_bits(owner.projection_height_word()), 60.0);
    owner.particles[0].position_words[1] = (-1.0_f32).to_bits();
    owner.advance(0, &[], &mut rng).unwrap();
    let z = f32::from_bits(owner.particles[0].position_words[2]);
    assert!((4.0..=5.0).contains(&z));
    assert_eq!(owner.capture().unwrap().parameter_words[0], 0x8500_0000);
}

#[test]
fn plane_controller_constructor_uses_one_word_without_rng_camera_or_legacy_context_writes() {
    let mut vm = CmvsPs2aVmState::new(0);
    vm.write_interpreter_word(3168, 0xdead_beef);
    vm.write_interpreter_word(3340, 0xfeed_face);
    let camera = vm.independent_views.clone();
    let rng = vm.prng_state;
    assert!(matches!(
        command(&mut vm, 661, &[1]).unwrap(),
        Some(CmvsPs2aVmAction::NativeSceneWorkerResourceChanged {
            slot: 1,
            retired_static_sounds: 0,
            stopped_streams: 0,
        })
    ));
    assert_eq!(vm.stack_cursor_bytes, 0);
    assert_eq!(vm.interpreter_words[&2952], 1);
    assert_eq!(vm.interpreter_words[&3168], 0xdead_beef);
    assert_eq!(vm.interpreter_words[&3340], 0xfeed_face);
    assert_eq!(vm.prng_state, rng);
    assert_eq!(vm.independent_views, camera);
    let candidate = vm.native_scene_workers.clone();
    assert_eq!(command(&mut vm, 661, &[4]).unwrap(), None);
    assert_eq!(vm.native_scene_workers, candidate);
    let owner = &vm.native_scene_workers[&1];
    assert_eq!(owner.mesh_count(), 0);
    assert_eq!(owner.basic_resource_count(), 3);
    assert_eq!(
        owner.sound_resources(),
        &CmvsNativeSceneKind55::SOUND_RESOURCES
    );
    for index in 0..3 {
        assert_eq!(
            owner.basic_resource(index),
            Some(CmvsNativeSceneKind55::RESOURCES[index])
        );
        assert_eq!(owner.basic_resource_index(index), Some(index));
        assert!(owner.particle_basic(index).is_none());
    }
}

#[test]
fn retiring_plane_micro_row_keeps_following_commands_and_unmodeled_range_aligned() {
    let mut vm = CmvsPs2aVmState::new(0);
    command(&mut vm, 668, &[1]).unwrap();
    assert_eq!(vm.interpreter_words[&10548], 1);
    assert!(vm.native_scene_workers.is_empty());
    assert!(vm.slot_objects.is_empty());
    command(&mut vm, 668, &[0]).unwrap();
    assert_eq!(vm.interpreter_words[&10548], 0);
    assert_eq!(vm.stack_cursor_bytes, 0);
}

#[test]
fn ordinary662_consumes_ignored_tail_and_reports_native_uninitialized_reads_without_faking_phase() {
    let mut vm = CmvsPs2aVmState::new(0);
    let camera = vm.independent_views.clone();
    let rng = vm.prng_state;
    command(&mut vm, 662, &[0, 0x7fc0_0000]).unwrap();
    assert_eq!(vm.stack_cursor_bytes, 0);
    assert_eq!(vm.independent_views, camera);
    assert_eq!(vm.prng_state, rng);
    vm.write_interpreter_word(81220, 77);
    let before = vm.native_scene_workers[&0].clone();
    let query = command(&mut vm, 686, &[0]).unwrap_err();
    assert_eq!(
        query.code(),
        "SHINKI_EMU_CMVS_NATIVE_SCENE_KIND56_INDETERMINATE"
    );
    assert_eq!(vm.interpreter_words[&81220], 77);
    let advance = tick(&mut vm, 200).unwrap_err();
    assert_eq!(
        advance.code(),
        "SHINKI_EMU_CMVS_NATIVE_SCENE_KIND56_INDETERMINATE"
    );
    assert_eq!(vm.native_scene_workers[&0], before);
}
