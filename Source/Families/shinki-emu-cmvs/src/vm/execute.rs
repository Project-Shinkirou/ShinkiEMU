mod crossfade_audio;
mod effect_channels;
mod effect_elements;
mod effect_quads;
mod filters;
mod memory;
mod messages;
mod native_audio;
mod native_micro;
mod objects;
mod presentation;
mod resources;
mod scripts;
mod settings;
mod system;
mod texture_children;
mod textures;

#[cfg(test)]
mod basic_rotation_tests;

use super::*;

/// Whether a channel object exists in the effect slot table, as written by
/// the channel-create command; configuration commands gate on this.
pub(super) fn effect_channel_exists(state: &CmvsPs2aVmState, channel: u32) -> bool {
    channel <= 7 && state.slot_objects.contains_key(&(742_u32 << 8 | channel))
}

/// Outcome of the recovered 0xA000 wait gate for one command execution.
enum WaitGate {
    /// Not a wait command, or its latch already lets dispatch continue
    /// through the normal pop path.
    Pass,
    /// The command waits: keep the PC and stack operands, end the frame.
    Wait(Option<CmvsPs2aVmAction>),
    /// Complete this wait and pop normally. The gate sets dispatch_stopped
    /// only for native completion returns that also end the frame.
    Complete(Option<CmvsPs2aVmAction>),
}

/// Native presentation waits share Shift/Ctrl skip groups, force 1464 and
/// the confirmation release edge; holding Enter is not a skip mode.
fn wait_completed(state: &CmvsPs2aVmState) -> bool {
    state.input_shift_held
        || state.input_control_held
        || state
            .interpreter_words
            .get(&1464)
            .copied()
            .unwrap_or_default()
            != 0
        || state.input_advance_release
}

