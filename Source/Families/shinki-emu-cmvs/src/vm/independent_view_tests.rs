use super::*;
use crate::scene::parent_load_tests::command;
use crate::system_save::slot_projection::SavedProjection;

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|v| v.to_bits()).collect()
}

#[test]
fn native_view_records_restore_both_typed_owners_and_capture_visible_motion_instant() {
    let mut vm = CmvsPs2aVmState::new(0);
    let mut projection = SavedProjection::default();
    for (index, tag) in [2178u16, 2179].into_iter().enumerate() {
        let mut record = 2u16.to_le_bytes().to_vec();
        record.extend_from_slice(&(7u32 + index as u32).to_le_bytes());
        for value in [2.0 + index as f32, 3.0, -4.0, 10.0, 20.0, 30.0, 65.0] {
            record.extend_from_slice(&value.to_le_bytes());
        }
        projection.decode_record(tag, &record).unwrap();
        assert_eq!(record.len(), 34);
        assert_eq!(projection.encode_record(tag).unwrap(), record);
    }
    restore_pair(&mut vm, &projection.views).unwrap();
    assert_eq!(capture_pair(&vm).unwrap(), projection.views);
    assert_ne!(vm.independent_views[0], vm.independent_views[1]);
    let secondary = vm.independent_views[1].clone();
    command(
        &mut vm,
        627,
        &[
            0,
            10.0_f32.to_bits(),
            3.0_f32.to_bits(),
            (-4.0_f32).to_bits(),
            1000,
        ],
    );
    tick(&mut vm, 250).unwrap();
    assert_eq!(
        f32::from_bits(vm.independent_views[0].position_words[0]),
        4.0
    );
    assert_eq!(vm.independent_views[1], secondary);
    let saved = capture_pair(&vm).unwrap();
    assert_eq!(saved[0].position[0], 4.0);
    let mut cold = CmvsPs2aVmState::new(0);
    restore_pair(&mut cold, &saved).unwrap();
    assert_eq!(capture_pair(&cold).unwrap(), saved);
    assert!(!pending(&cold));
    assert_eq!(
        cold.independent_views[0].motion,
        CmvsIndependentViewMotion::default()
    );
    // Private snapshots retain the active worker; native34B records do not.
    let encoded = serde_json::to_vec(&vm).unwrap();
    let mut resumed: CmvsPs2aVmState = serde_json::from_slice(&encoded).unwrap();
    tick(&mut resumed, 750).unwrap();
    assert_eq!(
        f32::from_bits(resumed.independent_views[0].position_words[0]),
        10.0
    );
    assert!(!pending(&resumed));
}

#[test]
fn native_view_direct_setters_do_not_cancel_worker_or_touch_plane_state() {
    let mut vm = CmvsPs2aVmState::new(0);
    let plane = vm.screen_words.clone();
    let second = vm.independent_views[1].clone();
    command(
        &mut vm,
        629,
        &[
            0,
            10.0_f32.to_bits(),
            2.0_f32.to_bits(),
            4.0_f32.to_bits(),
            15.0_f32.to_bits(),
            25.0_f32.to_bits(),
            1000,
        ],
    );
    command(&mut vm, 608, &bits(&[3.0, 4.0, 5.0]));
    command(&mut vm, 609, &bits(&[6.0]));
    command(&mut vm, 610, &bits(&[7.0]));
    command(&mut vm, 611, &bits(&[8.0]));
    command(&mut vm, 612, &bits(&[120.0]));
    command(&mut vm, 626, &[]);
    assert_eq!(vm.interpreter_words[&81220], 1);
    assert!(pending(&vm));
    command(&mut vm, 624, &[1]);
    assert_eq!(
        vm.independent_views[0].position_words,
        [10.0, 2.0, 4.0].map(f32::to_bits)
    );
    assert_eq!(
        vm.independent_views[0].angle_words,
        [15.0, 25.0, 8.0].map(f32::to_bits)
    );
    assert_eq!(
        vm.independent_views[0].field_of_view_word,
        120.0_f32.to_bits()
    );
    assert_eq!(vm.independent_views[1], second);
    assert_eq!(vm.screen_words, plane);
    command(&mut vm, 626, &[]);
    assert_eq!(vm.interpreter_words[&81220], 0);
}

#[test]
fn native_view_curves_unknown_modes_zero_duration_and_cancel_follow_worker_branches() {
    for (curve, midpoint) in [(0, 5.0), (1, 7.07), (2, 2.93), (99, 0.0)] {
        let mut vm = CmvsPs2aVmState::new(0);
        command(
            &mut vm,
            627,
            &[curve, 10.0_f32.to_bits(), 1.0_f32.to_bits(), 0, 1000],
        );
        tick(&mut vm, 500).unwrap();
        assert!(
            (f32::from_bits(vm.independent_views[0].position_words[0]) - midpoint).abs() < 0.001
        );
        tick(&mut vm, 500).unwrap();
        assert_eq!(
            vm.independent_views[0].position_words[0],
            10.0_f32.to_bits()
        );
        assert!(!pending(&vm));
    }
    let mut vm = CmvsPs2aVmState::new(0);
    command(
        &mut vm,
        628,
        &[99, 20.0_f32.to_bits(), 30.0_f32.to_bits(), 0],
    );
    tick(&mut vm, 0).unwrap();
    assert_eq!(
        vm.independent_views[0].angle_words[..2],
        [20.0, 30.0].map(f32::to_bits)
    );
    command(
        &mut vm,
        627,
        &[0, 10.0_f32.to_bits(), 1.0_f32.to_bits(), 0, 1000],
    );
    tick(&mut vm, 200).unwrap();
    let instant = vm.independent_views[0].position_words;
    command(&mut vm, 624, &[0]);
    tick(&mut vm, 2000).unwrap();
    assert_eq!(vm.independent_views[0].position_words, instant);
}

