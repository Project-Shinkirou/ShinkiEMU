use super::*;
use crate::{CmvsPs2aSourceSpan, CmvsPs2aValueExpression, CmvsPs2aValueExpressionToken};

const SPAN: CmvsPs2aSourceSpan = CmvsPs2aSourceSpan {
    offset: 0,
    byte_length: 2,
};

fn tag_zero_value(offset: u32) -> CmvsPs2aInstructionFrame {
    CmvsPs2aInstructionFrame::ValueExpression(CmvsPs2aValueExpression {
        span: SPAN,
        tokens: vec![CmvsPs2aValueExpressionToken::Value {
            span: CmvsPs2aSourceSpan {
                offset: 2,
                byte_length: 6,
            },
            opcode: 0x120,
            payload: offset,
        }],
    })
}

fn push_current() -> CmvsPs2aInstructionFrame {
    CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushCurrentValue { span: SPAN })
}

fn push_immediate(value: u32) -> CmvsPs2aInstructionFrame {
    CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
        span: CmvsPs2aSourceSpan {
            offset: 0,
            byte_length: 8,
        },
        stack_bytes: 4,
        value,
    })
}

fn seed_pool_bytes(state: &mut CmvsPs2aVmState, frame: u16, entries: &[(u32, &[u8])]) {
    let mut pool = state
        .frame_string_pool_bytes
        .remove(&frame)
        .unwrap_or_default();
    for (offset, value) in entries {
        let start = *offset as usize;
        if pool.len() < start {
            pool.resize(start, 0);
        }
        let end = start + value.len() + 1;
        if pool.len() < end {
            pool.resize(end, 0);
        }
        pool[start..start + value.len()].copy_from_slice(value);
        pool[start + value.len()] = 0;
    }
    state.frame_string_pool_bytes.insert(frame, pool);
}

fn run_native_audio_gain_setting(
    state: &mut CmvsPs2aVmState,
    command_id: u16,
    mode: u32,
    value: u32,
) -> Option<CmvsPs2aVmAction> {
    for word in [value, mode] {
        execute_cmvs390_frame(state, &push_immediate(word)).unwrap();
    }
    execute_cmvs390_frame(
        state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id,
        },
    )
    .unwrap()
}

#[test]
fn native_audio_gain_getters_use_ctor_defaults_and_ignore_the_value_word() {
    for command_id in 729..=733 {
        for ignored_value in [0, 1, 256, 0x8000_0000, u32::MAX] {
            let mut state = CmvsPs2aVmState::new(0);
            state.write_interpreter_word(81220, 0x1234_5678);
            state.write_interpreter_word(13224, 0);
            execute_cmvs390_frame(&mut state, &push_immediate(0x9876_5432)).unwrap();
            let mut expected = state.interpreter_words.clone();
            expected.insert(81220, 256);

            assert_eq!(
                run_native_audio_gain_setting(&mut state, command_id, 0, ignored_value),
                None,
            );
            assert_eq!(state.interpreter_words, expected);
            assert_eq!(state.stack_cursor_bytes, 4);
            assert_eq!(read_stack_word_from_top(&state, 4).unwrap(), 0x9876_5432);
        }
    }
}

#[test]
fn native_audio_gain_setters_signed_clamp_and_preserve_results_and_other_settings() {
    for (command_id, field_offset) in [
        (729, 1560),
        (730, 1564),
        (731, 1568),
        (732, 1572),
        (733, 1576),
    ] {
        for mode in [1, 2, 0x8000_0000, u32::MAX] {
            for (value, expected_gain) in [
                (0x8000_0000, 0),
                (u32::MAX, 0),
                (0, 0),
                (1, 1),
                (255, 255),
                (256, 256),
                (257, 256),
                (0x7fff_ffff, 256),
            ] {
                let mut state = CmvsPs2aVmState::new(0);
                for (offset, gain) in [(1560, 11), (1564, 22), (1568, 33), (1572, 44), (1576, 55)] {
                    state.write_interpreter_word(offset, gain);
                }
                state.write_interpreter_word(81220, 0x1234_5678);
                state.write_interpreter_word(81236, 0x8765_4321);
                state.write_interpreter_word(13224, 0);
                state.settings_words.insert(390, 77);
                execute_cmvs390_frame(&mut state, &push_immediate(0x9876_5432)).unwrap();
                let mut expected = state.interpreter_words.clone();
                expected.insert(field_offset, expected_gain);
                let settings_before = state.settings_words.clone();

                assert_eq!(
                    run_native_audio_gain_setting(&mut state, command_id, mode, value),
                    Some(CmvsPs2aVmAction::ApplyNativeAudioGainSetting { command_id }),
                );
                assert_eq!(state.interpreter_words, expected);
                assert_eq!(state.settings_words, settings_before);
                assert_eq!(state.stack_cursor_bytes, 4);
                assert_eq!(read_stack_word_from_top(&state, 4).unwrap(), 0x9876_5432);
            }
        }
    }
}

#[test]
fn native_audio_gain_round_trips_keep_all_five_groups_independent() {
    let mut state = CmvsPs2aVmState::new(0);
    let mut expected_gains = [256; 5];
    for (index, command_id) in (729..=733).enumerate() {
        let gain = 19 + index as u32 * 43;
        let result_before = state.interpreter_words.get(&81220).copied();
        assert_eq!(
            run_native_audio_gain_setting(&mut state, command_id, u32::MAX, gain),
            Some(CmvsPs2aVmAction::ApplyNativeAudioGainSetting { command_id }),
        );
        assert_eq!(state.interpreter_words.get(&81220).copied(), result_before);
        expected_gains[index] = gain;

        for (group, getter) in (729..=733).enumerate() {
            assert_eq!(
                run_native_audio_gain_setting(&mut state, getter, 0, u32::MAX),
                None
            );
            assert_eq!(state.interpreter_words[&81220], expected_gains[group]);
            let offset = 1560 + group as u32 * 4;
            assert_eq!(
                state.interpreter_words.get(&offset).copied(),
                (group <= index).then_some(expected_gains[group]),
            );
            assert_eq!(state.stack_cursor_bytes, 0);
        }
    }

    // Getter copies the stored native word verbatim, including loaded values.
    state.write_interpreter_word(1568, u32::MAX);
    assert_eq!(run_native_audio_gain_setting(&mut state, 731, 0, 0), None);
    assert_eq!(state.interpreter_words[&81220], u32::MAX);
    assert_eq!(state.interpreter_words[&1568], u32::MAX);
}

#[test]
fn native_audio_gain_missing_value_operand_fails_before_mutating_settings() {
    for command_id in 729..=733 {
        for mode in [0, 1, u32::MAX] {
            let mut state = CmvsPs2aVmState::new(0);
            state.write_interpreter_word(1560, 37);
            state.write_interpreter_word(81220, 0x1234_5678);
            execute_cmvs390_frame(&mut state, &push_immediate(mode)).unwrap();
            let words_before = state.interpreter_words.clone();
            let error = execute_cmvs390_frame(
                &mut state,
                &CmvsPs2aInstructionFrame::Command {
                    span: SPAN,
                    command_id,
                },
            )
            .unwrap_err();
            assert_eq!(error.code(), "SHINKI_EMU_CMVS_VM_STACK_UNDERFLOW");
            assert_eq!(state.interpreter_words, words_before);
            assert_eq!(state.stack_cursor_bytes, 4);
        }
    }
}

#[test]
fn emits_crossfade_audio_with_resource_reference_and_fade() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(&mut state, &push_immediate(1)).unwrap();
    execute_cmvs390_frame(&mut state, &push_immediate(7)).unwrap();
    execute_cmvs390_frame(&mut state, &tag_zero_value(19)).unwrap();
    execute_cmvs390_frame(&mut state, &push_current()).unwrap();
    let action = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 160,
        },
    )
    .unwrap();
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::PlayCrossfadeAudio {
            secondary: None,
            primary: CmvsPs2aPrivateStringReference {
                relative_offset: 19,
            },
            fade_ms: 7,
            playback_flag: true,
        })
    );
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn rejects_execution_and_snapshot_after_an_unrecovered_expression() {
    let mut state = CmvsPs2aVmState::new(4);
    let before = state.clone();
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(crate::CmvsPs2aStackExpression {
            span: SPAN,
            nodes: Vec::new(),
        }),
    )
    .unwrap_err();
    assert_eq!(error.code(), "SHINKI_EMU_CMVS_VM_STACK_EXPRESSION");
    assert!(state.execution_failed);
    assert_eq!(
        execute_cmvs390_frame(&mut state, &push_immediate(1))
            .unwrap_err()
            .code(),
        "SHINKI_EMU_CMVS_VM_FAILED"
    );
    assert_eq!(
        validate_cmvs390_vm_state(&state).unwrap_err().code(),
        "SHINKI_EMU_CMVS_VM_FAILED"
    );
    // Restoring a separately validated successful snapshot is still allowed.
    validate_cmvs390_vm_state(&before).unwrap();
    state = before;
    execute_cmvs390_frame(&mut state, &push_immediate(1)).unwrap();
}

#[test]
fn instruction_dispatch_reuses_large_vm_allocations() {
    let mut state = CmvsPs2aVmState::new(0);
    state.stack_bytes = vec![0; MAX_STACK_BYTES];
    state.stack_initialized = vec![false; MAX_STACK_BYTES];
    let allocation = state.stack_bytes.as_ptr();
    for value in 0..1024 {
        execute_cmvs390_frame(&mut state, &push_immediate(value)).unwrap();
        assert_eq!(state.stack_bytes.as_ptr(), allocation);
    }
    assert_eq!(state.stack_cursor_bytes, 4096);
    validate_cmvs390_vm_state(&state).unwrap();
}

#[test]
fn partial_instruction_failure_cannot_resume_by_resetting_dispatch_stop() {
    let mut state = CmvsPs2aVmState::new(u32::MAX - 1);
    assert!(execute_cmvs390_frame(&mut state, &push_immediate(42)).is_err());
    assert_eq!(state.stack_cursor_bytes, 4);
    state.dispatch_stopped = false;
    state.program_counter = 0;
    assert_eq!(
        execute_cmvs390_frame(&mut state, &push_immediate(1))
            .unwrap_err()
            .code(),
        "SHINKI_EMU_CMVS_VM_FAILED"
    );
    assert_eq!(state.stack_cursor_bytes, 4);
}

#[test]
fn dispatch_preconditions_do_not_poison_a_stopped_machine() {
    let mut state = CmvsPs2aVmState::new(0);
    state.dispatch_stopped = true;
    assert_eq!(
        execute_cmvs390_frame(&mut state, &push_immediate(1))
            .unwrap_err()
            .code(),
        "SHINKI_EMU_CMVS_VM_STOPPED"
    );
    state.dispatch_stopped = false;
    execute_cmvs390_frame(&mut state, &push_immediate(1)).unwrap();
    assert_eq!(state.stack_cursor_bytes, 4);
}

#[test]
fn evaluates_the_proven_literal_stack_expression_before_pushing() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(crate::CmvsPs2aStackExpression {
            span: CmvsPs2aSourceSpan {
                offset: 0,
                byte_length: 8,
            },
            nodes: vec![crate::CmvsPs2aStackExpressionNode::Value {
                span: CmvsPs2aSourceSpan {
                    offset: 2,
                    byte_length: 6,
                },
                opcode: 0x100,
                payload: 11,
            }],
        }),
    )
    .unwrap();
    execute_cmvs390_frame(&mut state, &push_current()).unwrap();
    assert_eq!(state.stack_bytes, 11u32.to_le_bytes());
}

#[test]
fn evaluates_the_proven_numeric_float_literal_and_branch_flag() {
    let mut state = CmvsPs2aVmState::new(0);
    let frame = CmvsPs2aInstructionFrame::NumericExpression(crate::CmvsPs2aNumericExpression {
        span: SPAN,
        nodes: vec![crate::CmvsPs2aNumericExpressionNode::Value {
            span: SPAN,
            opcode: 0x12a,
            payload: 1.5_f32.to_bits(),
        }],
    });
    execute_cmvs390_frame(&mut state, &frame).unwrap();
    assert_eq!(state.current_value, Some(1.5_f32.to_bits()));
    assert!(state.condition_flag);
    let zero = CmvsPs2aInstructionFrame::NumericExpression(crate::CmvsPs2aNumericExpression {
        span: SPAN,
        nodes: vec![crate::CmvsPs2aNumericExpressionNode::Value {
            span: SPAN,
            opcode: 0x12a,
            payload: 0.0_f32.to_bits(),
        }],
    });
    execute_cmvs390_frame(&mut state, &zero).unwrap();
    assert!(!state.condition_flag);
}