fn wait_gate(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(crate::CmvsPs2aCommandStackWordKind, u32)],
) -> Result<WaitGate, CoreError> {
    match effect_kind {
        CmvsPs2aCommandEffectKind::NativeAudio { command_id: 189 } => {
            let selector = values.first().map(|(_, value)| *value).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_NATIVE_AUDIO_CONTRACT",
                    "voice disposal poll has no selector",
                )
            })?;
            if selector > 3 {
                raise_error_flag(state, 10532, 0x400)?;
                return Ok(WaitGate::Wait(None));
            }
            Ok(if state.native_audio_present & (1 << (selector + 5)) != 0 {
                WaitGate::Wait(None)
            } else {
                WaitGate::Complete(None)
            })
        }
        CmvsPs2aCommandEffectKind::NativeIndependentView { command_id: 625 } => {
            state.write_interpreter_word(81220, 0);
            if !independent_view::pending(state) {
                return Ok(WaitGate::Complete(None));
            }
            let allow_skip = values.first().is_some_and(|(_, flag)| *flag != 0);
            let forced = state.interpreter_words.get(&1452).copied().unwrap_or(0) == 0
                || state.input_shift_held
                || state.input_control_held
                || state.interpreter_words.get(&1464).copied().unwrap_or(0) != 0;
            if allow_skip && (forced || state.input_advance_release) {
                if !forced {
                    state.input_advance_release = false;
                    state.input_advance_press = false;
                }
                independent_view::commit(state, true);
                state.write_interpreter_word(81220, 1);
                state.dispatch_stopped = true;
                return Ok(WaitGate::Complete(None));
            }
            Ok(WaitGate::Wait(None))
        }
        CmvsPs2aCommandEffectKind::WaitScreenCommit {
            result_field_offset,
            ..
        } => {
            state.write_interpreter_word(result_field_offset, 0);
            if !camera_transition::pending(state) {
                return Ok(WaitGate::Complete(None));
            }
            let allow_skip = values.first().is_some_and(|(_, flag)| *flag != 0);
            let forced = state.interpreter_words.get(&1452).copied().unwrap_or(0) == 0
                || state.input_shift_held
                || state.input_control_held
                || state.interpreter_words.get(&1464).copied().unwrap_or(0) != 0;
            if allow_skip && (forced || state.input_advance_release) {
                if !forced {
                    state.input_advance_release = false;
                    state.input_advance_press = false;
                }
                camera_transition::commit(state, true);
                state.write_interpreter_word(result_field_offset, 1);
                state.dispatch_stopped = true;
                return Ok(WaitGate::Complete(None));
            }
            Ok(WaitGate::Wait(None))
        }
        CmvsPs2aCommandEffectKind::WaitOrDestroyScreenShake {
            result_field_offset,
        } if state.screen_shake.is_some() && values.first().is_some_and(|(_, wait)| *wait != 0) => {
            state.write_interpreter_word(result_field_offset, 0);
            Ok(WaitGate::Wait(None))
        }
        CmvsPs2aCommandEffectKind::RunFilterGraphControl {
            table_byte_offset,
            slot,
            field_offset,
        } => {
            // `sub_47A6B0` clears the result word first on every execution,
            // then branches: a missing owner continues (0x4004); an owner
            // whose animation (`sub_458E50`: progress `[obj+0x10]` reached
            // `[obj+0x14]`) still runs publishes one into the result and
            // suspends the frame (0xC004); a finished animation lets
            // `sub_47A660` advance the chain state machine one frame per
            // call until it tears the owner down, and the next poll passes
            // with the owner destroyed. The headless chain plays
            // synchronously, so the animation is finished on the first
            // poll; the native one-frame-per-poll cadence is kept so the
            // surrounding per-frame build work (texture loads) runs at its
            // native tick rate instead of spinning every dispatch.
            state.write_interpreter_word(field_offset, 0);
            let handle_key = u32::from(table_byte_offset);
            let handle = state
                .interpreter_words
                .get(&handle_key)
                .copied()
                .unwrap_or(0);
            if handle == 0 {
                return Ok(WaitGate::Pass);
            }
            // The owner's seed words carry the animation duration (the
            // second contract word, in milliseconds — the logo chain builds
            // with 2000). Each poll advances the progress one frame the way
            // `sub_47A660` does; the teardown fires when the progress
            // reaches the duration, so the surrounding wait loop observes
            // the native per-frame cadence instead of an instant finish.
            const CHAIN_FRAME_MILLIS: u32 = 16;
            let seed = slot_object_key(table_byte_offset, u32::from(slot))
                .map(|key| {
                    state
                        .slot_object_seed_words
                        .get(&key)
                        .cloned()
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            // The seed vec holds the contract words top-down; the logo
            // chain build pushes [9, 2000, 0, flag] bottom-up, so the
            // 2000ms duration lands third from the top.
            let duration_ms = seed.get(2).copied().unwrap_or(0);
            let frames_needed = duration_ms / CHAIN_FRAME_MILLIS;
            let polls = state
                .interpreter_words
                .get(&(handle_key + 4096))
                .copied()
                .unwrap_or(0);
            if polls < frames_needed {
                state.write_interpreter_word(handle_key + 4096, polls + 1);
                state.write_interpreter_word(field_offset, 1);
                return Ok(WaitGate::Wait(None));
            }
            state.interpreter_words.remove(&(handle_key + 4096));
            destroy_filter_graph_owner(state, table_byte_offset, slot);
            state.write_interpreter_word(field_offset, 1);
            Ok(WaitGate::Pass)
        }
        CmvsPs2aCommandEffectKind::PresentSkippableWait {
            result_field_offset,
        } => {
            // `sub_47B4C0`: a zero mode waits one frame (0xA000); a
            // non-zero mode breaks out (sysreg0 = 1, context teardown,
            // 0xC004) when the skip configuration, a held confirm/cancel,
            // the force word at 1464, or an advance release appears.
            state.write_interpreter_word(result_field_offset, 0);
            let mode = values.first().map(|(_, value)| *value).unwrap_or(0);
            if mode == 0 {
                return Ok(WaitGate::Wait(None));
            }
            let skip_configuration = state.interpreter_words.get(&1452).copied().unwrap_or(0) == 0;
            if skip_configuration || wait_completed(state) {
                state.write_interpreter_word(result_field_offset, 1);
                state.dispatch_stopped = true;
                return Ok(WaitGate::Complete(None));
            }
            Ok(WaitGate::Wait(None))
        }
        CmvsPs2aCommandEffectKind::CommitTexturePresentation {
            present_object_field_offset,
            result_field_offset,
        } => {
            // `sub_47AF00`: the frame driver retires a naturally completed
            // owner. A non-zero mode also permits input to finish it early.
            state.write_interpreter_word(result_field_offset, 0);
            let exists = state
                .interpreter_words
                .get(&present_object_field_offset)
                .copied()
                .unwrap_or_default()
                != 0;
            if !exists {
                return Ok(WaitGate::Pass);
            }
            let mode = values.first().map(|(_, value)| *value).unwrap_or(0);
            if mode == 0 {
                return Ok(WaitGate::Wait(None));
            }
            let skip_configuration = state.interpreter_words.get(&1452).copied().unwrap_or(0) == 0;
            if skip_configuration || wait_completed(state) {
                if !skip_configuration
                    && !state.input_shift_held
                    && !state.input_control_held
                    && state.interpreter_words.get(&1464).copied().unwrap_or(0) == 0
                {
                    state.input_advance_release = false;
                    state.input_advance_press = false;
                }
                state.write_interpreter_word(result_field_offset, 1);
                state.dispatch_stopped = true;
                let action = crate::vm::texture_presentation::finish(state);
                return Ok(WaitGate::Complete(action));
            }
            Ok(WaitGate::Wait(None))
        }
        CmvsPs2aCommandEffectKind::DisplayChannelText => {
            use native_text::dialogue::Poll;
            match native_text::dialogue::poll(state, values)? {
                Poll::Layout {
                    channel,
                    slot,
                    source,
                } => Ok(WaitGate::Wait(Some(CmvsPs2aVmAction::NativeText(
                    CmvsNativeTextAction::DrawDialogue {
                        channel,
                        slot,
                        source,
                    },
                )))),
                Poll::Wait => Ok(WaitGate::Wait(None)),
                Poll::Complete => {
                    state.dispatch_stopped = true;
                    Ok(WaitGate::Complete(None))
                }
            }
        }
        CmvsPs2aCommandEffectKind::WaitChannelAdvance => {
            if native_text::advance_wait::confirm(state, values)? {
                Ok(WaitGate::Complete(None))
            } else {
                Ok(WaitGate::Wait(None))
            }
        }
        CmvsPs2aCommandEffectKind::WaitChannelAutoAdvance => {
            if native_text::advance_wait::automatic(state, values)? {
                Ok(WaitGate::Complete(None))
            } else {
                Ok(WaitGate::Wait(None))
            }
        }
        _ => Ok(WaitGate::Pass),
    }
}