#[test]
fn native_view_restore_and_commands_validate_all_candidates_before_publishing() {
    let mut vm = CmvsPs2aVmState::new(0);
    command(&mut vm, 608, &bits(&[2.0, 3.0, 4.0]));
    let before = vm.clone();
    let mut saved = capture_pair(&vm).unwrap();
    saved[0].position[0] = 9.0;
    saved[1].field_of_view = f32::NAN;
    assert!(restore_pair(&mut vm, &saved).is_err());
    assert_eq!(vm, before);
    assert!(
        execute(
            &mut vm,
            629,
            &[0, 1.0_f32.to_bits(), 0, 0, f32::INFINITY.to_bits(), 0, 100]
        )
        .is_err()
    );
    assert_eq!(vm, before);
    // Native does not constrain finite FOV to the conventional 0..180 range.
    execute(&mut vm, 612, &[270.0_f32.to_bits()]).unwrap();
    assert_eq!(capture_pair(&vm).unwrap()[0].field_of_view, 270.0);
}

#[test]
fn native_view_both_workers_tick_and_reset_preserves_enable_words() {
    let mut vm = CmvsPs2aVmState::new(0);
    vm.independent_views[1].enabled = 7;
    for (i, view) in vm.independent_views.iter_mut().enumerate() {
        view.start(
            627,
            &[
                0,
                (10.0_f32 + i as f32).to_bits(),
                1.0_f32.to_bits(),
                0,
                100,
            ],
        );
    }
    tick(&mut vm, 100).unwrap();
    assert_eq!(
        vm.independent_views[0].position_words[0],
        10.0_f32.to_bits()
    );
    assert_eq!(
        vm.independent_views[1].position_words[0],
        11.0_f32.to_bits()
    );
    reset(&mut vm);
    assert_eq!(vm.independent_views[0].enabled, 1);
    assert_eq!(vm.independent_views[1].enabled, 7);
    assert_eq!(
        vm.independent_views[1].position_words,
        [0, 1.0_f32.to_bits(), 0]
    );
}

#[test]
fn native_view_wait_preserves_pc_and_stack_until_worker_completes() {
    let mut vm = CmvsPs2aVmState::new(0);
    command(
        &mut vm,
        627,
        &[0, 10.0_f32.to_bits(), 1.0_f32.to_bits(), 0, 1000],
    );
    command(&mut vm, 625, &[0]);
    assert!(vm.dispatch_stopped);
    let held = (
        vm.program_counter,
        vm.stack_cursor_bytes,
        vm.stack_bytes.clone(),
        vm.stack_initialized.clone(),
    );
    let poll = |vm: &mut CmvsPs2aVmState| {
        vm.dispatch_stopped = false;
        crate::execute_cmvs390_frame(
            vm,
            &crate::CmvsPs2aInstructionFrame::Command {
                span: crate::CmvsPs2aSourceSpan {
                    offset: vm.program_counter,
                    byte_length: 2,
                },
                command_id: 625,
            },
        )
        .unwrap();
    };
    tick(&mut vm, 333).unwrap();
    poll(&mut vm);
    assert_eq!(
        (
            vm.program_counter,
            vm.stack_cursor_bytes,
            vm.stack_bytes.clone(),
            vm.stack_initialized.clone()
        ),
        held
    );
    tick(&mut vm, 667).unwrap();
    poll(&mut vm);
    assert_eq!(
        (vm.program_counter, vm.stack_cursor_bytes),
        (held.0 + 2, held.1 - 4)
    );
    assert!(!pending(&vm));
    assert!(!vm.dispatch_stopped);
}

#[test]
fn native_view_wait_forced_skip_precedes_and_preserves_confirmation_edges() {
    for condition in 0..6 {
        let mut vm = CmvsPs2aVmState::new(0);
        command(
            &mut vm,
            627,
            &[0, 10.0_f32.to_bits(), 1.0_f32.to_bits(), 0, 1000],
        );
        vm.write_interpreter_word(1452, u32::from(condition != 0));
        vm.write_interpreter_word(1464, u32::from(condition == 3));
        vm.input_shift_held = condition == 1;
        vm.input_control_held = condition == 2;
        vm.input_advance_press = true;
        vm.input_advance_release = condition != 5;
        command(&mut vm, 625, &[1]);
        assert!(vm.dispatch_stopped);
        assert_eq!(pending(&vm), condition == 5);
        assert_eq!(vm.interpreter_words[&81220], u32::from(condition != 5));
        if condition != 5 {
            assert_eq!(
                vm.independent_views[0].position_words[0],
                10.0_f32.to_bits()
            );
            assert_eq!(vm.input_advance_release, condition < 4);
            assert_eq!(vm.input_advance_press, condition < 4);
        }
    }
}
