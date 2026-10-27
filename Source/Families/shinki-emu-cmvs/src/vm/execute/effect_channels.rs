use super::*;
use crate::CmvsNativeTextOperation;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (CmvsPs2aCommandEffectKind::ConfigureEffectChannelText { operation }, values) => {
            let Some((_, channel)) = values.first() else {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                    "channel text setter has no owner",
                ));
            };
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, 0x10000)?;
                return Ok(None);
            }
            let slot = native_text::dialogue::text_slot(state, *channel as u8)?;
            let word = |value| (CmvsPs2aCommandStackWordKind::OpaqueU32, value);
            let mut arguments = [
                word(u32::from(slot)),
                word(u32::MAX),
                word(u32::MAX),
                word(u32::MAX),
            ];
            let count = match (operation, values) {
                (CmvsNativeTextOperation::Metrics, [_, (_, size)]) => {
                    arguments[1] = word(*size);
                    4
                }
                (CmvsNativeTextOperation::Metrics, [_, (_, character), (_, line)]) => {
                    arguments[2] = word(*character);
                    arguments[3] = word(*line);
                    4
                }
                (
                    CmvsNativeTextOperation::Cursor | CmvsNativeTextOperation::Colors,
                    [_, first, second],
                ) => {
                    arguments[1] = *first;
                    arguments[2] = *second;
                    3
                }
                (CmvsNativeTextOperation::ColorMode, [_, value]) => {
                    arguments[1] = *value;
                    2
                }
                _ => {
                    return Err(invalid(
                        "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                        "channel text setter has invalid operands",
                    ));
                }
            };
            native_text::execute(state, operation, &arguments[..count])
        }
        (
            CmvsPs2aCommandEffectKind::ForwardEffectChannelWord {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _value),
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
            Ok(Some(CmvsPs2aVmAction::ForwardEffectChannelWord {
                channel: *channel,
                value: values.get(1).map(|(_, v)| *v).unwrap_or(0),
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChannelOrigin { error_mask },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, mode),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
            ],
        ) => {
            if *channel > 7 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_4678C0`: mode 0 stores the coordinates directly, mode 1
            // adds them to the current origin, and any other mode stores the
            // first coordinate into both axes.
            let object = state.effect_channels.entry(*channel as u8).or_default();
            let (origin_x, origin_y) = object.origin();
            let (x, y) = match *mode {
                0 => (*first as i32, *second as i32),
                1 => (
                    origin_x.wrapping_add(*first as i32),
                    origin_y.wrapping_add(*second as i32),
                ),
                _ => (*first as i32, *first as i32),
            };
            object.set_origin(x, y);
            tracing::debug!(event = "cmvs.text.geometry", command = 336,
                channel, mode, first, second, pc = state.program_counter,
                frame = state.current_frame, tick = state.tick, origin = ?[x, y]);
            let text_slot = object.read_dword(0) as u8;
            if let Some(surface) = state.native_text.get_mut(&text_slot) {
                surface.position = [x, y];
            }
            if let Some(root) = state.effect_elements.get_mut(&(*channel as u8, u32::MAX)) {
                root.position = Some([x, y]);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectPlaybackField {
                error_mask,
                field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, index),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, value),
            ],
        ) => {
            // `sub_488FC0`/`sub_488F80`/`sub_488F40` reject an index at or
            // above 12 and an unregistered playback record with 0x1000.
            let occupied = *index < 12 && state.slot_objects.contains_key(&(750_u32 << 8 | *index));
            if !occupied {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let record = state.effect_playback.entry(*index as u8).or_default();
            match field_offset {
                41 => record.flag = *value,
                44 => record.value = *value,
                45 => record.value2 = *value,
                _ => {
                    return Err(invalid(
                        "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                        "unsupported playback field",
                    ));
                }
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChannelPlaybackMode { error_mask },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, mode),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
            ],
        ) => {
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .set_playback_mode(*mode as u16, *first as u16 as i16, *second as u16 as i16);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChannelValuePair { error_mask },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
            ],
        ) => {
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `467CF0` stores the alternate origin; only the per-frame
            // `467C60` view transition applies it to the live surfaces.
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .set_value_pair(*first, *second);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureEffectChannelFromName {
                result_field_offset,
                error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _name),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _mode),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _flag),
            ],
        ) => {
            // `sub_466ED0` applies the named source configuration to the
            // channel (the visual effect stays in the renderer) and reports
            // whether the channel settled: `*(this+2) == 0 && pending <= 0`.
            // The recovered channel objects carry no pending queue, so an
            // occupied channel always reports settled.
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            state.write_interpreter_word(result_field_offset, 1);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PrepareEffectChannelFromName { error_mask },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, name),
            ],
        ) => {
            // `sub_47E9E0`: case 345's wait twin. The native handler
            // returns `0xA000` while `sub_466ED0` reports the channel
            // unsettled and `0xC00C` (stop, pop 12) once the queue drains;
            // the headless channel objects settle immediately, so the
            // stop-dispatch half is the observable behaviour. Dead or
            // out-of-range channels raise the shared error flag and still
            // end the frame.
            resolve_resource_name(state, *name)?;
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
            }
            state.dispatch_stopped = true;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ApplyEffectChannelOperation { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            // `47FB20 -> 467A10 -> 464C00` clears the channel's text owner.
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let slot = state
                .effect_channels
                .get(&(*channel as u8))
                .map(|object| object.read_dword(0) as u8)
                .ok_or_else(|| {
                    invalid("SHINKI_EMU_CMVS_TEXT_OBJECT", "channel has no text owner")
                })?;
            let surface = state.native_text.get_mut(&slot).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_TEXT_OBJECT",
                    "channel text object is not initialized",
                )
            })?;
            surface.glyphs.clear();
            surface.cursor = [surface.rect[0], surface.rect[1]];
            state.native_text_reveals.remove(&slot);
            Ok(Some(CmvsPs2aVmAction::NativeText(
                CmvsNativeTextAction::Clear { slot },
            )))
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectState {
                first_field_offset,
                second_field_offset,
                third_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, 0x0002_0000)?;
                return Ok(None);
            }
            let hit = channel_input::query_channel_quads(state, *channel as u8)?;
            state.write_interpreter_word(first_field_offset, hit.index as u32);
            state.write_interpreter_word(second_field_offset, u32::from(hit.confirmed));
            state.write_interpreter_word(third_field_offset, u32::from(hit.held));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChannelPlaybackFlag {
                effect_table_dword_index,
                error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, flag),
            ],
        ) => {
            if *channel > 7 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (effect_table_dword_index << 8) | *channel;
            if !state.slot_objects.contains_key(&effect_key) {
                // The original dereferences the channel object without a
                // guard; the recovered subset fails fast instead.
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `47EFC0 -> 466F50`: DWORD 1 gates the scheduler; DWORD 2
            // is separate busy state, cleared only on an enable transition.
            let enabled = u32::from(*flag != 0);
            let object = state.effect_channels.entry(*channel as u8).or_default();
            if object.read_dword(2) != enabled {
                object.write_dword(2, enabled);
                object.write_dword(4, 0);
                let slot = object.read_dword(0) as u8;
                if let Some(root) = state.effect_elements.get_mut(&(*channel as u8, u32::MAX)) {
                    root.visible = enabled;
                }
                if let Some(surface) = state.native_text.get_mut(&slot) {
                    surface.visible = enabled != 0;
                }
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectChannelActivity {
                max_channel,
                effect_table_dword_index,
                error_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            let effect_key = (effect_table_dword_index << 8) | *channel;
            let live = *channel <= max_channel && state.slot_objects.contains_key(&effect_key);
            if !live {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `47F270` reads native DWORD 2, independently of enable.
            let activity = state
                .effect_channels
                .get(&(*channel as u8))
                .is_some_and(|object| object.read_dword(4) != 0);
            state.write_interpreter_word(result_field_offset, u32::from(activity));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryChannelOccupancy {
                result_field_offset,
                channel_table_dword_index,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            if *channel > 7 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let effect_key = (channel_table_dword_index << 8) | *channel;
            let occupied = state.slot_objects.contains_key(&effect_key);
            state.write_interpreter_word(result_field_offset, u32::from(occupied));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::DestroyEffectHitElement,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            // `sub_47E720`: an occupied channel drops its hit element
            // (`sub_446E70`); the native model tracks none beyond the
            // gate.
            if !effect_channel_exists(state, *channel) {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.write_interpreter_word(10532, flags | 0x0001_0000);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StorePlaybackRecordWord,
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, slot),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, value),
            ],
        ) => {
            // `sub_489080`: the bounded 750-table record accepts the word;
            // the recovered playback record keeps no extra native state.
            if *slot >= 12 {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.write_interpreter_word(10532, flags | 0x1000);
            } else {
                state.effect_playback.entry(*slot as u8).or_default();
                let _ = value;
            }
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::RunEffectTimeline, []) => {
            // The host consumes each image replacement in native order, then
            // clears the header. No source or GPU work occurs in the VM.
            Ok(Some(CmvsPs2aVmAction::FlushNativeTimeline))
        }
        (
            CmvsPs2aCommandEffectKind::ForwardEffectChannelQuad {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [(_, channel), (_, x), (_, y), (_, width), (_, height)],
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
            let slot = state
                .effect_channels
                .get(&(*channel as u8))
                .map(|object| object.read_dword(0) as u8)
                .ok_or_else(|| {
                    invalid("SHINKI_EMU_CMVS_TEXT_OBJECT", "channel has no text owner")
                })?;
            let surface = state.native_text.get_mut(&slot).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_TEXT_OBJECT",
                    "channel text object is not initialized",
                )
            })?;
            surface.rect = [*x as i32, *y as i32, *width as i32, *height as i32];
            surface.cursor = [*x as i32, *y as i32];
            tracing::debug!(event = "cmvs.text.geometry", command = 325,
                channel, slot, x, y, width, height, pc = state.program_counter,
                frame = state.current_frame, tick = state.tick, origin = ?surface.position);
            Ok(Some(CmvsPs2aVmAction::ForwardEffectChannelQuad {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::ForwardEffectChannelPair {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
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
            // `sub_464A60` stores the forwarded pair into the channel
            // object's parameter dwords 38/39 (word offsets 76/78).
            if let Some(channel_state) = state
                .effect_channels
                .get_mut(&u8::try_from(*channel).unwrap_or(0))
            {
                channel_state.write_dword(76, *first);
                channel_state.write_dword(78, *second);
            }
            Ok(Some(CmvsPs2aVmAction::ForwardEffectChannelPair {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::ForwardEffectChannelBlock {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, value),
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
            // `sub_466B70`: rebuild the root for a negative selector;
            // otherwise replace a bounded child, initially hidden.
            if (*value as i32) < 0 {
                state
                    .effect_elements
                    .retain(|(owner, _), _| *owner != *channel as u8);
                state
                    .effect_elements
                    .insert((*channel as u8, u32::MAX), CmvsEffectElement::default());
                if let Some(object) = state.effect_channels.get_mut(&(*channel as u8)) {
                    object.image_resource = None;
                    object.image_has_alpha = false;
                }
            } else if *value < 1024
                && state
                    .effect_elements
                    .contains_key(&(*channel as u8, u32::MAX))
            {
                state
                    .effect_elements
                    .insert((*channel as u8, *value), CmvsEffectElement::default());
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChannelEnabled {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
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
            // `47F160` -> `467CE0`: enable the alternate-origin swap
            // consumed by `467C60`, not the channel visibility flag.
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .set_flag(*enabled != 0);
            Ok(Some(CmvsPs2aVmAction::SetEffectChannelEnabled {
                channel: *channel,
                enabled: *enabled != 0,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::CreateEffectPlayback {
                max_index,
                error_flag_field_offset,
                error_flag_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, index),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, _name),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _extra),
            ],
        ) => {
            // `sub_487550`: build the per-index playback record the destroy
            // command (case 257) later frees. The resolved name and the
            // opaque word feed the native 0x238-byte record; the headless
            // graph keeps only the occupancy, so the error flag is the
            // observable half of the bound check.
            if *index > max_index {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            state.effect_playback.entry(*index as u8).or_default();
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ForwardEffectPlaybackSetup {
                max_index,
                error_flag_field_offset,
                error_flag_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, index),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, _name),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _extra),
            ],
        ) => {
            // `sub_489160`: an occupied record at the bound index consumes
            // the name and the opaque word; anything else only raises the
            // playback error mask on the pinned interpreter flag word.
            if *index > max_index || !state.effect_playback.contains_key(&(*index as u8)) {
                let flags = state
                    .interpreter_words
                    .get(&error_flag_field_offset)
                    .copied()
                    .unwrap_or_default();
                state.write_interpreter_word(error_flag_field_offset, flags | error_flag_mask);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureEffectPlaybackSlots {
                max_index,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index), ..],
        ) => {
            // `sub_4878C0`: the case-258 shape with three more operand
            // words consumed by the occupied record; the headless model
            // only keeps the occupancy check.
            if *index > max_index || !state.effect_playback.contains_key(&(*index as u8)) {
                let flags = state
                    .interpreter_words
                    .get(&error_flag_field_offset)
                    .copied()
                    .unwrap_or_default();
                state.write_interpreter_word(error_flag_field_offset, flags | error_flag_mask);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::CreateStageEffectChannel {
                max_channel,
                max_index,
                channel_error_mask,
                index_error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, index),
                ..,
            ],
        ) => {
            if *channel > max_channel {
                raise_error_flag(state, 10532, channel_error_mask)?;
                return Ok(None);
            }
            if *index > max_index {
                raise_error_flag(state, 10532, index_error_mask)?;
                return Ok(None);
            }
            rebuild_effect_channel(
                state,
                *channel,
                *index,
                [values[2].1 as i32, values[3].1 as i32],
                [values[4].1 as i32, values[5].1 as i32],
                None,
            )
        }
        (
            CmvsPs2aCommandEffectKind::CreateEffectChannel {
                effect_table_dword_index: _,
                playback_table_dword_index: _,
                max_channel,
                max_effect,
                channel_error_mask,
                effect_error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, effect),
                ..,
            ],
        ) => {
            if *channel > max_channel {
                raise_error_flag(state, 10532, channel_error_mask)?;
                return Ok(None);
            }
            if *effect > max_effect {
                raise_error_flag(state, 10532, effect_error_mask)?;
                return Ok(None);
            }
            rebuild_effect_channel(
                state,
                *channel,
                *effect,
                [values[2].1 as i32, values[3].1 as i32],
                [values[4].1 as i32, values[5].1 as i32],
                Some([values[6].1, values[7].1]),
            )
        }
        (
            CmvsPs2aCommandEffectKind::InstallChannelChildPayload {
                selector,
                channel_table_field_offset: _,
            },
            words,
        ) => {
            let values: Vec<u32> = words.iter().map(|(_, value)| *value).collect();
            if values.len() < 2 {
                return Err(CoreError::invalid(
                    "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                    "CMVS channel-child payload install lost its words",
                ));
            }
            let [channel, child, rest @ ..] = values.as_slice() else {
                return Err(CoreError::invalid(
                    "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                    "CMVS channel-child payload install lost its words",
                ));
            };
            let channel = *channel;
            let child_signed = *child as i32;
            if channel >= 0x100 {
                return Ok(None);
            }
            let presenter = state
                .channel_child_presenters
                .entry((channel, child_signed))
                .or_default();
            match selector {
                // `sub_42C1D0` -> `sub_432AB0`: resets the payload table
                // and stores the sign-extended byte argument.
                490 => {
                    presenter.payload_entries = [[0; 3]; 8];
                    presenter.payload_extra = [0; 8];
                    presenter.reset_value = (rest.first().copied().unwrap_or(0) as i8) as i32;
                }
                // `sub_42C280` -> `sub_432AF0`: table write by index.
                491 => {
                    if rest.len() >= 5 {
                        let index = rest[0].min(7) as usize;
                        presenter.payload_entries[index] = [rest[1], rest[4], rest[2]];
                        presenter.payload_extra[index] = rest[3];
                    }
                }
                // `sub_42C2E0` -> `sub_432820`: installs the (1, v, v, 0)
                // state into the payload head.
                492 => {
                    let value = rest.first().copied().unwrap_or(0);
                    presenter.payload_entries[0] = [1, value, value];
                }
                _ => {}
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureChannelChildPresenter {
                selector,
                channel_table_field_offset: _,
            },
            words,
        ) => {
            let values: Vec<u32> = words.iter().map(|(_, value)| *value).collect();
            let Some(channel) = values.first().copied() else {
                return Err(CoreError::invalid(
                    "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                    "CMVS channel-child presenter config lost its words",
                ));
            };
            let child_signed = values.get(1).copied().unwrap_or(u32::MAX) as i32;
            let rest = values.get(2..).unwrap_or(&[]);
            if channel >= 0x100 {
                return Ok(None);
            }
            let presenter = state
                .channel_child_presenters
                .entry((channel, child_signed))
                .or_default();
            match selector {
                // `sub_42BC90` -> `sub_431200`: create the quad.
                500 => {
                    presenter.quad = Some([0, 0, 0, 0]);
                }
                // `sub_4824D0` -> multi-arg presenter configure; the
                // recovered subset records the words for the reader.
                501 => {
                    if rest.len() >= 6 {
                        let index = rest[0].min(7) as usize;
                        presenter.payload_entries[index] = [rest[1], rest[5], rest[2]];
                        presenter.payload_extra[index] = rest[3];
                        if let Some(quad) = presenter.quad.as_mut() {
                            quad[0] = 1;
                            quad[3] = rest[4];
                        }
                    }
                }
                // `sub_42BDD0` -> `sub_431290`: set the quad's value word
                // (the u16 pair comes from the presenter's own stores).
                502 => {
                    if let Some(quad) = presenter.quad.as_mut() {
                        quad[0] = 1;
                        quad[3] = rest.first().copied().unwrap_or(0);
                    }
                }
                _ => {}
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

/// `47EA90` and `47ED00` replace both native owners before `466770`
/// applies their position/size. Only `47ED00` supplies a virtual extent.
fn rebuild_effect_channel(
    state: &mut CmvsPs2aVmState,
    channel: u32,
    effect: u32,
    position: [i32; 2],
    size: [i32; 2],
    virtual_size: Option<[u32; 2]>,
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    let identity = state.next_slot_object_id;
    state.next_slot_object_id = identity.checked_add(1).ok_or_else(|| {
        invalid(
            "SHINKI_EMU_CMVS_VM_SLOT_OBJECTS",
            "CMVS slot object identity space overflowed",
        )
    })?;
    state.slot_objects.insert(742 << 8 | channel, identity);
    let mut object = CmvsEffectChannel::default();
    object.write_dword(0, effect);
    object.set_origin(position[0], position[1]);
    object.write_dword(10, size[0] as u32);
    object.write_dword(12, size[1] as u32);
    object.write_dword(1264, u32::MAX);
    object.write_dword(1236, position[0] as u32);
    object.write_dword(1238, position[1] as u32);
    native_text::rebuild_channel(state, effect as u8, position, size, virtual_size)?;
    tracing::debug!(event = "cmvs.text.geometry", command = if virtual_size.is_some() { 349 } else { 320 },
        channel, effect, pc = state.program_counter, frame = state.current_frame,
        tick = state.tick, origin = ?position, size = ?size, virtual_size = ?virtual_size);
    state.effect_channels.insert(channel as u8, object);
    state
        .effect_elements
        .retain(|(owner, _), _| *owner != channel as u8);
    state.effect_elements.insert(
        (channel as u8, u32::MAX),
        CmvsEffectElement {
            visible: 1,
            ..Default::default()
        },
    );
    Ok(Some(CmvsPs2aVmAction::CreateEffectChannel {
        channel,
        effect,
    }))
}