pub(super) fn execute_command(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    stack_pop_bytes: u8,
    stack_words: &[crate::CmvsPs2aCommandStackWord],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    let action = execute_command_inner(state, effect_kind, stack_pop_bytes, stack_words)?;
    // `sub_46DAF0` updates byte 13224 after the handler returns. In
    // particular, script calls and coroutine resumes save the caller's
    // counter before a result without 0x2000 resets the live counter.
    if !state.wait_frame {
        state.write_interpreter_word(SCRIPT_FRAME_COUNTER_FIELD, 0);
    }
    Ok(action)
}

fn execute_command_inner(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    stack_pop_bytes: u8,
    stack_words: &[crate::CmvsPs2aCommandStackWord],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    // The pop width and the read set are independent in the original:
    // handlers such as `sub_483B10` inspect stack words without popping
    // them. Each read is bounds-checked individually below.
    if effect_kind == CmvsPs2aCommandEffectKind::StopDispatch {
        state.dispatch_stopped = true;
        return Ok(None);
    }
    let mut values = Vec::with_capacity(stack_words.len());
    for word in stack_words {
        let value = read_stack_word_from_top(state, word.offset_from_top_bytes)?;
        values.push((word.kind, value));
    }
    // The recovered 0xA000 wait class (`sub_47A6B0`, `sub_47AF00`): the
    // command polls its completion latch before consuming anything. While
    // it waits, the PC stays on the command word and the operands stay on
    // the stack so the next frame re-executes the same handler; a
    // completion falls through to the normal pop path after the handler
    // side effects.
    match wait_gate(state, effect_kind, &values)? {
        WaitGate::Wait(action) => {
            state.wait_frame = true;
            let delay = state
                .interpreter_words
                .get(&SCRIPT_FRAME_COUNTER_FIELD)
                .copied()
                .unwrap_or_default();
            state.write_interpreter_word(SCRIPT_FRAME_COUNTER_FIELD, delay.wrapping_add(1));
            // The native engine ends its frame at every wait: the entry
            // queue is rebuilt before the next continuation resumes.
            state.wait_boundary_hit = true;
            return Ok(action);
        }
        WaitGate::Complete(action) => {
            drop_stack_bytes(state, u16::from(stack_pop_bytes))?;
            return Ok(action);
        }
        WaitGate::Pass => {}
    }
    drop_stack_bytes(state, u16::from(stack_pop_bytes))?;
    match effect_kind {
        CmvsPs2aCommandEffectKind::NativeCustomGlyph { command_id } => {
            super::native_custom_glyph::execute(state, command_id, &values)
        }
        CmvsPs2aCommandEffectKind::NativeAudio { command_id } => {
            native_audio::execute(state, command_id, &values)
        }
        CmvsPs2aCommandEffectKind::NativeText { operation } => {
            super::native_text::execute(state, operation, &values)
        }
        CmvsPs2aCommandEffectKind::NativeMicroProgram {
            cluster,
            table_index,
        } => native_micro::execute(
            state,
            crate::command::native_table::micro_ops(cluster, table_index),
            &values,
        ),
        CmvsPs2aCommandEffectKind::NativeSceneWorker { command_id } => {
            let count = values.len();
            if count > 8 {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_NATIVE_SCENE_WORKER",
                    "invalid operand count",
                ));
            }
            let mut words = [0; 8];
            for (word, value) in words.iter_mut().zip(&values) {
                *word = value.1;
            }
            let (retired_static_sounds, stopped_streams) =
                native_scene_worker::retired_audio(state, words[0]);
            if command_id == 659 {
                if count != 1 {
                    return Err(invalid(
                        "SHINKI_EMU_CMVS_NATIVE_SCENE_WORKER",
                        "invalid controller operand count",
                    ));
                }
                if words[0] > 3 {
                    return Ok(None);
                }
                native_scene_worker::remove_slot(state, words[0]);
                return Ok(Some(CmvsPs2aVmAction::CreateNativeScene53 {
                    slot: words[0] as u8,
                    retired_static_sounds,
                    stopped_streams,
                }));
            }
            let created = if matches!(command_id, 657 | 658 | 660 | 661 | 662 | 663) {
                if count != 1 {
                    return Err(invalid(
                        "SHINKI_EMU_CMVS_NATIVE_SCENE_WORKER",
                        "invalid controller operand count",
                    ));
                }
                native_scene_worker::start_controller(state, command_id, words[0])?
            } else {
                let resource = text::resolve_tagged_string_bytes(state, words[1])?.into_owned();
                if command_id == 656 {
                    if count != 3 {
                        return Err(invalid(
                            "SHINKI_EMU_CMVS_NATIVE_SCENE_WORKER",
                            "invalid scene50 operand count",
                        ));
                    }
                    let second = text::resolve_tagged_string_bytes(state, words[2])?.into_owned();
                    native_scene_worker::start_scene50(state, words[0], [resource, second])?
                } else {
                    native_scene_worker::start(state, command_id, &words[..count], resource)?
                }
            };
            Ok(
                created.then_some(CmvsPs2aVmAction::NativeSceneWorkerResourceChanged {
                    slot: words[0] as u8,
                    retired_static_sounds,
                    stopped_streams,
                }),
            )
        }
        CmvsPs2aCommandEffectKind::NativeHitPolygon { command_id } => {
            let count = values.len();
            if count > 4 {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_NATIVE_COLLISION",
                    "invalid collision operand count",
                ));
            }
            let mut words = [0; 4];
            for (word, value) in words.iter_mut().zip(&values) {
                *word = value.1;
            }
            native_hit_polygon::execute(state, command_id, &words[..count])?;
            Ok(None)
        }
        CmvsPs2aCommandEffectKind::StoreComponentConstant { .. }
        | CmvsPs2aCommandEffectKind::PublishInputSelection { .. }
        | CmvsPs2aCommandEffectKind::StoreInterpreterConstant { .. }
        | CmvsPs2aCommandEffectKind::ClearInterpreterTableWord { .. }
        | CmvsPs2aCommandEffectKind::LoadInterpreterTableWord { .. }
        | CmvsPs2aCommandEffectKind::MarkInterpreterRecordUnused { .. }
        | CmvsPs2aCommandEffectKind::InterpreterFlagRoundTrip { .. }
        | CmvsPs2aCommandEffectKind::StorePrefixedInterpreterString { .. }
        | CmvsPs2aCommandEffectKind::RequestWindowCaption
        | CmvsPs2aCommandEffectKind::RequestWindowIcon { .. }
        | CmvsPs2aCommandEffectKind::StoreComponentWord { .. }
        | CmvsPs2aCommandEffectKind::StoreComponentBoolean { .. }
        | CmvsPs2aCommandEffectKind::QueryComponentWordBooleanToResult { .. }
        | CmvsPs2aCommandEffectKind::QueryComponentWordToResult { .. }
        | CmvsPs2aCommandEffectKind::StoreBoundedInterpreterString { .. }
        | CmvsPs2aCommandEffectKind::BuildPrefixedWindowCaption { .. }
        | CmvsPs2aCommandEffectKind::NoOpCommand
        | CmvsPs2aCommandEffectKind::LoadInterpreterWordToResult { .. }
        | CmvsPs2aCommandEffectKind::LoadInterpreterWordBooleanToResult { .. }
        | CmvsPs2aCommandEffectKind::AppendPrivateStringToList { .. }
        | CmvsPs2aCommandEffectKind::CopyPrivateListStringToSlot { .. }
        | CmvsPs2aCommandEffectKind::StoreListCountToResult { .. }
        | CmvsPs2aCommandEffectKind::StoreListWalkLengthToResult { .. }
        | CmvsPs2aCommandEffectKind::StoreInterpreterWord { .. }
        | CmvsPs2aCommandEffectKind::StoreInterpreterWordPair { .. }
        | CmvsPs2aCommandEffectKind::StoreRandomModulo { .. }
        | CmvsPs2aCommandEffectKind::StoreStringLength { .. }
        | CmvsPs2aCommandEffectKind::CopyTaggedStringLengthToResult { .. }
        | CmvsPs2aCommandEffectKind::StoreInterpreterTimestampedRecord { .. }
        | CmvsPs2aCommandEffectKind::StoreProcessGlobalWord { .. }
        | CmvsPs2aCommandEffectKind::StoreProcessGlobalBoolean { .. }
        | CmvsPs2aCommandEffectKind::MutateProcessFlagRange
        | CmvsPs2aCommandEffectKind::StoreProcessIndexedRange
        | CmvsPs2aCommandEffectKind::StoreProcessFloatRange
        | CmvsPs2aCommandEffectKind::ProjectPointToScreenWords { .. }
        | CmvsPs2aCommandEffectKind::QueryProcessSelection { .. }
        | CmvsPs2aCommandEffectKind::StoreProcessStringRange => {
            memory::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::ConfigureResourceSearch { .. }
        | CmvsPs2aCommandEffectKind::LoadChannelImage { .. }
        | CmvsPs2aCommandEffectKind::RegisterResourceChannelSlot { .. }
        | CmvsPs2aCommandEffectKind::ClearResourceSlot { .. }
        | CmvsPs2aCommandEffectKind::SetResourceSlotRegion { .. }
        | CmvsPs2aCommandEffectKind::AppendNativeMessageHistory
        | CmvsPs2aCommandEffectKind::ClearNativeMessageHistory => {
            resources::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::CreateSlotObject { .. }
        | CmvsPs2aCommandEffectKind::CreateSingletonSlotObject { .. }
        | CmvsPs2aCommandEffectKind::DestroySingletonSlotObject { .. }
        | CmvsPs2aCommandEffectKind::DestroySlotObject { .. }
        | CmvsPs2aCommandEffectKind::MoveSlotObject { .. }
        | CmvsPs2aCommandEffectKind::ReleaseSingleObject { .. } => {
            objects::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::CmvController { command_id } => {
            super::cmv::execute(state, command_id, &values)
        }
        CmvsPs2aCommandEffectKind::NativeIndependentView { command_id } => {
            let mut arguments = [0u32; 7];
            for (argument, (_, value)) in arguments.iter_mut().zip(&values) {
                *argument = *value;
            }
            independent_view::execute(state, command_id, &arguments[..values.len()])?;
            Ok(None)
        }
        CmvsPs2aCommandEffectKind::RunFilterGraphControl { .. }
        | CmvsPs2aCommandEffectKind::ReplaceFilterChainSlot { .. }
        | CmvsPs2aCommandEffectKind::DestroyFilterChainSlot { .. }
        | CmvsPs2aCommandEffectKind::QueryFilterChainSlot { .. }
        | CmvsPs2aCommandEffectKind::UpdateFilterChainRecord { .. }
        | CmvsPs2aCommandEffectKind::UpdateFilterChainParameterBlock { .. }
        | CmvsPs2aCommandEffectKind::ApplyFilterChainRecords { .. }
        | CmvsPs2aCommandEffectKind::CreateChainHotspotRecord { .. }
        | CmvsPs2aCommandEffectKind::ClickCommitFilterChain { .. }
        | CmvsPs2aCommandEffectKind::PollFilterChainSelection { .. }
        | CmvsPs2aCommandEffectKind::ReadActiveFilterChainSelection { .. } => {
            filters::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::ForwardEffectChannelWord { .. }
        | CmvsPs2aCommandEffectKind::SetEffectChannelOrigin { .. }
        | CmvsPs2aCommandEffectKind::ConfigureEffectChannelText { .. }
        | CmvsPs2aCommandEffectKind::SetEffectPlaybackField { .. }
        | CmvsPs2aCommandEffectKind::SetEffectChannelPlaybackMode { .. }
        | CmvsPs2aCommandEffectKind::SetEffectChannelValuePair { .. }
        | CmvsPs2aCommandEffectKind::ConfigureEffectChannelFromName { .. }
        | CmvsPs2aCommandEffectKind::ApplyEffectChannelOperation { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectState { .. }
        | CmvsPs2aCommandEffectKind::SetEffectChannelPlaybackFlag { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectChannelActivity { .. }
        | CmvsPs2aCommandEffectKind::QueryChannelOccupancy { .. }
        | CmvsPs2aCommandEffectKind::ForwardEffectChannelQuad { .. }
        | CmvsPs2aCommandEffectKind::ForwardEffectChannelPair { .. }
        | CmvsPs2aCommandEffectKind::ForwardEffectChannelBlock { .. }
        | CmvsPs2aCommandEffectKind::SetEffectChannelEnabled { .. }
        | CmvsPs2aCommandEffectKind::CreateEffectChannel { .. }
        | CmvsPs2aCommandEffectKind::CreateEffectPlayback { .. }
        | CmvsPs2aCommandEffectKind::CreateStageEffectChannel { .. }
        | CmvsPs2aCommandEffectKind::ForwardEffectPlaybackSetup { .. }
        | CmvsPs2aCommandEffectKind::ConfigureEffectPlaybackSlots { .. }
        | CmvsPs2aCommandEffectKind::PrepareEffectChannelFromName { .. }
        | CmvsPs2aCommandEffectKind::StorePlaybackRecordWord
        | CmvsPs2aCommandEffectKind::DestroyEffectHitElement
        | CmvsPs2aCommandEffectKind::InstallChannelChildPayload { .. }
        | CmvsPs2aCommandEffectKind::RunEffectTimeline
        | CmvsPs2aCommandEffectKind::ConfigureChannelChildPresenter { .. } => {
            effect_channels::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::SetNativeSaveEnabled => {
            let [(CmvsPs2aCommandStackWordKind::BooleanU32, enabled)] = values.as_slice() else {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_SAVE_PERMISSION",
                    "invalid native save permission operand",
                ));
            };
            let enabled = u32::from(*enabled != 0);
            state.write_interpreter_word(1596, enabled);
            state.write_interpreter_word(15064, enabled);
            Ok(None)
        }
        CmvsPs2aCommandEffectKind::ScanNativeSaveSlots => {
            let [(CmvsPs2aCommandStackWordKind::OpaqueU32, value)] = values.as_slice() else {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_SAVE_CATALOG",
                    "invalid native slot count operand",
                ));
            };
            let signed = *value as i32;
            let count = if signed < 0 {
                10
            } else {
                signed.min(256) as u16
            };
            Ok(Some(CmvsPs2aVmAction::ScanNativeSaveSlots { count }))
        }
        CmvsPs2aCommandEffectKind::StorePresentationFrameFields { .. }
        | CmvsPs2aCommandEffectKind::SetNativeViewSuppression { .. }
        | CmvsPs2aCommandEffectKind::ConfigureNativeAlternateView { .. }
        | CmvsPs2aCommandEffectKind::ConfigureNativeViewPolicy { .. }
        | CmvsPs2aCommandEffectKind::CommitScreenParams { .. }
        | CmvsPs2aCommandEffectKind::QueryScreenPending { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenRgb { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenRotation { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenScale { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenOffset { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenField { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenScalePair { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenFlag { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenPair { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenOffsetDirect { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenScaleDirect { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenRgbDirect { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenRotationDirect { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenFieldDirect { .. }
        | CmvsPs2aCommandEffectKind::ConfigureScreenScalePairDirect { .. }
        | CmvsPs2aCommandEffectKind::QueryTextureParentImageInfo { .. }
        | CmvsPs2aCommandEffectKind::QueryChannelChildState { .. }
        | CmvsPs2aCommandEffectKind::LoadTextureParentFromName { .. }
        | CmvsPs2aCommandEffectKind::StartCameraTransition { .. }
        | CmvsPs2aCommandEffectKind::QueryChannelTransitionBusy { .. }
        | CmvsPs2aCommandEffectKind::ReleaseChannelTransition
        | CmvsPs2aCommandEffectKind::ReloadTextureParentImage { .. }
        | CmvsPs2aCommandEffectKind::ReleaseTextureChildByIndex
        | CmvsPs2aCommandEffectKind::ConfigureTextureChildGeometry
        | CmvsPs2aCommandEffectKind::StoreTextureChildWord
        | CmvsPs2aCommandEffectKind::KickTextureChildDraw
        | CmvsPs2aCommandEffectKind::SetSystemVolume
        | CmvsPs2aCommandEffectKind::QuerySceneLayer { .. }
        | CmvsPs2aCommandEffectKind::ClearSceneLayer { .. }
        | CmvsPs2aCommandEffectKind::LayoutMessageText { .. }
        | CmvsPs2aCommandEffectKind::TogglePresentationMode { .. }
        | CmvsPs2aCommandEffectKind::ResetPresentationBuffers
        | CmvsPs2aCommandEffectKind::StoreTextureChildByIndex
        | CmvsPs2aCommandEffectKind::DestroyTextureChild { .. } => {
            presentation::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::SelectEffectChild { .. }
        | CmvsPs2aCommandEffectKind::SetEffectTextSurface { .. }
        | CmvsPs2aCommandEffectKind::SetEffectElementVisible { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectElementExists { .. }
        | CmvsPs2aCommandEffectKind::SetEffectElementSize { .. }
        | CmvsPs2aCommandEffectKind::SetEffectElementSource { .. }
        | CmvsPs2aCommandEffectKind::SetEffectElementPosition { .. }
        | CmvsPs2aCommandEffectKind::SetEffectElementAuxiliary { .. }
        | CmvsPs2aCommandEffectKind::SetEffectElementAuxiliaryPair { .. }
        | CmvsPs2aCommandEffectKind::ApplyEffectElementOperation { .. }
        | CmvsPs2aCommandEffectKind::ConfigureEffectElementBasic { .. }
        | CmvsPs2aCommandEffectKind::SetEffectChildEnabled { .. } => {
            effect_elements::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::ConfigureEffectQuadGeometry { .. }
        | CmvsPs2aCommandEffectKind::ConfigureEffectQuadRect { .. }
        | CmvsPs2aCommandEffectKind::SelectEffectQuad { .. }
        | CmvsPs2aCommandEffectKind::DeselectEffectQuad { .. }
        | CmvsPs2aCommandEffectKind::ActivateEffectQuad { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectQuadState { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectQuadActive { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectPosition { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectPointerHit { .. } => {
            effect_quads::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::ConsumeAdvanceLatch
        | CmvsPs2aCommandEffectKind::ReadInputLatches { .. }
        | CmvsPs2aCommandEffectKind::ReadAutomaticAdvance { .. }
        | CmvsPs2aCommandEffectKind::StoreTextureReadyFlag { .. }
        | CmvsPs2aCommandEffectKind::ClearTextureChannelTable { .. }
        | CmvsPs2aCommandEffectKind::LoadTextureParentResource { .. }
        | CmvsPs2aCommandEffectKind::StartTexturePresentation { .. }
        | CmvsPs2aCommandEffectKind::QueryTexturePresentation
        | CmvsPs2aCommandEffectKind::FinishTexturePresentation
        | CmvsPs2aCommandEffectKind::CommitTexturePresentation { .. }
        | CmvsPs2aCommandEffectKind::SetTextureParentSurface { .. } => {
            textures::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::QueryNewestSaveSlot { .. }
        | CmvsPs2aCommandEffectKind::SetNativeQueuePolicy
        | CmvsPs2aCommandEffectKind::QueryNativeSceneWorkerStatus
        | CmvsPs2aCommandEffectKind::RequestNativeSceneWorker
        | CmvsPs2aCommandEffectKind::SelectSystemCursor { .. }
        | CmvsPs2aCommandEffectKind::RequestSystemStateLoad { .. }
        | CmvsPs2aCommandEffectKind::CreateScreenShake { .. }
        | CmvsPs2aCommandEffectKind::QueryScreenShakeRemaining { .. }
        | CmvsPs2aCommandEffectKind::SetScreenShakePaused
        | CmvsPs2aCommandEffectKind::WaitOrDestroyScreenShake { .. }
        | CmvsPs2aCommandEffectKind::DestroyScreenShake
        | CmvsPs2aCommandEffectKind::OpenMovieStream { .. }
        | CmvsPs2aCommandEffectKind::ReleaseMovieObject
        | CmvsPs2aCommandEffectKind::QueryMovieState { .. }
        | CmvsPs2aCommandEffectKind::CaptureSaveSnapshot
        | CmvsPs2aCommandEffectKind::LoadNativeSaveSlot
        | CmvsPs2aCommandEffectKind::SaveNativeGameSlot
        | CmvsPs2aCommandEffectKind::ProbeNativeSaveSlot { .. }
        | CmvsPs2aCommandEffectKind::SetNativeSaveTitle { .. }
        | CmvsPs2aCommandEffectKind::LoadNativeSaveInfo
        | CmvsPs2aCommandEffectKind::SetNativeSaveMeta { .. }
        | CmvsPs2aCommandEffectKind::QueryScreenShakePaused { .. }
        | CmvsPs2aCommandEffectKind::QueryRendererDisplayMode { .. }
        | CmvsPs2aCommandEffectKind::StorePressLatch { .. }
        | CmvsPs2aCommandEffectKind::PushEffectTimelineCamera
        | CmvsPs2aCommandEffectKind::PushEffectTimelineElement
        | CmvsPs2aCommandEffectKind::PushEffectTimelineTransform { .. }
        | CmvsPs2aCommandEffectKind::ReleaseGraphicsContext { .. }
        | CmvsPs2aCommandEffectKind::QueryGraphicsContext { .. }
        | CmvsPs2aCommandEffectKind::RebuildGraphicsContext { .. }
        | CmvsPs2aCommandEffectKind::DestroySceneLayerWorker737
        | CmvsPs2aCommandEffectKind::RebuildSceneLayerChild
        | CmvsPs2aCommandEffectKind::ClearPresentBlock
        | CmvsPs2aCommandEffectKind::QueryPresentSlotHit { .. }
        | CmvsPs2aCommandEffectKind::ReadPresentCompleteFlag { .. }
        | CmvsPs2aCommandEffectKind::SetPresentAllFlag { .. }
        | CmvsPs2aCommandEffectKind::PushEffectTimelineChannel
        | CmvsPs2aCommandEffectKind::PushEffectTimelineDelay
        | CmvsPs2aCommandEffectKind::ReadEffectTimelineCount { .. }
        | CmvsPs2aCommandEffectKind::StoreWindowStyleSetting { .. }
        | CmvsPs2aCommandEffectKind::ReadEffectTimelineCursor { .. }
        | CmvsPs2aCommandEffectKind::ResetEffectTimeline
        | CmvsPs2aCommandEffectKind::StartEffectTimeline
        | CmvsPs2aCommandEffectKind::StopEffectTimeline
        | CmvsPs2aCommandEffectKind::BuildSaveSnapshot
        | CmvsPs2aCommandEffectKind::ReadCancelLatches { .. }
        | CmvsPs2aCommandEffectKind::ReadAuxLatches { .. }
        | CmvsPs2aCommandEffectKind::QueryPointerPosition { .. }
        | CmvsPs2aCommandEffectKind::HitTestPointerRect { .. }
        | CmvsPs2aCommandEffectKind::HitTestPointerRegion { .. } => {
            system::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::StoreSystemSettingBoolean { .. }
        | CmvsPs2aCommandEffectKind::CopySettingsFieldToInterpreterWord { .. }
        | CmvsPs2aCommandEffectKind::StoreIndexedSettingBoolean { .. }
        | CmvsPs2aCommandEffectKind::StoreDerivedSettingsPair { .. }
        | CmvsPs2aCommandEffectKind::StoreSettingsWordReset { .. }
        | CmvsPs2aCommandEffectKind::StoreSettingsPredicate { .. }
        | CmvsPs2aCommandEffectKind::DeriveSettingsSnapshot { .. }
        | CmvsPs2aCommandEffectKind::StoreSettingsRecord { .. }
        | CmvsPs2aCommandEffectKind::StoreSystemSettingBooleanPair { .. }
        | CmvsPs2aCommandEffectKind::StoreSystemSettingBoundedWord { .. }
        | CmvsPs2aCommandEffectKind::StoreSystemSettingNonNegativeWord { .. }
        | CmvsPs2aCommandEffectKind::NativeAudioGainSetting { .. }
        | CmvsPs2aCommandEffectKind::StoreSystemConfigBoolean { .. } => {
            settings::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::CallScript
        | CmvsPs2aCommandEffectKind::ReloadRootScript
        | CmvsPs2aCommandEffectKind::ResetEngineToMain
        | CmvsPs2aCommandEffectKind::SaveSystemState
        | CmvsPs2aCommandEffectKind::DestroyEffectPlayback { .. }
        | CmvsPs2aCommandEffectKind::SetNativeFullscreenSuspend
        | CmvsPs2aCommandEffectKind::ResumeInterpreterCoroutineRecord { .. }
        | CmvsPs2aCommandEffectKind::QueryScriptSlotOccupancy { .. } => {
            scripts::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::ConfigureTextureChildAudio { .. }
        | CmvsPs2aCommandEffectKind::DrawTextureChild { .. }
        | CmvsPs2aCommandEffectKind::QueryTextureChildState { .. }
        | CmvsPs2aCommandEffectKind::ReleaseTextureChildState { .. }
        | CmvsPs2aCommandEffectKind::ResetTextureChild { .. }
        | CmvsPs2aCommandEffectKind::SetTextureVisibility
        | CmvsPs2aCommandEffectKind::LoadTextureChildResource { .. }
        | CmvsPs2aCommandEffectKind::InitializeTextureChildSurface { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureChildRect { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureChildPosition { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureAuxiliaryPair { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureAuxiliaryWord { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureSolidSurface { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureScale
        | CmvsPs2aCommandEffectKind::ConfigureTextureBasicKind { .. }
        | CmvsPs2aCommandEffectKind::DetachTextureChildContext
        | CmvsPs2aCommandEffectKind::ConfigureTextureBlend
        | CmvsPs2aCommandEffectKind::DisableTextureShake
        | CmvsPs2aCommandEffectKind::ConfigureTextureFloatPosition
        | CmvsPs2aCommandEffectKind::ConfigureTextureRotation
        | CmvsPs2aCommandEffectKind::ConfigureTextureProjectionField { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureAlpha { .. } => {
            texture_children::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::FadeOutAudio
        | CmvsPs2aCommandEffectKind::StopAudio
        | CmvsPs2aCommandEffectKind::PlayAudio
        | CmvsPs2aCommandEffectKind::PlayPairedAudio => {
            crossfade_audio::execute(effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::FormatStringIntoSlot => {
            messages::execute(state, effect_kind, &values)
        }
        _ => Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
            format!(
                "CMVS PS2A command stack words do not match the recovered effect contract at pc {} in frame {}: {:?}",
                state.program_counter, state.current_frame, effect_kind
            ),
        )),
    }
}
