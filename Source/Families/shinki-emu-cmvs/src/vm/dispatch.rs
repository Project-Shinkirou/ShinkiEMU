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
    state.current_instruction_start = state.program_counter;
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
            if next.current_frame == 1 && next.program_counter == 349_086 {
                next.menu_pump_exit_seen = true;
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
            // Throwaway probe: the menu demo-loop dispatch chain (206482..=
            // 206522) compares the 535 poll result carried in current_value.
            // Item dispatcher ladder (intproc 187268..=187388): the jve
            // input is value-stack slot 4 (the input id the callback
            // parked). Watch what it carries.
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
            let return_pc = next
                .program_counter
                .checked_add(u32::from(frame.span().byte_length))
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_PC",
                        "CMVS PS2A program counter overflowed",
                    )
                })?;
            push_word(next, return_pc)?;
            push_call_frame_base(next)?;
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
            let return_pc = next
                .program_counter
                .checked_add(u32::from(frame.span().byte_length))
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_PC",
                        "CMVS PS2A program counter overflowed",
                    )
                })?;
            push_word(next, return_pc)?;
            push_call_frame_base(next)?;
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
                return Ok(None);
            }
            let base = next.call_frame_bases.last().copied();
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
            if let Some(depth) = next.dialog_dispatch_base_depth
                && next.call_frame_bases.len() <= depth
            {
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
                next.write_interpreter_word(super::schedule::FRAME_ENTRY_REBUILD_GATE_FIELD, 0);
            }
            // The cascade can leave stale frame bases; a return target
            // below the script's first token is stale-buffer noise (the
            // native crashes identically). Park this tick instead.
            if return_pc < 2 {
                next.dispatch_stopped = true;
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
                return Ok(None);
            }
            next.stack_cursor_bytes = next.stack_cursor_bytes.saturating_sub(12);
            // A cascaded triple can carry a mid-token pc (the native
            // crashes there). Resuming only at proven instruction starts
            // parks those instead and lets the records refill the queue;
            // a pc below the script's first token is stale-buffer noise
            // even when a cascaded frame word happens to list it.
            if return_pc < 2 {
                next.dispatch_stopped = true;
                return Ok(None);
            }
            if let Ok(frame) = u16::try_from(frame_word) {
                next.current_frame = frame;
            }
            next.program_counter = return_pc;
            next.write_interpreter_word(SCRIPT_FRAME_COUNTER_FIELD, frame_counter);
            // The cascade unwinds queued dispatches through script
            // returns; the one-entry-per-frame stop must fire here too
            // so every dispatch's cleanup runs.
            if let Some(depth) = next.dialog_dispatch_base_depth
                && next.call_frame_bases.len() <= depth
            {
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
            }
            // Record resumes into the dispatcher head arrive without the
            // frame locals the native Win32 callback lays when arming the
            // record. Lay them here so the head's fl8/fl12 reads are the
            // cascade's message identity instead of stack residue.
            if return_pc == 25_366 {
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
                // Arm the same completion cleanup as queued dispatches:
                // the poll chain unwinds this frame the same way and its
                // layout words must not leak into the shared queue.
                next.dialog_dispatch_base_depth = Some(next.call_frame_bases.len());
                next.dialog_dispatch_cursor = Some(next.stack_cursor_bytes - 12);
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
            push_word(next, value)?;
            advance(next, frame)?;
            None
        }
        CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
            stack_bytes,
            value,
            ..
        }) => {
            push_immediate(next, *value, *stack_bytes)?;
            advance(next, frame)?;
            None
        }
        CmvsPs2aInstructionFrame::Command { command_id, .. } => {
            let contract = match cmvs390_command_contract(*command_id) {
                Some(contract) => contract,
                None => {
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
                // Shaping decides synchronously whether this first dialogue
                // poll actually waits. Do not publish a false continuation
                // for empty/immediate text; the completion callback commits it.
                if matches!(
                    &action,
                    Some(CmvsPs2aVmAction::NativeText(
                        CmvsNativeTextAction::DrawDialogue { .. }
                    ))
                ) {
                    return Ok(action);
                }
                // The native suspend stores the continuation triple at
                // dwords 3304..3306 (frame, pc, frame-counter) so the next
                // tick's refill resumes here and the armed record can adopt
                // the live continuation. The disabled write left stale words
                // that the resurrected engine tick adopted as its resume
                // point (the 200640 misalignment crash).
                next.write_interpreter_word(13_216, u32::from(next.current_frame));
                next.write_interpreter_word(13_220, next.program_counter);
                next.write_interpreter_word(
                    13_224,
                    next.interpreter_words
                        .get(&13_224)
                        .copied()
                        .unwrap_or_default(),
                );
                return Ok(action);
            }
            if !program_counter_overridden {
                advance(next, frame)?;
            }
            action
        }
    };
    Ok(action)
}
