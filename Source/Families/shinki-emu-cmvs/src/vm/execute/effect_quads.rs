use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::ConfigureEffectQuadGeometry { error_mask },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, quad),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, sub),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, third),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, fifth),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, sixth),
            ],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_466CB0` truncates every value to `__int16` and stores the
            // six-word geometry block at `38*quad + 6*sub + 22`.
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .write_quad_geometry(
                    *quad,
                    *sub,
                    [
                        *first as u16 as i16,
                        *second as u16 as i16,
                        *third as u16 as i16,
                        *fourth as u16 as i16,
                        *fifth as u16 as i16,
                        *sixth as u16 as i16,
                    ],
                );
            Ok(Some(CmvsPs2aVmAction::ConfigureEffectQuad {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::ConfigureEffectQuadRect { error_mask },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, quad),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, first),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, second),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, third),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth),
            ],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_466D10` stores four `__int16` hit-rectangle words at
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .write_quad_rect(
                    *quad,
                    [
                        *first as u16 as i16,
                        *second as u16 as i16,
                        *third as u16 as i16,
                        *fourth as u16 as i16,
                    ],
                );
            Ok(Some(CmvsPs2aVmAction::ConfigureEffectQuad {
                channel: *channel,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::SelectEffectQuad { error_mask },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, quad),
            ],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_466D50`: the quad record's visibility flag becomes 1 and
            // the bound element is shown.
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .set_quad_visible(*quad, true);
            if let Some(element) = state.effect_elements.get_mut(&(*channel as u8, *quad + 16)) {
                element.visible = 1;
            }
            Ok(Some(CmvsPs2aVmAction::SelectEffectQuad {
                channel: *channel,
                quad: *quad,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::DeselectEffectQuad { error_mask },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, quad),
            ],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_466D90`: the quad record's visibility flag becomes 0 and
            // the bound element is hidden.
            state
                .effect_channels
                .entry(*channel as u8)
                .or_default()
                .set_quad_visible(*quad, false);
            if let Some(element) = state.effect_elements.get_mut(&(*channel as u8, *quad + 16)) {
                element.visible = 0;
            }
            Ok(Some(CmvsPs2aVmAction::DeselectEffectQuad {
                channel: *channel,
                quad: *quad,
            }))
        }
        (
            CmvsPs2aCommandEffectKind::ActivateEffectQuad { error_mask },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, quad),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, value),
            ],
        ) => {
            if *channel > 7 || *quad >= 32 {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, error_mask)?;
                return Ok(None);
            }
            // `sub_47F330` changes only the source rectangle and frame word;
            // it must not show a hidden quad.
            let frame = *value as u16;
            let channel_object = state.effect_channels.entry(*channel as u8).or_default();
            if channel_object.quad_frame(*quad) != frame {
                let source = channel_object
                    .quad_frame_geometry(*quad, frame)
                    .map(i32::from);
                if let Some(element) = state.effect_elements.get_mut(&(*channel as u8, *quad + 16))
                {
                    element.source = Some(source);
                }
                channel_object.set_quad_frame(*quad, frame);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectQuadState {
                effect_table_dword_index,
                empty_error_mask,
                result0_field_offset,
                result1_field_offset,
                result2_field_offset,
                result3_field_offset,
                result8_field_offset,
                result9_field_offset,
                ..
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, _index),
            ],
        ) => {
            let effect_key = (effect_table_dword_index << 8) | *channel;
            if *channel > 7 || !state.slot_objects.contains_key(&effect_key) {
                raise_error_flag(state, 10532, empty_error_mask)?;
                return Ok(None);
            }
            // The headless effect graph holds no animated quad state, so the
            // selected sub-object reports zeros for all six fields.
            for offset in [
                result0_field_offset,
                result1_field_offset,
                result2_field_offset,
                result3_field_offset,
                result8_field_offset,
                result9_field_offset,
            ] {
                state.write_interpreter_word(offset, 0);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectQuadActive {
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, quad),
            ],
        ) => {
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, 0x0002_0000)?;
                return Ok(None);
            }
            // `sub_47F470` reads the quad record's dword at byte offset
            // `76*quad + 40` and stores `(dword != 0)`: 1 while the record
            // is live. The message engine gates the page-advance path on
            // this flag, so it must follow the recovered visibility
            // transitions rather than a constant.
            let live = state
                .effect_channels
                .get(&(*channel as u8))
                .is_some_and(|object| object.quad_visible(*quad));
            state.write_interpreter_word(result_field_offset, u32::from(live));
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectPosition {
                result_x_field_offset,
                result_y_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, 0x0001_0000)?;
                return Ok(None);
            }
            // The headless engine has no animated effect position.
            state.write_interpreter_word(result_x_field_offset, 0);
            state.write_interpreter_word(result_y_field_offset, 0);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryEffectPointerHit {
                result_field_offset,
            },
            [
                (CmvsPs2aCommandStackWordKind::OpaqueU32, channel),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, rect_x),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, rect_y),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, rect_w),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, rect_h),
            ],
        ) => {
            // `sub_47F2C0` requires an occupied channel and forwards the
            // rectangle to the hit test (`sub_466A50`). The stage scale and
            // offset are identity in headless, so the test compares the
            // streamed pointer position words against the rectangle.
            if !effect_channel_exists(state, *channel) {
                raise_error_flag(state, 10532, 0x0001_0000)?;
                return Ok(None);
            }
            let pointer_x = state.pointer_x.max(0) as u32;
            let pointer_y = state.pointer_y.max(0) as u32;
            let x = *rect_x;
            let y = *rect_y;
            let hit = x <= pointer_x
                && y <= pointer_y
                && x.saturating_add(*rect_w) > pointer_x
                && y.saturating_add(*rect_h) > pointer_y;
            state.write_interpreter_word(result_field_offset, u32::from(hit));
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