#[test]
fn assigns_float_literal_through_the_recovered_numeric_variable_target() {
    let mut state = CmvsPs2aVmState::new(0);
    // Mirrors the recovered boundary form: the nested stack expression
    // computes the float-table index, then `floatvar[index] = 1.0`.
    let frame = CmvsPs2aInstructionFrame::NumericExpression(crate::CmvsPs2aNumericExpression {
        span: SPAN,
        nodes: vec![
            crate::CmvsPs2aNumericExpressionNode::Prefix {
                span: SPAN,
                opcode: 0x12b,
                payload: None,
                index: None,
                operands: vec![crate::CmvsPs2aStackExpression {
                    span: SPAN,
                    nodes: vec![crate::CmvsPs2aStackExpressionNode::Value {
                        span: SPAN,
                        opcode: 0x100,
                        payload: 120,
                    }],
                }],
            },
            crate::CmvsPs2aNumericExpressionNode::Value {
                span: SPAN,
                opcode: 0x12a,
                payload: 1.0_f32.to_bits(),
            },
            crate::CmvsPs2aNumericExpressionNode::Operator {
                span: SPAN,
                opcode: 0x170,
            },
        ],
    });
    execute_cmvs390_frame(&mut state, &frame).unwrap();
    assert_eq!(
        state.process_float_words.get(&120),
        Some(&1.0_f32.to_bits())
    );
    assert_eq!(state.current_value, Some(1.0_f32.to_bits()));
    assert!(state.condition_flag);
}

#[test]
fn blocks_numeric_assignment_targets_outside_the_recovered_subset() {
    let mut state = CmvsPs2aVmState::new(0);
    let frame = CmvsPs2aInstructionFrame::NumericExpression(crate::CmvsPs2aNumericExpression {
        span: SPAN,
        nodes: vec![
            crate::CmvsPs2aNumericExpressionNode::Value {
                span: SPAN,
                opcode: 0x12c,
                payload: 4,
            },
            crate::CmvsPs2aNumericExpressionNode::Value {
                span: SPAN,
                opcode: 0x12a,
                payload: 1.0_f32.to_bits(),
            },
            crate::CmvsPs2aNumericExpressionNode::Operator {
                span: SPAN,
                opcode: 0x170,
            },
        ],
    });
    let error = execute_cmvs390_frame(&mut state, &frame).unwrap_err();
    assert_eq!(error.code(), "SHINKI_EMU_CMVS_VM_NUMERIC_EXPRESSION");
}

#[test]
fn assigns_and_reads_frame_local_words_through_the_active_call_base() {
    let mut state = CmvsPs2aVmState::new(0);
    let assign = crate::CmvsPs2aStackExpression {
        span: SPAN,
        nodes: vec![
            crate::CmvsPs2aStackExpressionNode::Value {
                span: SPAN,
                opcode: 0x108,
                payload: 8,
            },
            crate::CmvsPs2aStackExpressionNode::Value {
                span: SPAN,
                opcode: 0x100,
                payload: 7,
            },
            crate::CmvsPs2aStackExpressionNode::Operator {
                span: SPAN,
                opcode: 0x170,
            },
        ],
    };
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(assign),
    )
    .unwrap();
    assert_eq!(&state.stack_bytes[8..12], &7_u32.to_le_bytes());
    assert_eq!(state.current_value, Some(7));
    // Script identity does not create an independent variable buffer;
    // only an intra-script call changes the recovered frame base.
    state.current_frame = 1;
    let read = crate::CmvsPs2aStackExpression {
        span: SPAN,
        nodes: vec![crate::CmvsPs2aStackExpressionNode::Value {
            span: SPAN,
            opcode: 0x108,
            payload: 8,
        }],
    };
    execute_cmvs390_frame(&mut state, &CmvsPs2aInstructionFrame::StackExpression(read)).unwrap();
    assert_eq!(state.current_value, Some(7));
}

#[test]
fn tracks_wider_immediate_stack_slots_without_inventing_gap_values() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 8,
            value: 1,
        }),
    )
    .unwrap();
    assert_eq!(state.stack_bytes.len(), 8);
    assert_eq!(
        state.stack_initialized,
        vec![true, true, true, true, false, false, false, false]
    );
    let error = read_stack_word_from_top(&state, 4).unwrap_err();
    assert_eq!(error.code(), "SHINKI_EMU_CMVS_VM_STACK_UNINITIALIZED");
}

#[test]
fn stores_command_30_word_without_assigning_game_semantics() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 0x1234_5678,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 30,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&10712), Some(&0x1234_5678));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_751_reads_input_modes_without_inventing_gpu_readiness() {
    for (skip, auto, force, expected) in [
        (false, false, 0, 0),
        (true, false, 0, 1),
        (false, true, 0, 1),
        (false, false, 1, 1),
    ] {
        let mut state = CmvsPs2aVmState::new(0);
        state.input_shift_held = skip;
        state.input_control_held = auto;
        state.write_interpreter_word(1464, force);
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: 751,
            },
        )
        .unwrap();
        assert_eq!(state.interpreter_words.get(&81220), Some(&expected));
        assert_eq!(state.stack_cursor_bytes, 0);
    }
}

#[test]
fn command_51_blocks_on_an_unoccupied_script_slot() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 3,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 51,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_flag_words.get(&10532), Some(&0x10));
}

#[test]
fn command_51_reports_a_slot_with_a_decoded_texture() {
    let mut state = CmvsPs2aVmState::new(0);
    state
        .slot_objects
        .insert(slot_object_key(1924, 3).unwrap(), 1);
    state
        .texture_parents
        .insert(3, CmvsTextureParentState::default());
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 3,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 51,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn channel_enable_controls_native_scheduler_and_visibility_without_destroying_owners() {
    let mut state = CmvsPs2aVmState::new(400);
    for word in [720, 1280, 200, 600, 20, 10, 3, 2] {
        push_word(&mut state, word).unwrap();
    }
    assert_eq!(
        run_command_with_action(&mut state, 349),
        Some(CmvsPs2aVmAction::CreateEffectChannel {
            channel: 2,
            effect: 3
        }),
    );
    let channel_identity = state.slot_objects[&(742 << 8 | 2)];
    let text_identity = state.slot_objects[&(750 << 8 | 3)];
    assert_eq!(state.effect_channels[&2].read_dword(2), 0);
    assert_eq!(state.effect_elements[&(2, u32::MAX)].visible, 1);
    assert!(state.native_text[&3].visible);
    state.write_interpreter_word(0x33D4, 1);
    state.write_interpreter_word(0x33DC + 28 * 12, 2);
    state.write_interpreter_word(0x33DC + 28 * 12 + 4, 128);
    let scheduled = |state: &CmvsPs2aVmState| {
        let mut snapshot = state.clone();
        snapshot.program_counter = 400;
        rebuild_frame_entry_queue(&mut snapshot).unwrap();
        (snapshot.current_frame, snapshot.program_counter)
    };
    assert_eq!(scheduled(&state), (0, 400));
    state.effect_channels.get_mut(&2).unwrap().write_dword(4, 9);
    push(&mut state, 7);
    push(&mut state, 2);
    run_command(&mut state, 334);
    assert_eq!(state.effect_channels[&2].read_dword(2), 1);
    assert_eq!(state.effect_channels[&2].read_dword(4), 0);
    assert_eq!(scheduled(&state), (2, 128));
    state.effect_channels.get_mut(&2).unwrap().write_dword(4, 9);
    push(&mut state, 1);
    push(&mut state, 2);
    run_command(&mut state, 334);
    assert_eq!(state.effect_channels[&2].read_dword(4), 9);
    push(&mut state, 2);
    run_command(&mut state, 333);
    assert_eq!(state.interpreter_words[&81220], 1);
    push(&mut state, 0);
    push(&mut state, 2);
    run_command(&mut state, 334);
    assert_eq!(state.effect_channels[&2].read_dword(2), 0);
    assert_eq!(state.effect_channels[&2].read_dword(4), 0);
    assert_eq!(state.effect_elements[&(2, u32::MAX)].visible, 0);
    assert!(!state.native_text[&3].visible);
    assert_eq!(scheduled(&state), (0, 400));
    push(&mut state, 2);
    run_command(&mut state, 333);
    assert_eq!(state.interpreter_words[&81220], 0);
    push(&mut state, 1);
    push(&mut state, 2);
    run_command(&mut state, 334);
    assert_eq!(state.effect_elements[&(2, u32::MAX)].visible, 1);
    assert!(state.native_text[&3].visible);
    assert_eq!(state.slot_objects[&(742 << 8 | 2)], channel_identity);
    assert_eq!(state.slot_objects[&(750 << 8 | 3)], text_identity);
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn a_decoy_channel_pointer_word_cannot_authorize_commands_or_scheduler() {
    let mut state = CmvsPs2aVmState::new(400);
    state.write_interpreter_word((742 + 2) * 4, 1);
    state
        .effect_channels
        .entry(2)
        .or_default()
        .write_dword(2, 1);
    state.write_interpreter_word(0x33D4, 1);
    state.write_interpreter_word(0x33DC + 28 * 12, 2);
    state.write_interpreter_word(0x33DC + 28 * 12 + 4, 128);
    rebuild_frame_entry_queue(&mut state).unwrap();
    assert_eq!((state.current_frame, state.program_counter), (0, 400));
    state.write_interpreter_word(81224, 99);
    push(&mut state, 2);
    run_command(&mut state, 350);
    assert_eq!(state.interpreter_words[&81224], 99);
    assert_ne!(state.interpreter_flag_words[&10532] & 0x10000, 0);
    state.slot_objects.insert(742 << 8 | 2, 1);
    state.interpreter_words.remove(&((742 + 2) * 4));
    push(&mut state, 2);
    run_command(&mut state, 350);
    assert_eq!(state.interpreter_words[&81224], 0);
}

#[test]
fn command_334_blocks_on_an_unoccupied_channel() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 1,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 5,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 334,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_flag_words.get(&10532), Some(&0x0001_0000));
}

#[test]
fn stores_command_29_word_without_assigning_game_semantics() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 0xabcd_1234,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 29,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&1600), Some(&0xabcd_1234));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn stores_command_144_constant_without_assigning_game_semantics() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 144,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&2940), Some(&1));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn registered_coroutine_returns_to_its_caller_with_the_saved_wait_counter() {
    let mut state = CmvsPs2aVmState::new(0);
    state.current_frame = 2;
    state.program_counter = 0x100;
    for value in [9_u32, 8, 7, 0x4320, 5] {
        push_word(&mut state, value).unwrap();
    }
    run_command(&mut state, 136);
    state.current_frame = 0;
    state.program_counter = 0x200;
    state.write_interpreter_word(SCRIPT_FRAME_COUNTER_FIELD, 0x77);
    // The public resume operand masks its label to the registered slot.
    push_word(&mut state, 0x145).unwrap();
    run_command(&mut state, 138);
    assert_eq!((state.current_frame, state.program_counter), (2, 0x4320));
    assert_eq!(state.stack_cursor_bytes, 12);
    run_command(&mut state, 144);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::ScriptReturn {
            span: SPAN,
            stack_bytes: 0,
        }),
    )
    .unwrap();
    assert_eq!((state.current_frame, state.program_counter), (0, 0x202));
    assert_eq!(state.interpreter_words[&SCRIPT_FRAME_COUNTER_FIELD], 0x77);
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn stack_expression_reads_and_writes_the_loaded_data_segment() {
    let mut state = CmvsPs2aVmState::new(0);
    state.script_data_segment_sizes.insert(0, 12);
    let assign = crate::CmvsPs2aStackExpression {
        span: SPAN,
        nodes: vec![
            crate::CmvsPs2aStackExpressionNode::Value {
                span: SPAN,
                opcode: 0x106,
                payload: 4,
            },
            crate::CmvsPs2aStackExpressionNode::Value {
                span: SPAN,
                opcode: 0x100,
                payload: 0x55,
            },
            crate::CmvsPs2aStackExpressionNode::Operator {
                span: SPAN,
                opcode: 0x170,
            },
        ],
    };
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(assign),
    )
    .unwrap();
    assert_eq!(
        state
            .script_data_segment_words
            .get(&0)
            .and_then(|words| words.get(&4)),
        Some(&0x55)
    );
    // Unwritten words read zero through the same tag family.
    let read = crate::CmvsPs2aStackExpression {
        span: SPAN,
        nodes: vec![crate::CmvsPs2aStackExpressionNode::Value {
            span: SPAN,
            opcode: 0x104,
            payload: 8,
        }],
    };
    execute_cmvs390_frame(&mut state, &CmvsPs2aInstructionFrame::StackExpression(read)).unwrap();
    assert_eq!(state.current_value, Some(0));
    // An offset whose dword end exceeds the declared length blocks.
    let out_of_bounds = crate::CmvsPs2aStackExpression {
        span: SPAN,
        nodes: vec![crate::CmvsPs2aStackExpressionNode::Value {
            span: SPAN,
            opcode: 0x107,
            payload: 12,
        }],
    };
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(out_of_bounds.clone()),
    )
    .unwrap_err();
    assert_eq!(error.code(), "SHINKI_EMU_CMVS_VM_DATA_SEGMENT");
    // A frame without a declared segment blocks as well.
    let mut state = CmvsPs2aVmState::new(0);
    state.current_frame = 3;
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(out_of_bounds.clone()),
    )
    .unwrap_err();
    assert_eq!(error.code(), "SHINKI_EMU_CMVS_VM_DATA_SEGMENT");
}

