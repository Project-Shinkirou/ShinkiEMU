use super::*;

#[path = "pointer_region.rs"]
mod pointer_region;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (CmvsPs2aCommandEffectKind::SetNativeQueuePolicy, [(_, enabled)]) => {
            state.native_mesh_equal_prepend = *enabled != 0;
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::QueryNativeSceneWorkerStatus, [(_, slot)]) => {
            if *slot > 3 {
                return Ok(None);
            }
            let owner = state
                .native_scene_workers
                .get(&(*slot as u8))
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_NATIVE_MESH",
                        "native scene status requires a supported live owner",
                    )
                })?;
            state.write_interpreter_word(81220, owner.status()?);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::RequestNativeSceneWorker,
            [(_, slot), (_, selector), (_, first), (_, second)],
        ) => {
            crate::vm::native_scene_worker::request(state, *slot, [*selector, *first, *second])?;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::OpenMovieStream {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, name)],
        ) => {
            // `sub_4891B0` resolves the movie name and opens the stream; the
            // open result word is written by the host after the archive
            // lookup, mirroring the native `sub_412800` boundary.
            let resource = resolve_resource_name(state, *name)?;
            Ok(Some(CmvsPs2aVmAction::OpenMovieStream {
                resource,
                result_field_offset,
            }))
        }
        (CmvsPs2aCommandEffectKind::ReleaseMovieObject, []) => {
            // `sub_489360` releases the movie object at dword 795.
            tracing::info!(
                event = "shinki.emu.cmvs.vm.movie_released",
                pc = state.program_counter,
                frame = state.current_frame,
                tick = state.tick
            );
            state.movie_object = None;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryMovieState {
                result_field_offset,
            },
            [],
        ) => {
            // `sub_489330` publishes the movie player's audio-clock frame
            // counter (`player->this[8]`, saturating at the index count)
            // while the object exists, or -1 once it is gone.
            let value = if state.movie_object.is_some() {
                state.movie_frame
            } else {
                0xffff_ffff
            };
            state.write_interpreter_word(result_field_offset, value);
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::CaptureSaveSnapshot, []) => {
            // `sub_471030` reads the interpreter snapshot dimensions and
            // hands the framebuffer to the host; the return word is 0 in
            // both success and readback failure, so dispatch continues.
            Ok(Some(CmvsPs2aVmAction::CaptureSaveSnapshot))
        }
        (
            CmvsPs2aCommandEffectKind::LoadNativeSaveSlot,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot)],
        ) => {
            // `sub_4859D0`: out-of-range slots are consumed and ignored;
            // valid slots hand the load to the host.
            if *slot > 999 {
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::LoadNativeSaveSlot { slot: *slot }))
        }
        (
            CmvsPs2aCommandEffectKind::SaveNativeGameSlot,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot)],
        ) => {
            // `sub_485BB0`: out-of-range slots are consumed and ignored.
            if *slot > 999 {
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::SaveNativeGameSlot { slot: *slot }))
        }
        (
            CmvsPs2aCommandEffectKind::ProbeNativeSaveSlot {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot)],
        ) => {
            if *slot > 999 {
                state.write_interpreter_word(result_field_offset, 0);
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::ProbeNativeSaveSlot {
                slot: *slot,
                result_field_offset,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SetNativeSaveTitle {
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, slot),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, title),
            ],
        ) => {
            if *slot > 999 {
                state.write_interpreter_word(result_field_offset, 0);
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::SetNativeSaveTitle {
                slot: *slot,
                title: resolve_resource_name(state, *title)?,
                result_field_offset,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::LoadNativeSaveInfo,
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, slot),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, parent),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, title_target),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, secondary_target),
            ],
        ) => {
            // `sub_4855C0`: negative slots, oversized channels and missing
            // files all publish failure (result word zero) and continue.
            Ok(Some(CmvsPs2aVmAction::LoadNativeSaveInfo {
                slot: *slot,
                parent: *parent,
                child: *child as i32,
                title_target: *title_target,
                secondary_target: *secondary_target,
                result_field_offset: 81220,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SetNativeSaveMeta {
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, slot),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, a),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, b),
            ],
        ) => {
            if *slot > 999 {
                state.write_interpreter_word(result_field_offset, 0);
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::SetNativeSaveMeta {
                slot: *slot,
                metadata: [*a, *b],
                result_field_offset,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::QueryNewestSaveSlot {
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _start),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _count),
            ],
        ) => {
            // No writable save files exist in the headless session, so the
            // newest-save scan returns the canonical "none" slot.
            state.write_interpreter_word(result_field_offset, u32::MAX);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SelectSystemCursor {
                manager_field_offset,
                current_index_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, index)],
        ) => {
            let key = component_field_key(manager_field_offset, current_index_field_offset);
            if state.component_words.get(&key) == Some(index) {
                return Ok(None);
            }
            state.component_words.insert(key, *index);
            Ok(Some(CmvsPs2aVmAction::SelectSystemCursor { index: *index }))
        }
        (CmvsPs2aCommandEffectKind::RequestSystemStateLoad { path_buffer_offset }, []) => {
            let request = CmvsPs2aStorageRequest::LoadSystemState { path_buffer_offset };
            state.storage_await = Some(request.clone());
            Ok(Some(CmvsPs2aVmAction::StorageRequest(request)))
        }
        (
            CmvsPs2aCommandEffectKind::CreateScreenShake {
                handle_field_offset,
            },
            [(_, first), (_, second), (_, third), (_, fourth)],
        ) => {
            state.screen_shake = Some(CmvsScreenShakeState {
                construction_words: [*first, *second, *third, *fourth],
                ..Default::default()
            });
            state.write_interpreter_word(u32::from(handle_field_offset), 1);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryScreenShakeRemaining {
                result_field_offset,
            },
            [],
        ) => {
            let remaining = state.screen_shake.as_ref().map_or(u32::MAX, |owner| {
                let count = owner.construction_words[3];
                if count == u32::MAX {
                    count
                } else {
                    count.wrapping_sub(owner.completed_periods)
                }
            });
            state.write_interpreter_word(result_field_offset, remaining);
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::SetScreenShakePaused, [(_, paused)]) => {
            if let Some(owner) = &mut state.screen_shake {
                owner.paused = *paused != 0;
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::WaitOrDestroyScreenShake {
                result_field_offset,
            },
            [(_, wait)],
        ) => {
            // A live true-branch is held by wait_gate (A000).
            if state.screen_shake.is_some() && *wait == 0 {
                state.screen_shake = None;
                state.write_interpreter_word(3256, 0);
                state.dispatch_stopped = true;
            }
            state.write_interpreter_word(result_field_offset, 1);
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::DestroyScreenShake, []) => {
            state.screen_shake = None;
            state.write_interpreter_word(3256, 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryScreenShakePaused {
                result_field_offset,
            },
            [],
        ) => {
            state.write_interpreter_word(
                result_field_offset,
                u32::from(
                    state
                        .screen_shake
                        .as_ref()
                        .is_some_and(|owner| owner.paused),
                ),
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryRendererDisplayMode {
                result_field_offset,
                ..
            },
            [],
        ) => {
            // 48A570 -> 432F70/432F30 reads the configured native stage,
            // independent of the host raster and window size.
            let [width, height] = state.native_render_size;
            if width == 0 || height == 0 {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_DISPLAY_UNBOUND",
                    "native display dimensions are not bound",
                ));
            }
            state.write_interpreter_word(result_field_offset, width);
            state.write_interpreter_word(result_field_offset + 4, height);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryPointerPosition {
                x_field_offset,
                y_field_offset,
            },
            [],
        ) => {
            // `sub_480280` publishes the persistent pointer into system
            // registers 1 and 2 through `sub_45CF40`.
            state.write_interpreter_word(x_field_offset, state.pointer_x as u32);
            state.write_interpreter_word(y_field_offset, state.pointer_y as u32);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::HitTestPointerRect {
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, x),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, y),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, width),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, height),
            ],
        ) => {
            // `sub_480060` rejects only when the pointer is strictly outside
            // the rectangle, so both bounds are inclusive.
            let x = *x as i32;
            let y = *y as i32;
            let hit = state.pointer_x >= x
                && state.pointer_x <= x.wrapping_add(*width as i32)
                && state.pointer_y >= y
                && state.pointer_y <= y.wrapping_add(*height as i32);
            state.write_interpreter_word(result_field_offset, u32::from(hit));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::HitTestPointerRegion {
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, shape),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, left),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, top),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, right),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, bottom),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, width),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, height),
            ],
        ) => {
            let hit = pointer_region::contains(
                state,
                [*shape, *left, *top, *right, *bottom, *width, *height],
            )?;
            state.write_interpreter_word(result_field_offset, u32::from(hit));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReadAuxLatches {
                held_field_offset,
                edge_field_offset,
            },
            [],
        ) => {
            // `sub_479AE0` publishes the skip-group latches; the pump maps
            // no physical key to this group, so the original latches stay
            // zero.
            state.write_interpreter_word(held_field_offset, 0);
            state.write_interpreter_word(edge_field_offset, 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReadCancelLatches {
                held_field_offset,
                edge_field_offset,
            },
            [],
        ) => {
            // `sub_479A70` publishes the cancel-group latches; the pump
            // tracks only the held state.
            let held = u32::from(state.input_cancel_held);
            state.write_interpreter_word(held_field_offset, held);
            state.write_interpreter_word(edge_field_offset, held);
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::ResetEffectTimeline, []) => {
            super::super::native_timeline::reset(state);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReadEffectTimelineCursor {
                result_field_offset,
            },
            [],
        ) => {
            state.write_interpreter_word(
                result_field_offset,
                super::super::native_timeline::cursor(state),
            );
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::StartEffectTimeline, []) => {
            super::super::native_timeline::start(state)?;
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::StopEffectTimeline, []) => {
            super::super::native_timeline::stop(state);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreWindowStyleSetting {
                setting_field_offset,
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, selector),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _value),
            ],
        ) => {
            // `sub_479860`: a non-zero selector applies the cursor style
            // through the window engine (unobservable here); zero reads
            // the style setting into the result field.
            if *selector == 0 {
                let value = state
                    .settings_words
                    .get(&setting_field_offset)
                    .copied()
                    .unwrap_or(0);
                state.write_interpreter_word(result_field_offset, value);
            }
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::BuildSaveSnapshot, []) => {
            let resume_pc = state
                .current_instruction_start
                .checked_add(2)
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_NATIVE_SLOT",
                        "native snapshot continuation PC overflowed",
                    )
                })?;
            // 485D10 restores the temporary PC increment, but leaves the
            // live counter at zero after capturing 4712F0.
            state.write_interpreter_word(13224, 0);
            Ok(Some(CmvsPs2aVmAction::BuildNativeSaveSnapshot {
                resume_pc,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::PushEffectTimelineDelay,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, duration)],
        ) => {
            super::super::native_timeline::push_delay(state, *duration)?;
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::SetPresentAllFlag { field_offset }, []) => {
            // The synchronous headless present pass completes the
            // present-all request immediately, so the present-complete
            // flag (dword 2411) rises with it.
            state.write_interpreter_word(field_offset, 1);
            state.write_interpreter_word(9644, 1);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReadPresentCompleteFlag {
                result_field_offset,
                complete_field_offset,
            },
            [],
        ) => {
            let complete = state
                .interpreter_words
                .get(&complete_field_offset)
                .copied()
                .unwrap_or(0);
            state.write_interpreter_word(result_field_offset, u32::from(complete != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryPresentSlotHit {
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, slot),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, guard),
            ],
        ) => {
            let key = (1924_u32 << 8) | ((*slot as u16) as u32);
            if *slot < 256 && state.slot_objects.contains_key(&key) && (*guard as i32) < 0 {
                let complete = state.interpreter_words.get(&9644).copied().unwrap_or(0);
                state.write_interpreter_word(9644, 0);
                state.write_interpreter_word(result_field_offset, u32::from(complete != 0));
            } else {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.write_interpreter_word(10532, flags | 0x10);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::DestroySceneLayerWorker737,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot)],
        ) => {
            // `sub_481590`: slots above three are ignored; the marker
            // word drops with the native worker teardown.
            if *slot <= 3 {
                let (retired_static_sounds, stopped_streams) =
                    crate::vm::native_scene_worker::retired_audio(state, *slot);
                crate::vm::native_scene_worker::remove_slot(state, *slot);
                state.write_interpreter_word(4 * 737 + 4 * *slot, 0);
                return Ok(Some(CmvsPs2aVmAction::NativeSceneWorkerResourceChanged {
                    slot: *slot as u8,
                    retired_static_sounds,
                    stopped_streams,
                }));
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::RebuildSceneLayerChild,
            [(_, slot), (_, parent), (_, tile), (_, parameter)],
        ) => {
            let (retired_static_sounds, stopped_streams) =
                crate::vm::native_scene_worker::retired_audio(state, *slot);
            let moved = super::super::native_scene_image::transfer(
                state,
                *slot,
                *parent,
                [*tile, *parameter],
            )?;
            Ok(
                moved.then_some(CmvsPs2aVmAction::TransferTextureParentToSceneWorker {
                    parent_slot: *parent as u8,
                    slot: *slot as u8,
                    retired_static_sounds,
                    stopped_streams,
                }),
            )
        }
        (CmvsPs2aCommandEffectKind::ClearPresentBlock, []) => {
            for field in [9632, 9636, 9640, 9644] {
                state.write_interpreter_word(field, 0);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PushEffectTimelineCamera,
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, mode),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, x),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, y),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, z),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, duration),
            ],
        ) => {
            super::super::native_timeline::push_camera(state, *mode, [*x, *y, *z], *duration)?;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PushEffectTimelineTransform { mode },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, target),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, x),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, y),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, z),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, curve),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, duration),
            ],
        ) => {
            super::super::native_timeline::push_transform(
                state,
                mode,
                *target,
                [*x, *y, *z],
                *curve,
                *duration,
            )?;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::RebuildGraphicsContext {
                result_field_offset,
            },
            _,
        ) => {
            state.write_interpreter_word(result_field_offset, 1);
            state.write_interpreter_word(3264, 1);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReleaseGraphicsContext {
                require_render_state,
            },
            [],
        ) => {
            // `sub_47B470`/`sub_47B400` destroy the native context object;
            // the headless marker at dword 816 tracks its presence.
            if !require_render_state
                || state.interpreter_words.get(&3264).copied().unwrap_or(0) != 0
            {
                state.write_interpreter_word(3264, 0);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryGraphicsContext {
                result_field_offset,
            },
            [],
        ) => {
            // `sub_47B440`: the render state, or -1 without a context.
            let present = state.interpreter_words.get(&3264).copied().unwrap_or(0) != 0;
            state.write_interpreter_word(result_field_offset, u32::from(present));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PushEffectTimelineChannel,
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, name),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, param),
            ],
        ) => {
            let resource = super::super::text::resolve_string_value(state, *name)?;
            super::super::native_timeline::push_channel(state, *channel, resource, *param)?;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PushEffectTimelineElement,
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, child),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, name),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, duration),
            ],
        ) => {
            let resource = super::super::text::resolve_string_value(state, *name)?;
            super::super::native_timeline::push_element(state, *child, resource, *duration)?;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReadEffectTimelineCount {
                result_field_offset,
            },
            [],
        ) => {
            state.write_interpreter_word(
                result_field_offset,
                super::super::native_timeline::count(state),
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StorePressLatch {
                result_field_offset,
            },
            [],
        ) => {
            // `sub_479C60` publishes the advance-group press latch
            // (`sub_45CCD0` reads manager dword 273). The latch persists
            // from the press edge until this query consumes it — the
            // compiler's press gate is its only consumer on this path.
            let press = state.input_advance_press;
            state.input_advance_press = false;
            state.write_interpreter_word(result_field_offset, u32::from(press));
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::ConsumeAdvanceLatch, []) => {
            // `sub_45CB80` consumes the advance-group latch edges; the
            // per-frame pump rebuilds them next frame. While a message quad
            // is live on channel 7 the message engine's hover query owns
            // the input focus: the scene page loop reads the latch without
            // clearing it, exactly as the native loop leaves the manager
            // latches to the message pipeline during message display.
            let message_holds_input = state
                .effect_channels
                .get(&7)
                .is_some_and(|object| object.quad_visible(12));
            if state.input_advance_release && !message_holds_input {
                state.input_advance_release = false;
            }
            // `sub_45CB80` clears the press latch (manager dword 273) under
            // the same focus rule.
            if state.input_advance_press && !message_holds_input {
                state.input_advance_press = false;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CmvsPs2aSourceSpan;

    #[test]
    fn native_snapshot_uses_the_real_framed_command_continuation() {
        let mut vm = CmvsPs2aVmState::new(0x100);
        for (pc, command_id) in [(0x100, 8), (0x102, 690)] {
            let action = execute_cmvs390_frame(
                &mut vm,
                &CmvsPs2aInstructionFrame::Command {
                    span: CmvsPs2aSourceSpan {
                        offset: pc,
                        byte_length: 2,
                    },
                    command_id,
                },
            )
            .unwrap();
            if command_id == 690 {
                assert_eq!(
                    action,
                    Some(CmvsPs2aVmAction::BuildNativeSaveSnapshot { resume_pc: 0x104 })
                );
            }
        }
        assert_eq!(vm.program_counter, 0x104);
        assert_eq!(vm.interpreter_words[&13224], 0);
    }

    #[test]
    fn overflowing_native_capture_never_emits_a_snapshot_or_resumes_partial_state() {
        let mut vm = CmvsPs2aVmState::new(u32::MAX - 1);
        vm.write_interpreter_word(13224, 77);
        assert!(
            execute_cmvs390_frame(
                &mut vm,
                &CmvsPs2aInstructionFrame::Command {
                    span: CmvsPs2aSourceSpan {
                        offset: u32::MAX - 1,
                        byte_length: 2
                    },
                    command_id: 690,
                }
            )
            .is_err()
        );
        assert_eq!(vm.interpreter_words[&13224], 77);
        assert!(validate_cmvs390_vm_state(&vm).is_err());
    }
}
