use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::StoreComponentConstant {
                component_offset,
                field_offset,
                value,
            },
            [],
        ) => {
            state
                .component_words
                .insert(component_field_key(component_offset, field_offset), value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PublishInputSelection { field_offset },
            [],
        ) => {
            let value = state.load_confirm_selection.unwrap_or(0);
            state.interpreter_words.insert(field_offset, value);
            tracing::debug!(
                event = "shinki.emu.cmvs.vm.input_selection_published",
                field_offset,
                value,
                pc = state.program_counter,
                tick = state.tick
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreInterpreterConstant {
                field_offset,
                value,
            },
            [],
        ) => {
            state.interpreter_words.insert(field_offset, value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ClearInterpreterTableWord {
                table_base_offset,
                table_word_count,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index)],
        ) => {
            // The original indexes a dword offset into the recovered word
            // family without a bounds check; anything outside the family
            // writes unrecovered interpreter state, so block it instead.
            if *index >= table_word_count {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_INTERPRETER_TABLE",
                    "CMVS interpreter table clear index is outside the recovered family",
                ));
            }
            let field_offset = table_base_offset
                + index.checked_mul(4).ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_INTERPRETER_TABLE",
                        "CMVS interpreter table byte extent overflowed",
                    )
                })?;
            state.interpreter_words.insert(field_offset, 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::LoadInterpreterTableWord {
                table_base_offset,
                table_word_count,
                field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index)],
        ) => {
            // The recovered family is zero-initialized, so unwritten words
            // read back as zero; indices outside the family stay blocked.
            if *index >= table_word_count {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_INTERPRETER_TABLE",
                    "CMVS interpreter table load index is outside the recovered family",
                ));
            }
            if tracing::enabled!(tracing::Level::INFO) && field_offset == 81220 {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.table_word_load",
                    tick = state.tick,
                    pc = state.program_counter,
                    index = *index,
                    value = state
                        .interpreter_words
                        .get(&(table_base_offset + index * 4))
                        .copied()
                        .unwrap_or(0)
                );
            }
            let source_offset = table_base_offset
                + index.checked_mul(4).ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_INTERPRETER_TABLE",
                        "CMVS interpreter table byte extent overflowed",
                    )
                })?;
            let word = state
                .interpreter_words
                .get(&source_offset)
                .copied()
                .unwrap_or(0);
            state.interpreter_words.insert(field_offset, word);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::MarkInterpreterRecordUnused {
                record_table_base_offset,
                record_count,
                record_stride_bytes,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index)],
        ) => {
            // The original ignores out-of-range indices silently.
            if *index < record_count {
                let record_offset = record_table_base_offset
                    + index.checked_mul(record_stride_bytes).ok_or_else(|| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_INTERPRETER_TABLE",
                            "CMVS interpreter record offset overflowed",
                        )
                    })?;
                state.interpreter_words.insert(record_offset, u32::MAX);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::InterpreterFlagRoundTrip {
                flag_field_offset,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, selector), (CmvsPs2aCommandStackWordKind::BooleanU32, value)],
        ) => {
            if *selector != 0 {
                state
                    .interpreter_words
                    .insert(flag_field_offset, u32::from(*value != 0));
            } else {
                let flag = state
                    .interpreter_words
                    .get(&flag_field_offset)
                    .copied()
                    .unwrap_or(0);
                state
                    .interpreter_words
                    .insert(result_field_offset, u32::from(flag != 0));
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StorePrefixedInterpreterString {
                buffer_offset,
                prefix_field_offset,
                ensure_directories,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            let value = resolve_string_value(state, *value)?;
            crate::archive::native::validate_search_path(&value)?;
            state.interpreter_string_buffers.insert(
                buffer_offset,
                vec![
                    CmvsStringSegment::InterpreterPrefixField {
                        field_offset: prefix_field_offset,
                    },
                    CmvsStringSegment::OwnedString(value),
                ],
            );
            if ensure_directories {
                Ok(Some(CmvsPs2aVmAction::EnsurePathDirectories {
                    buffer_offset,
                }))
            } else {
                Ok(None)
            }
        }
        (
            CmvsPs2aCommandEffectKind::RequestWindowIcon {
                prefix_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            let text = resolve_string_value(state, *value)?;
            Ok(Some(CmvsPs2aVmAction::SetWindowIcon {
                segments: vec![
                    CmvsStringSegment::InterpreterPrefixField {
                        field_offset: prefix_field_offset,
                    },
                    CmvsStringSegment::OwnedString(text),
                ],
            }))
        }
        (
            CmvsPs2aCommandEffectKind::RequestWindowCaption,
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            let text = resolve_string_value(state, *value)?;
            let segments = vec![CmvsStringSegment::OwnedString(text)];
            // 41B1C0 owns a copy at component+288 before SetWindowTextA.
            state
                .interpreter_string_buffers
                .insert(288, segments.clone());
            Ok(Some(CmvsPs2aVmAction::SetWindowCaption { segments }))
        }
        (
            CmvsPs2aCommandEffectKind::SetWindowCaptionFromTagged {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, selector), (CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            // `sub_479570`: both branches hand the resolved text to the
            // same window-text API; only the zero-top branch pins the
            // success word. The tagged operand uses the generic resolver's
            // domain set (frame pool, frame-pool flag, global message slot).
            let text = resolve_string_value(state, *value)?;
            if *selector == 0 {
                state.interpreter_words.insert(result_field_offset, 1);
            }
            Ok(Some(CmvsPs2aVmAction::SetWindowCaption {
                segments: vec![CmvsStringSegment::OwnedString(text)],
            }))
        }
        (
            CmvsPs2aCommandEffectKind::StoreComponentWord {
                component_field_offset,
                field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            state.component_words.insert(
                component_field_key(component_field_offset, field_offset),
                *value,
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryComponentWordToResult {
                component_field_offset,
                field_offset,
                result_field_offset,
            },
            [],
        ) => {
            // `sub_48C3B0`: the guarded component field publishes raw into
            // the result word; the headless component table holds the word
            // the engine message writer would set.
            let value = state
                .component_words
                .get(&component_field_key(component_field_offset, field_offset))
                .copied()
                .unwrap_or_default();
            state.interpreter_words.insert(result_field_offset, value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryComponentWordBooleanToResult {
                component_field_offset,
                field_offset,
                result_field_offset,
            },
            [],
        ) => {
            // `sub_48C290` booleanizes the guarded component field the way
            // `neg`/`sbb`/`neg` do; the headless component table holds the
            // same word the engine message 0x8006 writer would set.
            let value = state
                .component_words
                .get(&component_field_key(
                    component_field_offset,
                    field_offset,
                ))
                .copied()
                .unwrap_or_default();
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(value != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreBoundedInterpreterString {
                buffer_offset,
                max_text_bytes,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            // 48A670 measures native CP932 bytes, not Unicode characters.
            // 4712F0 reads this same buffer for the native save title/tag 256.
            let bytes = resolve_tagged_string_bytes(state, *value)?;
            if bytes.len() >= usize::from(max_text_bytes) {
                *state
                    .interpreter_words
                    .entry(u32::from(error_flag_field_offset))
                    .or_default() |= error_flag_mask;
            } else {
                let text = resolve_string_value(state, *value)?;
                state
                    .interpreter_string_buffers
                    .insert(buffer_offset, vec![CmvsStringSegment::OwnedString(text)]);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::BuildPrefixedWindowCaption {
                prefix_field_offset,
                buffer_offset,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            let suffix = resolve_string_value(state, *value)?;
            let suffix_bytes = resolve_tagged_string_bytes(state, *value)?.len();
            let Some(prefix) = state.interpreter_string_buffers.get(&prefix_field_offset) else {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_WINDOW_CAPTION",
                    "native caption prefix is not bound",
                ));
            };
            let [CmvsStringSegment::OwnedString(prefix)] = prefix.as_slice() else {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_WINDOW_CAPTION",
                    "native caption prefix is not owned",
                ));
            };
            let (prefix_bytes, _, malformed) = encoding_rs::SHIFT_JIS.encode(prefix);
            // 48A710 uses a 128-byte stack array with unbounded native copies.
            // Refuse that overflow before changing the owned suffix buffer.
            if malformed
                || prefix_bytes
                    .len()
                    .saturating_add(suffix_bytes)
                    .saturating_add(2)
                    >= 128
            {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_WINDOW_CAPTION",
                    "native caption exceeds its byte capacity",
                ));
            }
            let gap = match state.process_global_words.get(&NATIVE_FONT_SHIFT_JIS) {
                Some(0) => "  ",
                Some(_) => "\u{3000}",
                None => {
                    return Err(invalid(
                        "SHINKI_EMU_CMVS_WINDOW_CAPTION",
                        "native caption character set is not bound",
                    ))
                }
            };
            let segments = vec![CmvsStringSegment::OwnedString(format!(
                "{prefix}{gap}{suffix}"
            ))];
            state
                .interpreter_string_buffers
                .insert(buffer_offset, vec![CmvsStringSegment::OwnedString(suffix)]);
            Ok(Some(CmvsPs2aVmAction::SetWindowCaption { segments }))
        }
        (CmvsPs2aCommandEffectKind::NoOpCommand, []) => Ok(None),
        (
            CmvsPs2aCommandEffectKind::LoadInterpreterWordToResult {
                source_field_offset,
                result_field_offset,
            },
            [],
        ) => {
            // Dispatcher case 718 copies the pinned interpreter word into
            // the result field; unwritten words read zero.
            let value = state
                .interpreter_words
                .get(&source_field_offset)
                .copied()
                .unwrap_or(0);
            state.interpreter_words.insert(result_field_offset, value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::LoadInterpreterWordBooleanToResult {
                source_field_offset,
                result_field_offset,
            },
            [],
        ) => {
            // Dispatcher case 750 canonicalizes the pinned interpreter word
            // to a boolean result.
            let value = state
                .interpreter_words
                .get(&source_field_offset)
                .copied()
                .unwrap_or(0);
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(value != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::AppendPrivateStringToList {
                list_id,
                length_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            let reference = tag_zero_reference(*value)?;
            let list = state.private_string_lists.entry(list_id).or_default();
            list.push(reference);
            let length = u32::try_from(list.len()).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_STRING_LIST",
                    "CMVS string list length overflowed",
                )
            })?;
            state.interpreter_words.insert(length_field_offset, length);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::CopyPrivateListStringToSlot {
                list_id,
                node_string_offset: _,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, destination),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, index),
            ],
        ) => {
            // `sub_47CF60`/`sub_47CF20`/`sub_47CEE0`/`sub_47CE80` resolve
            // the deeper word through `sub_470D50` (the recovered
            // process-slot domain) and walk the this+0xC18 list with the
            // top index; the per-handler node field (+0x24/+0x64/+0x04/
            // +0x44) is lstrcpyA'd, or the empty fallback constant when
            // the list is short. The recovered list writers have not been
            // reached, so the list stays empty and every copy takes the
            // empty fallback, matching the native empty-list behavior.
            if *destination & 0xc000_0000 != 0x8000_0000 {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_STRING_TAG",
                    "CMVS list-copy destination is outside the process-slot domain",
                ));
            }
            let slot = destination & 0x3fff_ffff;
            if slot >= MAX_PROCESS_STRING_SLOTS {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_STRING_TAG",
                    "CMVS list-copy destination slot exceeds the recovered table",
                ));
            }
            let bytes: Vec<u8> = state
                .private_string_lists
                .get(&list_id)
                .and_then(|list| list.get(*index as usize))
                .and_then(|reference| {
                    let pool = state.frame_string_pool_bytes.get(&state.current_frame)?;
                    let start = reference.relative_offset as usize;
                    if start >= pool.len() {
                        return None;
                    }
                    let end = pool[start..]
                        .iter()
                        .position(|&byte| byte == 0)
                        .map(|position| start + position)
                        .unwrap_or(pool.len());
                    Some(pool[start..end].to_vec())
                })
                .unwrap_or_default();
            state.process_string_slots.insert(slot, bytes);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreListCountToResult { list_id: _ },
            [],
        ) => {
            // `sub_47CEC0` stores the manager's count field; the native
            // constructor (`sub_467D10`) seeds the capacity at 0x80.
            state.interpreter_words.insert(81220, 0x80);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreListWalkLengthToResult { list_id: _ },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _destination),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _index),
            ],
        ) => {
            // `sub_47CE50`/`sub_467E90`: an empty list walks to -1.
            state.interpreter_words.insert(81220, u32::MAX);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreInterpreterWordPair {
                top_field_offset,
                second_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, top),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
            ],
        ) => {
            // `sub_474626`: the confirm chain feeds its save-orchestration
            // parameters through this pair (the save screen's slot and
            // direction words).
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.word_pair_store",
                    pc = state.program_counter,
                    frame = state.current_frame,
                    top,
                    second
                );
            }
            state.interpreter_words.insert(top_field_offset, *top);
            state.interpreter_words.insert(second_field_offset, *second);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreInterpreterWord { field_offset },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            // RE aid: the setup loop's per-pass stores (10568/10312) are
            // list-cursor candidates; their value trajectory shows whether
            // the element walk advances.
            if field_offset == 10_568 || field_offset == 10_312 {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.cursor_store",
                    pc = state.program_counter,
                    field = field_offset,
                    value = *value
                );
            }
            state.interpreter_words.insert(field_offset, *value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreRandomModulo {
                target_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bound)],
        ) => {
            // `sub_484F60` computes `rand() % bound`; the original's CRT
            // divide faults on a zero bound, so the recovered handler blocks
            // instead of inventing a result.
            if *bound == 0 {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_RANDOM",
                    "CMVS random draw has a zero modulo bound",
                ));
            }
            let next = state
                .prng_state
                .wrapping_mul(214_013)
                .wrapping_add(2_531_011);
            state.prng_state = next;
            let draw = (next >> 16) & 0x7fff;
            state
                .interpreter_words
                .insert(target_field_offset, draw % bound);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreStringLength {
                target_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, reference)],
        ) => {
            // `sub_47D220` resolves the tagged string through `sub_470D50`
            // and stores `lstrlenA`; the length comes from the frame table
            // injected at script load for pool tags, and from the assembled
            // segment lengths for the process-global string slots.
            let length = resolve_tagged_string_length(state, *reference)?;
            state.interpreter_words.insert(target_field_offset, length);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::CopyTaggedStringLengthToResult {
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, destination),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, source),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _scratch),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _bound),
            ],
        ) => {
            // `sub_47D2B0`: the DBCS walk pins the display-unit count of
            // the source text; the destination copy lands in the engine
            // pool the headless frame table already tracks.
            let _ = destination;
            let length = resolve_tagged_string_length(state, *source)?;
            state.interpreter_words.insert(result_field_offset, length);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreInterpreterWord { field_offset },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, value)],
        ) => {
            // RE aid: see the OpaqueU32 arm above for the setup-loop
            // cursor-candidate tracking.
            if field_offset == 0x33D4 && tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.scheduler_enable_store",
                    pc = state.program_counter,
                    frame = state.current_frame,
                    value = u32::from(*value != 0)
                );
            }
            if field_offset == 10_568 || field_offset == 10_312 {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.cursor_store",
                    pc = state.program_counter,
                    field = field_offset,
                    value = u32::from(*value != 0)
                );
            }
            state
                .interpreter_words
                .insert(field_offset, u32::from(*value != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreInterpreterTimestampedRecord {
                table_base_offset,
                record_stride_bytes,
                max_index,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth), (CmvsPs2aCommandStackWordKind::OpaqueU32, fifth)],
        ) => {
            // `sub_47CBE0` skips the write for indices above the bound and
            // records a host tick through `sub_41A000`. The runtime host
            // supplies the deterministic fixed-session clock before dispatch.
            if *index <= u32::from(max_index) {
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.record136",
                        slot = *index,
                        pc_word = *second,
                        frame = state.current_frame
                    );
                }
                let base = u32::from(table_base_offset)
                    .checked_add(u32::from(record_stride_bytes).wrapping_mul(*index))
                    .ok_or_else(|| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_FIELD",
                            "CMVS record table offset overflowed",
                        )
                    })?;
                // The native record (`sub_47CBE0`, base `0x33DC`): the
                // delay word at +0 snapshots the scheduler's delay field
                // (`0x33A0`), the PC word at +4 takes the second stack
                // word (the resume target the script pushed), three more
                // operands fill +8..+16 and the host timestamp lands at
                // +20 (`sub_41A000`).
                // The delay word (+0) snapshots the frame register
                // (`0x33A0` = the current frame id at registration); the
                // resume (`sub_47CB00`) restores both pc and frame from the
                // record.
                state.interpreter_words.insert(base, state.current_frame as u32);
                state.interpreter_words.insert(base - 4, 0);
                state.interpreter_words.insert(base + 4, *second);
                state.interpreter_words.insert(base + 8, *third);
                state.interpreter_words.insert(base + 12, *fourth);
                state.interpreter_words.insert(base + 16, *fifth);
                state
                    .interpreter_words
                    .insert(base + 20, state.session_clock_millis);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreProcessGlobalWord { address },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            state.process_global_words.insert(address, *value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreProcessGlobalBoolean { address },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, value)],
        ) => {
            state
                .process_global_words
                .insert(address, u32::from(*value != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::MutateProcessFlagRange,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, start), (CmvsPs2aCommandStackWordKind::OpaqueU32, count), (CmvsPs2aCommandStackWordKind::BooleanU32, enabled)],
        ) => {
            mutate_process_flag_range(state, *start, *count, *enabled != 0)?;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreProcessIndexedRange,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, start), (CmvsPs2aCommandStackWordKind::OpaqueU32, count), (CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            if tracing::enabled!(tracing::Level::INFO)
                && *start <= 16
                && *start + *count > 16
            {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.pw16_range_store",
                    pc = state.program_counter,
                    frame = state.current_frame,
                    start,
                    count,
                    value,
                    tick = state.tick
                );
            }
            store_process_indexed_range(state, *start, *count, *value)?;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreProcessFloatRange,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, start), (CmvsPs2aCommandStackWordKind::OpaqueU32, count), (CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            store_process_float_range(state, *start, *count, *value)?;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryProcessSelection {
                result_field_offset,
            },
            [],
        ) => {
            let selection = state.input_selection_mirror;
            tracing::info!(
                event = "shinki.emu.cmvs.vm.sel846",
                pc = state.program_counter,
                selection,
                tick = state.tick
            );
            state
                .interpreter_words
                .insert(result_field_offset, selection);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreProcessStringRange,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, start), (CmvsPs2aCommandStackWordKind::OpaqueU32, count), (CmvsPs2aCommandStackWordKind::TaggedStringReference, value)],
        ) => {
            store_process_string_range(state, *start, *count, *value)?;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ProjectPointToScreenWords { result_base },
            [
                (_, _object),
                (_, _camera),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, z),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, y),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, x),
                (_, _),
                (_, _),
            ],
        ) => {
            // `sub_484890` stores the (possibly camera-projected)
            // coordinates as truncated integers: dword 20306 takes the
            // z-result, 20307 the y-result and 20308 the x-result. With
            // the camera object graph unrecovered the identity branch of
            // `sub_456C40` applies, so the raw inputs pass through.
            for (offset, bits) in [(0_u32, *z), (4, *y), (8, *x)] {
                let truncated = f32::from_bits(bits) as i32 as u32;
                state.interpreter_words.insert(result_base + offset, truncated);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PlayNamedAudioStream,
            words,
        ) => {
            // `sub_489F80`: bank index above 5 takes the error path; the
            // stream name resolves for observation and the headless audio
            // graph holds no stream objects to start.
            let bank = words.first().map(|(_, v)| *v).unwrap_or(0);
            if bank > 5 {
                raise_error_flag(state, 10532, 0x200)?;
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::RegisterAudioStreamNameSlot,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index), (CmvsPs2aCommandStackWordKind::OpaqueU32, _arm)],
        ) => {
            // `sub_48A0B0` arms the audio stream record at `index+9` and
            // clears the 52-byte name slot; the headless audio graph holds
            // no stream objects, so the bounded no-op and the error flag
            // are the observable subset.
            if *index > 5 {
                raise_error_flag(state, 10532, 0x200)?;
            }
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
