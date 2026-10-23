use super::*;

/// Executes exactly one framed instruction without cloning the VM.
/// Unsupported expressions, controls, tag domains and stack widths are
/// blocking. An execution error leaves partial state and permanently rejects
/// further dispatch and snapshot validation; it must not be retried in place.
pub fn execute_cmvs390_frame(
    state: &mut CmvsPs2aVmState,
    frame: &CmvsPs2aInstructionFrame,
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    if state.execution_failed {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_FAILED",
            "VM cannot execute after an instruction failure",
        ));
    }
    if state.dispatch_stopped {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_STOPPED",
            "CMVS PS2A dispatch has already stopped",
        ));
    }
    if state.storage_await.is_some() {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_STORAGE_AWAIT",
            "CMVS PS2A dispatch waits on an unresolved host request",
        ));
    }
    // Mark before entering the handler so an unwinding panic cannot leave a
    // resumable partially mutated VM either. Success is the only reset path.
    state.execution_failed = true;
    let result = execute_instruction(state, frame);
    if result.is_ok() {
        state.execution_failed = false;
    }
    result
}

fn execute_instruction(
    next: &mut CmvsPs2aVmState,
    frame: &CmvsPs2aInstructionFrame,
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    let depth_before = next.stack_cursor_bytes;
    let probe_zone = tracing::enabled!(tracing::Level::INFO)
        && ((next.current_frame == 0
            && ((48_200..=48_300).contains(&next.program_counter)
                || (49_000..=49_700).contains(&next.program_counter)
                || (162_700..=163_000).contains(&next.program_counter)
                || (202_400..=203_000).contains(&next.program_counter)
                || (172_100..=172_400).contains(&next.program_counter)
                || (157_400..=158_100).contains(&next.program_counter)
                || (113_000..=114_500).contains(&next.program_counter)
                || (156_600..=157_400).contains(&next.program_counter)
                || (166_900..=167_600).contains(&next.program_counter)
                || (204_000..=205_200).contains(&next.program_counter)
                || (185_400..=185_800).contains(&next.program_counter)))
            || (next.current_frame <= 2
                && ((370_400..=372_400).contains(&next.program_counter)
                    || (24_800..=28_500).contains(&next.program_counter)
                    || (183_600..=184_200).contains(&next.program_counter)
                    || (193_100..=193_280).contains(&next.program_counter)
                    || (248_540..=248_660).contains(&next.program_counter)
                    || (299_640..=299_760).contains(&next.program_counter)
                    || (301_200..=301_320).contains(&next.program_counter)
                    || (355_200..=356_500).contains(&next.program_counter)
                    || (368_800..=369_600).contains(&next.program_counter)
                    || (99_000..=99_500).contains(&next.program_counter)
                    || (147_900..=148_400).contains(&next.program_counter)
                    || (149_900..=150_400).contains(&next.program_counter)
                    || (150_400..=151_400).contains(&next.program_counter)
                    || (100_900..=101_400).contains(&next.program_counter)
                    || (57_724..=62_016).contains(&next.program_counter))));
    if probe_zone && next.program_counter == 162_934 {
        let c = next.stack_cursor_bytes as usize;
        let words: Vec<u32> = (1..=8)
            .filter_map(|i| {
                let end = c.saturating_sub(i * 4);
                next.stack_bytes
                    .get(end.saturating_sub(4)..end)
                    .and_then(|b| b.try_into().ok())
                    .map(u32::from_le_bytes)
            })
            .collect();
        tracing::info!(
            event = "shinki.emu.cmvs.vm.crash_tail_stack",
            pc = next.program_counter,
            cursor = next.stack_cursor_bytes,
            words = ?words
        );
    }
    if probe_zone {
        let node_tag = |n: &crate::ps2a::CmvsPs2aStackExpressionNode| match n {
            crate::ps2a::CmvsPs2aStackExpressionNode::Value {
                opcode, payload, ..
            } => format!("v{:x}={}", opcode, payload),
            crate::ps2a::CmvsPs2aStackExpressionNode::Operator { opcode, .. } => {
                format!("o{:x}", opcode)
            }
            crate::ps2a::CmvsPs2aStackExpressionNode::Nested { .. } => "n".to_string(),
            crate::ps2a::CmvsPs2aStackExpressionNode::Prefix {
                opcode, payload, ..
            } => format!("p{:x}+{:x?}", opcode, payload),
        };
        let control_name = |i: &crate::ps2a::CmvsPs2aControlInstruction| match i {
            crate::ps2a::CmvsPs2aControlInstruction::AbsoluteJump { target, .. } => {
                format!("jmp@{}", target)
            }
            crate::ps2a::CmvsPs2aControlInstruction::NameIndexJump {
                name_index, target, ..
            } => format!("nij{}@{}", name_index, target),
            crate::ps2a::CmvsPs2aControlInstruction::NameIndexCall {
                name_index, target, ..
            } => format!("nic{}@{}", name_index, target),
            crate::ps2a::CmvsPs2aControlInstruction::JumpWhenFlagClear { target, .. } => {
                format!("jfc@{}", target)
            }
            crate::ps2a::CmvsPs2aControlInstruction::JumpWhenFlagSet { target, .. } => {
                format!("jfs@{}", target)
            }
            crate::ps2a::CmvsPs2aControlInstruction::JumpWhenValueEquals {
                expected_value,
                target,
                ..
            } => format!("jve{}@{}", expected_value, target),
            crate::ps2a::CmvsPs2aControlInstruction::JumpWithOpaqueField { .. } => {
                "jof".to_string()
            }
            crate::ps2a::CmvsPs2aControlInstruction::StackReturn { .. } => "sret".to_string(),
            crate::ps2a::CmvsPs2aControlInstruction::StackDrop { .. } => "sdrp".to_string(),
            crate::ps2a::CmvsPs2aControlInstruction::FrameReturn { .. } => "fret".to_string(),
            crate::ps2a::CmvsPs2aControlInstruction::ScriptReturn { .. } => "scret".to_string(),
            crate::ps2a::CmvsPs2aControlInstruction::StateTargetCall { .. } => "stc".to_string(),
            crate::ps2a::CmvsPs2aControlInstruction::PushCurrentValue { .. } => "pcv".to_string(),
            crate::ps2a::CmvsPs2aControlInstruction::PushImmediate { .. } => "pim".to_string(),
        };
        let kind = match frame {
            crate::ps2a::CmvsPs2aInstructionFrame::Control(i) => control_name(i).to_string(),
            crate::ps2a::CmvsPs2aInstructionFrame::ValueExpression(e) => format!(
                "V[{}]",
                e.tokens
                    .iter()
                    .map(|t| match t {
                        crate::ps2a::CmvsPs2aValueExpressionToken::Value { opcode, .. } => {
                            format!("v{:x}", opcode)
                        }
                        crate::ps2a::CmvsPs2aValueExpressionToken::Operator { opcode, .. } => {
                            format!("o{:x}", opcode)
                        }
                        crate::ps2a::CmvsPs2aValueExpressionToken::NestedStackExpression {
                            ..
                        } => "n".to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            crate::ps2a::CmvsPs2aInstructionFrame::StackExpression(e) => format!(
                "S[{}]",
                e.nodes.iter().map(&node_tag).collect::<Vec<_>>().join(",")
            ),
            crate::ps2a::CmvsPs2aInstructionFrame::NumericExpression(e) => {
                format!("N{}", e.nodes.len())
            }
            crate::ps2a::CmvsPs2aInstructionFrame::Command { command_id, .. } => {
                format!("cmd{}", command_id)
            }
        };
        tracing::info!(
            event = "shinki.emu.cmvs.vm.cursor_probe",
            pc = next.program_counter,
            cursor = next.stack_cursor_bytes,
            frame = next.current_frame,
            kind
        );
    }
    if tracing::enabled!(tracing::Level::INFO)
        && next.current_frame == 0
        && matches!(next.program_counter, 2816 | 2840 | 3608)
    {
        tracing::info!(
            event = "shinki.emu.cmvs.vm.fetch_at_write_pc",
            pc = next.program_counter,
            tick = next.tick,
            depth = next.call_frame_bases.len()
        );
    }
    if tracing::enabled!(tracing::Level::INFO)
        && next.current_frame == 1
        && matches!(
            next.program_counter,
            251_436 | 252_206 | 252_230 | 186_524 | 186_616 | 186_636 | 23_110 | 352_782 | 149_070
        )
    {
        tracing::info!(
            event = "shinki.emu.cmvs.vm.element_chain_probe",
            pc = next.program_counter,
            tick = next.tick,
            word915 = next.process_indexed_words.get(&915).copied().unwrap_or(0),
            word916 = next.process_indexed_words.get(&916).copied().unwrap_or(0),
            sysreg0 = next
                .interpreter_words
                .get(&super::expression::SYSTEM_REGISTER_FIELD_BASE)
                .copied()
                .unwrap_or(0),
            word406 = next.process_indexed_words.get(&406).copied().unwrap_or(0),
            slot11_len = next
                .process_string_slots
                .get(&11)
                .map(|s| s.len())
                .unwrap_or(0),
            slot11 = String::from_utf8_lossy(
                next.process_string_slots
                    .get(&11)
                    .map(|s| s.as_slice())
                    .unwrap_or_default(),
            )
            .into_owned()
        );
    }
    let action = match frame {
        CmvsPs2aInstructionFrame::ValueExpression(expression) => {
            run_value_expression(next, expression)?;
            advance(next, frame)?;
            None
        }
        CmvsPs2aInstructionFrame::StackExpression(expression) => {
            run_stack_expression(next, expression)?;
            advance(next, frame)?;
            None
        }
        CmvsPs2aInstructionFrame::NumericExpression(expression) => {
            run_numeric_expression(next, expression)?;
            advance(next, frame)?;
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::AbsoluteJump {
            target,
            ..
        })
        | CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::JumpWithOpaqueField {
            target,
            ..
        })
        | CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::NameIndexJump {
            target,
            ..
        }) => {
            if tracing::enabled!(tracing::Level::INFO)
                && ((4900..=5000).contains(target) || (200_630..=200_650).contains(target))
            {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.jump_watch",
                    pc = next.program_counter,
                    frame = next.current_frame,
                    target
                );
            }
            next.program_counter = *target;
            if !next.program_counter.is_multiple_of(2) {
                return Err(CoreError::invalid(
                    "SHINKI_EMU_CMVS_VM_PC_UNALIGNED",
                    format!(
                        "CMVS PS2A PC set to odd {} at vm.rs:644",
                        next.program_counter
                    ),
                ));
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::JumpWhenFlagClear {
            target,
            ..
        }) => {
            // The original branch reads the persistent condition word
            // (`*(byte *)(this + 81212) & 1`), so keep the in-memory flag and
            // the interpreter word as one synchronized state.
            if next.current_frame != 0 && vm_trace_enabled() {
                tracing::trace!(
                    event = "shinki.emu.cmvs.vm.branch",
                    pc = next.program_counter,
                    frame = next.current_frame
                );
            }
            if tracing::enabled!(tracing::Level::INFO)
                && matches!(
                    next.program_counter,
                    179_806
                        | 179_862
                        | 179_916
                        | 179_966
                        | 180_028
                        | 180_050
                        | 180_054
                        | 180_058
                        | 187_742
                        | 189_424
                        | 218_556
                )
            {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.save_chain",
                    pc = next.program_counter,
                    pw77 = next.process_indexed_words.get(&77).copied().unwrap_or(0),
                    flag_word = next.interpreter_words.get(&81212).copied().unwrap_or(0),
                    tick = next.tick
                );
            }
            if tracing::enabled!(tracing::Level::INFO)
                && matches!(next.program_counter, 410_138 | 410_156 | 410_182 | 193_194)
            {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.loop_exit_poll",
                    pc = next.program_counter,
                    data276 = super::memory::read_script_data_segment_word(next, 276).unwrap_or(0),
                    pw16 = next.process_indexed_words.get(&16).copied().unwrap_or(0),
                    flag_word = next.interpreter_words.get(&81212).copied().unwrap_or(0),
                    tick = next.tick
                );
            }
            if tracing::enabled!(tracing::Level::INFO)
                && matches!(next.program_counter, 410_056 | 410_074)
            {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.loop_branch",
                    pc = next.program_counter,
                    reg0 = next.interpreter_words.get(&81220).copied().unwrap_or(0),
                    flag_word = next.interpreter_words.get(&81212).copied().unwrap_or(0),
                    cond = next.condition_flag,
                    tick = next.tick
                );
            }
            if next.current_frame == 1 && next.program_counter == 349_086 {
                next.menu_pump_exit_seen = true;
            }
            if tracing::enabled!(tracing::Level::INFO)
                && matches!(
                    next.program_counter,
                    195_472
                        | 195_540
                        | 195_728
                        | 195_754
                        | 195_784
                        | 212_384
                        | 212_452
                        | 212_476
                        | 212_500
                        | 212_560
                        | 194_784
                        | 194_852
                        | 194_920
                        | 194_944
                        | 194_986
                        | 195_844
                        | 195_870
                        | 195_894
                        | 195_912
                        | 195_914
                        | 195_932
                        | 195_938
                        | 195_950
                        | 195_846
                        | 195_872
                        | 195_876
                        | 195_880
                        | 195_884
                        | 195_888
                        | 369_668
                        | 369_776
                        | 369_800
                        | 369_836
                        | 369_842
                        | 370_170
                        | 370_188
                        | 370_194
                        | 370_224
                        | 370_252
                        | 370_590
                        | 370_640
                )
            {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.load_gate_probe",
                    pc = next.program_counter,
                    cond = next.condition_flag,
                    sysreg0 = next.interpreter_words.get(&81220).copied().unwrap_or(0),
                    sel2200 = next.process_indexed_words.get(&2200).copied().unwrap_or(0),
                    flag_word = next.interpreter_words.get(&81212).copied().unwrap_or(0),
                    pw2199 = next.process_indexed_words.get(&2199).copied().unwrap_or(0),
                    pw98 = next.process_indexed_words.get(&98).copied().unwrap_or(0),
                    data16 = super::memory::read_script_data_segment_word(next, 16).unwrap_or(0),
                    data20 = super::memory::read_script_data_segment_word(next, 20).unwrap_or(0),
                    local16 = super::memory::read_frame_local_word(next, -16).unwrap_or(0),
                    local16p = super::memory::read_frame_local_word(next, 16).unwrap_or(0),
                    tick = next.tick
                );
            }
            if next.current_frame == 1
                && next.program_counter == 179806
                && tracing::enabled!(tracing::Level::INFO)
            {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.gate_probe",
                    word77 = next.process_indexed_words.get(&77).copied().unwrap_or(0),
                    flag_word = next.interpreter_words.get(&81212).copied().unwrap_or(0)
                );
            }
            if !next.interpreter_words.get(&81212).copied().unwrap_or(0) & 1 == 1 {
                next.program_counter = *target;
                if !next.program_counter.is_multiple_of(2) {
                    return Err(CoreError::invalid(
                        "SHINKI_EMU_CMVS_VM_PC_UNALIGNED",
                        format!(
                            "CMVS PS2A PC set to odd {} at vm.rs:652",
                            next.program_counter
                        ),
                    ));
                }
            } else {
                advance(next, frame)?;
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::JumpWhenFlagSet {
            target,
            ..
        }) => {
            if next.current_frame != 0 && vm_trace_enabled() {
                tracing::trace!(
                    event = "shinki.emu.cmvs.vm.branch",
                    pc = next.program_counter,
                    frame = next.current_frame
                );
            }
            if next.interpreter_words.get(&81212).copied().unwrap_or(0) & 1 == 1 {
                next.program_counter = *target;
                if !next.program_counter.is_multiple_of(2) {
                    return Err(CoreError::invalid(
                        "SHINKI_EMU_CMVS_VM_PC_UNALIGNED",
                        format!(
                            "CMVS PS2A PC set to odd {} at vm.rs:663",
                            next.program_counter
                        ),
                    ));
                }
            } else {
                advance(next, frame)?;
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::JumpWhenValueEquals {
            expected_value,
            target,
            ..
        }) => {
            if tracing::enabled!(tracing::Level::INFO)
                && ((4900..=5000).contains(target) || (200_630..=200_650).contains(target))
            {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.cond_jump_watch",
                    pc = next.program_counter,
                    frame = next.current_frame,
                    target
                );
            }
            // Throwaway probe: the menu demo-loop dispatch chain (206482..=
            // 206522) compares the 535 poll result carried in current_value.
            if (206_482..=206_522).contains(&next.program_counter)
                && next.current_frame == 0
                && tracing::enabled!(tracing::Level::INFO)
            {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.menu_dispatch_compare",
                    pc = next.program_counter,
                    current = next.current_value.unwrap_or(0),
                    expected = *expected_value,
                    matched = next.current_value == Some(*expected_value)
                );
            }
            // Item dispatcher ladder (intproc 187268..=187388): the jve
            // input is value-stack slot 4 (the input id the callback
            // parked). Watch what it carries.
            if (187_268..=187_388).contains(&next.program_counter)
                && next.current_frame == 1
                && next.current_frame == 1
                && tracing::enabled!(tracing::Level::INFO)
            {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.item_dispatch_compare",
                    pc = next.program_counter,
                    current = next.current_value.unwrap_or(0),
                    expected = *expected_value,
                    matched = next.current_value == Some(*expected_value),
                    tick = next.tick
                );
            }
            // A consumed keyboard arm serves exactly one item dispatch.
            if (187_268..=187_388).contains(&next.program_counter)
                && next.current_frame == 1
                && next.current_value == Some(*expected_value)
                && next.input_selection_override == Some(*expected_value)
            {
                next.input_selection_override = None;
            }
            if next.current_value == Some(*expected_value) {
                next.program_counter = *target;
                if !next.program_counter.is_multiple_of(2) {
                    return Err(CoreError::invalid(
                        "SHINKI_EMU_CMVS_VM_PC_UNALIGNED",
                        format!(
                            "CMVS PS2A PC set to odd {} at vm.rs:675",
                            next.program_counter
                        ),
                    ));
                }
            } else {
                advance(next, frame)?;
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::NameIndexCall {
            target,
            ..
        }) => {
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    event = "shinki.emu.cmvs.vm.call_trace",
                    pc = next.program_counter,
                    target,
                    frame = next.current_frame,
                    stack = next.stack_cursor_bytes
                );
            }
            if tracing::enabled!(tracing::Level::INFO)
                && matches!(
                    next.program_counter,
                    180_028
                        | 180_050
                        | 180_054
                        | 180_058
                        | 187_742
                        | 189_424
                        | 218_556
                        | 218_962
                        | 300_844
                )
            {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.save_chain_call",
                    pc = next.program_counter,
                    target,
                    tick = next.tick
                );
            }
            let return_pc = next
                .program_counter
                .checked_add(u32::from(frame.span().byte_length))
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_PC",
                        "CMVS PS2A program counter overflowed",
                    )
                })?;
            crate::vm::stack::trace_push(next, 4, "call");
            push_word(next, return_pc)?;
            push_call_frame_base(next)?;
            if tracing::enabled!(tracing::Level::INFO) && *target == 42_3214 {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.svc_call",
                    pc = next.program_counter,
                    return_pc,
                    frame = next.current_frame,
                    stack = next.stack_cursor_bytes
                );
            }
            next.program_counter = *target;
            if !next.program_counter.is_multiple_of(2) {
                return Err(CoreError::invalid(
                    "SHINKI_EMU_CMVS_VM_PC_UNALIGNED",
                    format!(
                        "CMVS PS2A PC set to odd {} at vm.rs:696",
                        next.program_counter
                    ),
                ));
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::StateTargetCall {
            ..
        }) => {
            // The original case 1046 reads the target from the persistent
            // expression-result word at byte offset 81216 (`mov eax,
            // [edi+13D40h]`), not from the evaluator's in-register result,
            // so the value survives intervening command side effects.
            let target = next
                .interpreter_words
                .get(&VALUE_EXPRESSION_RESULT_FIELD)
                .copied()
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_CURRENT_VALUE",
                        "CMVS PS2A state-target call requires an evaluator result",
                    )
                })?;
            if tracing::enabled!(tracing::Level::INFO)
                && matches!(
                    next.program_counter,
                    180_028
                        | 180_050
                        | 180_054
                        | 180_058
                        | 187_742
                        | 189_424
                        | 218_556
                        | 218_962
                        | 300_844
                )
            {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.save_chain_call",
                    pc = next.program_counter,
                    target,
                    tick = next.tick
                );
            }
            let return_pc = next
                .program_counter
                .checked_add(u32::from(frame.span().byte_length))
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_PC",
                        "CMVS PS2A program counter overflowed",
                    )
                })?;
            crate::vm::stack::trace_push(next, 4, "call");
            push_word(next, return_pc)?;
            push_call_frame_base(next)?;
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.statetarget_call",
                    pc = next.program_counter,
                    frame = next.current_frame,
                    target
                );
            }
            next.program_counter = target;
            if !next.program_counter.is_multiple_of(2) {
                return Err(CoreError::invalid(
                    "SHINKI_EMU_CMVS_VM_PC_UNALIGNED",
                    format!(
                        "CMVS PS2A PC set to odd {} at vm.rs:727",
                        next.program_counter
                    ),
                ));
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::StackReturn {
            stack_adjust,
            ..
        }) => {
            // Native 0x411 pops the return PC, drops argument bytes and
            // decrements the call-depth dword without a bounds check — at
            // depth zero it wraps and the return still lands. Mirror the
            // tolerance: an empty sidebar just stays empty.
            // The cascade can fully drain the value stack; popping a
            // return from an empty buffer is stale noise. Park the tick
            // and let the scheduler refill from the records.
            if next.stack_cursor_bytes == 0 {
                next.dispatch_stopped = true;
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.stack_return_park",
                        pc = next.program_counter,
                        return_pc = 0_u32,
                        tick = next.tick
                    );
                }
                return Ok(None);
            }
            let base = next.call_frame_bases.last().copied();
            if tracing::enabled!(tracing::Level::INFO) && next.program_counter > 20_000 {
                let ret_word = base.and_then(|b| {
                    let start = (b - 4) as usize;
                    next.stack_bytes
                        .get(start..start + 4)
                        .and_then(|bytes| bytes.try_into().ok())
                        .map(u32::from_le_bytes)
                });
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.sret_pop_trace",
                    pc = next.program_counter,
                    frame = next.current_frame,
                    bases = next.call_frame_bases.len(),
                    cursor = next.stack_cursor_bytes,
                    base = ?base,
                    ret_word = ?ret_word
                );
            }
            let frame_bytes = match base {
                Some(base) if base >= 4 => {
                    // The jve1 confirmation chain can drain the value
                    // stack below the ring's own tail frame; reading a
                    // return word from a never-initialized region is
                    // stale noise in the native too (the raw read yields
                    // garbage that the loop discards). Park instead of
                    // failing the dispatch.
                    let initialized = next
                        .stack_initialized
                        .get((base - 4) as usize..base as usize)
                        .is_some_and(|b| b.iter().all(|&x| x));
                    if initialized {
                        next.stack_bytes
                            .get((base - 4) as usize..base as usize)
                            .and_then(|b| b.try_into().ok())
                            .map(|b: [u8; 4]| (base, u32::from_le_bytes(b)))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if frame_bytes.is_none() && next.stack_cursor_bytes > 0 {
                // No valid frame link: park this tick and let the
                // scheduler refill rather than unwinding on noise.
                next.dispatch_stopped = true;
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.stack_return_park",
                        pc = next.program_counter,
                        return_pc = 0_u32,
                        tick = next.tick
                    );
                }
                return Ok(None);
            }
            let return_pc = if let Some((base, pc)) = frame_bytes {
                // Native 0x411 returns through the call-frame link: the
                // callee may leave argument residue above the frame base
                // (commands that pop nothing), so the return address is
                // read at the recorded base and the whole frame is cut.
                next.stack_cursor_bytes = base - 4;
                pc
            } else {
                pop_word(next)?
            };
            // The native tail never bounds-checks the adjustment; a
            // record-resumed frame can sit at the buffer floor. Saturate
            // instead of failing the dispatch.
            if u32::from(*stack_adjust) <= next.stack_cursor_bytes {
                drop_stack_bytes(next, *stack_adjust)?;
            } else {
                next.stack_cursor_bytes = 0;
            }
            next.call_frame_bases.pop();
            // One entry per tick: once a queued dialog dispatch's frames
            // unwind back to the captured depth, stop dispatching and
            // let the next engine frame refill from the records.
            if let Some(depth) = next.dialog_dispatch_base_depth {
                if next.call_frame_bases.len() <= depth {
                    if let Some(cursor) = next.dialog_dispatch_cursor {
                        next.stack_cursor_bytes = cursor;
                        // Scrub the dispatch layout words so the poll
                        // family's continuation reads can never adopt
                        // them (the native callback's frame never leaks
                        // into the shared queue).
                        if cursor >= 12 {
                            let start = (cursor - 12) as usize;
                            if let Some(bytes) = next.stack_bytes.get_mut(start..start + 8) {
                                bytes.fill(0);
                            }
                        }
                    }
                    next.dialog_dispatch_base_depth = None;
                    next.dispatch_stopped = true;
                    // The native ring re-arms the next poll cycle with
                    // cmd137(1) once its dispatch unwinds: reopen the
                    // refill gate so later engine frames (button polls)
                    // rebuild from the records again.
                    next.interpreter_words
                        .insert(super::schedule::FRAME_ENTRY_REBUILD_GATE_FIELD, 0);
                    if tracing::enabled!(tracing::Level::INFO) {
                        tracing::info!(
                            event = "shinki.emu.cmvs.vm.dispatch_entry_complete",
                            pc = next.program_counter,
                            tick = next.tick
                        );
                    }
                }
            }
            // The cascade can leave stale frame bases; a return target
            // below the script's first token is stale-buffer noise (the
            // native crashes identically). Park this tick instead.
            if return_pc < 2 {
                next.dispatch_stopped = true;
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.stack_return_park",
                        pc = next.program_counter,
                        return_pc,
                        tick = next.tick
                    );
                }
                return Ok(None);
            }
            // A loaded save's value stack can carry a context triple
            // captured while the frame sat inside an expression; the
            // native tail resumes at the decoded instruction boundary.
            // Snap an interior return target forward to the next valid
            // instruction start instead of failing the dispatch.
            let resume_pc = match next.script_valid_starts.get(&next.current_frame) {
                Some(starts) if !starts.contains(&return_pc) => starts
                    .iter()
                    .copied()
                    .filter(|p| *p > return_pc)
                    .min()
                    .unwrap_or(return_pc),
                _ => return_pc,
            };
            if resume_pc != return_pc && tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.return_pc_snapped",
                    saved = return_pc,
                    resume = resume_pc,
                    frame = next.current_frame,
                    tick = next.tick
                );
            }
            next.program_counter = resume_pc;
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::StackDrop {
            stack_bytes,
            ..
        }) => {
            drop_stack_bytes(next, *stack_bytes)?;
            advance(next, frame)?;
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::FrameReturn { .. }) => {
            // Opcode 0x413 pops the return PC, then adjusts the stack by
            // word[81216] * 4 bytes and leaves the call frame. The original
            // dispatcher reads the adjustment from the persistent expression
            // result word (`mov ax, [edi+13D40h]; shl ax, 2`), not from the
            // evaluator's in-register value, so the two can diverge across
            // intervening command side effects.
            if next.call_frame_bases.len() <= 1 {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_CALL_DEPTH",
                    "CMVS frame return has no active call frame",
                ));
            }
            let return_pc = pop_word(next)?;
            let adjust = next
                .interpreter_words
                .get(&VALUE_EXPRESSION_RESULT_FIELD)
                .copied()
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_CURRENT_VALUE",
                        "CMVS frame return requires an evaluator result",
                    )
                })?;
            let adjust_bytes = u16::try_from(u32::from(adjust as u16) * 4).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_STACK",
                    "CMVS frame return stack adjustment overflowed",
                )
            })?;
            drop_stack_bytes(next, adjust_bytes)?;
            next.call_frame_bases.pop();
            next.program_counter = return_pc;
            if !next.program_counter.is_multiple_of(2) {
                return Err(CoreError::invalid(
                    "SHINKI_EMU_CMVS_VM_PC_UNALIGNED",
                    format!(
                        "CMVS PS2A PC set to odd {} at vm.rs:784",
                        next.program_counter
                    ),
                ));
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::ScriptReturn { .. }) => {
            // Native 0x414: pop frame, PC, then counter from the shared
            // value stack — the operand is not consumed. The original
            // buffer is never bounds-checked: when fewer than three words
            // remain, the pops still read the retained bytes below the
            // logical cursor, so a shallow call frame "returns" through
            // the stale words of the outer frames (an unwinding return).
            // Model exactly that: read the three words at and below the
            // cursor from the retained buffer, saturate the cursor, and
            // only adopt a frame id that fits.
            let read_stale = |next: &CmvsPs2aVmState, index: u32| -> u32 {
                let end = next.stack_cursor_bytes.saturating_sub(index * 4) as usize;
                let start = end.saturating_sub(4);
                next.stack_bytes
                    .get(start..end)
                    .and_then(|bytes| bytes.try_into().ok())
                    .map(u32::from_le_bytes)
                    .unwrap_or(0)
            };
            let frame_word = read_stale(next, 0);
            let return_pc = read_stale(next, 1);
            let frame_counter = read_stale(next, 2);
            let before_cursor = next.stack_cursor_bytes;
            // The dialog cascade overdraws the entry queue faster than
            // the scheduler refills it; an empty-return would "return"
            // into stale bytes (native crashes the same way). Park the
            // frame instead: stop dispatching this tick and let the next
            // rebuild refill the queue from the records. Partial stacks
            // keep the native stale-read tolerance.
            if before_cursor == 0 {
                next.stack_cursor_bytes = 0;
                next.dispatch_stopped = true;
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.script_return_park",
                        pc = next.program_counter,
                        tick = next.tick
                    );
                }
                return Ok(None);
            }
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.script_return",
                    pc = next.program_counter,
                    frame = next.current_frame,
                    cursor_before = before_cursor,
                    restored_frame = frame_word,
                    restored_pc = return_pc,
                    restored_counter = frame_counter
                );
            }
            next.stack_cursor_bytes = next.stack_cursor_bytes.saturating_sub(12);
            // A cascaded triple can carry a mid-token pc (the native
            // crashes there). Resuming only at proven instruction starts
            // parks those instead and lets the records refill the queue;
            // a pc below the script's first token is stale-buffer noise
            // even when a cascaded frame word happens to list it.
            if return_pc < 2 {
                next.dispatch_stopped = true;
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.script_return_park",
                        pc = next.program_counter,
                        return_pc,
                        tick = next.tick
                    );
                }
                return Ok(None);
            }
            if let Ok(frame) = u16::try_from(frame_word) {
                next.current_frame = frame;
            }
            next.program_counter = return_pc;
            next.interpreter_words
                .insert(SCRIPT_FRAME_COUNTER_FIELD, frame_counter);
            // The cascade unwinds queued dispatches through script
            // returns; the one-entry-per-frame stop must fire here too
            // so every dispatch's cleanup runs.
            if let Some(depth) = next.dialog_dispatch_base_depth {
                if next.call_frame_bases.len() <= depth {
                    if let Some(cursor) = next.dialog_dispatch_cursor {
                        next.stack_cursor_bytes = cursor;
                        if cursor >= 12 {
                            let start = (cursor - 12) as usize;
                            if let Some(bytes) = next.stack_bytes.get_mut(start..start + 8) {
                                bytes.fill(0);
                            }
                        }
                    }
                    next.dialog_dispatch_base_depth = None;
                    next.dialog_dispatch_cursor = None;
                    next.dispatch_stopped = true;
                    if tracing::enabled!(tracing::Level::INFO) {
                        tracing::info!(
                            event = "shinki.emu.cmvs.vm.dispatch_entry_complete",
                            pc = next.program_counter,
                            via = "script_return",
                            tick = next.tick
                        );
                    }
                }
            }
            // Record resumes into the dispatcher head arrive without the
            // frame locals the native Win32 callback lays when arming the
            // record. Lay them here so the head's fl8/fl12 reads are the
            // cascade's message identity instead of stack residue.
            if return_pc == 25_366 {
                super::stack::set_push_origin("dialog");
                // A record resume is a jump, not a call: the return slot
                // parks through the stale guard. The identity lands at
                // the head's fl8 (base - 8) and its mirror at fl12.
                let identity = next.process_indexed_words.get(&17).copied().unwrap_or(0);
                // The sentinel (-1) is the "no message" park: the native
                // entry gate drops it instead of running the dispatcher's
                // fall-through tail over stale operands.
                let identity = if identity == u32::MAX {
                    // A queued click is still pending: its identity is the
                    // message the ring just consumed; adopt it (with its
                    // frame). Otherwise no message is pending: the native
                    // entry gate drops the resume entirely.
                    match next.pending_dialog_dispatches.pop() {
                        Some((_, queued_frame, queued_id)) => {
                            next.current_frame = queued_frame;
                            queued_id
                        }
                        None => {
                            next.dispatch_stopped = true;
                            if tracing::enabled!(tracing::Level::INFO) {
                                tracing::info!(
                                    event = "shinki.emu.cmvs.vm.sentinel_entry_parked",
                                    pc = next.program_counter,
                                    tick = next.tick
                                );
                            }
                            return Ok(None);
                        }
                    }
                } else if identity == 0 {
                    // A zero identity is not a message either.
                    next.dispatch_stopped = true;
                    return Ok(None);
                } else {
                    identity
                };
                super::stack::push_word_public(next, identity)?;
                super::stack::push_word_public(next, identity)?;
                super::stack::push_word_public(next, 0)?;
                super::stack::push_call_frame_base(next)?;
                super::stack::set_push_origin("push");
                // Arm the same completion cleanup as queued dispatches:
                // the poll chain unwinds this frame the same way and its
                // layout words must not leak into the shared queue.
                next.dialog_dispatch_base_depth = Some(next.call_frame_bases.len());
                next.dialog_dispatch_cursor = Some(next.stack_cursor_bytes - 12);
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.dispatcher_locals_laid",
                        pc = next.program_counter,
                        tick = next.tick
                    );
                }
            }
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushCurrentValue {
            ..
        }) => {
            // Native case 1072 (`0x430`, `sub_46DAF0` at 0x46ddbd) pushes the
            // persistent expression-result word: `mov eax, [edi+13D40h]`,
            // i.e. interpreter dword 81216 — the same field every expression
            // evaluator stores its coerced result into (`sub_46F470` tail).
            // The in-register `current_value` mirrors it; the word is read
            // first so a command that stores through 81216 between the
            // expression and this op keeps the divergence native.
            let value = next
                .interpreter_words
                .get(&VALUE_EXPRESSION_RESULT_FIELD)
                .copied()
                .or(next.current_value)
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_CURRENT_VALUE",
                        "CMVS PS2A push requires an evaluator result",
                    )
                })?;
            super::stack::set_push_origin("0x430");
            push_word(next, value)?;
            super::stack::set_push_origin("push");
            advance(next, frame)?;
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            stack_bytes,
            value,
            ..
        }) => {
            super::stack::set_push_origin("0x440");
            push_immediate(next, *value, *stack_bytes)?;
            super::stack::set_push_origin("push");
            advance(next, frame)?;
            None
        }
        CmvsPs2aInstructionFrame::Command { command_id, .. } => {
            if tracing::enabled!(tracing::Level::DEBUG)
                && matches!(command_id, 40..=72 | 193 | 129 | 136 | 137 | 138 | 139 | 140 | 284..=288 | 320 | 333 | 334 | 348 | 349 | 751)
            {
                tracing::debug!(
                    event = "shinki.emu.cmvs.vm.texcmd",
                    pc = next.program_counter,
                    frame = next.current_frame,
                    id = command_id,
                    parents = next.texture_parents.len(),
                    children = next
                        .texture_children
                        .values()
                        .map(BTreeMap::len)
                        .sum::<usize>()
                );
            }
            let contract = match cmvs390_command_contract(*command_id) {
                Some(contract) => contract,
                None => {
                    if tracing::enabled!(tracing::Level::INFO) {
                        let c = next.stack_cursor_bytes as usize;
                        let words: Vec<u32> = (1..=10)
                            .filter_map(|i| {
                                let end = c.saturating_sub(i * 4);
                                next.stack_bytes
                                    .get(end.saturating_sub(4)..end)
                                    .and_then(|b| b.try_into().ok())
                                    .map(u32::from_le_bytes)
                            })
                            .collect();
                        tracing::info!(
                            event = "shinki.emu.cmvs.vm.missing_contract_args",
                            command = *command_id,
                            script_is_menu = next
                                .script_frames
                                .get(&next.current_frame)
                                .is_some_and(|identity| identity.script_uri.contains("menu")),
                            script_tag = next
                                .script_frames
                                .get(&next.current_frame)
                                .map(|identity| {
                                    let b = identity.script_hash.as_bytes();
                                    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
                                }),
                            segment_size = next
                                .script_data_segment_sizes
                                .get(&next.current_frame)
                                .copied()
                                .unwrap_or(0),
                            frame = next.current_frame,
                            cursor = next.stack_cursor_bytes,
                            top_words = ?words
                        );
                    }
                    return Err(CoreError::invalid(
                        "SHINKI_EMU_CMVS_VM_COMMAND",
                        format!(
                            "CMVS PS2A command {} has no recovered execution contract at pc {} in frame {}",
                            command_id, next.program_counter, next.current_frame
                        ),
                    ));
                }
            };
            // Case 138 transfers dispatch to the registered record address
            // itself; the interpreter loop must not advance the PC again
            // after its effect runs. `command 0` returns `0xC000` in the
            // original dispatcher: the negative bit ends the frame, but the
            // `0x4000` pop flag still applies, so the PC advances past the
            // command word before the dispatch yields.
            let program_counter_overridden = matches!(
                contract.effect_kind,
                CmvsPs2aCommandEffectKind::ResumeInterpreterCoroutineRecord { .. }
            );
            let action = execute_command(
                next,
                contract.effect_kind,
                contract.stack_pop_bytes,
                &contract.stack_words,
            )?;
            // The recovered 0xA000 wait class ends the frame without
            // advancing the PC or popping, so the command re-executes next
            // frame against the refreshed input snapshot.
            if std::mem::take(&mut next.wait_frame) {
                next.dispatch_stopped = true;
                // The native suspend stores the continuation triple at
                // dwords 3304..3306 (frame, pc, frame-counter) so the next
                // tick's refill resumes here and the armed record can adopt
                // the live continuation. The disabled write left stale words
                // that the resurrected engine tick adopted as its resume
                // point (the 200640 misalignment crash).
                next.interpreter_words
                    .insert(13_216, u32::from(next.current_frame));
                next.interpreter_words.insert(13_220, next.program_counter);
                next.interpreter_words.insert(
                    13_224,
                    next.interpreter_words
                        .get(&13_224)
                        .copied()
                        .unwrap_or_default(),
                );
                if tracing::enabled!(tracing::Level::INFO) {
                    let last = next.last_frame_stop_pcs.get(&next.current_frame).copied();
                    if last != Some(next.program_counter) {
                        next.last_frame_stop_pcs
                            .insert(next.current_frame, next.program_counter);
                        if next.current_frame == 0 {
                            tracing::info!(
                                event = "shinki.emu.cmvs.vm.f0_wait_transition",
                                pc = next.program_counter,
                                command_id,
                                stack = next.stack_cursor_bytes
                            );
                        }
                    }
                }
                return Ok(action);
            }
            if !program_counter_overridden {
                advance(next, frame)?;
            }
            action
        }
    };
    if next.current_frame == 0 && vm_trace_enabled() {
        tracing::trace!(
            event = "shinki.emu.cmvs.vm.f0_trace",
            pc = next.program_counter,
            stack = next.stack_cursor_bytes,
            stopped = next.dispatch_stopped
        );
    }
    if next.current_frame == 0
        && (40_000..210_000).contains(&next.program_counter)
        && vm_trace_enabled()
        && tracing::enabled!(tracing::Level::TRACE)
    {
        // RE aid: story-sequence pcs (sky01's own code around its
        // 199586 entry) — change-gated like the stop trace so the
        // story's progress through its opening sequence is visible.
        // The instruction kind names the wait when the story parks.
        if next.last_story_pc != Some(next.program_counter)
            || (next.dispatch_stopped && next.last_story_pc.is_some())
        {
            let kind = match frame {
                CmvsPs2aInstructionFrame::Command { command_id, .. } => {
                    format!("cmd{command_id}")
                }
                other => {
                    let text = format!("{other:?}");
                    // `Control(AbsoluteJump { .. })` → `AbsoluteJump`.
                    let inner = text
                        .split_once('(')
                        .and_then(|(_, rest)| rest.trim_start().split([' ', '{']).next())
                        .unwrap_or("");
                    if inner.is_empty() {
                        text.split('(').next().unwrap_or("?").to_string()
                    } else {
                        inner.to_string()
                    }
                }
            };
            if next.last_story_pc != Some(next.program_counter) {
                next.last_story_pc = Some(next.program_counter);
            }
            tracing::trace!(
                event = "shinki.emu.cmvs.vm.story_pc",
                pc = next.program_counter,
                kind = %kind,
                stack = next.stack_cursor_bytes,
                stopped = next.dispatch_stopped
            );
        }
    }
    // Tick-end pc transitions per frame: a stall names its waiter. Frame 0
    // keeps its dedicated event; the map feeds the idle snapshot.
    if next.dispatch_stopped {
        let last = next.last_frame_stop_pcs.get(&next.current_frame).copied();
        if last != Some(next.program_counter) {
            next.last_frame_stop_pcs
                .insert(next.current_frame, next.program_counter);
            if next.current_frame == 0 && tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.f0_stop_transition",
                    pc = next.program_counter,
                    stack = next.stack_cursor_bytes
                );
            }
        }
    }
    let depth_after = next.stack_cursor_bytes;
    let trace_pc = next.program_counter;
    let trace_frame = next.current_frame;
    if trace_frame != 0 && vm_trace_enabled() {
        tracing::trace!(
            event = "shinki.emu.cmvs.vm.ctl_trace",
            pc = trace_pc,
            frame = trace_frame,
            depth_before,
            depth_after
        );
    }
    if depth_after != depth_before && vm_trace_enabled() {
        tracing::trace!(
            event = "shinki.emu.cmvs.vm.depth_trace",
            pc = trace_pc,
            frame = trace_frame,
            depth_before,
            depth_after
        );
    }
    Ok(action)
}
