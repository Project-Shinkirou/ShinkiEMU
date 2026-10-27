use super::*;

const CONFIRM_TIMER: u32 = 0x004f_05f0;
const AUTO_TIMER: u32 = 0x004f_05ec;

fn word(state: &CmvsPs2aVmState, offset: u32) -> u32 {
    state.interpreter_words.get(&offset).copied().unwrap_or(0)
}

fn channel(state: &mut CmvsPs2aVmState, index: u32) -> Result<Option<(u8, u8)>, CoreError> {
    if index >= 8 || !state.slot_objects.contains_key(&(742 << 8 | index)) {
        raise_error_flag(state, 10532, 0x10000)?;
        return Ok(None);
    }
    let index = index as u8;
    Ok(Some((index, dialogue::text_slot(state, index)?)))
}

fn timer(state: &CmvsPs2aVmState, address: u32) -> u32 {
    state
        .process_global_words
        .get(&address)
        .copied()
        .unwrap_or(0)
}

fn indicators(state: &mut CmvsPs2aVmState, channel: u8, slot: u8, visible: bool) {
    let object = &state.effect_channels[&channel];
    let position = if visible && object.playback_mode() == 3 {
        let cursor = state.native_text[&slot].cursor;
        Some([
            cursor[0].wrapping_add(i32::from(object.read_i16(15))),
            cursor[1].wrapping_add(i32::from(object.read_i16(16))),
        ])
    } else {
        None
    };
    for child in [2, 3] {
        if let Some(element) = state.effect_elements.get_mut(&(channel, child)) {
            element.visible = u32::from(visible);
            if let Some(position) = position {
                element.position = Some(position);
            }
        }
    }
}

fn complete(state: &mut CmvsPs2aVmState, channel: u8, slot: u8, address: u32) -> bool {
    state.process_global_words.insert(address, 0);
    indicators(state, channel, slot, false);
    true
}

/// Shared timed branch of 467060/467510. The native globals are independent
/// for manual/automatic waits and survive until that wait actually completes.
fn timed_ready(state: &mut CmvsPs2aVmState, slot: u8, voice: u32, address: u32) -> bool {
    let elapsed = timer(state, address).wrapping_add(state.native_frame_delta_ms);
    state.process_global_words.insert(address, elapsed);
    if (voice as i32) >= 0 && word(state, 1520) != 0 {
        let voice_slot = voice % 4 + 5;
        let playing = state.native_audio_playing & (1 << voice_slot) != 0;
        if playing {
            state.process_global_words.insert(address, 0);
            return false;
        }
        elapsed >= word(state, 1516).wrapping_mul(100).wrapping_add(250)
    } else {
        let glyphs = state.native_text[&slot].glyphs.len() as u32;
        let required = advance_timing::auto_delay_ms(
            glyphs,
            word(state, 1504),
            word(state, 1508),
            word(state, 1512),
        );
        elapsed >= required
    }
}

/// 339/47F6E0 -> 467060: manual confirmation with optional auto/voice waiting.
pub(crate) fn confirm(
    state: &mut CmvsPs2aVmState,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<bool, CoreError> {
    let [(_, index), (_, read_flag), (_, voice)] = values else {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
            "confirm wait needs three operands",
        ));
    };
    let read = (*read_flag as i32) < 0 || state.process_flag_bits.contains(read_flag);
    let skip_allowed = if (*read_flag as i32) >= 0 && word(state, 1468) != 0 {
        if word(state, 1464) != 0 && !read {
            state.write_interpreter_word(1464, 0);
        }
        read
    } else {
        true
    };
    let forced = word(state, 10720) != 0;
    let Some((channel, slot)) = channel(state, *index)? else {
        return Ok(true);
    };
    if state.effect_channels[&channel].activity() {
        return Ok(false);
    }
    let blocked = (state.settings_words.get(&88).copied().unwrap_or(u32::MAX) as i32) >= 0;
    let held = state.input_shift_held
        || (word(state, 1464) != 0 || state.input_control_held) && skip_allowed
        || forced;
    if held && !blocked {
        return Ok(complete(state, channel, slot, CONFIRM_TIMER));
    }
    let hovering = channel_input::query_channel_quads(state, channel)?.index >= 0;
    if !hovering && !blocked && state.input_advance_release {
        state.input_advance_release = false;
        state.input_advance_press = false;
        return Ok(complete(state, channel, slot, CONFIRM_TIMER));
    }
    if word(state, 1500) != 0 && !blocked && timed_ready(state, slot, *voice, CONFIRM_TIMER) {
        return Ok(complete(state, channel, slot, CONFIRM_TIMER));
    }
    indicators(state, channel, slot, true);
    Ok(false)
}

/// 344/47F7D0 -> 467510 ignores the middle stack word and never consumes input.
pub(crate) fn automatic(
    state: &mut CmvsPs2aVmState,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<bool, CoreError> {
    let [(_, index), (_, voice)] = values else {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
            "auto wait needs channel and voice operands",
        ));
    };
    let Some((channel, slot)) = channel(state, *index)? else {
        return Ok(true);
    };
    if state.effect_channels[&channel].activity() {
        return Ok(false);
    }
    if timed_ready(state, slot, *voice, AUTO_TIMER) {
        return Ok(complete(state, channel, slot, AUTO_TIMER));
    }
    indicators(state, channel, slot, true);
    Ok(false)
}

#[cfg(test)]
mod tests;
