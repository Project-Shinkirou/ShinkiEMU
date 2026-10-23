use super::*;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::ResolveSpeakerEntry {
                table_dword_index: _,
                max_index,
                error_flag_field_offset,
                error_flag_mask,
                result_field_offset,
                voice0_field_offset,
                voice1_field_offset,
                voice2_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, slot), (CmvsPs2aCommandStackWordKind::TaggedStringReference, _name)],
        ) => {
            // `sub_488DE0` resolves the speaker name through the voice
            // entry table (`sub_463CC0`). The recovered subset has no
            // voice engine, so the three mapping words store defaults.
            if *slot > max_index {
                let flags = state
                    .interpreter_words
                    .get(&error_flag_field_offset)
                    .copied()
                    .unwrap_or(0);
                state
                    .interpreter_words
                    .insert(error_flag_field_offset, flags | error_flag_mask);
                return Ok(None);
            }
            state.interpreter_words.insert(voice0_field_offset, 0);
            state.interpreter_words.insert(voice1_field_offset, 0);
            state.interpreter_words.insert(voice2_field_offset, 0);
            state.interpreter_words.insert(result_field_offset, 1);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::FormatStringIntoSlot,
            [(CmvsPs2aCommandStackWordKind::TaggedStringReference, destination), (CmvsPs2aCommandStackWordKind::TaggedStringReference, format), (CmvsPs2aCommandStackWordKind::OpaqueU32, argument)],
        ) => {
            // `sub_418D70` is the CRT sprintf: the resolved format string
            // consumes the single integer argument and the result lands
            // NUL-terminated in the storage the first word resolves to.
            // Only the recovered one-integer directive subset is handled;
            // a format without a directive copies through verbatim.
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.sprintf_probe",
                    pc = state.program_counter,
                    frame = state.current_frame,
                    destination,
                    format,
                    argument
                );
            }
            let format = resolve_string_value(state, *format)?;
            // The native `sub_418D70` honours `%s` by treating the single
            // integer argument as a character pointer; in the recovered VM
            // the same word is a tagged string reference. A `%s` directive
            // therefore substitutes the resolved string and the remaining
            // format text copies through verbatim.
            let formatted = if let Some(percent) = format.find('%') {
                if format[percent..].starts_with("%s") {
                    let stem = resolve_string_value(state, *argument)?;
                    format[..percent].to_string() + &stem + &format[percent + 2..]
                } else {
                    format_sprintf_single_int(&format, *argument as i32)
                }
            } else {
                format_sprintf_single_int(&format, *argument as i32)
            };
            let mut bytes = formatted.into_bytes();
            bytes.push(0);
            match destination & 0xc000_0000 {
                0 => {
                    let offset = (destination & 0x3fff_ffff) as usize;
                    let pool = state
                        .frame_string_pool_bytes
                        .entry(state.current_frame)
                        .or_default();
                    if pool.len() < offset + bytes.len() {
                        pool.resize(offset + bytes.len(), 0);
                    }
                    pool[offset..offset + bytes.len()].copy_from_slice(&bytes);
                }
                0x8000_0000 => {
                    let slot = destination & 0x3fff_ffff;
                    if slot >= MAX_PROCESS_STRING_SLOTS {
                        return Err(invalid(
                            "SHINKI_EMU_CMVS_VM_STRING_TAG",
                            "formatted destination slot is outside the native table",
                        ));
                    }
                    validate_native_string_slot(&bytes[..bytes.len() - 1])?;
                    state.process_string_slots.insert(slot, bytes[..bytes.len() - 1].to_vec());
                }
                _ => {
                    return Err(invalid(
                        "SHINKI_EMU_CMVS_VM_STRING_TAG",
                        "formatted destination tag is outside the supported native domain",
                    ));
                }
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

/// The recovered one-integer subset of the CRT `sprintf` behind
/// `sub_418D70`: the first `%[flags][width]d/u/x/X` directive consumes the
/// integer; `%%` passes through; a format with no directive copies
/// verbatim, exactly like the original call with an unused argument.
fn format_sprintf_single_int(format: &str, value: i32) -> String {
    let mut out = String::with_capacity(format.len() + 8);
    let mut rest = format;
    let mut consumed = false;
    while let Some(percent) = rest.find('%') {
        let (head, tail) = rest.split_at(percent);
        out.push_str(head);
        let spec_end = tail[1..]
            .find(|c: char| !matches!(c, '-' | '0'..='9' | '+' | ' ' | '#'))
            .map(|index| index + 1)
            .unwrap_or(tail.len());
        let (spec, remainder) = tail.split_at(spec_end);
        let Some(directive) = remainder.chars().next() else {
            // Trailing bare percent: copy verbatim like the CRT.
            out.push_str(tail);
            return out;
        };
        let flags_width = &spec[1..];
        if directive == '%' {
            out.push('%');
        } else if !consumed && matches!(directive, 'd' | 'i' | 'u' | 'x' | 'X') {
            consumed = true;
            let width: usize = flags_width
                .trim_matches(|c: char| !c.is_ascii_digit())
                .parse()
                .unwrap_or(0);
            let formatted = match directive {
                'u' => format!("{:0>width$}", value as u32),
                'x' => format!("{:0>width$x}", value as u32),
                'X' => format!("{:0>width$X}", value as u32),
                _ => format!("{:0>width$}", value),
            };
            out.push_str(&formatted);
        } else {
            out.push_str(spec);
            out.push(directive);
        }
        rest = &remainder[directive.len_utf8()..];
    }
    out.push_str(rest);
    out
}
