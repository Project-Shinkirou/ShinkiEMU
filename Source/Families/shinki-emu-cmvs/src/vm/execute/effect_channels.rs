use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::BindMessageChannelOrRaise {
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, word)],
        ) => {
            // `sub_4845D0`: the zero path binds channel 15 (nothing to
            // bind headless); the non-zero path only raises the mask.
            if *word != 0 {
                let flags = state
                    .interpreter_words
                    .get(&error_flag_field_offset)
                    .copied()
                    .unwrap_or_default();
                state
                    .interpreter_words
                    .insert(error_flag_field_offset, flags | error_flag_mask);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreChannelVisibilityRecord {
                enabled_field_base_offset,
                touched_field_base_offset,
                record_stride_bytes,
                notify_gate_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::BooleanU32, enabled)],
        ) => {
            if *channel > 5 {
                let flags = state
                    .interpreter_flag_words
                    .entry(INTERPRETER_ERROR_FLAG_OFFSET)
                    .or_insert(0);
                *flags |= 0x200;
                return Ok(None);
            }
            let channel = u8::try_from(*channel).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_CHANNEL_RECORD",
                    "CMVS channel visibility record index overflowed",
                )
            })?;
            let stride = u16::from(record_stride_bytes);
            let enabled_offset = enabled_field_base_offset + stride * u16::from(channel);
            let touched_offset = touched_field_base_offset + stride * u16::from(channel);
            state
                .interpreter_words
                .insert(u32::from(enabled_offset), u32::from(*enabled != 0));
            state.interpreter_words.insert(u32::from(touched_offset), 1);
            // The notification call only happens when the gate field is
            // non-zero. The gate is unrecovered engine state retained in the
            // opaque word map and defaults to zero until proven otherwise.
            let gate = state
                .interpreter_words
                .get(&u32::from(notify_gate_field_offset))
                .copied()
                .unwrap_or(0);
            if gate != 0 {
                let target_index = channel.checked_add(9).ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_CHANNEL_RECORD",
                        "CMVS channel visibility notify index overflowed",
                    )
                })?;
                Ok(Some(CmvsPs2aVmAction::NotifyChannelVisibility {
                    target_index,
                    enabled: *enabled != 0,
                }))
            } else {
                Ok(None)
            }
        }
        (
            CmvsPs2aCommandEffectKind::ResetChannelVisibilityRecord {
                name_buffer_base_offset,
                touched_field_base_offset,
                record_stride_bytes,
                error_flag_mask,
                teardown_slot_table_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            if *channel > 5 {
                let flags = state
                    .interpreter_flag_words
                    .entry(INTERPRETER_ERROR_FLAG_OFFSET)
                    .or_insert(0);
                *flags |= error_flag_mask;
                return Ok(None);
            }
            let channel = u8::try_from(*channel).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_CHANNEL_RECORD",
                    "CMVS channel visibility record index overflowed",
                )
            })?;
            // The teardown helper waits until the slot at channel+9 drains;
            // its postcondition is an empty slot, so model the completed
            // teardown instead of the wait itself.
            let teardown_index = u32::from(channel.checked_add(9).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_CHANNEL_RECORD",
                    "CMVS channel teardown slot index overflowed",
                )
            })?);
            let teardown_key = slot_object_key(teardown_slot_table_offset, teardown_index)?;
            state.slot_objects.remove(&teardown_key);
            state.slot_object_seed_words.remove(&teardown_key);
            let stride = u16::from(record_stride_bytes);
            let buffer_offset = name_buffer_base_offset + stride * u16::from(channel);
            let touched_offset = touched_field_base_offset + stride * u16::from(channel);
            // The reset copies the unrecovered engine-owned `Default`
            // constant, so no script-owned content survives in the buffer.
            state
                .interpreter_string_buffers
                .insert(buffer_offset, Vec::new());
            state.interpreter_words.insert(u32::from(touched_offset), 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ForwardEffectChannelWord {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _value)],
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
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    event = "shinki.emu.cmvs.vm.forward_word",
                    channel = *channel,
                    value = values.get(1).map(|(_, v)| *v).unwrap_or(0)
                );
            }
            if tracing::enabled!(tracing::Level::INFO) && *channel == 0 {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.fade_forward_pc",
                    pc = state.program_counter,
                    frame = state.current_frame,
                    value = _value,
                    tick = state.tick
                );
            }
            Ok(Some(CmvsPs2aVmAction::ForwardEffectChannelWord {
                channel: *channel,
                value: values.get(1).map(|(_, v)| *v).unwrap_or(0),
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChannelOrigin { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, mode), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
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
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectPlaybackField {
                error_mask,
                field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index), (CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            // `sub_488FC0`/`sub_488F80`/`sub_488F40` reject an index at or
            // above 12 and an unregistered playback record with 0x1000.
            let occupied = *index < 12 && state.slot_objects.contains_key(&(750_u32 << 8 | *index));
            if !occupied {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            state
                .effect_playback
                .entry(*index as u8)
                .or_default()
                .set_field(field_offset, *value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChannelPlaybackMode { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, mode), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            if !effect_channel_is_occupied(state, *channel) {
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
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
        ) => {
            if !effect_channel_is_occupied(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
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
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _name), (CmvsPs2aCommandStackWordKind::OpaqueU32, _mode), (CmvsPs2aCommandStackWordKind::OpaqueU32, _flag)],
        ) => {
            // `sub_466ED0` applies the named source configuration to the
            // channel (the visual effect stays in the renderer) and reports
            // whether the channel settled: `*(this+2) == 0 && pending <= 0`.
            // The recovered channel objects carry no pending queue, so an
            // occupied channel always reports settled.
            if !effect_channel_is_occupied(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            state
                .interpreter_words
                .insert(result_field_offset, 1);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PrepareEffectChannelFromName { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name)],
        ) => {
            // `sub_47E9E0`: case 345's wait twin. The native handler
            // returns `0xA000` while `sub_466ED0` reports the channel
            // unsettled and `0xC00C` (stop, pop 12) once the queue drains;
            // the headless channel objects settle immediately, so the
            // stop-dispatch half is the observable behaviour. Dead or
            // out-of-range channels raise the shared error flag and still
            // end the frame.
            let name = resolve_resource_name(state, *name)?;
            if *channel > 7 || !effect_channel_is_occupied(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
            }
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.channel_prepare",
                    pc = state.program_counter,
                    channel = *channel,
                    name = name.as_str()
                );
            }
            state.dispatch_stopped = true;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StartEffectChannelPlayback { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, frames)],
        ) => {
            // `sub_47F7D0` (case 344): starts `sub_467510` on the occupied
            // channel and waits (`0xA000`) while the playback runs. The
            // recovered channel objects carry no pending playback, so the
            // completion return (`0x400C`, pop 12, no stop) applies at
            // once; the frame-count operand has nothing to drive.
            if *channel > 7 || !effect_channel_is_occupied(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
            } else {
                // `sub_467510` marks the channel element active (native
                // object field 8) for the playback duration: rec17 polls
                // that flag through cmd 333, and the message chain behind
                // it flips the settings request words when the element
                // state reaches its terminal value. The headless model
                // completes the playback synchronously, so the activity
                // is armed and the terminal element state is published in
                // one step: rec17 observes a completed message exactly as
                // the native renderer would report after the animation.
                state
                    .interpreter_words
                    .insert(effect_channel_activity_field(*channel), 1);
                state.interpreter_words.insert(81220, 1);
                state.interpreter_words.insert(81224, 13);
                if tracing::enabled!(tracing::Level::INFO) {
                    let ch = state.effect_channels.get(&(*channel as u8));
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.channel_playback",
                        pc = state.program_counter,
                        channel = *channel,
                        frames = *frames,
                        word618 = ch.map(|c| c.read_dword(618)).unwrap_or(0),
                        word619 = ch.map(|c| c.read_dword(619)).unwrap_or(0),
                        word622 = ch.map(|c| c.read_dword(622)).unwrap_or(0),
                        word18 = ch.map(|c| c.read_dword(18)).unwrap_or(0)
                    );
                }
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ApplyEffectChannelOperation { error_mask },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            // `sub_47FB20` -> `sub_467A10` -> `sub_464C00(channel[631])`; the
            // channel check is the VM-visible effect.
            if !effect_channel_is_occupied(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectState {
                first_field_offset,
                second_field_offset,
                third_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            if *channel > 7 || !effect_channel_is_occupied(state, *channel) {
                raise_error_flag(state, 10532, 0x0002_0000)?;
                return Ok(None);
            }
            // `sub_466870` hit-tests the pointer against the channel's
            // visible quads: the pressed quad is latched on the press edge
            // (`sub_45CCD0` reads manager dword 273), the hit index is
            // published, and a release on the latched quad (`sub_45CCB0`
            // reads dword 271) confirms the click. The pointer and the quad
            // rectangles share the stage's coordinate space.
            //
            // Only a release *inside* a visible quad (or a release while a
            // quad is latched) reaches `sub_45CB80`, which clears both
            // advance latches. A release that lands on empty stage keeps the
            // latch readable, which is what the scene's page loop polls.
            let press = state.input_advance_press;
            let release = state.input_advance_release;
            let channel = u8::try_from(*channel).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_EFFECT",
                    "CMVS effect channel index overflowed",
                )
            })?;
            let mut pressed = if press {
                -1
            } else {
                state
                    .effect_channels
                    .get(&channel)
                    .map(|object| object.read_dword(632) as i32)
                    .unwrap_or(-1)
            };
            let mut hit = -1_i32;
            let mut confirmed = 0_u32;
            let mut consumed = false;
            // The stage descriptor quad: the scenario wait gates compare the
            // hit against the layout word fw16 (`intproc` 191194).
            let stage_quad = state
                .frame_local_word(16)
                .filter(|quad| (1..32).contains(quad));
            {
                let object = state.effect_channels.entry(channel).or_default();
                let (origin_x, origin_y) = object.origin();
                for quad in 0..32_u32 {
                    if !object.quad_visible(quad) {
                        continue;
                    }
                    let [rect_x, rect_y, rect_w, rect_h] = object.quad_rect(quad);
                    let left = origin_x.wrapping_add(rect_x);
                    let top = origin_y.wrapping_add(rect_y);
                    let right = left.wrapping_add(rect_w);
                    let bottom = top.wrapping_add(rect_h);
                    if state.pointer_x < left
                        || state.pointer_y < top
                        || state.pointer_x > right
                        || state.pointer_y > bottom
                    {
                        continue;
                    }
                    if press {
                        pressed = quad as i32;
                    }
                    hit = quad as i32;
                    if release {
                        consumed = true;
                        if pressed == quad as i32 {
                            confirmed = 1;
                        }
                    }
                }
                // A keyboard confirm carries no pointer position, so its
                // press and release edges land on no quad rectangle. The
                // scenario wait gates compare the hit against the frame's
                // first pushed arg (fw-12 = 0 observed) — the message
                // window's clickable quad is 0, so the synthesized hit
                // targets quad 0 while it is visible.
                if (press || release) && hit < 0 {
                    let preferred = 0_u32;
                    let indicator =
                        [preferred, 12_u32, 13_u32]
                            .into_iter()
                            .chain((0..32_u32).filter(|quad| {
                                Some(*quad) != stage_quad && *quad != 12 && *quad != 13
                            }))
                            .find(|quad| object.quad_visible(*quad))
                            .or_else(|| (0..32_u32).find(|quad| object.quad_visible(*quad)));
                    if let Some(quad) = indicator {
                        hit = quad as i32;
                        if press {
                            pressed = quad as i32;
                        }
                        if release {
                            consumed = true;
                            if pressed == quad as i32 {
                                confirmed = 1;
                                // One full press+release cycle targeted this
                                // indicator; give the other window mode the
                                // next press.
                                state.message_fallback_parity = !state.message_fallback_parity;
                            }
                        }
                        if tracing::enabled!(tracing::Level::INFO) {
                            tracing::info!(
                                event = "shinki.emu.cmvs.vm.advance_latch_consumed",
                                consumer = "message_quad_fallback",
                                pc = state.program_counter,
                                frame = state.current_frame,
                                quad
                            );
                        }
                    }
                }
            }
            // The published pending flag follows the native latch lifetime
            // (`sub_45CCD0`/`sub_45CCB0`): it stays set from the press edge
            // until the release edge processes the click, and the release
            // tick itself publishes pending=0 alongside confirmed=1. The
            // message engine's page-advance branch requires exactly that
            // combination (`intproc` pc 190016/190140).
            let pending = u32::from((press || pressed >= 0) && !release);
            // The native input manager maps the twelve key bytes at
            // `+0x240` to selection ids 12..=1 when a key's held-frame
            // counter (`+0x930+4n`) reads exactly one — the press edge —
            // and `cmd 846` (`sub_479C90` -> `sub_45CBB0`) publishes that
            // id; the value persists until `cmd 847` clears it. Mirror
            // the quad id the same way — press edge, persistent — into
            // the independent process-word table at 2200 (byte 1388 of
            // the interpreter space is a live interpreter dword, not a
            // scratch slot; the earlier mirror there broke the save
            // chain by clobbering it).
            // Native sets the selection only from the twelve key
            // bytes; story dialogue clicks never touch it. Mirror the
            // confirmed quad id — menu session only — into the dedicated
            // VM field (2200 in the process table is a live script word).
            if press && hit >= 0 && state.menu_session_live {
                state.input_selection_mirror = hit as u32;
            }
            if release && pressed >= 0 {
                consumed = true;
                pressed = -1;
            }
            if consumed {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        event = "shinki.emu.cmvs.vm.release_consumed",
                        pc = state.program_counter,
                        hit,
                        pressed
                    );
                }
                state.input_advance_release = false;
            }
            if hit >= 0 && tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.quad_hit",
                    pc = state.program_counter,
                    channel,
                    hit,
                    confirmed,
                    pending,
                    pressed,
                    press = u32::from(press),
                    release = u32::from(release)
                );
            }
            state
                .effect_channels
                .entry(channel)
                .or_default()
                .write_dword(632, pressed as u32);
            state
                .interpreter_words
                .insert(first_field_offset, hit as u32);
            state
                .interpreter_words
                .insert(second_field_offset, confirmed);
            state.interpreter_words.insert(third_field_offset, pending);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChannelPlaybackFlag {
                effect_table_dword_index,
                error_mask,
                activity_state_key_base,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, flag)],
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
            let state_key = activity_state_key_base + *channel;
            if state
                .interpreter_words
                .get(&state_key)
                .copied()
                .unwrap_or_default()
                != *flag
            {
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.playback_flag",
                        pc = state.program_counter,
                        frame = state.current_frame,
                        channel = *channel,
                        flag
                    );
                }
                state.interpreter_words.insert(state_key, *flag);
                // Native object +8 is distinct from the +4 scheduler flag.
                state.interpreter_words.insert(activity_state_key_base + 0x100 + *channel, 0);
                // Playback start arms the native channel object's +0xD0
                // (+0xD4 cleared); the stop path (`sub_48C0E0`) later
                // clears +0xD0 and sets +0xD4. Headless stands in with a
                // bounded lifecycle so the element waiters observe the
                // completion transition.
                if *flag != 0 {
                    state
                        .channel_completion_at
                        .entry(u8::try_from(*channel).unwrap_or(0))
                        .and_modify(|at| *at = state.tick.saturating_add(45))
                        .or_insert(state.tick.saturating_add(45));
                } else {
                    state.channel_completion_at.remove(
                        &u8::try_from(*channel).unwrap_or(0),
                    );
                }
                // The scheduler's live-channel gate reads the object's
                // element-residency. The element-create command publishes
                // it for the story channels; channel 7 — the menu UI
                // surface, whose elements arrive through playback flags —
                // mirrors the residency here so the record-17 poll arms
                // through the menu session without pulsing the story's
                // BGM-gated records.
                if *channel == 7 && state.menu_session_live {
                    state.interpreter_words.insert(
                        super::super::execute::effect_channel_activity_field(*channel),
                        u32::from(*flag != 0),
                    );
                }
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectChannelActivity {
                activity_state_key_base,
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
            // sub_47F270 reads object +8, not the scheduler's +4 flag.
            // Keep separate shadow words; timer 17 and script PC cannot
            // stand in for completion of a channel operation.
            let activity = state.interpreter_words
                .get(&(activity_state_key_base + 0x100 + *channel))
                .is_some_and(|value| *value != 0);
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(activity));
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
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    event = "shinki.emu.cmvs.vm.occupancy",
                    channel = *channel,
                    occupied,
                    pc = state.program_counter
                );
            }
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(occupied));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::DestroyEffectHitElement,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            // `sub_47E720`: an occupied channel drops its hit element
            // (`sub_446E70`); the native model tracks none beyond the
            // gate.
            if *channel > 7 || !effect_channel_is_occupied(state, *channel) {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.interpreter_words.insert(10532, flags | 0x0001_0000);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StorePlaybackRecordWord,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::OpaqueU32, value)],
        ) => {
            // `sub_489080`: the bounded 750-table record accepts the word;
            // the recovered playback record keeps no extra native state.
            if *slot >= 12 {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.interpreter_words.insert(10532, flags | 0x1000);
            } else {
                state.effect_playback.entry(*slot as u8).or_default();
                let _ = value;
            }
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::RunEffectTimeline, []) => {
            // `sub_4247D0` walks the timeline: type-1 steps are timed
            // delays and type-3 steps act through occupied channels
            // (`sub_4242A0` builds the element on the channel). The
            // headless pipeline finishes the whole list synchronously,
            // exactly like the native renderer would after its animation
            // completes, and the consumed timeline is released.
            let steps: Vec<_> = std::mem::take(&mut state.effect_timeline);
            for step in steps {
                if step.step_type == 3 && step.channel <= 7 {
                    if effect_channel_is_occupied(state, step.channel) {
                        state
                            .effect_elements
                            .entry((step.channel as u8, 0))
                            .or_default();
                    } else {
                        let flags =
                            state.interpreter_words.get(&10532).copied().unwrap_or(0);
                        state.interpreter_words.insert(10532, flags | 0x0001_0000);
                    }
                }
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ForwardEffectChannelQuad {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _), (CmvsPs2aCommandStackWordKind::OpaqueU32, _), (CmvsPs2aCommandStackWordKind::OpaqueU32, _), (CmvsPs2aCommandStackWordKind::OpaqueU32, _)],
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
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second)],
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
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _value)],
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
            Ok(Some(CmvsPs2aVmAction::ForwardEffectChannelBlock {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SetEffectChannelEnabled {
                effect_table_dword_index,
                max_channel,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::BooleanU32, enabled)],
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
            // `sub_47F160` -> `sub_467CE0` stores the boolean at the channel
            // record's flag dword 622 alongside the recovered enable action.
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
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index), (CmvsPs2aCommandStackWordKind::TaggedStringReference, _name), (CmvsPs2aCommandStackWordKind::OpaqueU32, _extra)],
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
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index), (CmvsPs2aCommandStackWordKind::TaggedStringReference, _name), (CmvsPs2aCommandStackWordKind::OpaqueU32, _extra)],
        ) => {
            // `sub_489160`: an occupied record at the bound index consumes
            // the name and the opaque word; anything else only raises the
            // playback error mask on the pinned interpreter flag word.
            if *index > max_index || !state.effect_playback.contains_key(&(*index as u8)) {
                let flags = state.interpreter_words.get(&error_flag_field_offset).copied().unwrap_or_default();
                state.interpreter_words.insert(error_flag_field_offset, flags | error_flag_mask);
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
                let flags = state.interpreter_words.get(&error_flag_field_offset).copied().unwrap_or_default();
                state.interpreter_words.insert(error_flag_field_offset, flags | error_flag_mask);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::CreatePlaybackChannelElement {
                max_channel,
                max_index,
                channel_error_mask,
                index_error_mask,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, index),
                ..
            ],
        ) => {
            // `sub_47EA90`: the channel word and the playback index carry
            // separate bounds; the element build lands on the playback
            // record the headless graph tracks by occupancy only. The
            // scheduler's group-1 activation reads the channel object's
            // live-element dword (`[obj+4]`, zeroed by `sub_466630`), so a
            // successful in-bounds build marks the channel occupied for the
            // scan and a rebuild or destroy clears it again.
            if *channel > max_channel {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or_default();
                state.interpreter_words.insert(10532, flags | channel_error_mask);
            }
            if *index > max_index {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or_default();
                state.interpreter_words.insert(10532, flags | index_error_mask);
            }
            if *channel <= max_channel
                && *index <= max_index
                && effect_channel_exists(state, *channel)
            {
                state
                    .effect_elements
                    .entry((*channel as u8, (*index) & 0xffff))
                    .or_default();
                state
                    .interpreter_words
                    .insert(effect_channel_activity_field(*channel), 1);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::CreateEffectChannel {
                effect_table_dword_index,
                playback_table_dword_index,
                max_channel,
                max_effect,
                channel_error_mask,
                effect_error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, effect), ..],
        ) => {
            if *channel > max_channel {
                raise_error_flag(state, 10532, channel_error_mask)?;
                return Ok(None);
            }
            if *effect > max_effect {
                raise_error_flag(state, 10532, effect_error_mask)?;
                return Ok(None);
            }
            let effect_key = (effect_table_dword_index << 8) | *channel;
            let playback_key = (playback_table_dword_index << 8) | *effect;
            let identity = state.next_slot_object_id;
            state.next_slot_object_id = identity.checked_add(1).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_SLOT_OBJECTS",
                    "CMVS slot object identity space overflowed",
                )
            })?;
            state.slot_objects.insert(effect_key, identity);
            state.slot_objects.insert(playback_key, identity);
            // The recovered channel object starts as a zero-initialized
            // 0x3088-byte image; later cases write its quad records. The
            // rebuild also frees the previous channel's elements
            // (`sub_47ED00` destroys the old object first), so the
            // scheduler's live-element gate drops with them.
            state.effect_channels.entry(*channel as u8).or_default();
            state.effect_playback.entry(*effect as u8).or_default();
            state
                .effect_elements
                .retain(|(element_channel, element_playback), _| {
                    *element_channel != *channel as u8
                        && *element_playback != (*effect) & 0xffff
                });
            Ok(Some(CmvsPs2aVmAction::CreateEffectChannel {
                channel: *channel,
                effect: *effect,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::PlayChannelSoundAndComplete {
                effect_table_dword_index: _,
                max_channel,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _mode), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name)],
        ) => {
            // `sub_47E910` plays the named sound through the occupied
            // channel and waits (`0xA000`) until `sub_466DD0` reports
            // completion, then ends the frame (`0xC004`). The headless
            // audio graph completes immediately; the contract pop drops
            // the channel word like the original.
            if *channel > max_channel || !effect_channel_is_occupied(state, *channel) {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.interpreter_words.insert(10532, flags | error_mask);
            }
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.sound_name_word",
                    pc = state.program_counter,
                    frame = state.current_frame,
                    channel = *channel,
                    word = *name,
                    resolved = resolve_string_value(state, *name).unwrap_or_default()
                );
            }
            Ok(Some(CmvsPs2aVmAction::PlayChannelSound {
                channel: *channel,
                name: resolve_string_value(state, *name)?,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::WaitEffectChannelComplete {
                effect_table_dword_index: _,
                max_channel,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), ..],
        ) => {
            // `sub_47F6E0` polls the effect wait. The headless effect
            // graph finishes instantly, so the words are consumed and
            // dispatch continues; dead channels raise the mask.
            if *channel > max_channel || !effect_channel_is_occupied(state, *channel) {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.interpreter_words.insert(10532, flags | error_mask);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryChannelChildName {
                channel_table_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, name)],
        ) => {
            // `sub_483D70` -> `sub_446C20`: builds the channel's query
            // object with the resolved name and reports whether the named
            // element answers. The recovered subset resolves the name for
            // observation and answers geometrically: any presenter rect of
            // the channel containing the virtual pointer (pointer * 3/2 in
            // the 1920x1080 space) counts as the hit.
            let name_bytes = resolve_query_name(state, *name);
            let px = (i64::from(state.pointer_x) * 3) / 2;
            let py = (i64::from(state.pointer_y) * 3) / 2;
            let mut hit_child: Option<i32> = None;
            for ((ch, child), presenter) in &state.channel_child_presenters {
                if *ch != *channel {
                    continue;
                }
                let Some([x, y, w, h]) = presenter.rect else {
                    continue;
                };
                let (pos_x, pos_y) = (
                    presenter.values[0] as i32,
                    presenter.values[1] as i32,
                );
                let left = i64::from(pos_x + x);
                let top = i64::from(pos_y + y);
                let right = left + i64::from(w.max(0));
                let bottom = top + i64::from(h.max(0));
                if px >= left && py >= top && px < right && py < bottom {
                    hit_child = Some(*child);
                    break;
                }
            }
            if tracing::enabled!(tracing::Level::INFO) {
                let presenters: Vec<String> = state
                    .channel_child_presenters
                    .iter()
                    .filter(|((ch, _), _)| *ch == *channel)
                    .map(|((ch, child), p)| {
                        format!(
                            "{}/{} r{:?} v{:?}",
                            ch, child, p.rect, p.values
                        )
                    })
                    .collect();
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.name_query",
                    pc = state.program_counter,
                    tick = state.tick,
                    table = channel_table_field_offset,
                    channel = *channel,
                    name = ?name_bytes,
                    pointer_x = state.pointer_x,
                    pointer_y = state.pointer_y,
                    hit_child = ?hit_child,
                    presenters = ?presenters
                );
            }
            state
                .interpreter_words
                .insert(81220, u32::from(hit_child.is_some()));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::InstallChannelChildPayload {
                selector,
                channel_table_field_offset,
            },
            words,
 )
        => {
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
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.child_payload_install",
                    selector,
                    table = channel_table_field_offset,
                    pc = state.program_counter,
                    channel,
                    child = child_signed,
                    words = ?rest
                );
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureChannelChildPresenter {
                selector,
                channel_table_field_offset,
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
                // `sub_482F20` -> `sub_446900`: clears the channel object;
                // the recovered subset drops the channel's presenters.
                64 => {
                    state
                        .channel_child_presenters
                        .retain(|(ch, _), _| *ch != channel);
                }
                // `sub_446B10` -> `sub_42E8E0`: clamped byte at +0x44.
                72 => {
                    presenter.byte44 = rest.first().copied().unwrap_or(0).min(0xff);
                }
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
                // `sub_4838D0` -> `sub_446B30` -> `sub_42E950`: the
                // presenter rect at +0x08..+0x14 from the four words
                // below the child/channel pair.
                68 => {
                    if rest.len() >= 4 {
                        // Native order (w3, w4, w5, w6): the last word of
                        // the push sequence is the rect's first field.
                        presenter.rect =
                            Some([rest[0] as i32, rest[1] as i32, rest[2] as i32, rest[3] as i32]);
                    }
                }
                // `sub_483A00` (69): the position pair; `sub_483600`
                // (70): the secondary pair — the ch60 dialog config
                // carries (252, 342)/(252, 446) through case 69.
                69 => {
                    presenter.values[0] = rest.first().copied().unwrap_or(0);
                    presenter.values[1] = rest.get(1).copied().unwrap_or(0);
                }
                70 => {
                    presenter.values[2] = rest.first().copied().unwrap_or(0);
                    presenter.values[3] = rest.get(1).copied().unwrap_or(0);
                }
                // `sub_483B10` -> `sub_446BE0`: single-value store.
                71 => {
                    presenter.aux_word = rest.first().copied().unwrap_or(0);
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
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.child_presenter_config",
                    selector,
                    table = channel_table_field_offset,
                    tick = state.tick,
                    pc = state.program_counter,
                    channel,
                    child = child_signed,
                    words = ?rest
                );
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

/// Resolves a query-name word through the recovered tag domains for
/// observation; failures yield an empty vector.
fn resolve_query_name(state: &CmvsPs2aVmState, reference: u32) -> Vec<u8> {
    match reference & 0xc000_0000 {
        0 => {
            let offset = (reference & 0x3fff_ffff) as usize;
            state
                .frame_string_pool_bytes
                .get(&state.current_frame)
                .and_then(|pool| {
                    if offset >= pool.len() {
                        return None;
                    }
                    let end = pool[offset..]
                        .iter()
                        .position(|&byte| byte == 0)
                        .map(|position| offset + position)
                        .unwrap_or(pool.len());
                    Some(pool[offset..end].to_vec())
                })
                .unwrap_or_default()
        }
        0x8000_0000 => {
            let slot = reference & 0x3fff_ffff;
            state
                .process_string_slots
                .get(&slot)
                .cloned()
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}