#[test]
fn reload_root_script_invalidates_frame_zero_coroutine_records() {
    let mut state = CmvsPs2aVmState::new(0);
    state
        .frame_string_values
        .insert(0, BTreeMap::from([(0, b"root.ps3".to_vec())]));
    seed_pool_bytes(&mut state, 0, &[(0, b"root.ps3")]);
    for value in [9_u32, 8, 7, 0x4321, 5] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 136,
        },
    )
    .unwrap();
    // Reload invalidates continuations into the replaced root script.
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 0,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 128,
        },
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 5,
        }),
    )
    .unwrap();
    let failure = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 138,
        },
    )
    .unwrap_err();
    assert_eq!(failure.code(), "SHINKI_EMU_CMVS_VM_COROUTINE");
    assert!(state.execution_error().is_some());
}

#[test]
fn body_update_commands_write_their_result_registers() {
    let mut state = CmvsPs2aVmState::new(0);
    let run = |state: &mut CmvsPs2aVmState, id: u16| {
        execute_cmvs390_frame(
            state,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: id,
            },
        )
        .unwrap();
    };
    let push = |state: &mut CmvsPs2aVmState, values: &[u32]| {
        for value in values {
            execute_cmvs390_frame(
                state,
                &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                    span: SPAN,
                    stack_bytes: 4,
                    value: *value,
                }),
            )
            .unwrap();
        }
    };
    // 718 copies the pinned word; 750 canonicalizes it.
    state.write_interpreter_word(11792, 0xabc);
    run(&mut state, 718);
    assert_eq!(state.interpreter_words.get(&81220), Some(&0xabc));
    state.write_interpreter_word(1596, 5);
    run(&mut state, 750);
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    // 741 flag round trip: store then read back.
    push(&mut state, &[1, 1]);
    run(&mut state, 741);
    assert_eq!(state.interpreter_words.get(&1500), Some(&1));
    push(&mut state, &[0, 0]);
    run(&mut state, 741);
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    // 846 publishes input selection; 426 clears the latch pair.
    state.write_interpreter_word(81220, 9);
    run(&mut state, 846);
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
    state.write_interpreter_word(81236, 7);
    run(&mut state, 426);
    assert_eq!(state.interpreter_words.get(&81236), Some(&0));
    // 397 needs the effect channel object; an occupied slot reports
    // canonical zero quad fields.
    state.slot_objects.insert((742_u32 << 8) | 7, 1);
    push(&mut state, &[0xffff_ffff, 7]);
    run(&mut state, 397);
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
    assert_eq!(state.interpreter_words.get(&81252), Some(&0));
}

#[test]
fn effect_quad_visibility_drives_the_case_405_gate() {
    let mut state = CmvsPs2aVmState::new(0);
    state.slot_objects.insert((742_u32 << 8) | 7, 1);
    let run = |state: &mut CmvsPs2aVmState, id: u16| {
        execute_cmvs390_frame(
            state,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: id,
            },
        )
        .unwrap();
    };
    let push = |state: &mut CmvsPs2aVmState, values: &[u32]| {
        for value in values {
            execute_cmvs390_frame(
                state,
                &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                    span: SPAN,
                    stack_bytes: 4,
                    value: *value,
                }),
            )
            .unwrap();
        }
    };
    let quad_active = |state: &mut CmvsPs2aVmState| {
        state.write_interpreter_word(81220, 9);
        push(state, &[0, 7]);
        run(state, 405);
        state.interpreter_words.get(&81220).copied()
    };

    // A never-shown quad reads as inactive: case 405 stores 0.
    assert_eq!(quad_active(&mut state), Some(0));
    // Case 402 shows quad 0, so case 405 stores 1.
    push(&mut state, &[0, 7]);
    run(&mut state, 402);
    assert_eq!(quad_active(&mut state), Some(1));
    // Case 403 hides it again, so case 405 stores 0.
    push(&mut state, &[0, 7]);
    run(&mut state, 403);
    assert_eq!(quad_active(&mut state), Some(0));
    // Changing a frame, including rebinding it, does not show a hidden quad.
    for frame in [1, 1, 2] {
        push(&mut state, &[frame, 0, 7]);
        run(&mut state, 406);
        assert_eq!(quad_active(&mut state), Some(0));
        push(&mut state, &[0, 7]);
        run(&mut state, 402);
        assert_eq!(quad_active(&mut state), Some(1));
        push(&mut state, &[0, 7]);
        run(&mut state, 403);
        assert_eq!(quad_active(&mut state), Some(0));
    }
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn scene_layer_queries_publish_boolean_pairs_and_clears_leave_the_middle_word() {
    let mut state = CmvsPs2aVmState::new(0);
    let run = |state: &mut CmvsPs2aVmState, id: u16| {
        execute_cmvs390_frame(
            state,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: id,
            },
        )
        .unwrap();
    };

    // Case 426 publishes `(scene[298] != 0, scene[299] != 0)`.
    state.scene_words.insert(298, 7);
    run(&mut state, 426);
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    assert_eq!(state.interpreter_words.get(&81236), Some(&0));
    state.scene_words.insert(299, 3);
    run(&mut state, 426);
    assert_eq!(state.interpreter_words.get(&81236), Some(&1));

    // Case 427 clears base 298 and base + 2 (300), keeping 299.
    state.scene_words.insert(300, 9);
    run(&mut state, 427);
    assert_eq!(state.scene_words.get(&298), None);
    assert_eq!(state.scene_words.get(&300), None);
    assert_eq!(state.scene_words.get(&299), Some(&3));

    // Case 424 reads the separate group at base 289.
    state.scene_words.insert(289, 1);
    run(&mut state, 424);
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));

    // Case 432 reads the last group at base 307; case 433 clears 307 and 309.
    state.scene_words.insert(308, 5);
    run(&mut state, 432);
    assert_eq!(state.interpreter_words.get(&81236), Some(&1));
    run(&mut state, 433);
    assert_eq!(state.scene_words.get(&308), Some(&5));
}

#[test]
fn pointer_commands_read_the_streamed_position_and_hit_test_shapes() {
    let mut state = CmvsPs2aVmState::new(0);
    state.pointer_x = 100;
    state.pointer_y = 50;
    // Case 200 publishes the pointer into system registers 1/2.
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 200,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81224), Some(&100));
    assert_eq!(state.interpreter_words.get(&81228), Some(&50));

    let push = |state: &mut CmvsPs2aVmState, values: &[u32]| {
        for value in values {
            execute_cmvs390_frame(
                state,
                &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                    span: SPAN,
                    stack_bytes: 4,
                    value: *value,
                }),
            )
            .unwrap();
        }
    };
    // Case 202: inclusive rectangle (x=10, y=10, w=200, h=100).
    push(&mut state, &[100, 200, 10, 10]);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 202,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    // A point strictly outside the same rectangle misses.
    state.pointer_x = 211;
    push(&mut state, &[100, 200, 10, 10]);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 202,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));

    // Case 203 type 1: half-open rectangle (10,10)-(210,110).
    state.pointer_x = 100;
    push(&mut state, &[0, 0, 110, 210, 10, 10, 1]);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 203,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    // The right edge is exclusive for a GDI rectangle region.
    state.pointer_x = 210;
    push(&mut state, &[0, 0, 110, 210, 10, 10, 1]);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 203,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));

    // Curved regions use the actual GDI raster, including the excluded corner.
    for (shape, x, y, expected) in [
        (2, 100, 50, 1),
        (2, 10, 10, 0),
        (3, 100, 50, 1),
        (3, 10, 10, 0),
    ] {
        state.pointer_x = x;
        state.pointer_y = y;
        push(&mut state, &[40, 40, 110, 210, 10, 10, shape]);
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: 203,
            },
        )
        .unwrap();
        assert_eq!(state.interpreter_words.get(&81220), Some(&expected));
        assert_eq!(state.stack_cursor_bytes, 0);
    }
    // Type 4 reads interleaved process words, not the remaining stack operands.
    // Traversing the square twice distinguishes WINDING from even-odd fill.
    for (index, value) in [
        10, 10, 80, 10, 80, 80, 10, 80, 10, 10, 80, 10, 80, 80, 10, 80,
    ]
    .into_iter()
    .enumerate()
    {
        state
            .process_indexed_words
            .insert(100 + index as u32, value);
    }
    state.pointer_x = 50;
    state.pointer_y = 50;
    push(
        &mut state,
        &[u32::MAX, u32::MAX, u32::MAX, u32::MAX, 8, 100, 4],
    );
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 203,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    assert_eq!(state.stack_cursor_bytes, 0);

    // The native default branch publishes zero and consumes all seven words.
    push(&mut state, &[0, 0, 110, 210, 10, 10, 99]);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 203,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
    assert_eq!(state.stack_cursor_bytes, 0);

    // The native local array has 256 entries. Reject its memory-unsafe overflow
    // without publishing a fabricated miss.
    state.write_interpreter_word(81220, 0x55);
    push(&mut state, &[0, 0, 0, 0, 257, 100, 4]);
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 203,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "SHINKI_EMU_CMVS_VM_POINTER_REGION");
    assert_eq!(state.interpreter_words.get(&81220), Some(&0x55));
}

#[test]
fn command_138_blocks_on_a_missing_or_invalidated_coroutine_record() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [5_u32, 0x4321, 7, 8, 9] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    // No record was ever registered for label 5.
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 5,
        }),
    )
    .unwrap();
    let successful_snapshot = state.clone();
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 138,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "SHINKI_EMU_CMVS_VM_COROUTINE");

    // Case 139 invalidates the record PC; the resume must keep blocking.
    validate_cmvs390_vm_state(&successful_snapshot).unwrap();
    let mut state = successful_snapshot;
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 5,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 136,
        },
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 5,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 139,
        },
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 5,
        }),
    )
    .unwrap();
    let failure = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 138,
        },
    )
    .unwrap_err();
    assert_eq!(failure.code(), "SHINKI_EMU_CMVS_VM_COROUTINE");
    assert!(state.execution_error().is_some());
}

#[test]
fn stores_command_31_process_global_without_assigning_game_semantics() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 0x89ab_cdef,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 31,
        },
    )
    .unwrap();
    assert_eq!(
        state.process_global_words.get(&0x004f_0578),
        Some(&0x89ab_cdef)
    );
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn stores_command_940_as_a_canonical_process_global_boolean() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 7,
        }),
    )
    .unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 940,
        },
    )
    .unwrap();
    assert_eq!(state.process_global_words.get(&0x004f_0580), Some(&1));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn mutates_the_bounded_process_flag_range_from_command_145() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [5, 3, 1] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 145,
        },
    )
    .unwrap();
    assert_eq!(state.process_flag_bits, BTreeSet::from([1, 2, 3]));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn stores_the_bounded_process_indexed_range_from_command_146() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [0xfeed_beef, 3, 5] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 146,
        },
    )
    .unwrap();
    for index in 5..8 {
        assert_eq!(state.process_indexed_words.get(&index), Some(&0xfeed_beef));
    }
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn stores_raw_float_bits_from_command_147() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [1.25_f32.to_bits(), 2, 7] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 147,
        },
    )
    .unwrap();
    assert_eq!(state.process_float_words.get(&7), Some(&1.25_f32.to_bits()));
    assert_eq!(state.process_float_words.get(&8), Some(&1.25_f32.to_bits()));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn copies_native_string_values_from_command_148() {
    let mut state = CmvsPs2aVmState::new(0);
    state
        .frame_string_values
        .insert(0, BTreeMap::from([(11, b"menu.ps3".to_vec())]));
    seed_pool_bytes(&mut state, 0, &[(11, b"menu.ps3")]);
    for value in [11, 2, 3] {
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                span: SPAN,
                stack_bytes: 4,
                value,
            }),
        )
        .unwrap();
    }
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 148,
        },
    )
    .unwrap();
    let reference = b"menu.ps3".to_vec();
    assert_eq!(state.process_string_slots.get(&3), Some(&reference));
    assert_eq!(state.process_string_slots.get(&4), Some(&reference));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn stores_command_21_owned_path_with_symbolic_root() {
    let mut state = CmvsPs2aVmState::new(0);
    state
        .frame_string_values
        .insert(0, BTreeMap::from([(72, b"data/".to_vec())]));
    seed_pool_bytes(&mut state, 0, &[(72, b"data/")]);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 72,
        }),
    )
    .unwrap();
    let action = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 21,
        },
    )
    .unwrap();
    assert_eq!(action, None);
    assert_eq!(
        state.interpreter_string_buffers.get(&3356),
        Some(&vec![
            CmvsStringSegment::InterpreterPrefixField {
                field_offset: 10764
            },
            CmvsStringSegment::OwnedString("data/".into()),
        ])
    );
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_22_emits_a_directory_request_for_its_path_buffer() {
    let mut state = CmvsPs2aVmState::new(0);
    state
        .frame_string_values
        .insert(0, BTreeMap::from([(88, b"save/".to_vec())]));
    seed_pool_bytes(&mut state, 0, &[(88, b"save/")]);
    push(&mut state, 88);
    let action = run_command_with_action(&mut state, 22);
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::EnsurePathDirectories {
            buffer_offset: 5404
        })
    );
    assert_eq!(
        state.interpreter_string_buffers.get(&5404),
        Some(&vec![
            CmvsStringSegment::InterpreterPrefixField { field_offset: 7452 },
            CmvsStringSegment::OwnedString("save/".into()),
        ])
    );
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn blocks_command_21_unrecovered_string_domain() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 0xc000_0010,
        }),
    )
    .unwrap();
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 21,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "SHINKI_EMU_CMVS_VM_STRING_TAG");
    assert!(state.interpreter_string_buffers.is_empty());
    assert_eq!(state.stack_bytes.len(), 4);
}

