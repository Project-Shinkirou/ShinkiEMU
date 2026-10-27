use super::*;

/// Native `459BE0`, owned at game byte3256. DWORD5 pauses both the
/// producer and `459C70`'s offset consumer; it is not a screenshot flag.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsScreenShakeState {
    pub construction_words: [u32; 4],
    pub paused: bool,
    pub completed_periods: u32,
    pub phase: u32,
    pub elapsed_ms: u32,
    pub offset: [i32; 2],
}

fn arithmetic() -> CoreError {
    invalid(
        "SHINKI_EMU_CMVS_SCREEN_SHAKE_ARITHMETIC",
        "native screen shake division is invalid",
    )
}

fn signed_div(numerator: u32, denominator: u32) -> Result<i32, CoreError> {
    (numerator as i32)
        .checked_div(denominator as i32)
        .ok_or_else(arithmetic)
}

fn truncate_native(value: f64) -> i32 {
    // `419AD0` uses CVTTSD2SI, including its indefinite overflow result.
    if !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        i32::MIN
    } else {
        value as i32
    }
}

fn completed(owner: &CmvsScreenShakeState, steps: u32) -> bool {
    let limit = owner.construction_words[3];
    limit != u32::MAX
        && steps != 0
        && (u64::from(owner.completed_periods) + u64::from(steps) - 1).min(u64::from(u32::MAX))
            > u64::from(limit)
}

fn random(state: &mut u32) -> u32 {
    // Shared MSVCRT rand stream, identical to native command212.
    *state = state.wrapping_mul(214_013).wrapping_add(2_531_011);
    (*state >> 16) & 0x7fff
}

/// `425D70` calls `459D10` once with the native frame delta before drawing.
/// A completed owner is destroyed before `425F40` obtains its global offset.
pub(crate) fn advance(state: &mut CmvsPs2aVmState) -> Result<(), CoreError> {
    let Some(owner) = state.screen_shake.as_mut() else {
        return Ok(());
    };
    if owner.paused {
        return Ok(());
    }
    owner.elapsed_ms = owner.elapsed_ms.wrapping_add(state.native_frame_delta_ms);
    let [flags, amplitude, period, repeats] = owner.construction_words;
    let mut done = false;
    match flags & 0xf00 {
        0 => {
            if period == 0 {
                return Err(arithmetic());
            }
            let steps = owner.elapsed_ms / period;
            if steps != 0 {
                done = completed(owner, steps);
                owner.elapsed_ms %= period;
                let amplitude = amplitude as i32;
                match flags & 0xf0 {
                    axis @ (0 | 0x10) => {
                        let axis = usize::from(axis != 0);
                        let current = owner.offset[axis];
                        owner.offset[axis] = if current == 0 {
                            if steps & 1 != 0 {
                                amplitude
                            } else {
                                amplitude.wrapping_neg()
                            }
                        } else if steps & 1 != 0 {
                            current.wrapping_neg()
                        } else {
                            current
                        };
                    }
                    0x20 => {
                        let bound = amplitude as u32;
                        if bound == 0 {
                            return Err(arithmetic());
                        }
                        for _ in 0..steps {
                            owner.offset = std::array::from_fn(|_| {
                                (2_u32
                                    .wrapping_mul(random(&mut state.prng_state) % bound)
                                    .wrapping_sub(bound)) as i32
                            });
                        }
                    }
                    0x30 => {
                        let phase = owner.completed_periods.wrapping_add(steps - 1) & 3;
                        owner.offset = match phase {
                            0 => [amplitude.wrapping_neg(), amplitude.wrapping_neg()],
                            1 => [amplitude, amplitude],
                            2 => [amplitude, amplitude.wrapping_neg()],
                            _ => [amplitude.wrapping_neg(), amplitude],
                        };
                    }
                    _ => {}
                }
                owner.completed_periods = owner.completed_periods.wrapping_add(steps);
            }
        }
        0x100 => {
            if period == 0 {
                return Err(arithmetic());
            }
            let steps = owner.elapsed_ms / period;
            if steps != 0 {
                done = completed(owner, steps);
                owner.elapsed_ms %= period;
                owner.completed_periods = owner.completed_periods.wrapping_add(steps);
                owner.phase = (owner.phase.wrapping_sub(steps)) & 1;
            }
            let axis = match flags & 0xf0 {
                0 => Some(0),
                0x10 => Some(1),
                _ => None,
            };
            if let Some(axis) = axis {
                let ramp = signed_div(
                    2_u32.wrapping_mul(amplitude).wrapping_mul(owner.elapsed_ms),
                    period,
                )?;
                let amplitude = amplitude as i32;
                let value = if owner.phase != 0 {
                    if ramp >= amplitude {
                        amplitude.wrapping_mul(2).wrapping_sub(ramp)
                    } else {
                        ramp
                    }
                } else if ramp < amplitude {
                    ramp.wrapping_neg()
                } else {
                    ramp.wrapping_sub(amplitude.wrapping_mul(2))
                };
                owner.offset[axis] = value;
                owner.offset[1 - axis] = 0;
            }
        }
        0x200 => {
            let value = if owner.elapsed_ms >= period {
                owner.completed_periods = repeats;
                done = true;
                0
            } else {
                // Unsigned native multiply/divide; 419AD0 converts its
                // integral angle/amplitude to f64 and truncates cos to i32.
                let angle = 360_u32.wrapping_mul(owner.elapsed_ms).wrapping_mul(repeats) / period;
                owner.completed_periods = angle / 360;
                let degrees = (angle.wrapping_add(90) % 360) as f64;
                let x = truncate_native(
                    (degrees / 180.0 * std::f64::consts::PI).cos() * f64::from(amplitude as i32),
                );
                signed_div((x as u32).wrapping_mul(period - owner.elapsed_ms), period)?
            };
            match flags & 0xf0 {
                0 => owner.offset = [value, 0],
                0x10 => owner.offset = [0, value],
                _ => {}
            }
        }
        _ => {}
    }
    if done {
        state.screen_shake = None;
        state.write_interpreter_word(3256, 0);
    }
    Ok(())
}

