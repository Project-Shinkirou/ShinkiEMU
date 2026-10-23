use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
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
            state.interpreter_words.insert(result_field_offset, value);
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::CaptureSaveSnapshot, []) => {
            // `sub_471030` reads the interpreter snapshot dimensions and
            // hands the framebuffer to the host; the return word is 0 in
            // both success and readback failure, so dispatch continues.
            Ok(Some(CmvsPs2aVmAction::CaptureSaveSnapshot))
        }
        (CmvsPs2aCommandEffectKind::LoadNativeSaveSlot, [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot)]) => {
            // `sub_4859D0`: out-of-range slots are consumed and ignored;
            // valid slots hand the load to the host.
            if *slot > 999 {
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::LoadNativeSaveSlot { slot: *slot }))
        }
        (CmvsPs2aCommandEffectKind::SaveNativeGameSlot, [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot)]) => {
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
                state.interpreter_words.insert(result_field_offset, 0);
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
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, _title), (CmvsPs2aCommandStackWordKind::OpaqueU32, slot)],
        ) => {
            if *slot > 999 {
                state.interpreter_words.insert(result_field_offset, 0);
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::SetNativeSaveTitle {
                slot: *slot,
                result_field_offset,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::LoadNativeSaveInfo,
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, _str1), (CmvsPs2aCommandStackWordKind::TaggedStringReference, _str2), (CmvsPs2aCommandStackWordKind::OpaqueU32, _sprite), (CmvsPs2aCommandStackWordKind::OpaqueU32, _channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, slot)],
        ) => {
            // `sub_4855C0`: negative slots, oversized channels and missing
            // files all publish failure (result word zero) and continue.
            Ok(Some(CmvsPs2aVmAction::LoadNativeSaveInfo {
                slot: *slot,
                result_field_offset: 81220,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SetNativeSaveMeta {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _b), (CmvsPs2aCommandStackWordKind::OpaqueU32, _a), (CmvsPs2aCommandStackWordKind::OpaqueU32, slot)],
        ) => {
            if *slot > 999 {
                state.interpreter_words.insert(result_field_offset, 0);
                return Ok(None);
            }
            Ok(Some(CmvsPs2aVmAction::SetNativeSaveMeta {
                slot: *slot,
                result_field_offset,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::QueryNewestSaveSlot {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _start), (CmvsPs2aCommandStackWordKind::OpaqueU32, _count)],
        ) => {
            // No writable save files exist in the headless session, so the
            // newest-save scan returns the canonical "none" slot.
            state
                .interpreter_words
                .insert(result_field_offset, u32::MAX);
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
            CmvsPs2aCommandEffectKind::RecreateSaveImageOwner {
                handle_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth)],
        ) => {
            // `sub_47AFF0` frees the previous owner, then `sub_459BE0`
            // zeroes the flag and progress fields of the fresh owner. The
            // published handle word keeps the version-pinned field updated
            // for snapshot parity.
            state.save_image_owner = Some(CmvsSaveImageOwnerState {
                construction_words: [*first, *second, *third, *fourth],
                enabled: false,
                progress_delta: 0,
            });
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.save_image_created",
                    pc = state.program_counter,
                    words = ?[*first, *second, *third, *fourth]
                );
            }
            state.interpreter_words.insert(
                u32::from(handle_field_offset),
                state.save_image_owner.as_ref().map(|_| 1_u32).unwrap_or(0),
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QuerySaveImageProgress {
                result_field_offset,
            },
            [],
        ) => {
            // `sub_459CC0` returns the fourth construction word unchanged
            // when it is all-ones, otherwise reduced by the progress delta
            // the unrecovered render path updates.
            let value = match &state.save_image_owner {
                Some(owner) if owner.construction_words[3] == u32::MAX => u32::MAX,
                Some(owner) => owner.construction_words[3].wrapping_sub(owner.progress_delta),
                None => u32::MAX,
            };
            state.interpreter_words.insert(result_field_offset, value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreSaveImageEnabled,
            [(CmvsPs2aCommandStackWordKind::BooleanU32, enabled)],
        ) => {
            // `sub_47B0F0` only touches the flag of a present owner; the
            // pop happens regardless of the owner's existence.
            if let Some(owner) = state.save_image_owner.as_mut() {
                owner.enabled = *enabled != 0;
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SaveImageOwnerWaitOrDestroy {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::BooleanU32, wait)],
        ) => match (&state.save_image_owner, *wait != 0) {
            // `sub_47B160` writes one for a missing owner and returns
            // 0x4004.
            (None, _) => {
                state.interpreter_words.insert(result_field_offset, 1);
                Ok(None)
            }
            // The set-boolean branch waits for the screenshot render to
            // complete (dispatch result 0xA000); that path is not
            // recovered and blocks instead of spinning.
            (Some(_), true) => {
                state.interpreter_words.insert(result_field_offset, 0);
                Err(invalid(
                    "SHINKI_EMU_CMVS_VM_SAVE_IMAGE",
                    "CMVS screenshot completion wait is not recovered",
                ))
            }
            // The clear-boolean branch destroys the owner and reports
            // success (dispatch result 0xC004).
            (Some(_), false) => {
                state.save_image_owner = None;
                state.interpreter_words.insert(result_field_offset, 1);
                Ok(None)
            }
        },
        (CmvsPs2aCommandEffectKind::DestroySaveImageOwner, []) => {
            // `sub_47B120` frees a present owner without further effects.
            state.save_image_owner = None;
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QuerySaveImageEnabled {
                result_field_offset,
            },
            [],
        ) => {
            // `sub_47B0B0` combines the owner existence with its field-5
            // flag through `sub_432F60`.
            let value = u32::from(
                state
                    .save_image_owner
                    .as_ref()
                    .is_some_and(|owner| owner.enabled),
            );
            state.interpreter_words.insert(result_field_offset, value);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryRendererDisplayMode {
                result_field_offset,
                ..
            },
            [],
        ) => {
            // `sub_48A570` publishes the render manager's display-mode
            // registers (fields 6 and 7, read through `sub_4292D0`/
            // `sub_411E10`) into system registers 1 and 2. The scene layout
            // arithmetic branches on them (`proc280*2 + 450` horizontal,
            // `proc284*2 + 120` vertical): the recovered values for this
            // title's 800x600 render mode are 400 and 300, which place the
            // computed layout inside the 1280x720 stage.
            state.interpreter_words.insert(result_field_offset, 400);
            state.interpreter_words.insert(result_field_offset + 4, 300);
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
            state
                .interpreter_words
                .insert(x_field_offset, state.pointer_x as u32);
            state
                .interpreter_words
                .insert(y_field_offset, state.pointer_y as u32);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::HitTestPointerRect {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, x), (CmvsPs2aCommandStackWordKind::OpaqueU32, y), (CmvsPs2aCommandStackWordKind::OpaqueU32, width), (CmvsPs2aCommandStackWordKind::OpaqueU32, height)],
        ) => {
            // `sub_480060` rejects only when the pointer is strictly outside
            // the rectangle, so both bounds are inclusive.
            let x = *x as i32;
            let y = *y as i32;
            let hit = state.pointer_x >= x
                && state.pointer_x <= x.wrapping_add(*width as i32)
                && state.pointer_y >= y
                && state.pointer_y <= y.wrapping_add(*height as i32);
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(hit));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::HitTestPointerRegion {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, shape), (CmvsPs2aCommandStackWordKind::OpaqueU32, left), (CmvsPs2aCommandStackWordKind::OpaqueU32, top), (CmvsPs2aCommandStackWordKind::OpaqueU32, right), (CmvsPs2aCommandStackWordKind::OpaqueU32, bottom), (CmvsPs2aCommandStackWordKind::OpaqueU32, _width), (CmvsPs2aCommandStackWordKind::OpaqueU32, _height)],
        ) => match *shape {
            // `SetRect` + `CreateRectRgnIndirect`: a GDI rectangle region is
            // half-open, covering left/top and excluding right/bottom.
            1 => {
                let hit = state.pointer_x >= *left as i32
                    && state.pointer_x < *right as i32
                    && state.pointer_y >= *top as i32
                    && state.pointer_y < *bottom as i32;
                state
                    .interpreter_words
                    .insert(result_field_offset, u32::from(hit));
                Ok(None)
            }
            _ => Err(invalid(
                "SHINKI_EMU_CMVS_VM_POINTER_REGION",
                "CMVS pointer region shape rasterization is not recovered",
            )),
        },
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
            state.interpreter_words.insert(held_field_offset, 0);
            state.interpreter_words.insert(edge_field_offset, 0);
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
            state.interpreter_words.insert(held_field_offset, held);
            state.interpreter_words.insert(edge_field_offset, held);
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::ResetMenuWorkArea, []) => {
            // `sub_424790` clears the menu work area; the recovered
            // subset keeps no menu work state.
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReadMenuWorkSelection {
                result_field_offset,
            },
            [],
        ) => {
            // `sub_47C0B0` reads the active menu selection; without a
            // native menu the original returns -1.
            state
                .interpreter_words
                .insert(result_field_offset, u32::MAX);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::StoreWindowStyleSetting {
                setting_field_offset,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, selector), (CmvsPs2aCommandStackWordKind::OpaqueU32, _value)],
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
                state.interpreter_words.insert(result_field_offset, value);
            }
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::BuildSaveSnapshot, []) => {
            // `sub_4712F0` serializes the save blob outside the recovered
            // interpreter state; nothing observable changes here.
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PushEffectTimelineDelay,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, duration)],
        ) => {
            // `sub_4246C0` appends a type-1 delay record (duration at
            // record dword 96) while the sixteen-record timeline has room.
            if state.effect_timeline.len() < 16 {
                state.effect_timeline.push(CmvsEffectTimelineStep {
                    step_type: 1,
                    duration: *duration,
                    ..Default::default()
                });
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::SetPresentAllFlag { field_offset },
            [],
        ) => {
            // The synchronous headless present pass completes the
            // present-all request immediately, so the present-complete
            // flag (dword 2411) rises with it.
            state.interpreter_words.insert(field_offset, 1);
            state.interpreter_words.insert(9644, 1);
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
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(complete != 0));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryPresentSlotHit {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::OpaqueU32, guard)],
        ) => {
            let key = (1924_u32 << 8) | ((*slot as u16) as u32);
            if *slot < 256
                && state.slot_objects.contains_key(&key)
                && (*guard as i32) < 0
            {
                let complete = state
                    .interpreter_words
                    .get(&9644)
                    .copied()
                    .unwrap_or(0);
                state.interpreter_words.insert(9644, 0);
                state
                    .interpreter_words
                    .insert(result_field_offset, u32::from(complete != 0));
            } else {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.interpreter_words.insert(10532, flags | 0x10);
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
                state.interpreter_words.insert(4 * 737 + 4 * *slot, 0);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::RebuildSceneLayerChild,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::OpaqueU32, child_slot), ..],
        ) => {
            // `sub_480A80`: slots above three are ignored; a live 481
            // child entry dies with the rebuild.
            if *slot <= 3 {
                state.interpreter_words.insert(4 * 737 + 4 * *slot, 1);
                if let Some(child) = state
                    .interpreter_words
                    .get(&(4 * 481 + 4 * *child_slot))
                    .copied()
                {
                    if child != 0 {
                        state.interpreter_words.insert(4 * 481 + 4 * *child_slot, 0);
                    }
                }
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReadSlotStatusWord {
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, selector)],
        ) => {
            // `sub_48A3A0` (case 183): selectors 0-5 publish the native
            // slot-status word (canonical zero headless); above five
            // raises 0x200.
            if *selector > 5 {
                let flags = state.interpreter_words.get(&10532).copied().unwrap_or(0);
                state.interpreter_words.insert(10532, flags | 0x200);
            } else {
                state.interpreter_words.insert(result_field_offset, 0);
            }
            Ok(None)
        }
        (CmvsPs2aCommandEffectKind::ClearPresentBlock, []) => {
            for field in [9632, 9636, 9640, 9644] {
                state.interpreter_words.insert(field, 0);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PushEffectTimelineStep647,
            _,
        ) => {
            if state.effect_timeline.len() < 16 {
                state.effect_timeline.push(CmvsEffectTimelineStep {
                    step_type: 647,
                    ..Default::default()
                });
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PushEffectTimelineStep649,
            _,
        ) => {
            if state.effect_timeline.len() < 16 {
                state.effect_timeline.push(CmvsEffectTimelineStep {
                    step_type: 649,
                    ..Default::default()
                });
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PushEffectTimelineStep650,
            _,
        ) => {
            if state.effect_timeline.len() < 16 {
                state.effect_timeline.push(CmvsEffectTimelineStep {
                    step_type: 650,
                    ..Default::default()
                });
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::RebuildGraphicsContext {
                result_field_offset,
            },
            _,
        ) => {
            state.interpreter_words.insert(result_field_offset, 1);
            state.interpreter_words.insert(3264, 1);
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
                state.interpreter_words.insert(3264, 0);
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
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(present));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PushEffectTimelineChannel,
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::TaggedStringReference, name), (CmvsPs2aCommandStackWordKind::OpaqueU32, param)],
        ) => {
            // `sub_4245A0` appends a type-3 channel record: channel dword
            // 28, resolved resource name at +32, param dword 96.
            if state.effect_timeline.len() < 16 {
                state.effect_timeline.push(CmvsEffectTimelineStep {
                    step_type: 3,
                    duration: *param,
                    channel: *channel,
                    name: super::super::text::resolve_string_value(state, *name)?,
                });
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReadMenuWorkState {
                result_field_offset,
            },
            [],
        ) => {
            // `sub_47C090` reads the menu work area's result field; the
            // constructor zeroes it and only native config-menu code
            // writes it, so the recovered subset reads zero.
            state.interpreter_words.insert(result_field_offset, 0);
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
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.press_latch_probe",
                    pc = state.program_counter,
                    press,
                    held = state.input_confirm_held,
                    release = state.input_advance_release
                );
            }
            state
                .interpreter_words
                .insert(result_field_offset, u32::from(press));
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
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.advance_latch_consumed",
                        consumer = "scene_417",
                        pc = state.program_counter,
                        frame = state.current_frame
                    );
                }
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