fn push(state: &mut CmvsPs2aVmState, value: u32) {
    execute_cmvs390_frame(
        state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value,
        }),
    )
    .unwrap();
}

fn run_command(state: &mut CmvsPs2aVmState, command_id: u16) {
    execute_cmvs390_frame(
        state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id,
        },
    )
    .unwrap();
}

fn run_command_with_action(
    state: &mut CmvsPs2aVmState,
    command_id: u16,
) -> Option<CmvsPs2aVmAction> {
    execute_cmvs390_frame(
        state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id,
        },
    )
    .unwrap()
}

#[test]
fn command_32_creates_and_replaces_slot_objects_with_stable_identity() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    run_command(&mut state, 32);
    let key = slot_object_key(1924, 7).unwrap();
    assert_eq!(state.slot_objects.get(&key), Some(&1));
    assert_eq!(
        state.texture_parents.get(&7),
        Some(&CmvsTextureParentState::default())
    );
    assert_eq!(state.texture_children.get(&7), Some(&BTreeMap::new()));
    push(&mut state, 7);
    run_command(&mut state, 32);
    assert_eq!(state.slot_objects.get(&key), Some(&2));
    assert_eq!(state.texture_children.get(&7), Some(&BTreeMap::new()));
    assert_eq!(state.stack_cursor_bytes, 0);
    assert!(state.interpreter_flag_words.is_empty());
}

#[test]
fn command_32_out_of_bounds_slot_only_raises_the_error_flag() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 256);
    run_command(&mut state, 32);
    assert!(state.slot_objects.is_empty());
    assert_eq!(state.interpreter_flag_words.get(&10532), Some(&0x10));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_33_destroys_only_the_requested_slot_object() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 3);
    run_command(&mut state, 32);
    push(&mut state, 4);
    run_command(&mut state, 32);
    push(&mut state, 3);
    run_command(&mut state, 33);
    assert_eq!(state.slot_objects.len(), 1);
    assert_eq!(
        state.slot_objects.get(&slot_object_key(1924, 4).unwrap()),
        Some(&2)
    );
    assert!(!state.texture_children.contains_key(&3));
    assert!(state.texture_children.contains_key(&4));
}

#[test]
fn command_40_moves_the_source_object_and_clears_both_source_semantics() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 5);
    run_command(&mut state, 32);
    // Stack order: source pushed first, destination on top.
    push(&mut state, 5);
    push(&mut state, 9);
    run_command(&mut state, 40);
    assert_eq!(
        state.slot_objects.get(&slot_object_key(1924, 9).unwrap()),
        Some(&1)
    );
    assert!(
        !state
            .slot_objects
            .contains_key(&slot_object_key(1924, 5).unwrap())
    );
    assert!(!state.texture_children.contains_key(&5));
    assert!(state.texture_children.contains_key(&9));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn texture_alpha_is_signed_upper_clamped_and_parent_rebuild_resets_parameters() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    run_command(&mut state, 32);
    push(&mut state, 7);
    run_command(&mut state, 64);
    push(&mut state, 11);
    push(&mut state, 7);
    run_command(&mut state, 34);
    push(&mut state, 11);
    push(&mut state, 7);
    run_command(&mut state, 80);

    for (selector, alpha, expected) in [
        (u32::MAX, 256, 255),
        (u32::MAX - 1, (-7_i32) as u32, -7),
        (11, 700, 255),
        (11, (-9_i32) as u32, -9),
        (11, 0, 0),
    ] {
        for word in [alpha, selector, 7] {
            push(&mut state, word);
        }
        run_command(&mut state, 72);
        let actual = if (selector as i32) < 0 {
            state.texture_parents[&7].alpha
        } else {
            state.texture_children[&7][&11].alpha
        };
        assert_eq!(actual, Some(expected));
        assert_eq!(state.stack_cursor_bytes, 0);
    }
    assert!(state.channel_child_presenters.is_empty());

    for (command, words) in [
        (68, vec![60, 384, 900, 0, u32::MAX, 7]),
        (69, vec![0, (-48_i32) as u32, u32::MAX, 7]),
        (70, vec![20, 10, u32::MAX, 7]),
        (71, vec![3, u32::MAX, 7]),
    ] {
        for word in words {
            push(&mut state, word);
        }
        run_command(&mut state, command);
    }
    for word in [0x1234_5678, 480, 640, u32::MAX, 7] {
        push(&mut state, word);
    }
    run_command(&mut state, 67);
    assert_eq!(state.texture_parents[&7].rect_words, Some([0, 0, 640, 480]));
    assert_eq!(state.texture_parents[&7].solid_color, Some(0xff34_5678));
    assert_eq!(state.texture_parents[&7].alpha, Some(-7));
    assert_eq!(state.texture_parents[&7].position_words, Some([10, 20]));
    assert_eq!(
        state.texture_parents[&7].auxiliary_pair_words,
        Some([(-48_i32) as u32, 0])
    );
    assert_eq!(state.texture_parents[&7].auxiliary_word, Some(3));
    for word in [0x00ab_cdef, 20, 30, 11, 7] {
        push(&mut state, word);
    }
    run_command(&mut state, 67);
    assert_eq!(
        state.texture_children[&7][&11].rect_words,
        Some([0, 0, 30, 20])
    );
    assert_eq!(
        state.texture_children[&7][&11].solid_color,
        Some(0xffab_cdef)
    );
    assert_eq!(state.texture_children[&7][&11].alpha, Some(0));
    assert_eq!(state.stack_cursor_bytes, 0);
    let saved_parent = state.texture_parents[&7].clone();
    let saved_child = state.texture_children[&7][&11].clone();
    let encoded = postcard::to_allocvec(&state).unwrap();
    let mut restored: CmvsPs2aVmState = postcard::from_bytes(&encoded).unwrap();
    assert_eq!(restored.texture_parents[&7], saved_parent);
    assert_eq!(restored.texture_children[&7][&11], saved_child);

    push(&mut restored, 7);
    push(&mut restored, 9);
    run_command(&mut restored, 40);
    assert_eq!(restored.texture_parents[&9], saved_parent);
    assert_eq!(restored.texture_children[&9][&11], saved_child);
    assert!(!restored.texture_parents.contains_key(&7));
    push(&mut restored, 9);
    run_command(&mut restored, 64);
    let parent = &restored.texture_parents[&9];
    assert_eq!(parent.alpha, None);
    assert_eq!(parent.solid_color, None);
    assert_eq!(parent.rect_words, None);
    assert_eq!(parent.position_words, None);
    assert_eq!(parent.auxiliary_pair_words, None);
    assert_eq!(parent.auxiliary_word, None);
    assert_eq!(restored.texture_children[&9][&11], saved_child);
    push(&mut restored, 9);
    run_command(&mut restored, 33);
    assert!(!restored.texture_parents.contains_key(&9));
    assert!(!restored.texture_children.contains_key(&9));
}

#[test]
fn basic_surface_parameter_errors_preserve_native_bounds_and_reject_missing_surfaces() {
    for (command, arguments) in [(68, 4), (69, 2), (70, 2), (71, 1), (72, 1)] {
        for (parent, selector, expected_mask) in [(256, 0, 0x10), (8, 0, 0x10), (7, 1024, 0x20)] {
            let mut state = CmvsPs2aVmState::new(0);
            push(&mut state, 7);
            run_command(&mut state, 32);
            for _ in 0..arguments {
                push(&mut state, 0);
            }
            push(&mut state, selector);
            push(&mut state, parent);
            run_command(&mut state, command);
            assert_eq!(
                state.interpreter_flag_words.get(&10532),
                Some(&expected_mask)
            );
            assert_eq!(state.stack_cursor_bytes, 0);
            assert_eq!(state.texture_parents[&7], CmvsTextureParentState::default());
            assert!(state.channel_child_presenters.is_empty());
        }
        // Negative selectors address the root, but never allocate its basic
        // surface. Existing and absent child objects likewise need command 80.
        for selector in [u32::MAX, 11, 12] {
            let mut state = CmvsPs2aVmState::new(0);
            push(&mut state, 7);
            run_command(&mut state, 32);
            push(&mut state, 11);
            push(&mut state, 7);
            run_command(&mut state, 34);
            for _ in 0..arguments {
                push(&mut state, 0);
            }
            push(&mut state, selector);
            push(&mut state, 7);
            let error = execute_cmvs390_frame(
                &mut state,
                &CmvsPs2aInstructionFrame::Command {
                    span: SPAN,
                    command_id: command,
                },
            )
            .unwrap_err();
            assert_eq!(error.code(), "SHINKI_EMU_CMVS_VM_TEXTURE_SURFACE");
            assert!(!state.texture_parents[&7].surface_initialized);
            assert!(!state.texture_children[&7][&11].surface_initialized);
            assert!(state.channel_child_presenters.is_empty());
        }
    }
}

#[test]
fn solid_surface_missing_parent_raises_native_flag_but_missing_basic_is_an_error() {
    for parent in [7, 256] {
        let mut state = CmvsPs2aVmState::new(0);
        for word in [0, 480, 640, u32::MAX, parent] {
            push(&mut state, word);
        }
        run_command(&mut state, 67);
        assert_eq!(state.interpreter_flag_words.get(&10532), Some(&0x10));
        assert!(state.texture_parents.is_empty());
        assert_eq!(state.stack_cursor_bytes, 0);
    }
    for selector in [u32::MAX, 11, 1024, 65547] {
        let mut state = CmvsPs2aVmState::new(0);
        push(&mut state, 7);
        run_command(&mut state, 32);
        push(&mut state, 11);
        push(&mut state, 7);
        run_command(&mut state, 34);
        for word in [0, 480, 640, selector, 7] {
            push(&mut state, word);
        }
        let error = execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: 67,
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), "SHINKI_EMU_CMVS_VM_TEXTURE_SURFACE");
        assert!(state.interpreter_flag_words.is_empty());
        assert_eq!(state.texture_parents[&7].solid_color, None);
        assert_eq!(state.texture_children[&7][&11].solid_color, None);
    }
}

#[test]
fn basic_surface_commands_keep_quad_presenter_payload_queries_independent() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    run_command(&mut state, 32);
    push(&mut state, 11);
    push(&mut state, 7);
    run_command(&mut state, 34);
    push(&mut state, 11);
    push(&mut state, 7);
    run_command(&mut state, 80);
    push(&mut state, 11);
    push(&mut state, 7);
    run_command(&mut state, 500);
    for word in [28, 27, 26, 25, 24, 0, 11, 7] {
        push(&mut state, word);
    }
    run_command(&mut state, 501);
    for word in [99, 11, 7] {
        push(&mut state, word);
    }
    run_command(&mut state, 502);
    for word in [0, 11, 7] {
        push(&mut state, word);
    }
    run_command(&mut state, 72);
    for word in [0, 0, 11, 7] {
        push(&mut state, word);
    }
    run_command(&mut state, 52);
    assert_eq!(state.interpreter_words.get(&81220), Some(&28));
    assert_eq!(
        state.channel_child_presenters[&(7, 11)].quad,
        Some([1, 0, 0, 99])
    );
    assert_eq!(state.texture_children[&7][&11].alpha, Some(0));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn texture_child_bounds_follow_parent_error_and_child_noop_contracts() {
    let mut missing_parent = CmvsPs2aVmState::new(0);
    push(&mut missing_parent, 4);
    push(&mut missing_parent, 2);
    run_command(&mut missing_parent, 34);
    assert_eq!(
        missing_parent.interpreter_flag_words.get(&10532),
        Some(&0x10)
    );

    let mut out_of_range_child = CmvsPs2aVmState::new(0);
    push(&mut out_of_range_child, 2);
    run_command(&mut out_of_range_child, 32);
    push(&mut out_of_range_child, 1024);
    push(&mut out_of_range_child, 2);
    run_command(&mut out_of_range_child, 34);
    assert!(out_of_range_child.texture_children[&2].is_empty());
    assert!(out_of_range_child.interpreter_flag_words.is_empty());
}

#[test]
fn commands_493_and_494_validate_the_container_and_publish_the_idle_state() {
    let mut state = CmvsPs2aVmState::new(0);
    // A dead container slot raises 0x10 at byte offset 10532.
    push(&mut state, 0);
    push(&mut state, 7);
    run_command(&mut state, 494);
    assert_eq!(state.interpreter_flag_words.get(&10532), Some(&0x10));
    assert_eq!(state.interpreter_words.get(&81220), None);

    push(&mut state, 7);
    run_command(&mut state, 32);
    // A child selector at or above 0x400 raises 0x20.
    push(&mut state, 1024);
    push(&mut state, 7);
    run_command(&mut state, 493);
    assert_eq!(
        state.interpreter_flag_words.get(&10532),
        Some(&(0x10 | 0x20))
    );

    // The live query publishes the channel's native idle action state -1:
    // the recovered renderer attaches no pending loader actions.
    push(&mut state, 3);
    push(&mut state, 7);
    run_command(&mut state, 494);
    assert_eq!(state.interpreter_words.get(&81220), Some(&(-1_i32 as u32)));
}

