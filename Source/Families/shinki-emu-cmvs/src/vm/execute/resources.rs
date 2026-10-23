use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::ConfigureResourceSearch { bank },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::TaggedStringReference, resource)],
        ) => {
            // The recovered setup handlers leave the table untouched
            // when the channel is outside each bank's fixed slot table.
            let Ok(channel) = u8::try_from(*channel) else {
                return Ok(None);
            };
            if channel >= bank.slot_count() {
                return Ok(None);
            }
            let resource = resolve_string_value(state, *resource)?;
            crate::archive::native::validate_search_path(&resource)?;
            state
                .resource_search_paths
                .entry(bank)
                .or_default()
                .insert(channel, resource);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PlayChannelSound {
                channel_table_dword_index,
                max_channel,
                play_error_mask: _,
                channel_error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name)],
        ) => {
            if *channel > max_channel {
                raise_error_flag(state, 10532, channel_error_mask)?;
                return Ok(None);
            }
            let channel_key = (channel_table_dword_index << 8) | *channel;
            if !state.slot_objects.contains_key(&channel_key) {
                raise_error_flag(state, 10532, channel_error_mask)?;
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::PlayChannelSound {
                channel: *channel,
                name: resolve_string_value(state, *name)?,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::RegisterResourceChannelSlot {
                max_slot,
                gate_field_offset,
                error_flag_field_offset,
                error_flag_mask,
                ..
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name), (CmvsPs2aCommandStackWordKind::BooleanU32, enabled), (CmvsPs2aCommandStackWordKind::BooleanU32, loop_playback), (CmvsPs2aCommandStackWordKind::OpaqueU32, volume)],
        ) => {
            // `sub_48A110` rejects slots above 5 by raising the error-flag
            // mask without touching the record table.
            if *slot > u32::from(max_slot) {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            let resource = tag_zero_reference(*name)?;
            // The playback path (`sub_48DB70`) only runs when the gate field
            // is non-zero; that path is not recovered yet.
            if state
                .interpreter_words
                .get(&u32::from(gate_field_offset))
                .copied()
                .unwrap_or(0)
                != 0
            {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_RESOURCE_CHANNEL",
                    "CMVS resource channel playback is not recovered",
                ));
            }
            state.resource_channel_slots.insert(
                *slot,
                CmvsResourceChannelSlotRecord {
                    enabled: *enabled != 0,
                    volume: *volume,
                    loop_playback: *loop_playback != 0,
                    resource,
                },
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ClearResourceSlot {
                max_slot,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot)],
        ) => {
            // `sub_47F8D0` rejects slot indices at or above 8 with the
            // error mask; a valid index frees the slot, which is a no-op
            // when nothing occupies it.
            let Ok(slot) = u8::try_from(*slot) else {
                let flags = state
                    .interpreter_words
                    .get(&error_flag_field_offset)
                    .copied()
                    .unwrap_or(0);
                state
                    .interpreter_words
                    .insert(error_flag_field_offset, flags | error_flag_mask);
                return Ok(None);
            };
            if slot > max_slot {
                let flags = state
                    .interpreter_words
                    .get(&error_flag_field_offset)
                    .copied()
                    .unwrap_or(0);
                state
                    .interpreter_words
                    .insert(error_flag_field_offset, flags | error_flag_mask);
                return Ok(None);
            }
            state.resource_slots.remove(&slot);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::RunResourceSlotMethod {
                max_slot,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), ..],
        ) => {
            // `sub_482EA0`: the bound slot object consumes the surface
            // method words (the signed selector gates one argument push);
            // an unbound or out-of-range slot only raises the resource
            // error mask.
            if *slot > max_slot || !state.resource_channel_slots.contains_key(slot) {
                let flags = state.interpreter_words.get(&error_flag_field_offset).copied().unwrap_or_default();
                state.interpreter_words.insert(error_flag_field_offset, flags | error_flag_mask);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetResourceSlotRegion { max_slot },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::OpaqueU32, child), (CmvsPs2aCommandStackWordKind::OpaqueU32, region_x), (CmvsPs2aCommandStackWordKind::OpaqueU32, region_y)],
        ) => {
            // `sub_481CD0` reads the four stack words bottom-up: the slot
            // index (esi, deepest), the stack child id (edi, negative =
            // keep the current binding), and the display region pair
            // (ebx/eax) that `sub_445E80` lands on the slot object at
            // 0x1038/0x103c. The headless graph retains the region pair.
            if *slot > max_slot {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_RESOURCE_SLOT",
                    "CMVS resource slot index exceeded the recovered bound",
                ));
            }
            let _ = *child;
            let region = [*region_x, *region_y];
            state.resource_slot_regions.insert(*slot as u8, region);
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::ConfigureMoviePlayback, _) => {
            // `sub_468130` configures the movie player; the headless
            // subset has no video engine, so nothing is observable.
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
