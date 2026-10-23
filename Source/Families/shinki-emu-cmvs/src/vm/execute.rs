mod crossfade_audio;
mod effect_channels;
mod effect_elements;
mod effect_quads;
mod filters;
mod memory;
mod messages;
mod native_micro;
mod objects;
mod presentation;
mod resources;
mod scripts;
mod settings;
mod system;
mod texture_children;
mod textures;

use super::*;

/// The native occupancy array is `this[channel + 742]` in dword units; our
/// interpreter words are keyed by byte offsets, so channel `ch` lives at
/// `(742 + ch) * 4`.
pub(super) const EFFECT_CHANNEL_ACTIVITY_FIELD_BASE: u32 = 742 * 4;

pub(super) fn effect_channel_activity_field(channel: u32) -> u32 {
    EFFECT_CHANNEL_ACTIVITY_FIELD_BASE + 4 * channel
}

/// Whether an effect channel index below 8 holds a live activity entry, the
/// recovered guard every channel-scoped handler applies through
/// `this[channel + 742]`.
pub(super) fn effect_channel_is_occupied(state: &CmvsPs2aVmState, channel: u32) -> bool {
    channel <= 7
        && state
            .interpreter_words
            .get(&effect_channel_activity_field(channel))
            .is_some_and(|v| *v != 0)
}

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
    Wait,
    /// The wait completed this execution: apply the completion side
    /// effects (done inside the gate), then pop and advance normally and
    /// end the frame (`0xC004`).
    Complete,
}