#[test]
fn command_47_clears_all_top_level_texture_containers_and_children() {
    let mut state = CmvsPs2aVmState::new(0);
    for parent in [2, 7] {
        push(&mut state, parent);
        run_command(&mut state, 32);
        push(&mut state, 11);
        push(&mut state, parent);
        run_command(&mut state, 34);
    }
    run_command(&mut state, 47);
    assert!(state.texture_children.is_empty());
    assert!(!state.slot_objects.keys().any(|key| (*key >> 16) == 1924));
}

#[test]
fn command_143_requests_a_file_icon_without_misclassifying_it_as_title() {
    let mut state = CmvsPs2aVmState::new(0);
    state.process_string_slots.insert(2, b"game.ico".to_vec());
    push(&mut state, 0x8000_0002);
    let action = run_command_with_action(&mut state, 143);
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::SetWindowIcon {
            segments: vec![
                CmvsStringSegment::InterpreterPrefixField {
                    field_offset: 10764
                },
                CmvsStringSegment::OwnedString("game.ico".into()),
            ]
        })
    );
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_352_owns_its_title_and_preserves_empty_native_titles() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [b"title".to_vec(), Vec::new()] {
        state.process_string_slots.insert(2, value.clone());
        push(&mut state, 0x8000_0002);
        let action = run_command_with_action(&mut state, 352);
        state.process_string_slots.insert(2, b"changed".to_vec());
        let segments = vec![CmvsStringSegment::OwnedString(
            String::from_utf8(value).unwrap(),
        )];
        assert_eq!(
            action,
            Some(CmvsPs2aVmAction::SetWindowCaption {
                segments: segments.clone()
            })
        );
        assert_eq!(state.interpreter_string_buffers.get(&288), Some(&segments));
    }
}

#[test]
fn command_715_stores_the_component_word_from_the_stack() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 0x2026);
    run_command(&mut state, 715);
    assert_eq!(
        state.component_words.get(&component_field_key(1920, 76)),
        Some(&0x2026)
    );
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_848_resets_the_settings_object_before_storing() {
    let mut state = CmvsPs2aVmState::new(0);
    state.settings_words.insert(48, 999);
    push(&mut state, 1);
    run_command(&mut state, 848);
    assert_eq!(state.settings_words.get(&0), Some(&1));
    assert_eq!(state.settings_words.get(&48), Some(&0));
    assert_eq!(state.settings_words.get(&44), Some(&u32::MAX));
    assert_eq!(state.settings_words.get(&172), Some(&u32::MAX));
}

#[test]
fn command_849_stores_both_booleans_after_the_reset() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 5);
    push(&mut state, 0);
    run_command(&mut state, 849);
    assert_eq!(state.settings_words.get(&24), Some(&0));
    assert_eq!(state.settings_words.get(&28), Some(&1));
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn command_858_and_859_respect_their_recovered_bounds() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 91);
    run_command(&mut state, 858);
    assert!(!state.settings_words.contains_key(&36));
    push(&mut state, 90);
    run_command(&mut state, 858);
    assert_eq!(state.settings_words.get(&36), Some(&90));
    push(&mut state, u32::MAX); // -1 signed
    run_command(&mut state, 859);
    assert!(!state.settings_words.contains_key(&32));
    push(&mut state, 3);
    run_command(&mut state, 859);
    assert_eq!(state.settings_words.get(&32), Some(&3));
}

#[test]
fn command_136_records_the_fixed_session_clock() {
    let mut state = CmvsPs2aVmState::new(0);
    state.current_frame = 3;
    state.session_clock_millis = 1_234;
    push(&mut state, 50);
    push(&mut state, 40);
    push(&mut state, 30);
    push(&mut state, 20);
    push(&mut state, 2);
    run_command(&mut state, 136);
    let base = 13_276 + 28 * 2;
    // The frame word (+0) snapshots the frame register; the pc word (+4)
    // takes the second stack operand; operands fill +8..+16 and the host
    // clock lands at +20.
    assert_eq!(state.interpreter_words.get(&base), Some(&3));
    assert_eq!(state.interpreter_words.get(&(base + 4)), Some(&20));
    assert_eq!(state.interpreter_words.get(&(base + 8)), Some(&30));
    assert_eq!(state.interpreter_words.get(&(base + 12)), Some(&40));
    assert_eq!(state.interpreter_words.get(&(base + 16)), Some(&50));
    assert_eq!(state.interpreter_words.get(&(base + 20)), Some(&1_234));
}

#[test]
fn command_869_copies_the_complete_settings_snapshot() {
    let mut state = CmvsPs2aVmState::new(0);
    for (offset, value) in [(100, 1), (104, 2), (124, 3), (128, 4), (176, 5)] {
        state.settings_words.insert(offset, value);
    }
    state.settings_words.insert(120, 1);
    run_command(&mut state, 869);
    assert_eq!(state.interpreter_words.get(&81236), Some(&1));
    assert_eq!(state.interpreter_words.get(&81240), Some(&2));
    assert_eq!(state.interpreter_words.get(&81224), Some(&3));
    assert_eq!(state.interpreter_words.get(&81228), Some(&4));
    assert_eq!(state.interpreter_words.get(&81244), Some(&5));
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));

    state.settings_words.insert(120, 0);
    state.settings_words.insert(176, 9);
    state.write_interpreter_word(81236, 77);
    run_command(&mut state, 869);
    assert_eq!(state.interpreter_words.get(&81236), Some(&77));
    assert_eq!(state.interpreter_words.get(&81244), Some(&9));
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
}

#[test]
fn commands_528_530_and_531_manage_bounded_filter_chain_records() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    push(&mut state, 2);
    run_command(&mut state, 528);
    assert_eq!(state.filter_chain_channels.get(&2), Some(&7));

    push(&mut state, 44);
    push(&mut state, 2);
    run_command(&mut state, 530);
    assert_eq!(
        state
            .filter_chain_records
            .get(&2)
            .and_then(|records| records.get(&44))
            .map(|record| record.words),
        Some([0; 4])
    );
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));

    push(&mut state, 4);
    push(&mut state, 3);
    push(&mut state, 2);
    push(&mut state, 1);
    push(&mut state, 44);
    push(&mut state, 2);
    run_command(&mut state, 531);
    assert_eq!(
        state
            .filter_chain_records
            .get(&2)
            .and_then(|records| records.get(&44))
            .map(|record| record.words),
        Some([1, 2, 3, 4])
    );

    push(&mut state, 8);
    push(&mut state, 7);
    push(&mut state, 6);
    push(&mut state, 5);
    push(&mut state, 99);
    push(&mut state, 2);
    run_command(&mut state, 531);
    assert!(
        !state
            .filter_chain_records
            .get(&2)
            .is_some_and(|records| records.contains_key(&99))
    );

    push(&mut state, 4);
    push(&mut state, 3);
    push(&mut state, 2);
    push(&mut state, 1);
    push(&mut state, 44);
    push(&mut state, 6);
    run_command(&mut state, 531);
    assert_eq!(state.interpreter_words.get(&10532), Some(&0x100000));
}

#[test]
fn command_529_destroys_the_complete_filter_chain_bank() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    push(&mut state, 2);
    run_command(&mut state, 528);
    push(&mut state, 44);
    push(&mut state, 2);
    run_command(&mut state, 530);
    state.filter_chain_active_records.insert(2, 44);
    state.filter_chain_apply_revisions.insert(2, 3);

    push(&mut state, 2);
    run_command(&mut state, 529);
    assert!(
        !state
            .slot_objects
            .contains_key(&slot_object_key(3196, 2).unwrap())
    );
    assert!(!state.filter_chain_records.contains_key(&2));
    assert!(!state.filter_chain_record_order.contains_key(&2));
    assert!(!state.filter_chain_active_records.contains_key(&2));
    assert!(!state.filter_chain_channels.contains_key(&2));
    assert!(!state.filter_chain_apply_revisions.contains_key(&2));

    push(&mut state, 6);
    run_command(&mut state, 529);
    assert_eq!(state.interpreter_words.get(&10532), Some(&0x100000));
}

#[test]
fn command_532_uses_the_recovered_backend_parameter_layout() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    push(&mut state, 2);
    run_command(&mut state, 528);
    push(&mut state, 44);
    push(&mut state, 2);
    run_command(&mut state, 530);

    for value in [6, 5, 4, 3, 2, 1, 99, 1, 44, 2] {
        push(&mut state, value);
    }
    run_command(&mut state, 532);
    let block = CmvsFilterChainParameterBlock {
        word: 99,
        short_words: [1, 2, 3, 4, 5, 6],
    };
    let record = state
        .filter_chain_records
        .get(&2)
        .and_then(|records| records.get(&44))
        .unwrap();
    assert_eq!(record.parameter_blocks.get(&1), Some(&block));
    assert!(!record.parameter_blocks.contains_key(&2));

    state.process_global_words.insert(0x004f_057c, 1);
    for value in [16, 15, 14, 13, 12, 11, 199, 1, 44, 2] {
        push(&mut state, value);
    }
    run_command(&mut state, 532);
    let mirrored = CmvsFilterChainParameterBlock {
        word: 199,
        short_words: [11, 12, 13, 14, 15, 16],
    };
    let record = state
        .filter_chain_records
        .get(&2)
        .and_then(|records| records.get(&44))
        .unwrap();
    assert_eq!(record.parameter_blocks.get(&1), Some(&mirrored));
    assert_eq!(record.parameter_blocks.get(&2), Some(&mirrored));
}

#[test]
fn command_533_applies_only_a_live_filter_chain_bank() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    push(&mut state, 2);
    run_command(&mut state, 528);

    push(&mut state, 2);
    run_command(&mut state, 533);
    push(&mut state, 2);
    run_command(&mut state, 533);
    assert_eq!(state.filter_chain_apply_revisions.get(&2), Some(&2));

    push(&mut state, 6);
    run_command(&mut state, 533);
    assert_eq!(state.interpreter_words.get(&10532), Some(&0x100000));
    assert!(!state.filter_chain_apply_revisions.contains_key(&6));
}

#[test]
fn command_535_polls_the_input_snapshot_and_preserves_insertion_order() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    push(&mut state, 2);
    run_command(&mut state, 528);
    for record in [44, 11, 29] {
        push(&mut state, record);
        push(&mut state, 2);
        run_command(&mut state, 530);
    }
    push(&mut state, 11);
    push(&mut state, 2);
    run_command(&mut state, 530);
    assert_eq!(
        state.filter_chain_record_order.get(&2),
        Some(&vec![44, 11, 29])
    );

    push(&mut state, 2);
    assert_eq!(run_command_with_action(&mut state, 535), None);
    assert_eq!(state.interpreter_words.get(&81220), Some(&u32::MAX));

    // A down edge moves the highlight to the first record in insertion
    // order; a later up edge walks back to the newest record.
    state.input_down_edge = true;
    push(&mut state, 2);
    run_command(&mut state, 535);
    state.input_down_edge = false;
    assert_eq!(state.filter_chain_active_records.get(&2), Some(&44));
    state.input_up_edge = true;
    push(&mut state, 2);
    run_command(&mut state, 535);
    state.input_up_edge = false;
    assert_eq!(state.filter_chain_active_records.get(&2), Some(&29));
    // The native lock-then-confirm contract (`Menu_pump` field 12): the
    // press edge latches the hovered record and the poll reports the idle
    // -1; the release edge confirms the lock and reports the record id.
    // The poll's hover pass runs first, so record 11 gets a rectangle and
    // the pointer rests on it — the native click confirms the record under
    // the cursor.
    push(&mut state, 40);
    push(&mut state, 100);
    push(&mut state, 200);
    push(&mut state, 100);
    push(&mut state, 11);
    push(&mut state, 2);
    run_command(&mut state, 531);
    state.pointer_x = 150;
    state.pointer_y = 220;
    state.input_advance_press = true;
    push(&mut state, 2);
    run_command(&mut state, 535);
    assert_eq!(state.interpreter_words.get(&81220), Some(&u32::MAX));
    assert_eq!(state.filter_chain_locked_records.get(&2), Some(&11));
    assert!(state.input_advance_press);
    state.input_advance_press = false;
    state.input_advance_release = true;
    push(&mut state, 2);
    run_command(&mut state, 535);
    state.input_advance_release = false;
    assert_eq!(state.interpreter_words.get(&81220), Some(&11));
    assert!(!state.filter_chain_locked_records.contains_key(&2));
    // A release without a matching lock reports -10 and consumes both
    // advance latches (`sub_45CB80`).
    state.input_advance_release = true;
    push(&mut state, 2);
    run_command(&mut state, 535);
    assert_eq!(
        state.interpreter_words.get(&81220),
        Some(&((-10_i32) as u32))
    );
    assert!(!state.input_advance_release);
    assert!(!state.input_advance_press);

    // The native pointer hover pass (`sub_468740`): a record whose
    // `cmd 531` rectangle contains the pointer becomes active without any
    // click — first hit in registration order wins — while a pointer off
    // every rectangle clears the active record. The rectangle is
    // half-open on the x+w and y+h edges.
    let mut hover = CmvsPs2aVmState::new(0);
    push(&mut hover, 7);
    push(&mut hover, 2);
    run_command(&mut hover, 528);
    for record in [44, 11, 29] {
        push(&mut hover, record);
        push(&mut hover, 2);
        run_command(&mut hover, 530);
    }
    // Record 44 covers (100,100)-(200,140), 11 covers (100,200)-(200,240)
    // and 29 covers (100,300)-(200,340): words are x, y, w, h.
    for (record, y) in [(44, 100), (11, 200), (29, 300)] {
        push(&mut hover, 40);
        push(&mut hover, 100);
        push(&mut hover, y);
        push(&mut hover, 100);
        push(&mut hover, record);
        push(&mut hover, 2);
        run_command(&mut hover, 531);
    }
    hover.pointer_x = 150;
    hover.pointer_y = 220;
    push(&mut hover, 2);
    run_command(&mut hover, 535);
    assert_eq!(hover.filter_chain_active_records.get(&2), Some(&11));

    // The right edge is exclusive, and a miss clears the active record.
    hover.pointer_x = 200;
    push(&mut hover, 2);
    run_command(&mut hover, 535);
    assert!(!hover.filter_chain_active_records.contains_key(&2));
    // The first hit in registration order wins.
    hover.pointer_x = 150;
    hover.pointer_y = 120;
    push(&mut hover, 2);
    run_command(&mut hover, 535);
    assert_eq!(hover.filter_chain_active_records.get(&2), Some(&44));
    // Off every rectangle the poll reports the idle -1.
    hover.pointer_x = 3000;
    push(&mut hover, 2);
    run_command(&mut hover, 535);
    assert_eq!(hover.interpreter_words.get(&81220), Some(&u32::MAX));

    let mut missing = CmvsPs2aVmState::new(0);
    push(&mut missing, 6);
    assert_eq!(run_command_with_action(&mut missing, 535), None);
    assert_eq!(missing.interpreter_words.get(&10532), Some(&0x100000));
}

