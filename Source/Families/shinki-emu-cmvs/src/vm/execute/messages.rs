use super::*;
mod native_format;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::FormatStringIntoSlot,
            [
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, destination),
                (CmvsPs2aCommandStackWordKind::TaggedStringReference, format),
                (CmvsPs2aCommandStackWordKind::OpaqueU32, argument),
            ],
        ) => {
            // `sub_418D70` uses type-before-width directives, not CRT
            // printf. Preserve CP932 bytes, including full-width output.
            let format = resolve_tagged_string_bytes(state, *format)?;
            let mut bytes = native_format::render(&format, *argument, || {
                resolve_tagged_string_bytes(state, *argument)
            })?;
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
                    state
                        .process_string_slots
                        .insert(slot, bytes[..bytes.len() - 1].to_vec());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_zero_padding_uses_type_before_width_and_preserves_variant_suffix() {
        let bytes = native_format::render(b"mask%z3a.pb3", 2, || unreachable!()).unwrap();
        assert_eq!(bytes, b"mask002a.pb3");
    }
}
