use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::SelectEffectChild {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _child),
            ],
        ) => {
            if *channel > max_channel {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (effect_table_dword_index << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::SelectEffectChild {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectTextSurface { error_mask },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, index),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, first),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, second),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, value),
            ],
        ) => {
            // `sub_488E70` rejects an index at or above 12 and an
            // unregistered playback record with 0x1000, then forwards to
            // `sub_462C10`, which stores the value and both strings.
            let first_bytes = text::resolve_tagged_string_bytes(state, *first)?.into_owned();
            let second_bytes = text::resolve_tagged_string_bytes(state, *second)?.into_owned();
            let occupied = *index < 12 && state.slot_objects.contains_key(&(750_u32 << 8 | *index));
            if !occupied {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // RE aid: the phantom-line stall feeds pool-relative zeros
            // here; the pc names the library path that builds the line.
            tracing::info!(
                event = "shinki.emu.cmvs.vm.text_surface_set",
                pc = state.program_counter,
                frame = state.current_frame,
                index = *index,
                first = *first,
                second = *second,
                value = *value
            );
            for bytes in [&first_bytes, &second_bytes] {
                text::validate_native_string_slot(bytes)?;
                if bytes.len() >= 64 {
                    return Err(invalid(
                        "SHINKI_EMU_CMVS_TEXT_BOUNDARY",
                        "kinsoku list exceeds its native buffer",
                    ));
                }
            }
            let record = state.effect_playback.entry(*index as u8).or_default();
            record.text_surface.value = *value;
            record.text_surface.boundaries = Some([first_bytes, second_bytes]);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectElementVisible { error_mask },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                (CmvsPs2aCommandStackWordKind::BooleanU32, value),
            ],
        ) => {
            // `sub_47D850` requires channel and element; `sub_445EA0` stores
            // the boolean at the element's visibility dword.
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let Some(element) = state.effect_elements.get_mut(&(*channel as u8, *child)) else {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            };
            element.visible = u32::from(*value != 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectElementExists {
                error_mask,
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
            ],
        ) => {
            // `sub_47D630` only requires the channel; a missing element simply
            // reports zero.
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let exists = state
                .effect_elements
                .contains_key(&(*channel as u8, *child));
            state.write_interpreter_word(result_field_offset, u32::from(exists));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectElementSource { error_mask },
            [
                (_, channel),
                (_, child),
                (_, x),
                (_, y),
                (_, width),
                (_, height),
            ],
        ) => {
            let child = if (*child as i32) < 0 {
                u32::MAX
            } else {
                *child
            };
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let Some(element) = state.effect_elements.get_mut(&(*channel as u8, child)) else {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            };
            element.source = Some([*x as i32, *y as i32, *width as i32, *height as i32]);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectElementPosition { error_mask },
            [(_, channel), (_, child), (_, x), (_, y)],
        ) => {
            let child = if (*child as i32) < 0 {
                u32::MAX
            } else {
                *child
            };
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let Some(element) = state.effect_elements.get_mut(&(*channel as u8, child)) else {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            };
            element.position = Some([*x as i32, *y as i32]);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectElementSize { error_mask },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
            ],
        ) => {
            let child = if (*child as i32) < 0 {
                u32::MAX
            } else {
                *child
            };
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let Some(element) = state.effect_elements.get_mut(&(*channel as u8, child)) else {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            };
            element.width = *first as i32;
            element.height = *second as i32;
            Ok(None)
        }
        (
            kind @ CmvsPs2aCommandEffectKind::SetEffectElementAuxiliary { error_mask }
            | kind @ CmvsPs2aCommandEffectKind::SetEffectElementAuxiliaryPair { error_mask },
            [(_, channel), (_, child)],
        ) => {
            let child = if (*child as i32) < 0 {
                u32::MAX
            } else {
                *child
            };
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let Some(element) = state.effect_elements.get_mut(&(*channel as u8, child)) else {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            };
            // `445E40`/`445E60` clear the native inheritance bits.
            element.inheritance_flags &= !if matches!(
                kind,
                CmvsPs2aCommandEffectKind::SetEffectElementAuxiliary { .. }
            ) {
                1
            } else {
                0x100
            };
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation { error_mask },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                ..,
            ],
        ) => {
            // `sub_47DC80` -> `sub_445340` + `sub_42BF50`, forwarding the
            // element's texture object; the channel and element checks are
            // the VM-visible effects.
            if !effect_channel_exists(state, *channel)
                || !state
                    .effect_elements
                    .contains_key(&(*channel as u8, *child))
            {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureEffectElementBasic { command_id: 377 },
            [(_, channel), (_, child)],
        ) => {
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, 0x10000)?;
                return Ok(None);
            }
            // `466C50` destroys the 630 owner for any negative selector;
            // `4443B0` bounds-checks and silently ignores missing children.
            if (*child as i32) < 0 {
                state
                    .effect_elements
                    .retain(|(owner, _), _| *owner != *channel as u8);
                if let Some(channel) = state.effect_channels.get_mut(&(*channel as u8)) {
                    channel.image_resource = None;
                    channel.image_has_alpha = false;
                }
            } else if *child < 1024 {
                state.effect_elements.remove(&(*channel as u8, *child));
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureEffectElementBasic { command_id },
            [(_, channel), (_, child), tail @ ..],
        ) => {
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, 0x10000)?;
                return Ok(None);
            }
            let child = if (*child as i32) < 0 {
                u32::MAX
            } else {
                *child
            };
            let Some(element) = state.effect_elements.get_mut(&(*channel as u8, child)) else {
                raise_error_flag(state, 10532, 0x10000)?;
                return Ok(None);
            };
            match (command_id, tail) {
                (384, []) => element.reset_basic(true),
                (385, []) => element.reset_basic(false),
                (388, [(_, order)]) => element.order = *order as i32,
                (389, [(_, x), (_, y)]) => element.pivot = [*x as i32, *y as i32],
                (390, [(_, alpha)]) => element.alpha = (*alpha as i32).min(255),
                (391, [(_, mode)]) if *mode < 4 => element.blend_mode = *mode as u8,
                (391, [_]) => {}
                (395, [(_, x), (_, y)]) => element.scale_words = [*x, *y],
                _ => {
                    return Err(invalid(
                        "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                        "channel basic setter has invalid operands",
                    ));
                }
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChildEnabled {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _child),
                (CmvsPs2aCommandStackWordKind::BooleanU32, enabled),
            ],
        ) => {
            if *channel > max_channel {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (effect_table_dword_index << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::SetEffectChildEnabled {
                channel: *channel,
                enabled: *enabled != 0,
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