#[test]
fn script_return_reads_stale_words_below_a_shallow_cursor() {
    // Native 0x414 never bounds-checks: with fewer than three words live it
    // still pops three, reading the retained bytes below the cursor. With
    // two live words [w1, w2] the frame adopts w2 and the PC becomes w1;
    // the missing third word reads as zero.
    let mut state = CmvsPs2aVmState::new(400);
    state.current_frame = 0;
    state.program_counter = 21_642;
    push_word(&mut state, 4_000).unwrap();
    push_word(&mut state, 9).unwrap();
    let ret = CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::ScriptReturn {
        span: SPAN,
        stack_bytes: 0,
    });
    execute_cmvs390_frame(&mut state, &ret).unwrap();
    assert_eq!(state.stack_cursor_bytes, 0);
    // frame = the top word (9), pc = the second word (4000), counter = 0.
    assert_eq!(state.program_counter, 4_000);
    assert_eq!(state.current_frame, 9);
    assert_eq!(state.interpreter_words[&SCRIPT_FRAME_COUNTER_FIELD], 0);
}

#[test]
fn command_538_commits_by_key_and_warps_the_pointer() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 7);
    push(&mut state, 2);
    run_command(&mut state, 528);
    push(&mut state, 44);
    push(&mut state, 2);
    run_command(&mut state, 530);
    push(&mut state, 40);
    push(&mut state, 100);
    push(&mut state, 100);
    push(&mut state, 100);
    push(&mut state, 44);
    push(&mut state, 2);
    run_command(&mut state, 531);

    // `sub_485F50` → `sub_468390`: the pushed key selects the record; the
    // warp flag moves the pointer onto the record's rectangle center so
    // the following poll's hover pass keeps the committed record active.
    state.pointer_x = 3000;
    state.pointer_y = 3000;
    push(&mut state, 1);
    push(&mut state, 44);
    push(&mut state, 2);
    assert_eq!(run_command_with_action(&mut state, 538), None);
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    assert_eq!(state.filter_chain_active_records.get(&2), Some(&44));
    assert_eq!(state.pointer_x, 150);
    assert_eq!(state.pointer_y, 120);
    push(&mut state, 2);
    run_command(&mut state, 535);
    assert_eq!(state.filter_chain_active_records.get(&2), Some(&44));

    // An unknown key reports 0 and commits nothing.
    push(&mut state, 0);
    push(&mut state, 777);
    push(&mut state, 2);
    run_command(&mut state, 538);
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
    assert_eq!(state.filter_chain_active_records.get(&2), Some(&44));
}

#[test]
fn command_539_installs_a_single_hotspot_record_from_the_channel_quad() {
    let mut state = CmvsPs2aVmState::new(0);
    // The channel's quad 0 at origin (10, 20) covers (110, 120)-(210, 160).
    let object = state.effect_channels.entry(3).or_default();
    object.set_origin(10, 20);
    object.write_quad_rect(0, [100, 100, 100, 40]);

    push(&mut state, 0);
    push(&mut state, 0);
    push(&mut state, 3);
    push(&mut state, 4);
    assert_eq!(run_command_with_action(&mut state, 539), None);
    assert_eq!(state.filter_chain_record_order.get(&4), Some(&vec![0_u32]));
    let record = state
        .filter_chain_records
        .get(&4)
        .and_then(|records| records.get(&0))
        .unwrap();
    assert_eq!(record.words, [110, 120, 100, 40]);
    // The hotspot installs with nothing hovered; process word 2212 (the
    // layout-completion callback) rises with the install.
    assert!(!state.filter_chain_active_records.contains_key(&4));
    assert_eq!(state.process_indexed_words.get(&2212), Some(&1));

    // A poll over the hotspot confirms key 0 only when the pointer rests
    // inside the channel quad's rectangle.
    state.pointer_x = 160;
    state.pointer_y = 140;
    push(&mut state, 4);
    run_command(&mut state, 535);
    assert_eq!(state.filter_chain_active_records.get(&4), Some(&0));
    state.input_advance_press = true;
    push(&mut state, 4);
    run_command(&mut state, 535);
    state.input_advance_press = false;
    state.input_advance_release = true;
    push(&mut state, 4);
    run_command(&mut state, 535);
    state.input_advance_release = false;
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
}

#[test]
fn command_548_first_poll_publishes_one_and_suspends_the_frame() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [40, 30, 20, 1] {
        push(&mut state, value);
    }
    run_command(&mut state, 544);
    let stack_before = state.stack_cursor_bytes;
    push(&mut state, 1);
    let pc_before = state.program_counter;
    assert_eq!(run_command_with_action(&mut state, 548), None);
    // A live owner suspends dispatch without advancing the command or
    // consuming the operand needed by the next poll.
    assert!(state.dispatch_stopped);
    assert_eq!(state.stack_cursor_bytes, stack_before + 4);
    assert_eq!(state.program_counter, pc_before);
    assert_eq!(read_stack_word_from_top(&state, 4).unwrap(), 1);
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
}

#[test]
fn command_548_second_poll_tears_the_owner_down_and_passes() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [40, 30, 20, 1] {
        push(&mut state, value);
    }
    run_command(&mut state, 544);
    push(&mut state, 1);
    let _ = run_command_with_action(&mut state, 548);
    // The frame ended suspended; the next tick re-executes the same poll.
    state.dispatch_stopped = false;
    // The re-executed poll runs `sub_47A660`'s teardown: the owner handle
    // clears, the result keeps its one and the operand pops like the
    // native 0x4004 continuation.
    let stack_before = state.stack_cursor_bytes;
    assert_eq!(run_command_with_action(&mut state, 548), None);
    assert!(!state.wait_frame);
    assert_eq!(state.stack_cursor_bytes, stack_before - 4);
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    assert!(!state.slot_objects.contains_key(&(3252_u32 << 8)));
    assert_eq!(state.interpreter_words.get(&3252), Some(&0));
}

#[test]
fn wait_repolls_preserve_operands_and_script_calls_save_the_poll_count() {
    for polls in [1_u32, 2] {
        let mut state = CmvsPs2aVmState::new(100);
        seed_pool_bytes(&mut state, 0, &[(0, b"callee.ps3")]);
        push_word(&mut state, 0xCAFE).unwrap();
        push_word(&mut state, 0).unwrap();
        let operands = state.stack_bytes.clone();
        for _ in 0..polls {
            run_command(&mut state, 563);
            assert!(state.dispatch_stopped);
            assert_eq!(state.program_counter, 100);
            assert_eq!(state.stack_cursor_bytes, 8);
            assert_eq!(state.stack_bytes, operands);
            state.dispatch_stopped = false;
        }
        assert_eq!(pop_word(&mut state).unwrap(), 0);
        push_word(&mut state, 0).unwrap();
        push_word(&mut state, 0).unwrap();
        assert_eq!(
            run_command_with_action(&mut state, 129),
            Some(CmvsPs2aVmAction::CallScript {
                frame: 1,
                name: "callee.ps3".into(),
            })
        );
        // The call saves the native consecutive-wait count before its
        // own completion clears the destination's live counter.
        assert_eq!(pop_word(&mut state).unwrap(), 0);
        assert_eq!(pop_word(&mut state).unwrap(), 102);
        assert_eq!(pop_word(&mut state).unwrap(), polls);
        assert_eq!(pop_word(&mut state).unwrap(), 0xCAFE);
    }
}

#[test]
fn command_completion_clears_the_counter_before_the_next_script_call() {
    for command_id in [144, 563, 0] {
        let mut state = CmvsPs2aVmState::new(100);
        seed_pool_bytes(&mut state, 0, &[(0, b"callee.ps3")]);
        state.write_interpreter_word(SCRIPT_FRAME_COUNTER_FIELD, 17);
        if command_id == 563 {
            push_word(&mut state, 1).unwrap();
            state.input_shift_held = true;
        }
        run_command(&mut state, command_id);
        assert_eq!(state.program_counter, 102);
        assert_eq!(state.stack_cursor_bytes, 0);
        if command_id != 144 {
            assert!(state.dispatch_stopped);
        }
        if command_id == 563 {
            assert_eq!(state.interpreter_words.get(&81220), Some(&1));
        }
        state.dispatch_stopped = false;
        push_word(&mut state, 0).unwrap();
        push_word(&mut state, 0).unwrap();
        run_command(&mut state, 129);
        assert_eq!(pop_word(&mut state).unwrap(), 0);
        assert_eq!(pop_word(&mut state).unwrap(), 104);
        assert_eq!(pop_word(&mut state).unwrap(), 0);
    }
}

#[test]
fn command_548_zero_control_takes_the_same_two_poll_cadence() {
    let mut state = CmvsPs2aVmState::new(0);
    for value in [40, 30, 20, 1] {
        push(&mut state, value);
    }
    run_command(&mut state, 544);
    push(&mut state, 0);
    assert_eq!(run_command_with_action(&mut state, 548), None);
    assert!(state.dispatch_stopped);
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
}

#[test]
fn command_548_without_an_owner_handle_pops_and_continues() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 1);
    let stack_before = state.stack_cursor_bytes;
    assert_eq!(run_command_with_action(&mut state, 548), None);
    assert!(!state.wait_frame);
    assert!(!state.dispatch_stopped);
    assert_eq!(state.stack_cursor_bytes, stack_before - 4);
    assert_eq!(state.interpreter_words.get(&81220), Some(&0));
}

#[test]
fn command_692_updates_native_save_permission_and_query() {
    let mut state = CmvsPs2aVmState::new(0);
    assert!(!state.native_save_enabled());
    run_command(&mut state, 750);
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    for (value, expected) in [(0, 0), (1, 1), (u32::MAX, 1), (0, 0)] {
        push(&mut state, value);
        assert_eq!(run_command_with_action(&mut state, 692), None);
        assert_eq!(state.interpreter_words.get(&1596), Some(&expected));
        assert_eq!(state.interpreter_words.get(&15064), Some(&expected));
        assert_eq!(state.native_save_enabled(), expected != 0);
        run_command(&mut state, 750);
        assert_eq!(state.interpreter_words.get(&81220), Some(&expected));
        let encoded = postcard::to_allocvec(&state).unwrap();
        assert_eq!(
            postcard::from_bytes::<CmvsPs2aVmState>(&encoded).unwrap(),
            state
        );
    }
    state.write_interpreter_word(15064, 1);
    assert!(!state.native_save_enabled());
}

fn stack_expression(nodes: Vec<crate::CmvsPs2aStackExpressionNode>) -> CmvsPs2aInstructionFrame {
    CmvsPs2aInstructionFrame::StackExpression(crate::CmvsPs2aStackExpression { span: SPAN, nodes })
}

fn literal_node(payload: u32) -> crate::CmvsPs2aStackExpressionNode {
    crate::CmvsPs2aStackExpressionNode::Value {
        span: SPAN,
        opcode: 0x100,
        payload,
    }
}

#[test]
fn stack_expression_literal_sets_value_result_slot_and_flag() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(&mut state, &stack_expression(vec![literal_node(9)])).unwrap();
    assert_eq!(state.current_value, Some(9));
    assert!(state.condition_flag);
    assert_eq!(state.process_indexed_words.get(&4096), Some(&9));
}

