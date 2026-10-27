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
            state.write_interpreter_word(
                release_field_offset,
                u32::from(state.input_advance_release),
            );
            state.write_interpreter_word(held_field_offset, u32::from(state.input_confirm_held));
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::ConsumeAdvanceLatch, []) => {
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
            // Native groups 08/09 are Shift/Ctrl, not confirmation or auto UI.
            let overridden = state
                .interpreter_words
                .get(&override_field_offset)
                .copied()
                .unwrap_or_default()
                != 0;
            let automatic = state.input_shift_held || state.input_control_held || overridden;
            state.write_interpreter_word(result_field_offset, u32::from(automatic));
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
                .retain(|key, _| (*key >> 8) != u32::from(table));
            state
                .slot_object_seed_words
                .retain(|key, _| (*key >> 8) != u32::from(table));
            state.texture_children.clear();
            state.texture_parents.clear();
            state.texture_reloads.clear();
            Ok(Some(CmvsPs2aVmAction::ClearTextureParentSurfaces))
        }
        (
            CmvsPs2aCommandEffectKind::LoadTextureParentResource {
                texture_table_base_offset,
                max_parent_slot,
                result_field_offset,
                error_flag_field_offset,
                error_flag_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, name),
            ],
        ) => {
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
            parent_state.image_info = [0; 3];
            parent_state.color_filter = None;
            // The host blocks the step if VFS resolution or decode fails;
            // continuation therefore observes the original non-zero loader
            // result only after this action has been accepted.
            state.write_interpreter_word(result_field_offset, 1);
            Ok(Some(CmvsPs2aVmAction::LoadTextureParentResource {
                parent_slot,
                resource,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::StartTexturePresentation {
                clear_background: false,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, name),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, duration),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _software_feather),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, showing),
            ],
        ) => {
            let mask = resolve_resource_name(state, *name)?;
            crate::vm::texture_presentation::begin(
                state,
                mask,
                *parent,
                *duration,
                *showing != 0,
                false,
            )
        }
        (
            CmvsPs2aCommandEffectKind::StartTexturePresentation {
                clear_background: true,
            },
            [
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, name),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, duration),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _software_feather),
            ],
        ) => {
            let mask = resolve_resource_name(state, *name)?;
            crate::vm::texture_presentation::begin(state, mask, 0, *duration, false, true)
        }
        (CmvsPs2aCommandEffectKind::QueryTexturePresentation, []) => {
            let remaining = state
                .texture_presentation
                .as_ref()
                .map_or(u32::MAX, |presentation| presentation.remaining_ms());
            state.write_interpreter_word(81220, remaining);
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::FinishTexturePresentation, []) => {
            Ok(crate::vm::texture_presentation::finish(state))
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
            CmvsPs2aCommandEffectKind::SetTextureParentSurface {
                initialized,
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
            let image = state
                .texture_parents
                .get_mut(&(*parent as u8))
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_TEXTURE",
                        "live texture parent has no specialized state",
                    )
                })?;
            image.surface_initialized = initialized;
            // 446900/4469A0 replace or free CBasic and all of its effect owners.
            image.basic_effects = Default::default();
            if initialized {
                image.alpha = None;
                image.solid_color = None;
                image.scale_words = None;
                image.basic_kind = 0;
                image.blend_mode = None;
                image.float_position_words = [0; 3];
                image.projection_depth_word = 0;
                image.projection_height_word = 0;
                image.rotation_hundredths = 0;
                image.rect_words = None;
                image.position_words = None;
                image.auxiliary_pair_words = None;
                image.auxiliary_word = None;
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
