use super::dialogue_layout::{DialogueLayoutOptions, DialogueReveal};
use super::*;

/// One active native glyph node (`sub_4646D0` / `sub_462C70`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeTextReveal {
    pub glyph_index: usize,
    pub delay_ms: u32,
    pub elapsed_ms: u32,
    pub effect: u32,
    pub duration_ms: u32,
    pub started: bool,
}

pub(crate) enum Poll {
    Layout {
        channel: u8,
        slot: u8,
        source: String,
    },
    Wait,
    Complete,
}

fn word(state: &CmvsPs2aVmState, offset: u32) -> u32 {
    state.interpreter_words.get(&offset).copied().unwrap_or(0)
}

pub(in crate::vm) fn text_slot(state: &CmvsPs2aVmState, channel: u8) -> Result<u8, CoreError> {
    let slot = state
        .effect_channels
        .get(&channel)
        .map(|object| object.read_dword(0))
        .filter(|slot| *slot < 12)
        .ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_TEXT_OBJECT",
                "channel has no native text owner",
            )
        })? as u8;
    if !state.native_text.contains_key(&slot) {
        return Err(invalid(
            "SHINKI_EMU_CMVS_TEXT_OBJECT",
            "channel text object is not initialized",
        ));
    }
    Ok(slot)
}

/// Native 47E910 resolves the text and checks the read bitmap before the channel guard.
pub(crate) fn poll(
    state: &mut CmvsPs2aVmState,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Poll, CoreError> {
    let [(_, channel), (_, read_flag), (_, source)] = values else {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
            "dialogue needs three operands",
        ));
    };
    let source = if word(state, schedule::SCRIPT_FRAME_COUNTER_FIELD) == 0 {
        Some(text::resolve_string_value(state, *source)?)
    } else {
        // Re-polls resolve the native reference but do not decode/copy text.
        text::resolve_tagged_string_bytes(state, *source)?;
        None
    };
    if (*read_flag as i32) >= 0
        && word(state, 1468) != 0
        && !state.process_flag_bits.contains(read_flag)
        && word(state, 1464) != 0
    {
        state.write_interpreter_word(1464, 0);
    }
    if *channel >= 8 || !state.slot_objects.contains_key(&(742 << 8 | *channel)) {
        raise_error_flag(state, 10532, 0x10000)?;
        return Ok(Poll::Complete);
    }
    let channel = *channel as u8;
    let slot = text_slot(state, channel)?;
    if let Some(source) = source {
        return Ok(Poll::Layout {
            channel,
            slot,
            source,
        });
    }
    if ready(state, channel, slot) {
        Ok(Poll::Complete)
    } else {
        Ok(Poll::Wait)
    }
}

/// No input is consumed when the queue has already completed, or a held/forced
/// mode flushes it. Only the native release-edge branch consumes both groups.
fn ready(state: &mut CmvsPs2aVmState, channel: u8, slot: u8) -> bool {
    if state
        .effect_channels
        .get(&channel)
        .is_some_and(CmvsEffectChannel::activity)
    {
        return false;
    }
    if state
        .native_text_reveals
        .get(&slot)
        .is_none_or(Vec::is_empty)
    {
        return true;
    }
    let forced = word(state, 1464) != 0
        || word(state, 10720) != 0
        || state.input_shift_held
        || state.input_control_held;
    if forced || state.input_advance_release {
        state.native_text_reveals.remove(&slot);
        if !forced {
            state.input_advance_release = false;
            state.input_advance_press = false;
        }
        return true;
    }
    false
}

pub(crate) fn options(state: &CmvsPs2aVmState, slot: u8) -> DialogueLayoutOptions {
    let record = state.effect_playback.get(&slot);
    DialogueLayoutOptions {
        ruby_enabled: record.is_some_and(|record| record.flag != 0),
        ruby_size: record.map_or(12, |record| record.value),
        ruby_spacing: record.map_or(0, |record| record.value2),
        reserve_ruby: record.is_some_and(|record| record.flag != 0)
            && state
                .native_text
                .get(&slot)
                .is_some_and(|surface| surface.reserve_ruby),
        interval_ms: if word(state, 1464) != 0 {
            0
        } else {
            word(state, 1488).wrapping_mul(
                state
                    .native_text
                    .get(&slot)
                    .map_or(30, |surface| surface.glyph_interval_ms),
            ) / 10
        },
    }
}