/// `459C70`: the low nibble selects which native queue receives the offset.
/// `4454D0` additionally gates this result with the inherited header bit0x100.
pub(crate) fn offset(state: &CmvsPs2aVmState, queue_identity: u32) -> [f32; 2] {
    let Some(owner) = &state.screen_shake else {
        return [0.0; 2];
    };
    let excluded = if queue_identity == 0 { 2 } else { 1 };
    if owner.paused || owner.construction_words[0] & 0xf == excluded {
        [0.0; 2]
    } else {
        owner.offset.map(|value| value as f32)
    }
}

/// Basic/text commands are expressed in BASE-width units. Native `a3` is in MODE pixels.
pub(crate) fn logical_offset(
    state: &CmvsPs2aVmState,
    stage_width: u32,
    queue_identity: u32,
) -> Result<[f32; 2], CoreError> {
    let value = offset(state, queue_identity);
    if value == [0.0; 2] {
        return Ok(value);
    }
    let render_width = state.native_render_size[0];
    if stage_width == 0 || render_width == 0 {
        return Err(invalid(
            "SHINKI_EMU_CMVS_DISPLAY_UNBOUND",
            "screen shake requires a native MODE width",
        ));
    }
    let scale = stage_width as f32 / render_width as f32;
    Ok(value.map(|value| value * scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_frame_clock_drives_pause_period_count_and_owner_destruction() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.screen_shake = Some(CmvsScreenShakeState {
            construction_words: [0x30, 3, 10, 1],
            ..Default::default()
        });
        vm.native_frame_delta_ms = 10;
        advance(&mut vm).unwrap();
        assert_eq!(offset(&vm, 0), [-3.0, -3.0]);
        assert_eq!(vm.screen_shake.as_ref().unwrap().completed_periods, 1);
        vm.screen_shake.as_mut().unwrap().paused = true;
        advance(&mut vm).unwrap();
        assert_eq!(offset(&vm, 0), [0.0, 0.0]);
        assert_eq!(vm.screen_shake.as_ref().unwrap().completed_periods, 1);
        vm.screen_shake.as_mut().unwrap().paused = false;
        advance(&mut vm).unwrap();
        assert_eq!(offset(&vm, 0), [3.0, 3.0]);
        advance(&mut vm).unwrap();
        assert!(vm.screen_shake.is_none());
    }

    #[test]
    fn random_shake_uses_the_existing_native_crt_stream_and_queue_selector() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.screen_shake = Some(CmvsScreenShakeState {
            construction_words: [0x21, 4, 10, u32::MAX],
            ..Default::default()
        });
        vm.native_frame_delta_ms = 10;
        advance(&mut vm).unwrap();
        assert_eq!(offset(&vm, 0), [-2.0, 2.0]);
        assert_eq!(offset(&vm, 1), [0.0, 0.0]);
        assert_eq!(vm.prng_state, 3_357_800_067);
    }

    #[test]
    fn linear_and_damped_native_shake_keep_integral_projection_and_pause() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.screen_shake = Some(CmvsScreenShakeState {
            construction_words: [0x110, 8, 16, u32::MAX],
            ..Default::default()
        });
        vm.native_frame_delta_ms = 4;
        advance(&mut vm).unwrap();
        assert_eq!(offset(&vm, 0), [0.0, -4.0]);
        vm.native_frame_delta_ms = 12;
        advance(&mut vm).unwrap();
        assert_eq!(offset(&vm, 0), [0.0, 0.0]);
        vm.native_frame_delta_ms = 4;
        advance(&mut vm).unwrap();
        assert_eq!(offset(&vm, 0), [0.0, 4.0]);
        vm.screen_shake = Some(CmvsScreenShakeState {
            construction_words: [0x200, 8, 16, 1],
            ..Default::default()
        });
        advance(&mut vm).unwrap();
        assert_eq!(offset(&vm, 0), [-6.0, 0.0]);
        vm.native_frame_delta_ms = 12;
        advance(&mut vm).unwrap();
        assert!(vm.screen_shake.is_none());
    }
}