#[test]
fn stack_expression_assignment_writes_the_process_indexed_table() {
    let mut state = CmvsPs2aVmState::new(0);
    // [0x101, 0x100, 0x100, 0x170]: process_var[5] := 77
    let expression = stack_expression(vec![
        crate::CmvsPs2aStackExpressionNode::Prefix {
            span: SPAN,
            opcode: 0x101,
            payload: None,
            index: None,
            operands: vec![crate::CmvsPs2aStackExpression {
                span: SPAN,
                nodes: vec![literal_node(5)],
            }],
        },
        literal_node(77),
        crate::CmvsPs2aStackExpressionNode::Operator {
            span: SPAN,
            opcode: 0x170,
        },
    ]);
    execute_cmvs390_frame(&mut state, &expression).unwrap();
    assert_eq!(state.process_indexed_words.get(&5), Some(&77));
    assert_eq!(state.current_value, Some(77));
    assert_eq!(state.process_indexed_words.get(&4096), Some(&77));
    assert!(state.condition_flag);
}

#[test]
fn stack_expression_assignment_to_system_registers_matches_original_bounds() {
    // Tag 0x10f targets write interpreter system registers 0..=6 and
    // silently ignore higher indices, matching the original store.
    let mut state = CmvsPs2aVmState::new(0);
    store_stack_expression_target(
        &mut state,
        super::StackExpressionEntry {
            tag: 0x10f,
            value: 2,
        },
        33,
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81228), Some(&33));
    store_stack_expression_target(
        &mut state,
        super::StackExpressionEntry {
            tag: 0x10f,
            value: 9,
        },
        44,
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81256), None);
}

#[test]
fn stack_expression_unrecovered_operator_stays_fail_closed() {
    let mut state = CmvsPs2aVmState::new(0);
    let expression = stack_expression(vec![
        literal_node(1),
        literal_node(2),
        crate::CmvsPs2aStackExpressionNode::Operator {
            span: SPAN,
            opcode: 0x173,
        },
    ]);
    let error = execute_cmvs390_frame(&mut state, &expression).unwrap_err();
    assert_eq!(error.code(), "SHINKI_EMU_CMVS_VM_STACK_EXPRESSION");
    assert!(state.current_value.is_none());
    assert!(state.process_indexed_words.is_empty());
}

#[test]
fn command_296_blocks_dispatch_behind_a_storage_request() {
    let mut state = CmvsPs2aVmState::new(0);
    let action = run_command_with_action(&mut state, 296);
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::StorageRequest(
            CmvsPs2aStorageRequest::LoadSystemState {
                path_buffer_offset: 5404
            }
        ))
    );
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            span: SPAN,
            stack_bytes: 4,
            value: 1,
        }),
    )
    .unwrap_err();
    assert_eq!(error.code(), "SHINKI_EMU_CMVS_VM_STORAGE_AWAIT");
}

#[test]
fn stack_expression_binary_operators_follow_the_original_order() {
    // Left operand is pushed first, right operand second (on top); the
    // evaluator computes LEFT op RIGHT.
    fn eval_binary(opcode: u16, left: u32, right: u32) -> CmvsPs2aVmState {
        let mut state = CmvsPs2aVmState::new(0);
        let expression = crate::CmvsPs2aStackExpression {
            span: SPAN,
            nodes: vec![
                crate::CmvsPs2aStackExpressionNode::Value {
                    span: SPAN,
                    opcode: 0x100,
                    payload: left,
                },
                crate::CmvsPs2aStackExpressionNode::Value {
                    span: SPAN,
                    opcode: 0x100,
                    payload: right,
                },
                crate::CmvsPs2aStackExpressionNode::Operator { span: SPAN, opcode },
            ],
        };
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::StackExpression(expression),
        )
        .unwrap();
        state
    }
    // 0x16a: signed greater-than, left vs right.
    assert_eq!(eval_binary(0x16a, 7, 3).current_value, Some(1));
    assert_eq!(eval_binary(0x16a, 3, 7).current_value, Some(0));
    assert_eq!(eval_binary(0x16a, 3, 3).current_value, Some(0));
    // 0x164: left - right.
    assert_eq!(eval_binary(0x164, 9, 4).current_value, Some(5));
    assert_eq!(
        eval_binary(0x164, 4, 9).current_value,
        Some(u32::from_ne_bytes((-5_i32).to_ne_bytes()))
    );
    // 0x161: division with the guarded zero-divisor path.
    assert_eq!(eval_binary(0x161, 8, 2).current_value, Some(4));
    assert_eq!(eval_binary(0x161, 8, 0).current_value, Some(0));
    // 0x168: left shift masked to five bits.
    assert_eq!(eval_binary(0x168, 1, 4).current_value, Some(16));
    assert_eq!(eval_binary(0x168, 1, 36).current_value, Some(16));
    // 0x171 / 0x172 equality pair.
    assert_eq!(eval_binary(0x171, 5, 5).current_value, Some(1));
    assert_eq!(eval_binary(0x172, 5, 6).current_value, Some(1));
    // The terminator stores the result in process index 4096.
    let state = eval_binary(0x163, 2, 3);
    assert_eq!(state.process_indexed_words.get(&4096), Some(&5));
    assert!(state.condition_flag);
}

#[test]
fn stack_expression_flag_reads_and_writes_use_the_process_bitmap() {
    // Prefix 0x102 pushes the flag index; coercion reads the bitmap.
    let mut state = CmvsPs2aVmState::new(0);
    state.process_flag_bits.insert(41);
    let expression = crate::CmvsPs2aStackExpression {
        span: SPAN,
        nodes: vec![crate::CmvsPs2aStackExpressionNode::Prefix {
            span: SPAN,
            opcode: 0x102,
            payload: None,
            index: None,
            operands: vec![crate::CmvsPs2aStackExpression {
                span: SPAN,
                nodes: vec![literal_node(41)],
            }],
        }],
    };
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(expression),
    )
    .unwrap();
    assert_eq!(state.current_value, Some(1));

    // Assignment through tag 0x102 sets and clears flag bits.
    fn assign_flag(state: &mut CmvsPs2aVmState, index: u32, value: u32) {
        let expression = crate::CmvsPs2aStackExpression {
            span: SPAN,
            nodes: vec![
                crate::CmvsPs2aStackExpressionNode::Prefix {
                    span: SPAN,
                    opcode: 0x102,
                    payload: None,
                    index: None,
                    operands: vec![crate::CmvsPs2aStackExpression {
                        span: SPAN,
                        nodes: vec![literal_node(index)],
                    }],
                },
                literal_node(value),
                crate::CmvsPs2aStackExpressionNode::Operator {
                    span: SPAN,
                    opcode: 0x170,
                },
            ],
        };
        execute_cmvs390_frame(
            state,
            &CmvsPs2aInstructionFrame::StackExpression(expression),
        )
        .unwrap();
    }
    assign_flag(&mut state, 77, 1);
    assert!(state.process_flag_bits.contains(&77));
    assign_flag(&mut state, 41, 0);
    assert!(!state.process_flag_bits.contains(&41));
}

#[test]
fn apply_system_save_overwrites_flag_and_word_regions() {
    let mut state = CmvsPs2aVmState::new(0);
    state.process_flag_bits.insert(100);
    state.process_flag_bits.insert(3000);
    state.process_indexed_words.insert(2500, 9);
    let mut flags = vec![0_u8; 0x7F00];
    flags[0] = 0b101;
    flags[1] = 0x80;
    let mut cg_words = vec![0_u32; 2048];
    cg_words[500] = 1234;
    let save = crate::CmvsSystemSave {
        flags,
        cg_words,
        bgm_words: vec![0_u32; 1024],
        title_slots: vec![Vec::new(); 64],
        lists: [Vec::new(), Vec::new()],
    };
    apply_system_save(&mut state, save).unwrap();
    // Bits below the region survive.
    assert!(state.process_flag_bits.contains(&100));
    // Bits inside the region are replaced by the save bytes.
    assert!(!state.process_flag_bits.contains(&3000));
    assert!(state.process_flag_bits.contains(&2048));
    assert!(state.process_flag_bits.contains(&2050));
    assert!(state.process_flag_bits.contains(&(2048 + 15)));
    // The CG word table lands at process indexed words 2048..4096.
    assert_eq!(state.process_indexed_words.get(&2500), Some(&0));
    assert_eq!(state.process_indexed_words.get(&(2048 + 500)), Some(&1234));
    assert_eq!(state.process_float_words.get(&1024), Some(&0));
    assert_eq!(state.process_string_slots.get(&64), Some(&Vec::new()));
}

#[test]
fn command_152_appends_to_the_string_list_and_reports_length() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 21);
    run_command(&mut state, 152);
    push(&mut state, 22);
    run_command(&mut state, 152);
    let list = state.private_string_lists.get(&152).unwrap();
    assert_eq!(
        list.as_slice(),
        [
            CmvsPs2aPrivateStringReference {
                relative_offset: 21
            },
            CmvsPs2aPrivateStringReference {
                relative_offset: 22
            },
        ]
    );
    assert_eq!(state.interpreter_words.get(&81220), Some(&2));
}

#[test]
fn command_212_stores_deterministic_random_draws_modulo_the_bound() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 5);
    run_command(&mut state, 212);
    // MSVC LCG from seed 1: first draw is 41, reduced modulo 5.
    assert_eq!(state.interpreter_words.get(&81220), Some(&1));
    push(&mut state, 5);
    run_command(&mut state, 212);
    assert_eq!(state.interpreter_words.get(&81220), Some(&2));
    // A zero bound blocks instead of inventing a draw.
    push(&mut state, 0);
    let error = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 212,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "SHINKI_EMU_CMVS_VM_RANDOM");
}

#[test]
fn command_153_skips_the_action_when_the_cursor_index_is_unchanged() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 2);
    let action = run_command_with_action(&mut state, 153);
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::SelectSystemCursor { index: 2 })
    );
    push(&mut state, 2);
    let action = run_command_with_action(&mut state, 153);
    assert_eq!(action, None);
    push(&mut state, 3);
    let action = run_command_with_action(&mut state, 153);
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::SelectSystemCursor { index: 3 })
    );
}

#[test]
fn command_177_out_of_bounds_channel_raises_mask_0x200() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 1);
    push(&mut state, 6);
    run_command(&mut state, 177);
    assert_eq!(state.interpreter_flag_words.get(&10532), Some(&0x200));
    assert_eq!(state.stack_cursor_bytes, 0);
    assert!(!state.interpreter_words.contains_key(&(1640 + 52 * 6)));
    assert!(!state.interpreter_words.contains_key(&(1656 + 52 * 6)));
}

#[test]
fn command_688_stores_the_frame_pair_and_active_flag() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 12);
    push(&mut state, 0);
    run_command(&mut state, 688);
    assert_eq!(state.interpreter_words.get(&10296), Some(&0));
    assert_eq!(state.interpreter_words.get(&10300), Some(&12));
    assert_eq!(state.interpreter_words.get(&10292), Some(&0));
}

#[test]
fn command_691_requests_native_slot_scan_without_partial_catalog_updates() {
    for (input, count) in [(u32::MAX, 10), (400, 256), (128, 128), (0, 0)] {
        let mut state = CmvsPs2aVmState::new(0);
        push(&mut state, input);
        assert_eq!(
            run_command_with_action(&mut state, 691),
            Some(CmvsPs2aVmAction::ScanNativeSaveSlots { count })
        );
        assert!(state.native_save_catalog.is_none());
    }
}

#[test]
fn command_40_out_of_bounds_pair_only_raises_the_error_flag() {
    let mut state = CmvsPs2aVmState::new(0);
    push(&mut state, 5);
    run_command(&mut state, 32);
    push(&mut state, 5);
    push(&mut state, 300);
    run_command(&mut state, 40);
    assert_eq!(
        state.slot_objects.get(&slot_object_key(1924, 5).unwrap()),
        Some(&1)
    );
    assert_eq!(state.interpreter_flag_words.get(&10532), Some(&0x10));
}

#[test]
fn stores_component_constant_from_command_845() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 845,
        },
    )
    .unwrap();
    assert_eq!(
        state
            .component_words
            .get(&component_field_key(0x0cc8, 0x0568)),
        Some(&1)
    );
}

#[test]
fn executes_a_name_index_call_and_exact_stack_return() {
    let mut state = CmvsPs2aVmState::new(10);
    for value in [u32::MAX, 2, 9] {
        push_word(&mut state, value).unwrap();
    }
    let call = CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::NameIndexCall {
        span: CmvsPs2aSourceSpan {
            offset: 10,
            byte_length: 4,
        },
        name_index: 0,
        target: 40,
    });
    execute_cmvs390_frame(&mut state, &call).unwrap();
    assert_eq!(state.program_counter, 40);
    assert_eq!(state.stack_cursor_bytes, 16);
    assert_eq!(state.call_frame_bases, vec![0, 16]);
    assert_eq!(read_frame_local_word(&state, -16).unwrap(), u32::MAX);
    assert_eq!(read_frame_local_word(&state, -12).unwrap(), 2);
    assert_eq!(read_frame_local_word(&state, -8).unwrap(), 9);
    let returned = CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::StackReturn {
        span: SPAN,
        stack_adjust: 12,
    });
    execute_cmvs390_frame(&mut state, &returned).unwrap();
    assert_eq!(state.program_counter, 14);
    assert_eq!(state.stack_cursor_bytes, 0);
    assert_eq!(state.call_frame_bases, vec![0]);
}
#[test]
fn native_input_poll_retains_edges_until_explicit_consume() {
    let mut state = CmvsPs2aVmState::new(0);
    state.input_confirm_held = true;
    state.input_advance_press = true;
    state.input_advance_release = true;
    let command = |command_id| CmvsPs2aInstructionFrame::Command {
        span: SPAN,
        command_id,
    };
    execute_cmvs390_frame(&mut state, &command(416)).unwrap();
    assert_eq!(state.interpreter_words[&81220], 1);
    assert_eq!(state.interpreter_words[&81236], 1);
    execute_cmvs390_frame(&mut state, &command(416)).unwrap();
    assert_eq!(state.interpreter_words[&81220], 1);
    execute_cmvs390_frame(&mut state, &command(417)).unwrap();
    assert!(!state.input_advance_press);
    assert!(!state.input_advance_release);
    assert!(state.input_confirm_held);
    execute_cmvs390_frame(&mut state, &command(416)).unwrap();
    assert_eq!(state.interpreter_words[&81220], 0);
    assert_eq!(state.interpreter_words[&81236], 1);
}

