use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ChannelQuadHit {
    pub index: i32,
    pub confirmed: bool,
    pub held: bool,
}

/// Native 466870. Keyboard edges never synthesize a pointer hit.
pub(super) fn query_channel_quads(
    state: &mut CmvsPs2aVmState,
    channel: u8,
) -> Result<ChannelQuadHit, CoreError> {
    let mut result = ChannelQuadHit {
        index: -1,
        confirmed: false,
        held: false,
    };
    let object = state.effect_channels.get(&channel).ok_or_else(|| {
        invalid(
            "SHINKI_EMU_CMVS_VM_EFFECT",
            "live channel has no native object",
        )
    })?;
    if !state.native_pointer_present {
        if !state.input_confirm_held {
            state
                .effect_channels
                .get_mut(&channel)
                .unwrap()
                .write_dword(1264, u32::MAX);
        }
        return Ok(result);
    }
    let [width, height] = state.native_stage_size;
    if width == 0 || height == 0 {
        return Err(invalid(
            "SHINKI_EMU_CMVS_INPUT_GEOMETRY",
            "pointer stage is not bound",
        ));
    }
    let virtual_size = state
        .native_text
        .get(&(object.read_dword(0) as u8))
        .map(|surface| surface.virtual_size)
        .unwrap_or([width, height]);
    let pointer_x = i64::from(state.pointer_x) * i64::from(virtual_size[0]) / i64::from(width);
    let pointer_y = i64::from(state.pointer_y) * i64::from(virtual_size[1]) / i64::from(height);
    let object = state.effect_channels.get_mut(&channel).unwrap();
    if state.input_advance_press {
        object.write_dword(1264, u32::MAX);
    }
    let (origin_x, origin_y) = object.origin();
    for quad in 0..32 {
        if !object.quad_visible(quad) {
            continue;
        }
        // The native hit tester reads unsigned words even though the setter
        // truncates signed operands to int16.
        let [x, y, w, h] = object.quad_rect(quad).map(|value| i32::from(value as u16));
        let left = origin_x.wrapping_add(x);
        let top = origin_y.wrapping_add(y);
        if pointer_x < i64::from(left)
            || pointer_y < i64::from(top)
            || pointer_x > i64::from(left.wrapping_add(w))
            || pointer_y > i64::from(top.wrapping_add(h))
        {
            continue;
        }
        if state.input_advance_press {
            object.write_dword(1264, quad);
        }
        result.index = quad as i32;
        if state.input_advance_release {
            state.input_advance_release = false;
            state.input_advance_press = false;
            if object.read_dword(1264) == quad {
                result.confirmed = true;
            }
        }
        if state.input_confirm_held {
            result.held = true;
        }
    }
    if state.input_advance_release && (object.read_dword(1264) as i32) >= 0 {
        state.input_advance_release = false;
        state.input_advance_press = false;
        object.write_dword(1264, u32::MAX);
    }
    if !state.input_confirm_held {
        object.write_dword(1264, u32::MAX);
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
