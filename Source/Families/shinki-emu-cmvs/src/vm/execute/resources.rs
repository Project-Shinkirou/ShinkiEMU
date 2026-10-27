use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::ConfigureResourceSearch { bank },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, resource),
            ],
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
            CmvsPs2aCommandEffectKind::LoadChannelImage {
                channel_table_dword_index,
                max_channel,
                load_error_mask,
                channel_error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, name),
            ],
        ) => {
            if *channel > max_channel {
                raise_error_flag(state, 10532, channel_error_mask)?;
                return Ok(None);
            }
            let channel_key = (channel_table_dword_index << 8) | *channel;
            if !state.slot_objects.contains_key(&channel_key)
                || !state.effect_channels.contains_key(&(*channel as u8))
            {
                raise_error_flag(state, 10532, channel_error_mask)?;
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::LoadChannelImage {
                channel: *channel as u8,
                resource: resolve_resource_name(state, *name)?,
                load_error_mask,
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
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, slot),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, name),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, volume),
                (CmvsPs2aCommandStackWordKind::BooleanU32, loop_playback),
                (CmvsPs2aCommandStackWordKind::BooleanU32, start_playback),
            ],
        ) => {
            let source = resolve_string_value(state, *name)?;
            // `sub_48A110` rejects slots above 5 by raising the error-flag
            // mask without touching the record table.
            if *slot > u32::from(max_slot) {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state.write_interpreter_word(field, flags | error_flag_mask);
                return Ok(None);
            }
            let playback_enabled = state
                .interpreter_words
                .get(&u32::from(gate_field_offset))
                .copied()
                .unwrap_or(0)
                != 0;
            let action = if playback_enabled {
                Some(CmvsPs2aVmAction::ConfigureResourceAudio {
                    slot: *slot as u8,
                    fade_in_ms: 0,
                })
            } else {
                None
            };
            state.resource_channel_slots.insert(
                *slot,
                CmvsResourceChannelSlotRecord {
                    start_playback: *start_playback != 0,
                    volume: *volume,
                    loop_playback: *loop_playback != 0,
                    source,
                },
            );
            for (offset, word) in [
                (32, u32::from(*loop_playback != 0)),
                (36, *volume),
                (40, 256),
                (44, 0),
                (48, u32::from(*start_playback != 0)),
            ] {
                state.write_interpreter_word(1608 + slot * 52 + offset, word);
            }
            Ok(action)
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
                state.write_interpreter_word(error_flag_field_offset, flags | error_flag_mask);
                return Ok(None);
            };
            if slot > max_slot {
                let flags = state
                    .interpreter_words
                    .get(&error_flag_field_offset)
                    .copied()
                    .unwrap_or(0);
                state.write_interpreter_word(error_flag_field_offset, flags | error_flag_mask);
                return Ok(None);
            }
            state.resource_slots.remove(&slot);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetResourceSlotRegion { max_slot },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, slot),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, region_x),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, region_y),
            ],
        ) => {
            // 481CD0 -> 445E80 writes the selected graphics object's
            // reference size, not a separate slot-region shadow table.
            if *slot > max_slot {
                raise_error_flag(state, 10532, 0x10)?;
                return Ok(None);
            }
            let parent = state
                .texture_parents
                .get_mut(&(*slot as u8))
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_TEXTURE",
                        "native reference size selected a missing parent",
                    )
                })?;
            if (*child as i32) < 0 {
                parent.reference_size = Some([*region_x, *region_y]);
            } else {
                state
                    .texture_children
                    .get_mut(&(*slot as u8))
                    .and_then(|children| {
                        u16::try_from(*child)
                            .ok()
                            .and_then(|id| children.get_mut(&id))
                    })
                    .ok_or_else(|| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_TEXTURE_CHILD",
                            "native reference size selected a missing child",
                        )
                    })?
                    .reference_size = Some([*region_x, *region_y]);
            }
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::AppendNativeMessageHistory, values) if values.len() == 10 => {
            let strings = [
                super::super::text::resolve_tagged_string_bytes(state, values[0].1)?.into_owned(),
                super::super::text::resolve_tagged_string_bytes(state, values[1].1)?.into_owned(),
                super::super::text::resolve_tagged_string_bytes(state, values[2].1)?.into_owned(),
                super::super::text::resolve_tagged_string_bytes(state, values[3].1)?.into_owned(),
            ];
            let words = std::array::from_fn(|index| values[index + 4].1);
            state
                .message_history
                .append(super::super::message_history::MessageEntry {
                    strings,
                    values: words,
                })?;
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::ClearNativeMessageHistory, []) => {
            state.message_history.entries.clear();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn register(state: &mut CmvsPs2aVmState, name: u32) -> Option<CmvsPs2aVmAction> {
        execute(
            state,
            CmvsPs2aCommandEffectKind::RegisterResourceChannelSlot {
                max_slot: 5,
                table_base_offset: 1608,
                record_stride_bytes: 52,
                gate_field_offset: 1556,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x200,
            },
            &[
                (CmvsPs2aCommandStackWordKind::OpaqueU32, 1),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, name),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, 192),
                (CmvsPs2aCommandStackWordKind::BooleanU32, 1),
                (CmvsPs2aCommandStackWordKind::BooleanU32, 1),
            ],
        )
        .unwrap()
    }

    #[test]
    fn disabled_resource_audio_retains_native_volume_and_start_metadata() {
        let mut state = CmvsPs2aVmState::new(0);
        state.write_interpreter_word(1556, 0);
        state
            .frame_string_pool_bytes
            .insert(0, b"rain.ogg\0".to_vec());
        assert!(register(&mut state, 0).is_none());
        let record = &state.resource_channel_slots[&1];
        assert_eq!(record.source, "rain.ogg");
        assert_eq!(record.volume, 192);
        assert!(record.start_playback && record.loop_playback);
        assert_eq!(state.interpreter_words[&(1608 + 52 + 40)], 256);
    }
}
