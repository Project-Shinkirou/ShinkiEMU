use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::ReadInputLatches {
                release_field_offset,
                held_field_offset,
            },
            [],
        ) => {
            // `sub_479C20` copies the input manager's advance-group latches
            // (`sub_45CCB0` reads manager dwords 271/272): the release edge
            // and the held state the host pump rebuilt for this frame.
            if tracing::enabled!(tracing::Level::INFO) && state.current_frame == 0 {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.latch_read",
                    pc = state.program_counter,
                    release = u32::from(state.input_advance_release),
                    held = u32::from(state.input_confirm_held),
                    tick = state.tick
                );
            }
            state
                .interpreter_words
                .insert(release_field_offset, u32::from(state.input_advance_release));
            state
                .interpreter_words
                .insert(held_field_offset, u32::from(state.input_confirm_held));
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::ConsumeAdvanceLatch, []) => {
            if tracing::enabled!(tracing::Level::INFO)
                && (state.input_advance_release || state.input_advance_press)
            {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.latch_consume",
                    pc = state.program_counter,
                    frame = state.current_frame,
                    release = u32::from(state.input_advance_release),
                    press = u32::from(state.input_advance_press),
                    tick = state.tick
                );
            }
            state.input_advance_release = false;
            state.input_advance_press = false;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReadAutomaticAdvance {
                result_field_offset,
                override_field_offset,
            },
            [],
        ) => {
            // Native manager fields 293/296 are input modes, not GPU fences.
            let overridden = state
                .interpreter_words
                .get(&override_field_offset)
                .copied()
                .unwrap_or_default()
                != 0;
            let automatic = state.input_skip_mode || state.input_auto_mode || overridden;
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(automatic));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ClearTextureChannelTable {
                channel_table_base_offset,
            },
            [],
        ) => {
            let table = u16::try_from(channel_table_base_offset).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_TEXTURE",
                    "texture table offset overflowed",
                )
            })?;
            state
                .slot_objects
                .retain(|key, _| (*key >> 16) != u32::from(table));
            state
                .slot_object_seed_words
                .retain(|key, _| (*key >> 16) != u32::from(table));
            for parent in state.texture_parents.keys().copied().collect::<Vec<u8>>() {
                state.retain_texture_slot_placement(parent);
            }
            state.texture_children.clear();
            state.texture_parents.clear();
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::LoadTextureParentResource {
                texture_table_base_offset,
                max_parent_slot,
                result_field_offset,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name)],
        ) => {
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.texture_parent_load",
                    pc = state.current_instruction_start,
                    frame = state.current_frame,
                    parent,
                    name_word = name,
                    tick = state.tick
                );
            }
            // Resolve and copy before later script or global-slot writes.
            let resource = resolve_resource_name(state, *name)?;
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            let parent_slot = *parent as u8;
            let parent_state = state.texture_parents.get_mut(&parent_slot).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_TEXTURE",
                    "CMVS live texture parent has no specialized state",
                )
            })?;
            parent_state.resource = Some(resource.clone());
            parent_state.resource_frame = Some(state.current_frame);
            // The host blocks the step if VFS resolution or decode fails;
            // continuation therefore observes the original non-zero loader
            // result only after this action has been accepted.
            state.interpreter_words.insert(result_field_offset, 1);
            Ok(Some(CmvsPs2aVmAction::LoadTextureParentResource {
                parent_slot,
                resource,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::LoadTextureParentResourceExtended {
                texture_table_base_offset,
                result_field_offset,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name), (CmvsPs2aCommandStackWordKind::OpaqueU32, _word_a), (CmvsPs2aCommandStackWordKind::OpaqueU32, _word_b), (CmvsPs2aCommandStackWordKind::OpaqueU32, _word_c)],
        ) => {
            let resource = resolve_resource_name(state, *name)?;
            if *parent >= 256 {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            let parent_slot = *parent as u8;
            // `sub_47ACA0` creates the container when absent and replaces
            // any previous owner before the load; only occupancy and the
            // resource reference are observable.
            let key = slot_object_key(
                u16::try_from(texture_table_base_offset).map_err(|_| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_SLOT_OBJECTS",
                        "CMVS slot table offset overflowed",
                    )
                })?,
                *parent,
            )?;
            state.slot_objects.entry(key).or_insert_with(|| {
                let identity = state.next_slot_object_id;
                state.next_slot_object_id += 1;
                identity
            });
            let parent_state = state.texture_parents.entry(parent_slot).or_default();
            parent_state.resource = Some(resource.clone());
            parent_state.resource_frame = Some(state.current_frame);
            state.interpreter_words.insert(result_field_offset, 1);
            state.interpreter_words.insert(3260, 1);
            Ok(Some(CmvsPs2aVmAction::LoadTextureParentResource {
                parent_slot,
                resource,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::CommitTexturePresentation {
                result_field_offset: _,
                present_object_field_offset: _,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _)],
        ) => {
            // Entirely resolved by the recovered 0xA000 wait gate in
            // `execute_command`; this arm only serves the continue path
            // (missing presentation object), which pops and advances
            // normally.
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::InitializeTextureParentSurface {
                texture_table_base_offset,
                max_parent_slot,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, parent)],
        ) => {
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            state
                .texture_parents
                .get_mut(&(*parent as u8))
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_TEXTURE",
                        "CMVS live texture parent has no specialized state",
                    )
                })?
                .surface_initialized = true;
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
