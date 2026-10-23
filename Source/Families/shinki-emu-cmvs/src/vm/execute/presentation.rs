use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::StorePresentationFrameFields {
                top_field_offset,
                second_field_offset,
                active_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, top), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            state
                .interpreter_words
                .insert(u32::from(top_field_offset), *top);
            state
                .interpreter_words
                .insert(u32::from(second_field_offset), *second);
            state.interpreter_words.insert(
                u32::from(active_field_offset),
                u32::from(*top != 0 && *second != 0),
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::CommitScreenParams { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, commit)],
        ) => {
            // `sub_484D00` -> `sub_457B90`: only a pending commit is applied.
            if *commit != 0 && state.screen_pending {
                state.screen_pending = false;
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::WaitScreenCommit {
                error_mask: _,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _selector)],
        ) => {
            // `sub_484D40` blocks the frame with 0xA000 while a commit is
            // pending. The recovered subset has no asynchronous render state,
            // so a pending commit is completed here and reported through the
            // result field.
            let committed = state.screen_pending;
            state.screen_pending = false;
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(committed));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryScreenPending {
                result_field_offset,
            },
            [],
        ) => {
            // `sub_484CD0` publishes `sub_456E70`'s pending flag.
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(state.screen_pending));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenRgb { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::OpaqueU32, red), (CmvsPs2aCommandStackWordKind::OpaqueU32, green), (CmvsPs2aCommandStackWordKind::OpaqueU32, blue)],
        ) => {
            // `sub_456DF0` stores `(float)(int)(value * 100.0) / 100.0` in the
            // screen object's fields 1..3.
            let hundredths = |bits: u32| -> u32 {
                ((f32::from_bits(bits) * 100.0) as i32 as f32 / 100.0).to_bits()
            };
            state.screen_words.insert(1, hundredths(*red));
            state.screen_words.insert(2, hundredths(*green));
            state.screen_words.insert(3, hundredths(*blue));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenRotation { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::OpaqueU32, angle)],
        ) => {
            // `sub_456C00`: field 14 keeps `|value * 100| % 36000 / 100.0`.
            let scaled = (f32::from_bits(*angle) * 100.0) as i32;
            let magnitude = scaled.unsigned_abs() % 36_000;
            state
                .screen_words
                .insert(14, (magnitude as f32 / 100.0).to_bits());
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenScale { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            // `sub_456D80`: fields 9/10 hold the values, 15/16 their halves.
            let first = f32::from_bits(*first);
            let second = f32::from_bits(*second);
            state.screen_words.insert(9, first.to_bits());
            state.screen_words.insert(10, second.to_bits());
            state.screen_words.insert(15, (first * 0.5).to_bits());
            state.screen_words.insert(16, (second * 0.5).to_bits());
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenOffset { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third)],
        ) => {
            // `sub_456DC0`: fields 7/8 and field 6.
            state.screen_words.insert(7, *first);
            state.screen_words.insert(8, *second);
            state.screen_words.insert(6, *third);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenField {
                error_mask: _,
                field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            // `sub_456690`/`sub_457C90` store one dword at the screen field.
            state.screen_words.insert(field_offset, *value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenScalePair { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            // `sub_456D60` stores the pair at fields 15/16.
            state.screen_words.insert(15, *first);
            state.screen_words.insert(16, *second);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenFlag { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::BooleanU32, value)],
        ) => {
            // `sub_457CA0` stores the boolean at field 37.
            state.screen_words.insert(37, u32::from(*value != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenPair { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _target), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            // `sub_456BE0` stores the pair at fields 17/18.
            state.screen_words.insert(17, *first);
            state.screen_words.insert(18, *second);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenOffsetDirect { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third)],
        ) => {
            // `sub_456DC0` without a target selector.
            state.screen_words.insert(7, *first);
            state.screen_words.insert(8, *second);
            state.screen_words.insert(6, *third);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureScreenScaleDirect { error_mask: _ },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
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
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, red), (CmvsPs2aCommandStackWordKind::OpaqueU32, green), (CmvsPs2aCommandStackWordKind::OpaqueU32, blue)],
        ) => {
            let hundredths = |bits: u32| -> u32 {
                ((f32::from_bits(bits) * 100.0) as i32 as f32 / 100.0).to_bits()
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
            let scaled = (f32::from_bits(*angle) * 100.0) as i32;
            let magnitude = scaled.unsigned_abs() % 36_000;
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
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            state.screen_words.insert(15, *first);
            state.screen_words.insert(16, *second);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreScriptSlotPresentationState {
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
            // A texture parent's decoded resource is the canonical
            // presentation state for a script slot (`sub_484020`'s status
            // triple collapses to ready-with-resource in headless).
            let has_resource = state
                .texture_parents
                .get(&(*slot as u8))
                .is_some_and(|texture| texture.resource.is_some());
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(has_resource));
            if has_resource {
                state.interpreter_words.insert(result_field_offset + 4, 0);
                state.interpreter_words.insert(result_field_offset + 8, 0);
                state.interpreter_words.insert(result_field_offset + 12, 1);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryChannelChildState {
                slot_table_dword_index,
                error_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::OpaqueU32, _select), (CmvsPs2aCommandStackWordKind::OpaqueU32, _id), (CmvsPs2aCommandStackWordKind::OpaqueU32, _param)],
        ) => {
            // `sub_483FB0` -> `sub_444F10`: the channel-bound child's
            // vtable query result lands in the pinned word; with no bound
            // child the native returns 0.
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
            let child_signed = *_select as i32;
            let payload_value = state
                .channel_child_presenters
                .get(&(*slot, child_signed))
                .map(|presenter| {
                    let index = ((*_id as usize) & 7).min(7);
                    presenter.payload_entries[index][1]
                })
                .unwrap_or(0);
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.child_state_query",
                    pc = state.program_counter,
                    channel = *slot,
                    child = child_signed,
                    id = *_id,
                    param = *_param,
                    value = payload_value
                );
            }
            state
                .interpreter_words
                .insert(result_field_offset, payload_value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::LoadTextureParentFromName {
                slot_table_dword_index,
                error_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::OpaqueU32, _name), (CmvsPs2aCommandStackWordKind::OpaqueU32, _arg1), (CmvsPs2aCommandStackWordKind::OpaqueU32, _arg2), (CmvsPs2aCommandStackWordKind::OpaqueU32, _arg3)],
        ) => {
            // `sub_483EB0`: the resolved name loads into the 784-table
            // texture-parent slot; the synchronous archive always succeeds
            // and the boolean lands in the pinned result word.
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
            state.interpreter_words.insert(result_field_offset, 1);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreTransitionParams,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, mode), (CmvsPs2aCommandStackWordKind::OpaqueU32, _rate1), (CmvsPs2aCommandStackWordKind::OpaqueU32, _rate2), (CmvsPs2aCommandStackWordKind::OpaqueU32, _rate3), (CmvsPs2aCommandStackWordKind::OpaqueU32, _flag)],
        ) => {
            // `sub_4576F0`: the transition component's mode (0/1/2 →
            // 0/65793/131586), three rounded rate floats and a flag land in
            // the renderer; no interpreter state changes.
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.transition_params",
                    pc = state.program_counter,
                    mode = *mode
                );
            }
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
            // `sub_47A4C0` -> `sub_444CB0`: the 481-table channel object's
            // transition words (dwords 3100/3101 = u16 words 6200/6202)
            // gate the result; the synchronous headless renderer completes
            // transitions immediately, so unbound words report idle.
            let slot_key = u16::try_from(slot_table_dword_index)
                .ok()
                .and_then(|table| slot_object_key(table, *channel).ok());
            let occupied = slot_key
                .as_ref()
                .is_some_and(|key| state.slot_objects.contains_key(key));
            if !occupied {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            let busy = state
                .effect_channels
                .get(&(*channel as u8))
                .is_some_and(|object| {
                    object.read_dword(6200) != 0 || object.read_dword(6202) != 0
                });
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(busy));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReleaseChannelTransition,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _channel)],
        ) => {
            // `sub_47A490` -> `sub_444BE0`: releases the channel's
            // transition sub-objects; the recovered channel words are never
            // armed, so the release is unobservable.
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReloadSlotTextResource {
                result_field_offset,
                table_byte_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name), (CmvsPs2aCommandStackWordKind::OpaqueU32, _param), (CmvsPs2aCommandStackWordKind::OpaqueU32, _flag)],
        ) => {
            // `sub_425B60`: the paired slot entry must already be live —
            // the handler returns failure without creating anything when
            // `this+481+slot` is zero. A live entry is destroyed and
            // recreated with the next deterministic identity, and the
            // synchronous headless archive loads the named resource at
            // once, so the pinned result word takes the success value.
            let key = slot_object_key(table_byte_offset, *slot)?;
            let succeeded = if state.slot_objects.contains_key(&key) {
                let identity = state.next_slot_object_id;
                state.next_slot_object_id = identity
                    .checked_add(1)
                    .ok_or_else(|| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_SLOT_OBJECTS",
                            "CMVS slot object identity space overflowed",
                        )
                    })?;
                state.slot_objects.insert(key, identity);
                true
            } else {
                false
            };
            if succeeded && tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.slot_text_reload",
                    pc = state.program_counter,
                    slot = *slot,
                    name = resolve_resource_name(state, *name)?.as_str()
                );
            }
            state.interpreter_words.insert(result_field_offset, u32::from(succeeded));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::KickTextureChildDraw,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _child)],
        ) => {
            // `sub_4822F0` -> `sub_42BC00`: the draw-state advance is a
            // renderer-side void; no interpreter state changes.
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreTextureChildByIndex,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _index), (CmvsPs2aCommandStackWordKind::OpaqueU32, _value), ..],
        ) => {
            // `sub_483530`: the same 481-table slot gate as case 505; the
            // child write is renderer-side state.
            if *channel >= 256 || !state.slot_objects.contains_key(&(481_u32 << 8 | *channel)) {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.interpreter_words.insert(10532, flags | 0x10);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReleaseTextureChildByIndex,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, index)],
        ) => {
            // `sub_482130`: the 481-table slot must be live (0x10) and the
            // index below 1024 (0x20); the release is renderer-side, the
            // recovered subset keeps only the flag gates.
            if *channel >= 256 || !state.slot_objects.contains_key(&(481_u32 << 8 | *channel)) {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.interpreter_words.insert(10532, flags | 0x10);
            } else if *index >= 1024 {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.interpreter_words.insert(10532, flags | 0x20);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureTextureChildGeometry,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, index), ..],
        ) => {
            // `sub_482220`: the same 481-table gates as case 505; the
            // nine-word geometry hand-off is renderer-side state.
            if *channel >= 256 || !state.slot_objects.contains_key(&(481_u32 << 8 | *channel)) {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.interpreter_words.insert(10532, flags | 0x10);
            } else if *index >= 1024 {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.interpreter_words.insert(10532, flags | 0x20);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreTextureChildWord,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, index), (CmvsPs2aCommandStackWordKind::OpaqueU32, _value)],
        ) => {
            // `sub_482360`: the same 481-table gates as case 505; the
            // child word store is renderer-side state.
            if *channel >= 256 || !state.slot_objects.contains_key(&(481_u32 << 8 | *channel)) {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.interpreter_words.insert(10532, flags | 0x10);
            } else if *index >= 1024 {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.interpreter_words.insert(10532, flags | 0x20);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::TriggerEffectSlot,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _slot)],
        ) => {
            // `sub_47CAC0`: the renderer-side trigger has no interpreter
            // state; the slot bound check raises 0x1000000 above two.
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
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(first != 0));
            state
                .interpreter_words
                .insert(aux_field_offset, u32::from(second != 0));
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
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, selector), (CmvsPs2aCommandStackWordKind::TaggedStringReference, _text), (CmvsPs2aCommandStackWordKind::OpaqueU32, _scale), (CmvsPs2aCommandStackWordKind::BooleanU32, _bold)],
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
                state
                    .interpreter_words
                    .insert(error_flag_field_offset, flags | error_flag_mask);
                return Ok(None);
            }
            state.interpreter_words.insert(result_field_offset, 1);
            state.interpreter_words.insert(extent_field_offset, 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::DisposeSceneLayerWorker {
                table_dword_index,
                max_layer,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, layer)],
        ) => {
            // `sub_4898C0` joins the layer worker thread (`sub_48CAB0`).
            // The recovered subset holds no worker threads, so the handle
            // dword is cleared and the dispose completes immediately.
            if *layer > max_layer {
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
            state.interpreter_words.remove(&(table_dword_index + layer));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryAuxiliarySlotStatus {
                result_field_offset,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index)],
        ) => {
            // `sub_48C230` resolves the auxiliary slot object at
            // `this+592+(index+5)` and reports `sub_48C160`'s status, or
            // `-1` when the entry is absent or not fully armed. The
            // table-592 object graph is unrecovered, so absence is the
            // faithful observable.
            if *index > 3 {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.interpreter_words.insert(10532, flags | error_mask);
            } else {
                state
                    .interpreter_words
                    .insert(result_field_offset, u32::MAX);
            }
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
            CmvsPs2aCommandEffectKind::SelectPresentationSlot {
                slot_table_dword_index,
                max_slot,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::OpaqueU32, _selector)],
        ) => {
            let _ = slot_table_dword_index;
            if *slot > max_slot {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
                return Ok(None);
            }
            // The chosen slot becomes the presentation target (`sub_445E60`);
            // the headless compositor keeps it in the interpreter words.
            state.interpreter_words.insert(0x7421_0020, *slot);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SelectRenderViewport {
                slot_table_dword_index,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::OpaqueU32, _viewport)],
        ) => {
            let key = slot_object_key(
                u16::try_from(slot_table_dword_index).map_err(|_| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_SLOT_OBJECTS",
                        "CMVS slot table offset overflowed",
                    )
                })?,
                *slot,
            )?;
            if *slot >= 256 || !state.slot_objects.contains_key(&key) {
                raise_error_flag(state, error_flag_field_offset, error_flag_mask)?;
            }
            // The renderer viewport selection has no observable headless
            // counterpart; the compositor renders the full stage.
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