/// The completion latch shared by the case-548 and case-555 handlers
/// (`sub_47A6B0`, `sub_47AF00`): the skip configuration, a held confirm or
/// cancel button, the force word at 1464, or a release edge on the advance
/// group completes the wait.
fn wait_completed(state: &CmvsPs2aVmState) -> bool {
    state.input_confirm_held
        || state.input_cancel_held
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
            state.interpreter_words.insert(field_offset, 0);
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
                state.interpreter_words.insert(handle_key + 4096, polls + 1);
                state.interpreter_words.insert(field_offset, 1);
                return Ok(WaitGate::Wait);
            }
            state.interpreter_words.remove(&(handle_key + 4096));
            destroy_filter_graph_owner(state, table_byte_offset, slot);
            state.interpreter_words.insert(field_offset, 1);
            Ok(WaitGate::Pass)
        }
        CmvsPs2aCommandEffectKind::PresentSkippableWait {
            result_field_offset,
        } => {
            // `sub_47B4C0`: a zero mode waits one frame (0xA000); a
            // non-zero mode breaks out (sysreg0 = 1, context teardown,
            // 0xC004) when the skip configuration, a held confirm/cancel,
            // the force word at 1464, or an advance release appears.
            state.interpreter_words.insert(result_field_offset, 0);
            let mode = values.first().map(|(_, value)| *value).unwrap_or(0);
            if mode == 0 {
                return Ok(WaitGate::Wait);
            }
            let skip_configuration = state.interpreter_words.get(&1452).copied().unwrap_or(0) == 0;
            if skip_configuration || wait_completed(state) {
                state.interpreter_words.insert(result_field_offset, 1);
                state.dispatch_stopped = true;
                return Ok(WaitGate::Complete);
            }
            Ok(WaitGate::Wait)
        }
        CmvsPs2aCommandEffectKind::CommitTexturePresentation {
            present_object_field_offset,
            result_field_offset,
        } => {
            // `sub_47AF00`: a missing presentation object continues with a
            // zero result (0x4004). A non-zero mode completes through the
            // same latch and commits the presented texture (0xC004); a
            // zero mode waits until some other path destroys the object.
            state.interpreter_words.insert(result_field_offset, 0);
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
                return Ok(WaitGate::Wait);
            }
            let skip_configuration = state.interpreter_words.get(&1452).copied().unwrap_or(0) == 0;
            if skip_configuration || wait_completed(state) {
                state.interpreter_words.insert(result_field_offset, 1);
                state.dispatch_stopped = true;
                return Ok(WaitGate::Complete);
            }
            Ok(WaitGate::Wait)
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
    // The pop width and the read set are independent in the original:
    // handlers such as `sub_483B10` inspect stack words without popping
    // them. Each read is bounds-checked individually below.
    if effect_kind == CmvsPs2aCommandEffectKind::StopDispatch {
        state.dispatch_stopped = true;
        return Ok(None);
    }
    let tagged_before = if tracing::enabled!(tracing::Level::INFO)
        && state.current_frame == 0
        && (47_000..=48_300).contains(&state.program_counter)
    {
        Some(count_tagged_stack_words(state))
    } else {
        None
    };
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
        WaitGate::Wait => {
            state.wait_frame = true;
            let delay = state
                .interpreter_words
                .get(&52896)
                .copied()
                .unwrap_or_default();
            state.interpreter_words.insert(52896, delay.wrapping_add(1));
            // The native engine ends its frame at every wait: the entry
            // queue is rebuilt before the next continuation resumes.
            state.wait_boundary_hit = true;
            return Ok(None);
        }
        WaitGate::Complete => {
            drop_stack_bytes(state, u16::from(stack_pop_bytes))?;
            return Ok(None);
        }
        WaitGate::Pass => {}
    }
    if effect_trace_enabled() && is_effect_trace_target(effect_kind) {
        tracing::trace!(
            event = "shinki.emu.cmvs.vm.effect",
            pc = state.program_counter,
            frame = state.current_frame
        );
    }
    // The native dispatcher resets the consecutive-command counter
    // (`this[13224]`, byte 52896) unless the command's result carries the
    // 0x2000 flag; only the wait family keeps it counting.
    state.interpreter_words.insert(52896, 0);
    drop_stack_bytes(state, u16::from(stack_pop_bytes))?;
    if tracing::enabled!(tracing::Level::INFO) && state.current_frame == 2 {
        tracing::info!(
            event = "shinki.emu.cmvs.vm.intcode_cmd",
            pc = state.program_counter,
            command = effect_kind_variant_name(&effect_kind),
            tick = state.tick
        );
    }
    if tracing::enabled!(tracing::Level::INFO)
        && ((190_800..=191_400).contains(&state.program_counter)
            || (360_400..=361_100).contains(&state.program_counter)
            || (194_900..=196_150).contains(&state.program_counter)
            || (278_700..=280_000).contains(&state.program_counter))
    {
        tracing::info!(
            event = "shinki.emu.cmvs.vm.menu_save_zone",
            pc = state.program_counter,
            frame = state.current_frame,
            command = effect_kind_variant_name(&effect_kind),
            cursor = state.stack_cursor_bytes,
            tick = state.tick
        );
    }
    if let Some(before) = tagged_before {
        let after = count_tagged_stack_words(state);
        if after != before {
            tracing::info!(
                event = "shinki.emu.cmvs.vm.tagged_delta",
                pc = state.program_counter,
                command = effect_kind_variant_name(&effect_kind),
                before,
                after,
                cursor = state.stack_cursor_bytes
            );
        }
    }
    match effect_kind {
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
        | CmvsPs2aCommandEffectKind::SetWindowCaptionFromTagged { .. }
        | CmvsPs2aCommandEffectKind::StoreComponentWord { .. }
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
        | CmvsPs2aCommandEffectKind::RegisterAudioStreamNameSlot
        | CmvsPs2aCommandEffectKind::PlayNamedAudioStream
        | CmvsPs2aCommandEffectKind::StoreProcessStringRange => {
            memory::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::ConfigureResourceSearch { .. }
        | CmvsPs2aCommandEffectKind::PlayChannelSound { .. }
        | CmvsPs2aCommandEffectKind::RegisterResourceChannelSlot { .. }
        | CmvsPs2aCommandEffectKind::ClearResourceSlot { .. }
        | CmvsPs2aCommandEffectKind::SetResourceSlotRegion { .. }
        | CmvsPs2aCommandEffectKind::RunResourceSlotMethod { .. }
        | CmvsPs2aCommandEffectKind::ConfigureMoviePlayback => {
            resources::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::CreateSlotObject { .. }
        | CmvsPs2aCommandEffectKind::CreateSingletonSlotObject { .. }
        | CmvsPs2aCommandEffectKind::DestroySingletonSlotObject { .. }
        | CmvsPs2aCommandEffectKind::DestroySlotObject { .. }
        | CmvsPs2aCommandEffectKind::MoveSlotObject { .. }
        | CmvsPs2aCommandEffectKind::ReleaseSingleObject { .. }
        | CmvsPs2aCommandEffectKind::CreateBoundedSlotObject { .. }
        | CmvsPs2aCommandEffectKind::DestroyBoundedSlotObject { .. } => {
            objects::execute(state, effect_kind, &values)
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
        CmvsPs2aCommandEffectKind::StoreChannelVisibilityRecord { .. }
        | CmvsPs2aCommandEffectKind::BindMessageChannelOrRaise { .. }
        | CmvsPs2aCommandEffectKind::ResetChannelVisibilityRecord { .. }
        | CmvsPs2aCommandEffectKind::ForwardEffectChannelWord { .. }
        | CmvsPs2aCommandEffectKind::SetEffectChannelOrigin { .. }
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
        | CmvsPs2aCommandEffectKind::CreatePlaybackChannelElement { .. }
        | CmvsPs2aCommandEffectKind::ForwardEffectPlaybackSetup { .. }
        | CmvsPs2aCommandEffectKind::ConfigureEffectPlaybackSlots { .. }
        | CmvsPs2aCommandEffectKind::PlayChannelSoundAndComplete { .. }
        | CmvsPs2aCommandEffectKind::PrepareEffectChannelFromName { .. }
        | CmvsPs2aCommandEffectKind::StartEffectChannelPlayback { .. }
        | CmvsPs2aCommandEffectKind::StorePlaybackRecordWord
| CmvsPs2aCommandEffectKind::DestroyEffectHitElement
        | CmvsPs2aCommandEffectKind::WaitEffectChannelComplete { .. }
        | CmvsPs2aCommandEffectKind::InstallChannelChildPayload { .. }
        | CmvsPs2aCommandEffectKind::ConfigureChannelChildPresenter { .. }
        | CmvsPs2aCommandEffectKind::QueryChannelChildName { .. } => {
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
            state.interpreter_words.insert(1596, enabled);
            state.interpreter_words.insert(15064, enabled);
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
        | CmvsPs2aCommandEffectKind::CommitScreenParams { .. }
        | CmvsPs2aCommandEffectKind::WaitScreenCommit { .. }
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
        | CmvsPs2aCommandEffectKind::StoreScriptSlotPresentationState { .. }
        | CmvsPs2aCommandEffectKind::QueryChannelChildState { .. }
        | CmvsPs2aCommandEffectKind::LoadTextureParentFromName { .. }
        | CmvsPs2aCommandEffectKind::StoreTransitionParams
        | CmvsPs2aCommandEffectKind::QueryChannelTransitionBusy { .. }
        | CmvsPs2aCommandEffectKind::ReleaseChannelTransition
        | CmvsPs2aCommandEffectKind::ReloadSlotTextResource { .. }
        | CmvsPs2aCommandEffectKind::QueryAuxiliarySlotStatus { .. }
        | CmvsPs2aCommandEffectKind::ReleaseTextureChildByIndex
        | CmvsPs2aCommandEffectKind::ConfigureTextureChildGeometry
        | CmvsPs2aCommandEffectKind::StoreTextureChildWord
| CmvsPs2aCommandEffectKind::KickTextureChildDraw
        | CmvsPs2aCommandEffectKind::TriggerEffectSlot
        | CmvsPs2aCommandEffectKind::SetSystemVolume
        | CmvsPs2aCommandEffectKind::QuerySceneLayer { .. }
        | CmvsPs2aCommandEffectKind::ClearSceneLayer { .. }
        | CmvsPs2aCommandEffectKind::LayoutMessageText { .. }
        | CmvsPs2aCommandEffectKind::DisposeSceneLayerWorker { .. }
        | CmvsPs2aCommandEffectKind::TogglePresentationMode { .. }
        | CmvsPs2aCommandEffectKind::ResetPresentationBuffers
        | CmvsPs2aCommandEffectKind::SelectPresentationSlot { .. }
        | CmvsPs2aCommandEffectKind::StoreTextureChildByIndex
| CmvsPs2aCommandEffectKind::SelectRenderViewport { .. } => {
            presentation::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::SelectEffectChild { .. }
        | CmvsPs2aCommandEffectKind::SetEffectTextSurface { .. }
        | CmvsPs2aCommandEffectKind::SetEffectElementVisible { .. }
        | CmvsPs2aCommandEffectKind::QueryEffectElementExists { .. }
        | CmvsPs2aCommandEffectKind::SetEffectElementSize { .. }
        | CmvsPs2aCommandEffectKind::SetEffectElementAuxiliary { .. }
        | CmvsPs2aCommandEffectKind::SetEffectElementAuxiliaryPair { .. }
        | CmvsPs2aCommandEffectKind::ApplyEffectElementOperation { .. }
        | CmvsPs2aCommandEffectKind::EffectChildCommand { .. }
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
        | CmvsPs2aCommandEffectKind::LoadTextureParentResourceExtended { .. }
        | CmvsPs2aCommandEffectKind::CommitTexturePresentation { .. }
        | CmvsPs2aCommandEffectKind::InitializeTextureParentSurface { .. } => {
            textures::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::QueryNewestSaveSlot { .. }
        | CmvsPs2aCommandEffectKind::SelectSystemCursor { .. }
        | CmvsPs2aCommandEffectKind::RequestSystemStateLoad { .. }
        | CmvsPs2aCommandEffectKind::RecreateSaveImageOwner { .. }
        | CmvsPs2aCommandEffectKind::QuerySaveImageProgress { .. }
        | CmvsPs2aCommandEffectKind::StoreSaveImageEnabled
        | CmvsPs2aCommandEffectKind::SaveImageOwnerWaitOrDestroy { .. }
        | CmvsPs2aCommandEffectKind::DestroySaveImageOwner
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
        | CmvsPs2aCommandEffectKind::QuerySaveImageEnabled { .. }
        | CmvsPs2aCommandEffectKind::QueryRendererDisplayMode { .. }
        | CmvsPs2aCommandEffectKind::StorePressLatch { .. }| CmvsPs2aCommandEffectKind::RunEffectTimeline
        | CmvsPs2aCommandEffectKind::PushEffectTimelineStep647
| CmvsPs2aCommandEffectKind::PushEffectTimelineStep649
        | CmvsPs2aCommandEffectKind::PushEffectTimelineStep650
         | CmvsPs2aCommandEffectKind::ReleaseGraphicsContext { .. }
        | CmvsPs2aCommandEffectKind::QueryGraphicsContext { .. }
        | CmvsPs2aCommandEffectKind::RebuildGraphicsContext { .. }
         | CmvsPs2aCommandEffectKind::DestroySceneLayerWorker737
| CmvsPs2aCommandEffectKind::RebuildSceneLayerChild
| CmvsPs2aCommandEffectKind::ReadSlotStatusWord { .. }
| CmvsPs2aCommandEffectKind::ClearPresentBlock
         | CmvsPs2aCommandEffectKind::QueryPresentSlotHit { .. }
         | CmvsPs2aCommandEffectKind::ReadPresentCompleteFlag { .. }
         | CmvsPs2aCommandEffectKind::SetPresentAllFlag { .. }
         | CmvsPs2aCommandEffectKind::PushEffectTimelineChannel
         | CmvsPs2aCommandEffectKind::PushEffectTimelineDelay
         | CmvsPs2aCommandEffectKind::ReadMenuWorkState { .. }
        | CmvsPs2aCommandEffectKind::StoreWindowStyleSetting { .. }
        | CmvsPs2aCommandEffectKind::ReadMenuWorkSelection { .. }
        | CmvsPs2aCommandEffectKind::ResetMenuWorkArea
        | CmvsPs2aCommandEffectKind::BuildSaveSnapshot
        | CmvsPs2aCommandEffectKind::ReadCancelLatches { .. }
        | CmvsPs2aCommandEffectKind::ReadAuxLatches { .. }
        | CmvsPs2aCommandEffectKind::QueryPointerPosition { .. }
        | CmvsPs2aCommandEffectKind::HitTestPointerRect { .. }
        | CmvsPs2aCommandEffectKind::HitTestPointerRegion { .. } => {
            system::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::StoreSettingBooleanPlain { .. }
        | CmvsPs2aCommandEffectKind::StoreSystemSettingBoolean { .. }
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
        | CmvsPs2aCommandEffectKind::StoreSystemConfigBoolean { .. } => {
            settings::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::CallScript
        | CmvsPs2aCommandEffectKind::ReloadRootScript
        | CmvsPs2aCommandEffectKind::ResetEngineToMain
        | CmvsPs2aCommandEffectKind::SaveSystemState
        | CmvsPs2aCommandEffectKind::DestroyEffectPlayback { .. }
        | CmvsPs2aCommandEffectKind::RebuildChainGraphSuspend { .. }
        | CmvsPs2aCommandEffectKind::ResumeInterpreterCoroutineRecord { .. }
        | CmvsPs2aCommandEffectKind::QueryScriptSlotOccupancy { .. } => {
            scripts::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::ConfigureTextureChildAudio { .. }
        | CmvsPs2aCommandEffectKind::DrawTextureChild { .. }
        | CmvsPs2aCommandEffectKind::QueryTextureChildState { .. }
        | CmvsPs2aCommandEffectKind::ReleaseTextureChildState { .. }
        | CmvsPs2aCommandEffectKind::ResetTextureChild { .. }
        | CmvsPs2aCommandEffectKind::LoadTextureChildResource { .. }
        | CmvsPs2aCommandEffectKind::InitializeTextureChildSurface { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureChildRect { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureChildPosition { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureAuxiliaryPair { .. }
        | CmvsPs2aCommandEffectKind::ConfigureTextureAuxiliaryWord { .. } => {
            texture_children::execute(state, effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::FadeOutAudio
        | CmvsPs2aCommandEffectKind::StopAudio
        | CmvsPs2aCommandEffectKind::PlayAudio
        | CmvsPs2aCommandEffectKind::PlayPairedAudio => {
            crossfade_audio::execute(effect_kind, &values)
        }
        CmvsPs2aCommandEffectKind::ResolveSpeakerEntry { .. }
        | CmvsPs2aCommandEffectKind::FormatStringIntoSlot => {
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

fn count_tagged_stack_words(state: &CmvsPs2aVmState) -> usize {
    state
        .stack_bytes
        .as_chunks::<4>()
        .0
        .iter()
        .take((state.stack_cursor_bytes / 4) as usize)
        .filter(|chunk| {
            let word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            word & 0xc000_0000 != 0
        })
        .count()
}

fn effect_kind_variant_name(kind: &CmvsPs2aCommandEffectKind) -> String {
    format!("{kind:?}")
}
