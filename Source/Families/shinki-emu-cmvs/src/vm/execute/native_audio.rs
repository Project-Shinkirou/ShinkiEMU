use super::*;
use crate::vm::{
    CmvsNativeAudioAction as Action, CmvsNativeAudioControl as Control,
    CmvsNativeAudioQuery as Query,
};

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    command_id: u16,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    let word = |index: usize| {
        values.get(index).map(|(_, value)| *value).ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_CONTRACT",
                "native audio operand is missing",
            )
        })
    };
    let selector = word(0)?;
    //4844C0 and489E20 resolve470D50 before checking their selector.
    let name = if matches!(command_id, 175 | 186 | 576 | 584) {
        Some(resolve_string_value(state, word(1)?)?)
    } else {
        None
    };
    if matches!(command_id, 576..=580 | 584..=587) {
        let fixed = command_id <= 580;
        if if fixed { selector != 0 } else { selector >= 24 } {
            raise_error_flag(state, 10532, 0x200)?;
            return Ok(None);
        }
        let action = match command_id {
            576 => Action::LoadFixedStream {
                name: name.expect("resolved fixed stream source"),
                volume: word(2)?,
                fade_in_ms: word(3)?,
                looping: word(4)? != 0,
                start: word(5)? != 0,
            },
            // Bus39 represents native stream15; bus15 is static0.
            577 => Action::Control {
                channel: 39,
                control: Control::Dispose,
            },
            578 => Action::Query {
                channel: 39,
                query: Query::Position,
            },
            579 => Action::Query {
                channel: 39,
                query: Query::Levels,
            },
            580 => Action::Control {
                channel: 39,
                control: Control::Volume(word(1)?),
            },
            584 => Action::LoadStatic {
                slot: selector as u8,
                name: name.expect("resolved static source"),
            },
            585 => Action::FreeStatic {
                slot: selector as u8,
            },
            586 => {
                if state.interpreter_words.get(&1556).copied().unwrap_or(1) == 0 {
                    return Ok(None);
                }
                Action::PlayStatic {
                    slot: selector as u8,
                    volume: word(1)?,
                    looping: word(2)? != 0,
                }
            }
            587 => Action::StopStatic {
                slot: selector as u8,
            },
            _ => unreachable!(),
        };
        return Ok(Some(CmvsPs2aVmAction::NativeAudio(action)));
    }
    let resource = matches!(command_id, 177..=186);
    if selector >= if resource { 6 } else { 4 } {
        raise_error_flag(state, 10532, if resource { 0x200 } else { 0x400 })?;
        return Ok(None);
    }
    let channel = selector as u8 + if resource { 9 } else { 5 };
    let action = match command_id {
        175 => {
            if state.interpreter_words.get(&1552).copied().unwrap_or(1) == 0 {
                state.write_interpreter_word(81220, 0);
                return Ok(None);
            }
            Action::LoadVoice {
                channel,
                name: name.expect("resolved voice source"),
                volume: word(2)?,
                looping: word(3)? != 0,
                rate_bits: word(4)?,
            }
        }
        186 => {
            let volume = word(2)?;
            let fade_in_ms = word(3)?;
            let looping = word(4)? != 0;
            let start = word(5)? != 0;
            state.resource_channel_slots.insert(
                selector,
                CmvsResourceChannelSlotRecord {
                    source: name.expect("resolved resource source"),
                    volume,
                    loop_playback: looping,
                    start_playback: start,
                },
            );
            for (offset, value) in [
                (32, u32::from(looping)),
                (36, volume),
                (40, 256),
                (44, 0),
                (48, u32::from(start)),
            ] {
                state.write_interpreter_word(1608 + selector * 52 + offset, value);
            }
            return Ok(
                (state.interpreter_words.get(&1556).copied().unwrap_or(1) != 0).then_some(
                    CmvsPs2aVmAction::ConfigureResourceAudio {
                        slot: selector as u8,
                        fade_in_ms,
                    },
                ),
            );
        }
        170 | 183 => Action::Query {
            channel,
            query: Query::Position,
        },
        171 => Action::Query {
            channel,
            query: Query::DecodedBytes,
        },
        174 => Action::Query {
            channel,
            query: Query::Levels,
        },
        167 | 177 | 180 => {
            let looping = word(1)? != 0;
            if resource {
                ensure_record(state, selector);
                state.write_interpreter_word(1640 + selector * 52, u32::from(looping));
                state.write_interpreter_word(1656 + selector * 52, 1);
                if state.interpreter_words.get(&1556).copied().unwrap_or(1) == 0 {
                    return Ok(None);
                }
            }
            Action::Control {
                channel,
                control: if command_id == 180 {
                    Control::Restart { looping }
                } else {
                    Control::Start { looping }
                },
            }
        }
        168 | 179 | 188 => {
            if resource {
                ensure_record(state, selector).source.clear();
                state.write_interpreter_word(1656 + selector * 52, 0);
            }
            Action::Control {
                channel,
                control: Control::Dispose,
            }
        }
        169 | 182 => {
            if resource {
                ensure_record(state, selector).source.clear();
            }
            Action::Control {
                channel,
                control: Control::FadeOut {
                    duration_ms: word(1)?,
                },
            }
        }
        172 | 184 => {
            let volume = word(1)?;
            if resource {
                ensure_record(state, selector);
                state.write_interpreter_word(1648 + selector * 52, volume);
            }
            Action::Control {
                channel,
                control: Control::Volume(volume),
            }
        }
        173 | 185 => {
            let pan = word(1)? as i32;
            if resource {
                ensure_record(state, selector);
                state.write_interpreter_word(1652 + selector * 52, pan as u32);
            }
            Action::Control {
                channel,
                control: Control::Pan(pan),
            }
        }
        178 => {
            ensure_record(state, selector);
            state.write_interpreter_word(1656 + selector * 52, 0);
            Action::Control {
                channel,
                control: Control::Pause,
            }
        }
        _ => {
            return Err(invalid(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_CONTRACT",
                "unrecognized native audio command",
            ));
        }
    };
    Ok(Some(CmvsPs2aVmAction::NativeAudio(action)))
}

fn ensure_record(state: &mut CmvsPs2aVmState, slot: u32) -> &mut CmvsResourceChannelSlotRecord {
    state
        .resource_channel_slots
        .entry(slot)
        .or_insert_with(|| CmvsResourceChannelSlotRecord {
            source: String::new(),
            volume: 0,
            loop_playback: false,
            start_playback: false,
        })
}
