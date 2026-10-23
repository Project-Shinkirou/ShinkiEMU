use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::DrawTextureChild {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                parent_error_mask,
                child_error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), ..],
        ) => {
            // Case 500 (`sub_4823E0`): a slot index at or above 0x100 or an
            // empty table-1924 container raises 0x10; a child selector at or
            // above 0x400 raises 0x20. The engine draw chain that follows is
            // not interpreter-visible, so the recovered subset ends here.
            if *parent > max_parent_slot
                || !state.slot_objects.contains_key(&slot_object_key(
                    u16::try_from(texture_table_base_offset).map_err(|_| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_TEXTURE",
                            "texture table offset overflowed",
                        )
                    })?,
                    *parent,
                )?)
            {
                raise_error_flag(state, error_flag_field_offset, parent_error_mask)?;
                return Ok(None);
            }
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.texture_child_draw",
                    parent,
                    child,
                    pc = state.program_counter,
                    frame = state.current_frame,
                    tick = state.tick
                );
            }
            if *child > max_child_id {
                raise_error_flag(state, error_flag_field_offset, child_error_mask)?;
                return Ok(None);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryTextureChildState {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                container_error_flag_field_offset,
                container_error_mask,
                child_error_flag_field_offset,
                child_error_mask,
                result_field_offset,
            },
            [(
                CmvsPs2aCommandStackWordKind::OpaqueU32,
                container_selector,
            ), (
                CmvsPs2aCommandStackWordKind::OpaqueU32,
                child_selector,
            )],
        ) => {
            // Cases 494 (`sub_4829C0`) and 509 (`sub_4821A0`): the TOP
            // operand selects the 256-slot texture channel table — a dead
            // channel slot raises 0x10 and leaves the result word
            // untouched. The operand below it is the child selector: at or
            // above 0x400 it raises 0x20; the -1 sentinel queries the
            // channel object itself instead of resolving a registered
            // child (`sub_444EF0`). The live query publishes the resolved
            // object's action state through `sub_42C270` (case 494) or
            // `sub_42BB90` (case 509): -1 when no action is attached, -2
            // when the holder is disabled, otherwise the action word. The
            // recovered renderer attaches no pending actions to these
            // container objects, so the observable state is the native
            // idle -1 — the value the original's wait loops treat as
            // "nothing is playing, continue past the wait".
            let table_base = u16::try_from(texture_table_base_offset).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_TEXTURE",
                    "texture table offset overflowed",
                )
            })?;
            let channel_live = state
                .slot_objects
                .contains_key(&slot_object_key(table_base, *container_selector)?);
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.texture_child_probe",
                    pc = state.program_counter,
                    frame = state.current_frame,
                    tick = state.tick,
                    container = container_selector,
                    child = child_selector,
                    table = texture_table_base_offset,
                    live = channel_live
                );
            }
            if *container_selector > max_parent_slot || !channel_live {
                raise_error_flag(
                    state,
                    container_error_flag_field_offset,
                    container_error_mask,
                )?;
                return Ok(None);
            }
            // The original bounds check is signed (`v5 >= 1024`): the -1
            // sentinel passes exactly like any small selector.
            if (*child_selector as i32) > max_child_id as i32 {
                raise_error_flag(state, child_error_flag_field_offset, child_error_mask)?;
                return Ok(None);
            }
            let idle_state = (-1_i32) as u32;
            state
                .interpreter_words
                .insert(result_field_offset, idle_state);
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.texture_child_state",
                    pc = state.program_counter,
                    frame = state.current_frame,
                    tick = state.tick,
                    container = container_selector,
                    child = child_selector,
                    result = idle_state
                );
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReleaseTextureChildState {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                container_error_flag_field_offset,
                container_error_mask,
                child_error_flag_field_offset,
                child_error_mask,
            },
            [(
                CmvsPs2aCommandStackWordKind::OpaqueU32,
                container_selector,
            ), (
                CmvsPs2aCommandStackWordKind::OpaqueU32,
                child_selector,
            )],
        ) => {
            // Case 493 (`sub_482B00`): the TOP operand selects the channel
            // container exactly as in case 494, then `sub_42C2A0` joins
            // the child's loader thread and frees its state object. The
            // headless texture pipeline decodes synchronously, so the
            // teardown has no interpreter-visible effect.
            if *container_selector > max_parent_slot
                || !state.slot_objects.contains_key(&slot_object_key(
                    u16::try_from(texture_table_base_offset).map_err(|_| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_TEXTURE",
                            "texture table offset overflowed",
                        )
                    })?,
                    *container_selector,
                )?)
            {
                raise_error_flag(
                    state,
                    container_error_flag_field_offset,
                    container_error_mask,
                )?;
                return Ok(None);
            }
            if (*child_selector as i32) > max_child_id as i32 {
                raise_error_flag(state, child_error_flag_field_offset, child_error_mask)?;
                return Ok(None);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ResetTextureChild {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child)],
        ) => {
            if *parent > u32::from(max_parent_slot)
                || !state.slot_objects.contains_key(&slot_object_key(
                    u16::try_from(texture_table_base_offset).map_err(|_| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_TEXTURE",
                            "texture table offset overflowed",
                        )
                    })?,
                    *parent,
                )?)
            {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            if *child > u32::from(max_child_id) {
                return Ok(None);
            }
            state
                .texture_children
                .entry(*parent as u8)
                .or_default()
                .insert(*child as u16, CmvsTextureChildState::default());
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::LoadTextureChildResource {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name)],
        ) => {
            let resource = resolve_resource_name(state, *name)?;
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            if *child > u32::from(max_child_id) {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_TEXTURE_CHILD",
                    "CMVS texture resource targets an out-of-range child id",
                ));
            }
            let child_id = *child as u16;
            let child_state = state
                .texture_children
                .entry(*parent as u8)
                .or_default()
                .get_mut(&child_id)
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_TEXTURE_CHILD",
                        "CMVS texture resource targets a child that was not created",
                    )
                })?;
            child_state.resource = Some(resource.clone());
            child_state.resource_frame = Some(state.current_frame);
            Ok(Some(CmvsPs2aVmAction::LoadTextureResource {
                parent_slot: *parent as u8,
                child_id,
                resource,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::InitializeTextureChildSurface {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child)],
        ) => {
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            let child_id = u16::try_from(*child).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_TEXTURE_CHILD",
                    "CMVS texture surface child id cannot be represented",
                )
            })?;
            if child_id > max_child_id {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_TEXTURE_CHILD",
                    "CMVS texture surface targets an out-of-range child id",
                ));
            }
            state
                .texture_children
                .entry(*parent as u8)
                .or_default()
                .get_mut(&child_id)
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_TEXTURE_CHILD",
                        "CMVS texture surface targets a child that was not created",
                    )
                })?
                .surface_initialized = true;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureTextureChildAudio {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                container_error_flag_field_offset,
                container_error_flag_mask,
                child_error_flag_field_offset,
                child_error_flag_mask,
            },
            values,
        ) => {
            // Cases 72/490 forward three words and case 491 seven; the
            // slot and child ids lead both layouts.
            let Some((CmvsPs2aCommandStackWordKind::OpaqueU32, slot)) = values.first() else {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                    "CMVS child audio rebind requires the container slot word",
                ));
            };
            let Some((CmvsPs2aCommandStackWordKind::OpaqueU32, child)) = values.get(1) else {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                    "CMVS child audio rebind requires the child id word",
                ));
            };
            // `sub_4837F0` validates the container and the child bound;
            // the audio rebinding itself stays engine-side.
            if !texture_parent_is_live(
                state,
                texture_table_base_offset,
                u8::try_from(max_parent_slot).unwrap_or(u8::MAX),
                *slot,
            )? {
                let flags = state
                    .interpreter_words
                    .get(&container_error_flag_field_offset)
                    .copied()
                    .unwrap_or(0);
                state.interpreter_words.insert(
                    container_error_flag_field_offset,
                    flags | container_error_flag_mask,
                );
                return Ok(None);
            }
            if *child > max_child_id {
                let flags = state
                    .interpreter_words
                    .get(&child_error_flag_field_offset)
                    .copied()
                    .unwrap_or(0);
                state
                    .interpreter_words
                    .insert(child_error_flag_field_offset, flags | child_error_flag_mask);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureTextureChildRect {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                parent_error_mask,
                child_error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth)],
        ) => {
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, parent_error_mask)?;
                return Ok(None);
            }
            let child_selector = *child as i32;
            if child_selector >= 0 && *child > u32::from(max_child_id) {
                raise_error_flag(state, error_flag_field_offset, child_error_mask)?;
                return Ok(None);
            }
            let words = [*first, *second, *third, *fourth];
            if child_selector < 0 {
                state
                    .texture_parents
                    .get_mut(&(*parent as u8))
                    .expect("live texture parent disappeared")
                    .rect_words = Some(words);
            } else {
                state
                    .texture_children
                    .get_mut(&(*parent as u8))
                    .and_then(|children| children.get_mut(&(*child as u16)))
                    .ok_or_else(|| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_TEXTURE_CHILD",
                            "CMVS texture rectangle targets a child that was not created",
                        )
                    })?
                    .rect_words = Some(words);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureTextureChildPosition {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                parent_error_mask,
                child_error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, parent_error_mask)?;
                return Ok(None);
            }
            let child_selector = *child as i32;
            if child_selector >= 0 && *child > u32::from(max_child_id) {
                raise_error_flag(state, error_flag_field_offset, child_error_mask)?;
                return Ok(None);
            }
            let words = [*first, *second];
            if child_selector < 0 {
                state
                    .texture_parents
                    .get_mut(&(*parent as u8))
                    .expect("live texture parent disappeared")
                    .position_words = Some(words);
            } else {
                state
                    .texture_children
                    .get_mut(&(*parent as u8))
                    .and_then(|children| children.get_mut(&(*child as u16)))
                    .ok_or_else(|| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_TEXTURE_CHILD",
                            "CMVS texture position targets a child that was not created",
                        )
                    })?
                    .position_words = Some(words);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureTextureAuxiliaryPair {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                parent_error_mask,
                child_error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, parent_error_mask)?;
                return Ok(None);
            }
            let child_selector = *child as i32;
            if child_selector >= 0 && *child > u32::from(max_child_id) {
                raise_error_flag(state, error_flag_field_offset, child_error_mask)?;
                return Ok(None);
            }
            let words = [*first, *second];
            if child_selector < 0 {
                state
                    .texture_parents
                    .get_mut(&(*parent as u8))
                    .expect("live texture parent disappeared")
                    .auxiliary_pair_words = Some(words);
            } else {
                state
                    .texture_children
                    .get_mut(&(*parent as u8))
                    .and_then(|children| children.get_mut(&(*child as u16)))
                    .ok_or_else(|| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_TEXTURE_CHILD",
                            "CMVS texture auxiliary pair targets a child that was not created",
                        )
                    })?
                    .auxiliary_pair_words = Some(words);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureTextureAuxiliaryWord {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                parent_error_mask,
                child_error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), (CmvsPs2aCommandStackWordKind::OpaqueU32, word)],
        ) => {
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, parent_error_mask)?;
                return Ok(None);
            }
            let child_selector = *child as i32;
            if child_selector >= 0 && *child > u32::from(max_child_id) {
                raise_error_flag(state, error_flag_field_offset, child_error_mask)?;
                return Ok(None);
            }
            let child_id = if child_selector < 0 {
                state
                    .texture_parents
                    .get_mut(&(*parent as u8))
                    .expect("live texture parent disappeared")
                    .auxiliary_word = Some(*word);
                None
            } else {
                state
                    .texture_children
                    .get_mut(&(*parent as u8))
                    .and_then(|children| children.get_mut(&(*child as u16)))
                    .ok_or_else(|| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_TEXTURE_CHILD",
                            "CMVS texture auxiliary word targets a child that was not created",
                        )
                    })?
                    .auxiliary_word = Some(*word);
                Some(*child as u16)
            };
            Ok(Some(CmvsPs2aVmAction::CommitTextureSurface {
                parent_slot: *parent as u8,
                child_id,
            }))
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