#[test]
fn native_crossfade_stop_preserves_duration_and_consumes_only_its_argument() {
    let mut state = CmvsPs2aVmState::new(0);
    execute_cmvs390_frame(&mut state, &push_immediate(17)).unwrap();
    execute_cmvs390_frame(&mut state, &push_immediate(250)).unwrap();
    let action = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 162,
        },
    )
    .unwrap();
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::FadeOutAudio { fade_ms: 250 })
    );
    assert_eq!(state.stack_cursor_bytes, 4);
    let action = execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 161,
        },
    )
    .unwrap();
    assert_eq!(action, Some(CmvsPs2aVmAction::StopAudio));
    assert_eq!(state.stack_cursor_bytes, 4);
}

#[test]
fn source_rebuild_gate_preserves_suspended_stack_until_reenabled() {
    let mut state = CmvsPs2aVmState::new(64);
    state.write_interpreter_word(0x33D4, 1);
    state.write_interpreter_word(2679 * 4, 1);
    // Queue slot three is an always-active coroutine entry: the record
    // layout keeps the delay word at +0 and the resume pc at +4
    // (`sub_46DF10`, fix pinned by the 0x33DC record base).
    let entry = 13276 + 28 * 3;
    state.write_interpreter_word(entry + 4, 96);
    state.write_interpreter_word(entry, 0);
    execute_cmvs390_frame(&mut state, &push_immediate(123)).unwrap();
    let cursor = state.stack_cursor_bytes;
    rebuild_frame_entry_queue(&mut state).unwrap();
    // The closed gate preserves the suspended stack.
    assert_eq!(state.stack_cursor_bytes, 4);
    state.write_interpreter_word(2679 * 4, 0);
    rebuild_frame_entry_queue(&mut state).unwrap();
    assert_eq!(state.stack_cursor_bytes, cursor + 12);
    assert_eq!(state.program_counter, 96);
}

#[test]
fn process_word77_or_zero_sets_the_flag_word() {
    // The intproc load gate (`pc 179782`): `word77 | 0` must publish its
    // result into the condition word 81212 for the following
    // JumpWhenFlagClear.
    let mut state = CmvsPs2aVmState::new(0);
    state.process_indexed_words.insert(77, 1);
    let expression = crate::CmvsPs2aStackExpression {
        span: SPAN,
        nodes: vec![
            crate::CmvsPs2aStackExpressionNode::Prefix {
                span: SPAN,
                opcode: 0x101,
                payload: None,
                index: None,
                operands: vec![crate::CmvsPs2aStackExpression {
                    span: SPAN,
                    nodes: vec![crate::CmvsPs2aStackExpressionNode::Value {
                        span: SPAN,
                        opcode: 0x100,
                        payload: 77,
                    }],
                }],
            },
            crate::CmvsPs2aStackExpressionNode::Value {
                span: SPAN,
                opcode: 0x100,
                payload: 0,
            },
            crate::CmvsPs2aStackExpressionNode::Operator {
                span: SPAN,
                opcode: 0x169,
            },
        ],
    };
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::StackExpression(expression),
    )
    .unwrap();
    assert_eq!(state.interpreter_words.get(&81212), Some(&1));
}

#[test]
fn scheduler_dispatches_channel_seven_then_other_entries_then_suspension() {
    let mut state = CmvsPs2aVmState::new(400);
    state.write_interpreter_word(0x33D4, 1);
    state.write_interpreter_word(SCRIPT_FRAME_COUNTER_FIELD, 7);
    push_word(&mut state, 0xCAFE).unwrap();
    for (slot, frame, pc) in [(3, 1, 96), (17, 2, 128)] {
        let base = 0x33DC + 28 * slot;
        state.write_interpreter_word(base, frame);
        state.write_interpreter_word(base + 4, pc);
    }
    state.slot_objects.insert((742 << 8) | 7, 1);
    state
        .effect_channels
        .entry(7)
        .or_default()
        .write_dword(2, 1);
    rebuild_frame_entry_queue(&mut state).unwrap();
    assert_eq!((state.current_frame, state.program_counter), (2, 128));
    let ret = CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::ScriptReturn {
        span: SPAN,
        stack_bytes: 0,
    });
    execute_cmvs390_frame(&mut state, &ret).unwrap();
    assert_eq!((state.current_frame, state.program_counter), (1, 96));
    execute_cmvs390_frame(&mut state, &ret).unwrap();
    assert_eq!((state.current_frame, state.program_counter), (0, 400));
    assert_eq!(state.interpreter_words[&SCRIPT_FRAME_COUNTER_FIELD], 7);
    assert_eq!(pop_word(&mut state).unwrap(), 0xCAFE);
    assert_eq!(state.stack_cursor_bytes, 0);
    assert_eq!(state.call_frame_bases, vec![0]);
}

#[test]
fn scheduler_clears_activity_flags_even_when_rebuild_is_gated() {
    let mut state = CmvsPs2aVmState::new(400);
    state.write_interpreter_word(0x29DC, 1);
    let base = 0x33DC + 28 * 63;
    state.write_interpreter_word(base - 4, 1);
    state.write_interpreter_word(base, 2);
    state.write_interpreter_word(base + 4, 128);
    rebuild_frame_entry_queue(&mut state).unwrap();
    assert_eq!(state.interpreter_words[&(base - 4)], 0);
    assert_eq!(state.interpreter_words[&base], 2);
    assert_eq!(state.interpreter_words[&(base + 4)], 128);
    assert_eq!(state.program_counter, 400);
}

#[test]
fn command_137_does_not_overwrite_wait_timer_two() {
    let mut state = CmvsPs2aVmState::new(0);
    state.write_interpreter_word(0x33AC + 8, 900);
    push_word(&mut state, 1).unwrap();
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 137,
        },
    )
    .unwrap();
    assert_eq!(state.interpreter_words[&0x33D4], 1);
    assert_eq!(state.interpreter_words[&(0x33AC + 8)], 900);
}

#[test]
fn wait_timer_commands_reset_and_read_selected_native_timers() {
    for selector in [0, 1, 9] {
        let mut state = CmvsPs2aVmState::new(0);
        let timer = 0x33AC + selector * 4;
        state.write_interpreter_word(timer, 900);
        state.write_interpreter_word(13220, 0x1234);
        push_word(&mut state, selector).unwrap();
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: 11,
            },
        )
        .unwrap();
        assert_eq!(state.interpreter_words[&timer], 0);
        assert_eq!(state.interpreter_words[&13220], 0x1234);
        state.write_interpreter_word(timer, 1500);
        push_word(&mut state, selector).unwrap();
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: 12,
            },
        )
        .unwrap();
        assert_eq!(state.interpreter_words[&81220], 1500);
    }
}

#[test]
fn texture_transition_preserves_the_parent_image_instead_of_loading_its_mask_as_color() {
    let mut state = CmvsPs2aVmState::new(0);
    state.slot_objects.insert(1924 << 8 | 28, 1);
    state.texture_parents.insert(
        28,
        CmvsTextureParentState {
            resource: Some("background.pb3".into()),
            surface_initialized: true,
            ..Default::default()
        },
    );
    seed_pool_bytes(&mut state, 0, &[(0, b"mask.pb3")]);
    for value in [1, 750, 1500, 28, 0] {
        push_word(&mut state, value).unwrap();
    }
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::Command {
            span: SPAN,
            command_id: 554,
        },
    )
    .unwrap();
    assert_eq!(
        state.texture_parents[&28].resource.as_deref(),
        Some("background.pb3")
    );
    assert_eq!(state.slot_objects[&(1924 << 8 | 28)], 1);
}

#[test]
fn named_parent_load_uses_the_live_481_table_and_preserves_object_identity() {
    let mut state = CmvsPs2aVmState::new(0);
    state.slot_objects.insert(1924 << 8 | 20, 91);
    state
        .texture_parents
        .insert(20, CmvsTextureParentState::default());
    state.interpreter_flag_words.insert(10532, 0x20);
    seed_pool_bytes(&mut state, 0, &[(0, b"picture.pb3")]);
    for word in [0, 0x00404040, 2, 0, 20] {
        push_word(&mut state, word).unwrap();
    }
    run_command(&mut state, 59);
    assert_eq!(state.interpreter_flag_words[&10532], 0x20);
    assert_eq!(state.interpreter_words[&81220], 1);
    assert_eq!(state.slot_objects[&(1924 << 8 | 20)], 91);
    assert_eq!(
        state.texture_parents[&20].color_filter,
        Some([2, 0x00404040, 0])
    );
    assert_eq!(
        state.texture_parents[&20].resource.as_deref(),
        Some("picture.pb3")
    );
}

#[test]
fn an_unrelated_784_entry_cannot_authorize_a_parent_image_load() {
    let mut state = CmvsPs2aVmState::new(0);
    state.slot_objects.insert(784 << 8 | 20, 91);
    state.interpreter_flag_words.insert(10532, 0x20);
    state.write_interpreter_word(81220, 777);
    seed_pool_bytes(&mut state, 0, &[(0, b"picture.pb3")]);
    for word in [0, 0, 0, 0, 20] {
        push_word(&mut state, word).unwrap();
    }
    run_command(&mut state, 59);
    assert_eq!(state.interpreter_flag_words[&10532], 0x30);
    assert_eq!(state.interpreter_words[&81220], 777);
    assert!(!state.texture_parents.contains_key(&20));
}

#[test]
fn rebuilding_an_effect_channel_drops_its_previous_image_and_origin() {
    let mut state = CmvsPs2aVmState::new(0);
    let mut channel = CmvsEffectChannel {
        image_resource: Some("cmvs:/old.pb3".into()),
        ..Default::default()
    };
    channel.set_origin(42, 19);
    state.effect_channels.insert(1, channel.clone());
    state.effect_channels.insert(2, channel);
    for word in [0, 0, 0, 0, 0, 0, 3, 1] {
        push_word(&mut state, word).unwrap();
    }
    run_command(&mut state, 349);
    assert_eq!(state.effect_channels[&1].image_resource, None);
    assert_eq!(state.effect_channels[&1].origin(), (0, 0));
    assert_eq!(state.effect_channels[&1].read_dword(0), 3);
    assert_eq!(
        state.effect_channels[&2].image_resource.as_deref(),
        Some("cmvs:/old.pb3")
    );
}

#[test]
fn current_stage_constructor_rebuilds_channel_and_text_without_a_virtual_extent() {
    let mut state = CmvsPs2aVmState::new(0);
    for word in [720, 1280, 200, 600, 20, 10, 3, 2] {
        push_word(&mut state, word).unwrap();
    }
    run_command(&mut state, 349);
    let channel_identity = state.slot_objects[&(742 << 8 | 2)];
    let text_identity = state.slot_objects[&(750 << 8 | 3)];
    state.effect_channels.get_mut(&2).unwrap().image_resource = Some("cmvs:/old.pb3".into());
    state.effect_channels.get_mut(&2).unwrap().write_dword(2, 1);
    state
        .effect_elements
        .insert((2, 4), CmvsEffectElement::default());
    for word in [80, 300, 40, 30, 3, 2] {
        push_word(&mut state, word).unwrap();
    }
    assert_eq!(
        run_command_with_action(&mut state, 320),
        Some(CmvsPs2aVmAction::CreateEffectChannel {
            channel: 2,
            effect: 3
        }),
    );
    assert_ne!(state.slot_objects[&(742 << 8 | 2)], channel_identity);
    assert_ne!(state.slot_objects[&(750 << 8 | 3)], text_identity);
    let channel = &state.effect_channels[&2];
    assert_eq!(channel.origin(), (30, 40));
    assert_eq!(channel.image_resource, None);
    assert_eq!(channel.read_dword(2), 0);
    assert_eq!(channel.read_dword(4), 0);
    assert!(!state.effect_elements.contains_key(&(2, 4)));
    assert_eq!(state.effect_elements[&(2, u32::MAX)].visible, 1);
    let surface = &state.native_text[&3];
    assert_eq!(surface.position, [30, 40]);
    assert_eq!(surface.rect, [0, 0, 300, 80]);
    assert!(surface.visible);
}
