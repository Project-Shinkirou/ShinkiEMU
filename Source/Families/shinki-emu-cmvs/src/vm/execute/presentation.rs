use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::SetNativeViewSuppression { error_mask },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, index),
                (CmvsPs2aCommandStackWordKind::BooleanU32, enabled),
            ],
        ) => {
            // `488EF0` requires an occupied playback owner even though
            // `429850` updates the single renderer-global DWORD43.
            if *index >= 12 || !state.slot_objects.contains_key(&(750 << 8 | *index)) {
                raise_error_flag(state, 10532, error_mask)?;
            } else {
                state.native_view.suppress_alternate = *enabled != 0;
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureNativeAlternateView {
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, selector),
                (CmvsPs2aCommandStackWordKind::BooleanU32, enabled),
            ],
        ) => {
            if *selector != 0 {
                state
                    .native_view
                    .set_alternate(*enabled != 0, state.native_render_size);
            } else {
                state.write_interpreter_word(
                    result_field_offset,
                    u32::from(state.native_view.alternate_enabled),
                );
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureNativeViewPolicy { command_id },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            // Policy DWORD24/25 are distinct from renderer DWORD43:
            // 279 overrides only the renderer, while a changed policy
            // reapplies its own retained flag through 42B490/42B3D0.
            let flag_key = component_field_key(1920, 96);
            let flag = state
                .component_words
                .get(&flag_key)
                .copied()
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VIEW_POLICY",
                        "native view policy flag is not bound",
                    )
                })?;
            let (key, current, compared, stored) = match command_id {
                713 => (
                    flag_key,
                    flag,
                    u32::from(*value != 0),
                    u32::from(*value != 0),
                ),
                765 => {
                    let key = component_field_key(1920, 100);
                    let current = state.component_words.get(&key).copied().ok_or_else(|| {
                        invalid(
                            "SHINKI_EMU_CMVS_VIEW_POLICY",
                            "native view policy mode is not bound",
                        )
                    })?;
                    // 478AC0 compares the raw operand before 42B3D0 clamps it.
                    (key, current, *value, if *value <= 3 { *value } else { 1 })
                }
                _ => {
                    return Err(invalid(
                        "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                        "native view policy command is not supported",
                    ));
                }
            };
            if current == compared {
                return Ok(None);
            }
            state.component_words.insert(key, stored);
            state.native_view.suppress_alternate = if command_id == 713 {
                stored != 0
            } else {
                flag != 0
            };
            if command_id == 713 && state.native_view.fullscreen {
                state.native_view.reset_viewport(state.native_render_size);
                // 4789B0 rebuilds windowed then fullscreen, preserving the
                // final host mode, and returns C004 only on this changed path.
                state.write_interpreter_word(10516, 0);
                state.write_interpreter_word(10520, 0);
                state.write_interpreter_word(10708, 1);
                state.dispatch_stopped = true;
                return Ok(Some(CmvsPs2aVmAction::SetNativeFullscreen {
                    enabled: true,
                }));
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StorePresentationFrameFields {
                top_field_offset,
                second_field_offset,
                active_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, top),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
            ],
        ) => {
            state.write_interpreter_word(u32::from(top_field_offset), *top);
            state.write_interpreter_word(u32::from(second_field_offset), *second);
            state.write_interpreter_word(
                u32::from(active_field_offset),
                u32::from(*top != 0 && *second != 0),
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::CommitScreenParams { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, commit)],
        ) => {
            super::super::camera_transition::commit(state, *commit != 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryScreenPending {
                result_field_offset,
            },
            [],
        ) => {
            state.write_interpreter_word(
                result_field_offset,
                u32::from(super::super::camera_transition::pending(state)),
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenRgb { error_mask: _ },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, target),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, red),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, green),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, blue),
            ],
        ) => {
            // `sub_456DF0` stores `(float)(int)(value * 100.0) / 100.0` in the
            // screen object's fields 1..3.
            let hundredths = |bits: u32| -> u32 {
                super::super::camera_transition::hundredths(f32::from_bits(bits)).to_bits()
            };
            let words = camera_words_mut(state, *target);
            words.insert(1, hundredths(*red));
            words.insert(2, hundredths(*green));
            words.insert(3, hundredths(*blue));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenRotation { error_mask: _ },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, target),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, angle),
            ],
        ) => {
            // `sub_456C00`: field 14 keeps `|value * 100| % 36000 / 100.0`.
            let scaled =
                super::super::camera_transition::cvtt(f64::from(f32::from_bits(*angle) * 100.0));
            let magnitude = scaled.wrapping_abs() % 36_000;
            camera_words_mut(state, *target).insert(14, (magnitude as f32 / 100.0).to_bits());
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenScale { error_mask: _ },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, target),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
            ],
        ) => {
            // `sub_456D80`: fields 9/10 hold the values, 15/16 their halves.
            let first = f32::from_bits(*first);
            let second = f32::from_bits(*second);
            let words = camera_words_mut(state, *target);
            words.insert(9, first.to_bits());
            words.insert(10, second.to_bits());
            words.insert(15, (first * 0.5).to_bits());
            words.insert(16, (second * 0.5).to_bits());
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenOffset { error_mask: _ },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, target),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, third),
            ],
        ) => {
            // `sub_456DC0`: fields 7/8 and field 6.
            let words = camera_words_mut(state, *target);
            words.insert(7, *first);
            words.insert(8, *second);
            words.insert(6, *third);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenField {
                error_mask: _,
                field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, target),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, value),
            ],
        ) => {
            // `sub_456690`/`sub_457C90` store one dword at the screen field.
            camera_words_mut(state, *target).insert(field_offset, *value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenScalePair { error_mask: _ },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, target),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
            ],
        ) => {
            // `sub_456D60` stores the pair at fields 15/16.
            let words = camera_words_mut(state, *target);
            words.insert(15, *first);
            words.insert(16, *second);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenFlag { error_mask: _ },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, target),
                (CmvsPs2aCommandStackWordKind::BooleanU32, value),
            ],
        ) => {
            // `sub_457CA0` stores the boolean at field 37.
            camera_words_mut(state, *target).insert(37, u32::from(*value != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenPair { error_mask: _ },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, target),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
            ],
        ) => {
            // `sub_456BE0` stores the pair at fields 17/18.
            let words = camera_words_mut(state, *target);
            words.insert(17, *first);
            words.insert(18, *second);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenOffsetDirect { error_mask: _ },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, third),
            ],
        ) => {
            // `sub_456DC0` without a target selector.
            state.screen_words.insert(7, *first);
            state.screen_words.insert(8, *second);
            state.screen_words.insert(6, *third);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenScaleDirect { error_mask: _ },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
            ],
        ) => {
            // `sub_456D80` without a target selector.
            let first = f32::from_bits(*first);
            let second = f32::from_bits(*second);
            state.screen_words.insert(9, first.to_bits());
            state.screen_words.insert(10, second.to_bits());
            state.screen_words.insert(15, (first * 0.5).to_bits());
            state.screen_words.insert(16, (second * 0.5).to_bits());
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenRgbDirect { error_mask: _ },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, red),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, green),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, blue),
            ],
        ) => {
            let hundredths = |bits: u32| -> u32 {
                super::super::camera_transition::hundredths(f32::from_bits(bits)).to_bits()
            };
            state.screen_words.insert(1, hundredths(*red));
            state.screen_words.insert(2, hundredths(*green));
            state.screen_words.insert(3, hundredths(*blue));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenRotationDirect { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, angle)],
        ) => {
            let scaled =
                super::super::camera_transition::cvtt(f64::from(f32::from_bits(*angle) * 100.0));
            let magnitude = scaled.wrapping_abs() % 36_000;
            state
                .screen_words
                .insert(14, (magnitude as f32 / 100.0).to_bits());
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenFieldDirect {
                error_mask: _,
                field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            state.screen_words.insert(field_offset, *value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenScalePairDirect { error_mask: _ },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
            ],
        ) => {
            state.screen_words.insert(15, *first);
            state.screen_words.insert(16, *second);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryTextureParentImageInfo {
                slot_table_dword_index,
                error_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot)],
        ) => {
            let slot_key = u16::try_from(slot_table_dword_index)
                .ok()
                .and_then(|table| slot_object_key(table, *slot).ok());
            let occupied = slot_key
                .as_ref()
                .is_some_and(|key| state.slot_objects.contains_key(key));
            if !occupied {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // 484020 reports the allocated image provider, then its actual
            // dimensions and source-alpha flag. An absent provider leaves
            // the three auxiliary result words untouched.
            let info = state
                .texture_parents
                .get(&(*slot as u8))
                .filter(|texture| texture.resource.is_some())
                .map(|texture| texture.image_info);
            state.write_interpreter_word(result_field_offset, u32::from(info.is_some()));
            if let Some(info) = info {
                for (index, value) in info.into_iter().enumerate() {
                    state.write_interpreter_word(
                        result_field_offset + 4 * (index as u32 + 1),
                        value,
                    );
                }
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryChannelChildState {
                slot_table_dword_index,
                error_mask,
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, slot),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _select),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _id),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _param),
            ],
        ) => {
            // `sub_483FB0` -> `sub_444F10`: the channel-bound child's
            // vtable query result lands in the pinned word; with no bound
            // child the native returns 0.
            let slot_key = slot_table_dword_index
                .checked_mul(4)
                .and_then(|offset| u16::try_from(offset).ok())
                .and_then(|table| slot_object_key(table, *slot).ok());
            let occupied = slot_key
                .as_ref()
                .is_some_and(|key| state.slot_objects.contains_key(key));
            if !occupied {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let child_signed = *_select as i32;
            let payload_value = state
                .channel_child_presenters
                .get(&(*slot, child_signed))
                .map(|presenter| {
                    let index = ((*_id as usize) & 7).min(7);
                    presenter.payload_entries[index][1]
                })
                .unwrap_or(0);
            state.write_interpreter_word(result_field_offset, payload_value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::LoadTextureParentFromName {
                texture_table_byte_offset,
                error_mask,
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, slot),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, name),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, mode),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, color),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, kernel),
            ],
        ) => {
            let resource = resolve_resource_name(state, *name)?;
            let slot_key = u16::try_from(texture_table_byte_offset)
                .ok()
                .and_then(|table| slot_object_key(table, *slot).ok());
            let occupied = slot_key
                .as_ref()
                .is_some_and(|key| state.slot_objects.contains_key(key));
            tracing::info!(
                event = "shinki.emu.cmvs.vm.from_name_load",
                frame = state.current_frame,
                occupied,
                table = texture_table_byte_offset,
                image_mode = *mode,
                color = *color,
                kernel = *kernel,
                pc = state.program_counter,
                slot,
                name_word = name,
                tick = state.tick
            );
            if !occupied {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let parent_slot = *slot as u8;
            let parent = state.texture_parents.get_mut(&parent_slot).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_TEXTURE",
                    "live image parent has no specialized state",
                )
            })?;
            parent.resource = Some(resource.clone());
            parent.resource_frame = Some(state.current_frame);
            parent.image_info = [0; 3];
            parent.color_filter = Some([*mode, *color, *kernel]);
            state.write_interpreter_word(result_field_offset, 1);
            Ok(Some(CmvsPs2aVmAction::LoadTextureParentResource {
                parent_slot,
                resource,
            }))
        }
        (CmvsPs2aCommandEffectKind::StartCameraTransition { command_id }, values) => {
            let mut arguments = [0u32; 7];
            if values.len() > arguments.len() {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                    "camera operand count exceeds its native frame",
                ));
            }
            for (argument, (_, value)) in arguments.iter_mut().zip(values) {
                *argument = *value;
            }
            super::super::camera_transition::start(state, command_id, &arguments[..values.len()])?;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryChannelTransitionBusy {
                slot_table_dword_index,
                error_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            // This is the parent image table, expressed in native dwords.
            let slot_key = slot_table_dword_index
                .checked_mul(4)
                .and_then(|offset| u16::try_from(offset).ok())
                .and_then(|table| slot_object_key(table, *channel).ok());
            let occupied = slot_key
                .as_ref()
                .is_some_and(|key| state.slot_objects.contains_key(key));
            if !occupied {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let busy = state
                .texture_reloads
                .get(&(*channel as u8))
                .is_some_and(crate::vm::texture_reload::TextureReload::busy);
            state.write_interpreter_word(result_field_offset, u32::from(busy));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReleaseChannelTransition,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            let parent_slot = u8::try_from(*channel).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_TEXTURE",
                    "image transition parent is out of range",
                )
            })?;
            state.texture_reloads.remove(&parent_slot);
            Ok(Some(CmvsPs2aVmAction::ReleaseTextureReload { parent_slot }))
        }
        (
            CmvsPs2aCommandEffectKind::ReloadTextureParentImage {
                result_field_offset,
                table_byte_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, slot),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, name),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, duration),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, secondary),
            ],
        ) => {
            let resource = resolve_resource_name(state, *name)?;
            let key = slot_object_key(table_byte_offset, *slot)?;
            tracing::info!(
                event = "cmvs.vm.surface_reload_request",
                frame = state.current_frame,
                pc = state.program_counter,
                slot,
                occupied = state.slot_objects.contains_key(&key),
                name_bytes = resource.len(),
                duration_ms = *duration,
                secondary_bank = *secondary != 0,
                tick = state.tick,
            );
            state.write_interpreter_word(result_field_offset, 0);
            if !state.slot_objects.contains_key(&key) {
                return Ok(None);
            }
            let parent_slot = *slot as u8;
            crate::vm::texture_reload::begin(state, parent_slot, *duration)?;
            Ok(Some(CmvsPs2aVmAction::ReloadTextureParentImage {
                parent_slot,
                resource,
                bank: if *secondary == 0 {
                    CmvsResourceSearchBank::Texture
                } else {
                    CmvsResourceSearchBank::Native0d00
                },
                result_field_offset,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::KickTextureChildDraw,
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _child),
            ],
        ) => {
            // `sub_4822F0` -> `sub_42BC00`: the draw-state advance is a
            // renderer-side void; no interpreter state changes.
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreTextureChildByIndex,
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _index),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _value),
                ..,
            ],
        ) => {
            // `sub_483530`: the same 481-table slot gate as case 505; the
            // child write is renderer-side state.
            if *channel >= 256 || !state.slot_objects.contains_key(&(481_u32 << 8 | *channel)) {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.write_interpreter_word(10532, flags | 0x10);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReleaseTextureChildByIndex,
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, index),
            ],
        ) => {
            // `sub_482130`: the 481-table slot must be live (0x10) and the
            // index below 1024 (0x20); the release is renderer-side, the
            // recovered subset keeps only the flag gates.
            if *channel >= 256 || !state.slot_objects.contains_key(&(481_u32 << 8 | *channel)) {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.write_interpreter_word(10532, flags | 0x10);
            } else if *index >= 1024 {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.write_interpreter_word(10532, flags | 0x20);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureTextureChildGeometry,
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, index),
                ..,
            ],
        ) => {
            // `sub_482220`: the same 481-table gates as case 505; the
            // nine-word geometry hand-off is renderer-side state.
            if *channel >= 256 || !state.slot_objects.contains_key(&(481_u32 << 8 | *channel)) {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.write_interpreter_word(10532, flags | 0x10);
            } else if *index >= 1024 {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.write_interpreter_word(10532, flags | 0x20);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreTextureChildWord,
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, index),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _value),
            ],
        ) => {
            // `sub_482360`: the same 481-table gates as case 505; the
            // child word store is renderer-side state.
            if *channel >= 256 || !state.slot_objects.contains_key(&(481_u32 << 8 | *channel)) {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.write_interpreter_word(10532, flags | 0x10);
            } else if *index >= 1024 {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.write_interpreter_word(10532, flags | 0x20);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetSystemVolume,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _volume)],
        ) => {
            // `sub_489CB0`: the clamped volume applies system-side; no
            // interpreter state changes.
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QuerySceneLayer {
                base_word,
                result_field_offset,
                aux_field_offset,
            },
            [],
        ) => {
            // `sub_47A0B0`/`sub_479D70`/`sub_479E30`/`sub_47A070`/`sub_479FB0`
            // publish `(scene[base] != 0, scene[base + 1] != 0)`.
            let first = state.scene_words.get(&base_word).copied().unwrap_or(0);
            let second = state
                .scene_words
                .get(&(base_word + 1))
                .copied()
                .unwrap_or(0);
            state.write_interpreter_word(result_field_offset, u32::from(first != 0));
            state.write_interpreter_word(aux_field_offset, u32::from(second != 0));
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word }, []) => {
            // `sub_4616E0`/`sub_4612B0`/`sub_461310`/`sub_4616C0`/
            // `sub_4613D0` clear the group's base and `base + 2` words and
            // leave `base + 1` untouched.
            state.scene_words.remove(&base_word);
            state.scene_words.remove(&(base_word + 2));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::LayoutMessageText {
                max_selector,
                error_flag_field_offset,
                error_flag_mask,
                result_field_offset,
                extent_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, selector),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, _text),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _scale),
                (CmvsPs2aCommandStackWordKind::BooleanU32, _bold),
            ],
        ) => {
            // `sub_489630` measures the resolved message string through the
            // window engine's text layout (`sub_48DB70`), reporting the
            // layout result and the measured extent. The headless page
            // renderer lays the message text out itself, so the layout
            // reports success and the extent stays zero.
            if *selector > max_selector {
                let flags = state
                    .interpreter_words
                    .get(&error_flag_field_offset)
                    .copied()
                    .unwrap_or(0);
                state.write_interpreter_word(error_flag_field_offset, flags | error_flag_mask);
                return Ok(None);
            }
            state.write_interpreter_word(result_field_offset, 1);
            state.write_interpreter_word(extent_field_offset, 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::TogglePresentationMode {
                first_field_offset,
                second_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, enabled)],
        ) => Ok(Some(CmvsPs2aVmAction::TogglePresentationMode {
            first_field_offset,
            second_field_offset,
            enabled: *enabled != 0,
        })),
        (CmvsPs2aCommandEffectKind::ResetPresentationBuffers, []) => {
            // `sub_45C5A0` clears presentation-manager counters only; the
            // headless presentation has no observable counterpart.
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::DestroyTextureChild {
                texture_table_byte_offset,
                error_flag_field_offset,
                error_flag_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, slot),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
            ],
        ) => {
            if *slot >= 256 {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            let key = slot_object_key(texture_table_byte_offset, *slot)?;
            if !state.slot_objects.contains_key(&key) {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            if *child >= 1024 {
                return Ok(None);
            }
            if let Some(children) = state.texture_children.get_mut(&(*slot as u8)) {
                children.remove(&(*child as u16));
            }
            state
                .channel_child_presenters
                .remove(&(*slot, *child as i32));
            Ok(Some(CmvsPs2aVmAction::RemoveTextureChildSurface {
                parent_slot: *slot as u8,
                child_id: *child as u16,
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

fn camera_words_mut(state: &mut CmvsPs2aVmState, target: u32) -> &mut BTreeMap<u32, u32> {
    if target == 0 {
        &mut state.screen_words
    } else {
        state.screen_camera_words.entry(target).or_default()
    }
}
