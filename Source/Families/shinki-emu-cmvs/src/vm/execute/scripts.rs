use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::CallScript,
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, slot),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, name),
            ],
        ) => {
            if *slot >= crate::CMVS_MAX_SCRIPT_CALL_INDEX {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_SCRIPT_FRAME",
                    "CMVS nested script frame index exceeds the recovered bound",
                ));
            }
            let frame = u16::try_from(*slot + 1).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_SCRIPT_FRAME",
                    "CMVS nested script frame index overflowed",
                )
            })?;
            let name = resolve_resource_name(state, *name)?;
            // The loader pops both operands through the contract's
            // `stack_pop_bytes` before this branch runs; the dispatcher
            // result 0 adds no further pop.  It then pushes the frame
            // counter, the advanced PC and the previous frame index before
            // the host transfers dispatch to the loaded script.
            let frame_counter = state
                .interpreter_words
                .get(&SCRIPT_FRAME_COUNTER_FIELD)
                .copied()
                .unwrap_or(0);
            let return_pc = state.program_counter.checked_add(2).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_PC",
                    "CMVS PS2A program counter overflowed",
                )
            })?;
            push_word(state, frame_counter)?;
            push_word(state, return_pc)?;
            push_word(state, u32::from(state.current_frame))?;
            state.current_frame = frame;
            Ok(Some(CmvsPs2aVmAction::CallScript { frame, name }))
        }
        (
            CmvsPs2aCommandEffectKind::ReloadRootScript,
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, name)],
        ) => {
            let name = resolve_resource_name(state, *name)?;
            Ok(Some(reload_root_script(state, name)))
        }
        (CmvsPs2aCommandEffectKind::ResetEngineToMain, []) => {
            // `sub_477AC0` frees every engine-side table before the reload:
            // the 256-slot channel table, the filter-chain banks, texture
            // parents and children, native text surfaces, effect channels,
            // elements and playback, resource slots and the component word
            // graph. Fonts follow their text objects in the teardown.
            state.slot_objects.clear();
            state.texture_parents.clear();
            state.texture_children.clear();
            state.texture_presentation = None;
            state.texture_reloads.clear();
            state.effect_channels.clear();
            state.effect_elements.clear();
            state.effect_playback.clear();
            state.native_text.clear();
            state.native_text_reveals.clear();
            state.native_text_fonts.clear();
            state.native_font_owners.clear();
            state.native_glyphs.clear();
            state.resource_slots.clear();
            state.resource_channel_slots.clear();
            state.component_words.clear();
            // The teardown also parks the engine-state dwords the reload
            // path repopulates: the scene-active flag rises while the
            // reload-request and screen fields drop to their boot values.
            for (field, value) in [
                (9652_u32, 1_u32),
                (1464, 0),
                (1500, 0),
                (10572, 0),
                (10568, 1),
                (10708, 0),
                (10576, 0),
                (3260, 0),
            ] {
                state.write_interpreter_word(field, value);
            }
            let action = reload_root_script(state, "main.ps3".to_string());
            // Post-reload the handler clears the record-28 fallback entry
            // (flag 0, frame 0, pc -1) so the scheduler fallback stays dead
            // until the boot flow registers it again.
            let base = COROUTINE_RECORD_TABLE_BASE + COROUTINE_RECORD_STRIDE * 28;
            state.write_interpreter_word(base - 4, 0);
            state.write_interpreter_word(base, 0);
            state.write_interpreter_word(base + 4, u32::MAX);
            Ok(Some(action))
        }
        (CmvsPs2aCommandEffectKind::SaveSystemState, []) => {
            // `sub_427E80` serializes the system state synchronously; the
            // host performs the file write on the action boundary.
            Ok(Some(CmvsPs2aVmAction::SaveSystemState))
        }
        (
            CmvsPs2aCommandEffectKind::OpenScenarioStream {
                loading_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, name)],
        ) => {
            // `sub_479060`: resolve the resource name, mark the stream
            // loading (interpreter dword at byte offset 0x29d4) and suspend
            // the calling coroutine until the load completes. The headless
            // archive resolves synchronously, so the suspension is the
            // observable half: the dispatch ends with the pc already past
            // the command, and the scheduler resumes the coroutine next
            // frame (`0xC004`).
            let reference = resolve_resource_name(state, *name)?;
            state.write_interpreter_word(loading_field_offset, 1);
            state.dispatch_stopped = true;
            Ok(Some(CmvsPs2aVmAction::OpenScenarioStream {
                resource: reference,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SetNativeFullscreenSuspend,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, top)],
        ) => {
            let enabled = *top != 0;
            if state.native_view.fullscreen != enabled {
                state.write_interpreter_word(10516, 0);
                state.write_interpreter_word(10520, 0);
            }
            state.native_view.fullscreen = enabled;
            state.native_view.reset_viewport(state.native_render_size);
            state.write_interpreter_word(10708, 1);
            state.dispatch_stopped = true;
            Ok(Some(CmvsPs2aVmAction::SetNativeFullscreen { enabled }))
        }
        (
            CmvsPs2aCommandEffectKind::ResumeInterpreterCoroutineRecord {
                record_table_base_offset,
                record_stride_bytes,
                max_index_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, label)],
        ) => {
            // `sub_47CB00` masks the label like the record writer does and
            // indexes the same 28-byte table.
            let label = *label & u32::from(max_index_mask);
            let base = u32::from(record_table_base_offset)
                .checked_add(u32::from(record_stride_bytes).wrapping_mul(label))
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_FIELD",
                        "CMVS coroutine record offset overflowed",
                    )
                })?;
            // The native record keeps the frame id at +0 (`0x33DC`, the
            // frame register snapshot at registration) and the registered
            // pc at +4 (`0x33E0`); the resume (`sub_47CB00`) restores both
            // the pc and the frame registers from the record.
            let (recorded_frame, recorded_pc) = match (
                state.interpreter_words.get(&base),
                state.interpreter_words.get(&(base + 4)),
            ) {
                (Some(frame), Some(pc)) => (*frame, *pc),
                _ => {
                    return Err(invalid(
                        "SHINKI_EMU_CMVS_VM_COROUTINE",
                        "CMVS coroutine label has no registered continuation record",
                    ));
                }
            };
            // Native jumps through the registered address. An invalidated
            // target is an invalid continuation, never successful playback.
            if recorded_pc == u32::MAX {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_COROUTINE",
                    "CMVS coroutine continuation was invalidated",
                ));
            }
            let frame = u16::try_from(recorded_frame).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_COROUTINE",
                    "CMVS coroutine record frame index overflowed",
                )
            })?;
            // The original advances the PC past the opcode before pushing
            // the return record (`sub_47CB00`); the frame-counter value is
            // read after that advance, matching the original field order.
            let return_pc = state.program_counter.checked_add(2).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_PC",
                    "CMVS PS2A program counter overflowed",
                )
            })?;
            // sub_47CB00 saves the caller's counter before resetting it;
            // the destination frame id is not a counter.
            let caller_counter = state
                .interpreter_words
                .get(&SCRIPT_FRAME_COUNTER_FIELD)
                .copied()
                .unwrap_or_default();
            push_word(state, caller_counter)?;
            push_word(state, return_pc)?;
            push_word(state, u32::from(state.current_frame))?;
            state.write_interpreter_word(SCRIPT_FRAME_COUNTER_FIELD, 0);

            state.current_frame = frame;
            state.program_counter = recorded_pc;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryScriptSlotOccupancy {
                slot_table_dword_index,
                max_slot,
                error_flag_field_offset,
                error_flag_mask,
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, slot),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, selector),
            ],
        ) => {
            // 480560 addresses this[481 + slot] in DWORDs. Non-negative
            // selectors query the actual child table; any negative selects root.
            if *slot > max_slot {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            let key = slot_object_key(
                slot_table_dword_index
                    .checked_mul(4)
                    .and_then(|offset| u16::try_from(offset).ok())
                    .ok_or_else(|| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_SLOT_OBJECTS",
                            "CMVS slot object table offset overflowed",
                        )
                    })?,
                *slot,
            )?;
            let channel_live = state.slot_objects.contains_key(&key);
            let occupied = channel_live
                && ((*selector as i32) < 0
                    || (*selector < 1024
                        && state
                            .texture_children
                            .get(&(*slot as u8))
                            .is_some_and(|children| children.contains_key(&(*selector as u16)))));
            state.write_interpreter_word(result_field_offset, u32::from(occupied));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::DestroyEffectPlayback { max_index },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index)],
        ) => {
            // `sub_488CF0`: free the playback record at the stack-top index
            // and clear its table entry; the headless record carries no
            // native resources. The playback's channel elements die with it
            // (`sub_466740` walks the object graph), dropping the
            // scheduler's live-element gate for their channels.
            if *index <= max_index {
                state.effect_playback.remove(&(*index as u8));
            }
            state
                .effect_elements
                .retain(|(_, element_playback), _| *element_playback != (*index) & 0xffff);
            state.write_interpreter_word(81220, u32::from(*index <= max_index));
            Ok(None)
        }
        _ => Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
            format!(
                "CMVS PS2A command stack words do not match the recovered effect contract at pc {} frame {}: {:?}",
                state.program_counter, state.current_frame, effect_kind
            ),
        )),
    }
}