/// Font-dependent layout is synchronous at the Family boundary. Complete an
/// immediate/empty first poll in this same frame, rather than adding a fake wait.
pub(crate) fn layout_complete(
    state: &mut CmvsPs2aVmState,
    channel: u8,
    slot: u8,
    reveals: Vec<DialogueReveal>,
) -> Result<(), CoreError> {
    let effect = word(state, 1480);
    let duration_ms = match word(state, 1484) {
        0 => 50,
        1 => 200,
        2 => 350,
        3 => 500,
        4 => 650,
        _ => 0,
    };
    let nodes = reveals
        .into_iter()
        .map(|reveal| CmvsNativeTextReveal {
            glyph_index: reveal.glyph_index,
            delay_ms: reveal.delay_ms,
            elapsed_ms: 0,
            effect,
            duration_ms,
            started: false,
        })
        .collect();
    state.native_text_reveals.insert(slot, nodes);
    if ready(state, channel, slot) {
        // execute_command suspended before popping while the renderer shaped.
        stack::drop_stack_bytes(state, 12)?;
        state.program_counter = state.program_counter.checked_add(2).ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_VM_PROGRAM_COUNTER",
                "dialogue continuation overflowed",
            )
        })?;
        state.write_interpreter_word(schedule::SCRIPT_FRAME_COUNTER_FIELD, 0);
        state.wait_boundary_hit = false;
    } else {
        state.write_interpreter_word(13216, u32::from(state.current_frame));
        state.write_interpreter_word(13220, state.program_counter);
    }
    Ok(())
}

/// The native frame driver calls 462C70 after dispatch, before composition.
pub(crate) fn advance(state: &mut CmvsPs2aVmState, delta_ms: u32) {
    for nodes in state.native_text_reveals.values_mut() {
        nodes.retain_mut(|node| {
            let elapsed = if node.delay_ms > delta_ms {
                node.delay_ms -= delta_ms;
                return true;
            } else {
                let remaining = delta_ms - node.delay_ms;
                node.delay_ms = 0;
                remaining
            };
            if elapsed == 0 && node.effect != 0 {
                return true;
            }
            node.started = true;
            node.elapsed_ms = node.elapsed_ms.wrapping_add(elapsed);
            matches!(node.effect, 1 | 2) && node.elapsed_ms < node.duration_ms
        });
    }
}

/// None is an unrevealed glyph; the two factors are opacity and horizontal scale.
pub(crate) fn visual(
    state: &CmvsPs2aVmState,
    slot: u8,
    index: usize,
    size: u32,
) -> Option<[f32; 2]> {
    let Some(nodes) = state.native_text_reveals.get(&slot) else {
        return Some([1.0; 2]);
    };
    let Ok(position) = nodes.binary_search_by_key(&index, |node| node.glyph_index) else {
        return Some([1.0; 2]);
    };
    let node = &nodes[position];
    if !node.started {
        return None;
    }
    match node.effect {
        1 => Some([
            (255 * u64::from(node.elapsed_ms) / u64::from(node.duration_ms)) as f32 / 255.0,
            1.0,
        ]),
        2 => {
            let box_width = u64::from(size) + 4;
            let width = u64::from(node.elapsed_ms) * box_width / u64::from(node.duration_ms);
            if width == 0 {
                None
            } else {
                Some([1.0, width as f32 / box_width as f32])
            }
        }
        _ => Some([1.0; 2]),
    }
}

pub(super) fn validate(state: &CmvsPs2aVmState) -> Result<(), CoreError> {
    for (slot, nodes) in &state.native_text_reveals {
        let surface = state.native_text.get(slot).ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_TEXT_OBJECT",
                "reveal queue has no text owner",
            )
        })?;
        let mut previous = None;
        for node in nodes {
            if node.glyph_index >= surface.glyphs.len()
                || previous.is_some_and(|index| index >= node.glyph_index)
                || node.started
                    && (!matches!(node.effect, 1 | 2) || node.elapsed_ms >= node.duration_ms)
            {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_TEXT_OBJECT",
                    "invalid native glyph reveal queue",
                ));
            }
            previous = Some(node.glyph_index);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
