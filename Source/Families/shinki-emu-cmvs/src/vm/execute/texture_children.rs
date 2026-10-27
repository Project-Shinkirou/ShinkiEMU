use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (CmvsPs2aCommandEffectKind::DisableTextureShake, [(_, parent), (_, selector)]) => {
            if !texture_parent_is_live(state, 1924, 255, *parent)? {
                raise_error_flag(state, 10532, 0x10)?;
                return Ok(None);
            }
            if (*selector as i32) < 0 {
                state
                    .texture_parents
                    .get_mut(&(*parent as u8))
                    .unwrap()
                    .screen_shake_disabled = true;
            } else {
                let child = u16::try_from(*selector)
                    .ok()
                    .filter(|child| *child < 1024)
                    .and_then(|child| {
                        state
                            .texture_children
                            .get_mut(&(*parent as u8))
                            .and_then(|children| children.get_mut(&child))
                    })
                    .ok_or_else(|| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_TEXTURE_CHILD",
                            "native shake opt-out requires a live child",
                        )
                    })?;
                child.screen_shake_disabled = true;
            }
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::DetachTextureChildContext, [(_, parent), (_, child)]) => {
            if *parent >= 256 || (*child as i32) < 0 {
                raise_error_flag(state, 10532, 0x10)?;
                return Ok(None);
            }
            let child = u16::try_from(*child)
                .ok()
                .filter(|child| *child < 1024)
                .and_then(|child| {
                    state
                        .texture_children
                        .get_mut(&(*parent as u8))
                        .and_then(|children| children.get_mut(&child))
                })
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_TEXTURE_CHILD",
                        "native parent context detach requires a live child",
                    )
                })?;
            child.parent_context_detached = true;
            Ok(None)
        }
        (
            kind @ (CmvsPs2aCommandEffectKind::ConfigureTextureBasicKind { .. }
            | CmvsPs2aCommandEffectKind::ConfigureTextureBlend
            | CmvsPs2aCommandEffectKind::ConfigureTextureFloatPosition
            | CmvsPs2aCommandEffectKind::ConfigureTextureRotation
            | CmvsPs2aCommandEffectKind::ConfigureTextureProjectionField { .. }),
            values,
        ) => {
            let [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, selector),
                tail @ ..,
            ] = values
            else {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                    "basic command requires parent and surface selector",
                ));
            };
            if !texture_parent_is_live(state, 1924, 255, *parent)? {
                raise_error_flag(state, 10532, 0x10)?;
                return Ok(None);
            }
            let selector = *selector as i32;
            let unbounded = matches!(
                kind,
                CmvsPs2aCommandEffectKind::ConfigureTextureBasicKind {
                    command_id: 66 | 76
                }
            );
            if selector >= 1024 && !unbounded {
                raise_error_flag(state, 10532, 0x20)?;
                return Ok(None);
            }
            require_texture_basic_surface(state, *parent as u8, selector)?;
            let (basic_kind, blend, position, depth, height, rotation) = if selector < 0 {
                let image = state.texture_parents.get_mut(&(*parent as u8)).unwrap();
                (
                    &mut image.basic_kind,
                    &mut image.blend_mode,
                    &mut image.float_position_words,
                    &mut image.projection_depth_word,
                    &mut image.projection_height_word,
                    &mut image.rotation_hundredths,
                )
            } else {
                let image = state
                    .texture_children
                    .get_mut(&(*parent as u8))
                    .unwrap()
                    .get_mut(&(selector as u16))
                    .unwrap();
                (
                    &mut image.basic_kind,
                    &mut image.blend_mode,
                    &mut image.float_position_words,
                    &mut image.projection_depth_word,
                    &mut image.projection_height_word,
                    &mut image.rotation_hundredths,
                )
            };
            let word = |index: usize| -> Result<u32, CoreError> {
                match tail.get(index) {
                    Some((CmvsPs2aCommandStackWordKind::OpaqueU32, value)) => Ok(*value),
                    _ => Err(invalid(
                        "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                        "basic command requires raw parameter words",
                    )),
                }
            };
            match kind {
                CmvsPs2aCommandEffectKind::ConfigureTextureBasicKind { command_id } => {
                    *basic_kind = match command_id {
                        66 => 2,
                        76 => 2 + u8::from(word(0)? != 0),
                        78 => u8::from(word(0)? != 0),
                        85 => match word(0)? {
                            1 => 1,
                            2 => 5,
                            _ => 0,
                        },
                        _ => {
                            return Err(invalid(
                                "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                                "unknown basic kind command",
                            ));
                        }
                    };
                }
                CmvsPs2aCommandEffectKind::ConfigureTextureBlend => {
                    let value = word(0)?;
                    if value <= 3 {
                        *blend = Some(value as u8);
                    }
                }
                CmvsPs2aCommandEffectKind::ConfigureTextureFloatPosition => {
                    *position = [word(0)?, word(1)?, 0];
                }
                CmvsPs2aCommandEffectKind::ConfigureTextureRotation => {
                    *rotation = ((word(0)? as i32).wrapping_abs() % 360) * 100;
                }
                CmvsPs2aCommandEffectKind::ConfigureTextureProjectionField { field_index } => {
                    let value = word(0)?;
                    match field_index {
                        11..=13 => position[usize::from(field_index - 11)] = value,
                        15 => *depth = value,
                        16 => *height = value,
                        _ => {
                            return Err(invalid(
                                "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                                "unknown basic projection field",
                            ));
                        }
                    }
                }
                _ => unreachable!(),
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetTextureVisibility,
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, selector),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, enabled),
            ],
        ) => {
            if !texture_parent_is_live(state, 1924, 255, *parent)? {
                raise_error_flag(state, 10532, 0x10)?;
                return Ok(None);
            }
            if (*selector as i32) < 0 {
                state
                    .texture_parents
                    .get_mut(&(*parent as u8))
                    .ok_or_else(|| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_TEXTURE",
                            "live parent has no image state",
                        )
                    })?
                    .hidden = *enabled == 0;
            } else {
                let child = u16::try_from(*selector)
                    .ok()
                    .filter(|child| *child < 1024)
                    .and_then(|child| {
                        state
                            .texture_children
                            .get_mut(&(*parent as u8))?
                            .get_mut(&child)
                    })
                    .ok_or_else(|| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_TEXTURE",
                            "visibility selected a missing native child",
                        )
                    })?;
                child.hidden = *enabled == 0;
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::DrawTextureChild {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                parent_error_mask,
                child_error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                ..,
            ],
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
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, container_selector),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child_selector),
            ],
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
            // CImageVideo's 4430E0 creates a provider/basic, not this +0x78
            // holder. Its playing/frame queries are 794/798, independently
            // of this native idle -1; CMV completion must not fabricate an
            // action holder or change case 494 into a playback query.
            let table_base = u16::try_from(texture_table_base_offset).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_TEXTURE",
                    "texture table offset overflowed",
                )
            })?;
            let channel_live = state
                .slot_objects
                .contains_key(&slot_object_key(table_base, *container_selector)?);
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
            state.write_interpreter_word(result_field_offset, idle_state);
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
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, container_selector),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child_selector),
            ],
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
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
            ],
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
            Ok(Some(CmvsPs2aVmAction::RemoveTextureChildSurface {
                parent_slot: *parent as u8,
                child_id: *child as u16,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::LoadTextureChildResource {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                error_flag_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, name),
            ],
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
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
            ],
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
            let image = state
                .texture_children
                .entry(*parent as u8)
                .or_default()
                .get_mut(&child_id)
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_TEXTURE_CHILD",
                        "CMVS texture surface targets a child that was not created",
                    )
                })?;
            image.surface_initialized = true;
            image.basic_effects = Default::default();
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
            // The slot and child ids lead the forwarded engine-value layouts.
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
            // Validate the container and child bound before forwarding.
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
                state.write_interpreter_word(
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
                state.write_interpreter_word(
                    child_error_flag_field_offset,
                    flags | child_error_flag_mask,
                );
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
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, third),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth),
            ],
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
            require_texture_basic_surface(state, *parent as u8, child_selector)?;
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
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
            ],
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
            require_texture_basic_surface(state, *parent as u8, child_selector)?;
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
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
            ],
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
            require_texture_basic_surface(state, *parent as u8, child_selector)?;
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
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, word),
            ],
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
            require_texture_basic_surface(state, *parent as u8, child_selector)?;
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
        (
            CmvsPs2aCommandEffectKind::ConfigureTextureSolidSurface {
                texture_table_base_offset,
                max_parent_slot,
                error_flag_field_offset,
                parent_error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, width),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, height),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, color),
            ],
        ) => {
            if !texture_parent_is_live(state, texture_table_base_offset, max_parent_slot, *parent)?
            {
                raise_error_flag(state, error_flag_field_offset, parent_error_mask)?;
                return Ok(None);
            }
            let selector = *child as i32;
            require_texture_basic_surface(state, *parent as u8, selector)?;
            let rect = Some([0, 0, *width, *height]);
            let solid_color = Some(*color | 0xff00_0000);
            if selector < 0 {
                let surface = state
                    .texture_parents
                    .get_mut(&(*parent as u8))
                    .expect("validated texture parent disappeared");
                surface.rect_words = rect;
                surface.solid_color = solid_color;
            } else {
                let surface = state
                    .texture_children
                    .get_mut(&(*parent as u8))
                    .and_then(|children| children.get_mut(&(*child as u16)))
                    .expect("validated texture child disappeared");
                surface.rect_words = rect;
                surface.solid_color = solid_color;
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureTextureAlpha {
                texture_table_base_offset,
                max_parent_slot,
                max_child_id,
                error_flag_field_offset,
                parent_error_mask,
                child_error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, word),
            ],
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
            require_texture_basic_surface(state, *parent as u8, child_selector)?;
            let alpha = Some((*word as i32).min(255));
            if child_selector < 0 {
                state
                    .texture_parents
                    .get_mut(&(*parent as u8))
                    .expect("validated texture parent disappeared")
                    .alpha = alpha;
            } else {
                state
                    .texture_children
                    .get_mut(&(*parent as u8))
                    .and_then(|children| children.get_mut(&(*child as u16)))
                    .expect("validated texture child disappeared")
                    .alpha = alpha;
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureTextureScale,
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, x),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, y),
            ],
        ) => {
            if !texture_parent_is_live(state, 1924, 255, *parent)? {
                raise_error_flag(state, 10532, 0x10)?;
                return Ok(None);
            }
            let selector = *child as i32;
            if selector >= 1024 {
                raise_error_flag(state, 10532, 0x20)?;
                return Ok(None);
            }
            require_texture_basic_surface(state, *parent as u8, selector)?;
            if selector < 0 {
                state
                    .texture_parents
                    .get_mut(&(*parent as u8))
                    .unwrap()
                    .scale_words = Some([*x, *y]);
            } else {
                state
                    .texture_children
                    .get_mut(&(*parent as u8))
                    .unwrap()
                    .get_mut(&(*child as u16))
                    .unwrap()
                    .scale_words = Some([*x, *y]);
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

/// Native parameter setters dereference the selected CBasicSurface directly.
/// A container or child without that surface is an execution error, not a
/// successful write to parameters that a later allocation would discard.
fn require_texture_basic_surface(
    state: &CmvsPs2aVmState,
    parent: u8,
    selector: i32,
) -> Result<(), CoreError> {
    let initialized = if selector < 0 {
        state
            .texture_parents
            .get(&parent)
            .map(|surface| surface.surface_initialized)
    } else {
        u16::try_from(selector)
            .ok()
            .filter(|child| *child < 1024)
            .and_then(|child| state.texture_children.get(&parent)?.get(&child))
            .map(|surface| surface.surface_initialized)
    };
    if initialized == Some(true) {
        Ok(())
    } else {
        Err(invalid(
            "SHINKI_EMU_CMVS_VM_TEXTURE_SURFACE",
            "CMVS parameter setter selected a missing native basic surface",
        ))
    }
}