/// `sub_4781B0`: resets the data stack, the frame stack-table words and the
/// current frame before the new root script takes over, and invalidates the
/// root-frame coroutine records lazily. The expression-result field zeroes
/// with its flag bit.
fn reload_root_script(state: &mut CmvsPs2aVmState, name: String) -> CmvsPs2aVmAction {
    state.stack_bytes.clear();
    state.stack_initialized.clear();
    state.stack_cursor_bytes = 0;
    state.call_frame_bases.clear();
    // The native first dispatch after `sub_4781B0` re-establishes the
    // root triple (`this[3306/3305/3304]`) on the cleared stack, so a
    // drained yield lands on the script entry instead of session junk.
    for word in [0u32, 0, 0] {
        state.stack_bytes.extend_from_slice(&word.to_le_bytes());
        state.stack_initialized.push(true);
        state.stack_cursor_bytes += 4;
    }
    state.call_frame_bases.push(state.stack_cursor_bytes);
    state.current_frame = 0;
    state.current_value = None;
    state.condition_flag = false;
    state.write_interpreter_word(80952, 0);
    state.write_interpreter_word(81208, 0);
    state.write_interpreter_word(81212, 0);
    state.write_interpreter_word(81216, 0);
    for label in 0..COROUTINE_RECORD_COUNT {
        let base = COROUTINE_RECORD_TABLE_BASE + COROUTINE_RECORD_STRIDE * label;
        let frame = state.interpreter_words.get(&base).copied();
        // A record whose frame word was never written (absent) or belongs
        // to the reloaded root is invalidated; records from other resident
        // frames (e.g. intproc) stay live.
        let invalidatable = matches!(frame, None | Some(0));
        if invalidatable && state.interpreter_words.contains_key(&(base + 4)) {
            state.write_interpreter_word(base + 4, u32::MAX);
            state.write_interpreter_word(base - 4, 0);
        }
    }
    CmvsPs2aVmAction::ReloadRootScript { name }
}
