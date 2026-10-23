use super::*;
use std::borrow::Cow;

#[cfg(test)]
mod tests;

pub(super) fn tag_zero_reference(value: u32) -> Result<CmvsPs2aPrivateStringReference, CoreError> {
    // This entry point accepts only the active frame's tag-zero pool.
    // Commands with other recovered domains use their explicit resolver.
    if value & 0xc000_0000 != 0 {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_STRING_TAG",
            "CMVS PS2A string reference uses an unrecovered tag domain",
        ));
    }
    Ok(CmvsPs2aPrivateStringReference {
        relative_offset: value & 0x3fff_ffff,
    })
}

/// Native global slots own bytes; resolving never follows mutable aliases.
pub(super) fn resolve_tagged_string_bytes(
    state: &CmvsPs2aVmState,
    word: u32,
) -> Result<Cow<'_, [u8]>, CoreError> {
    match word & 0xc000_0000 {
        0 => {
            // `sub_470D50` tag 0 reads `poolA_base[current slot] + offset`
            // with no string-start or bounds validation: arbitrary offsets
            // stay readable, NUL terminates, and reads past the pool yield
            // nothing. There is no cross-pool or writer-pool fallback.
            let offset = (word & 0x3fff_ffff) as usize;
            let pool = state
                .frame_string_pool_bytes
                .get(&state.current_frame)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let bytes: &[u8] = pool.get(offset..).unwrap_or(&[]);
            let end = bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(bytes.len());
            Ok(Cow::Owned(bytes[..end].to_vec()))
        }
        0x4000_0000 => data_segment_string(state, word & 0x3fff_ffff).map(Cow::Owned),
        0x8000_0000 if word & 0x3fff_ffff < MAX_PROCESS_STRING_SLOTS => Ok(Cow::Borrowed(
            state
                .process_string_slots
                .get(&(word & 0x3fff_ffff))
                .map(Vec::as_slice)
                .unwrap_or_default(),
        )),
        _ => Err(invalid(
            "SHINKI_EMU_CMVS_VM_STRING_TAG",
            "string tag or slot is outside the supported native domain",
        )),
    }
}

pub(super) fn validate_native_string_slot(bytes: &[u8]) -> Result<(), CoreError> {
    if bytes.len() >= 1024 || bytes.contains(&0) {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_STRING_BOUND",
            "string exceeds native slot capacity or contains NUL",
        ));
    }
    let (_, malformed) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes);
    if malformed {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_STRING_ENCODING",
            "native string contains invalid Shift-JIS",
        ));
    }
    Ok(())
}

pub(crate) fn resolve_resource_name(
    state: &CmvsPs2aVmState,
    value: u32,
) -> Result<String, CoreError> {
    let text = resolve_string_value(state, value)?;
    if text.is_empty() {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_SCRIPT_NAME",
            "resource name is empty",
        ));
    }
    Ok(text)
}

pub(super) fn resolve_string_value(
    state: &CmvsPs2aVmState,
    value: u32,
) -> Result<String, CoreError> {
    let bytes = resolve_tagged_string_bytes(state, value)?;
    let (text, malformed) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(&bytes);
    if malformed || bytes.contains(&0) {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_STRING_ENCODING",
            "native string contains invalid Shift-JIS",
        ));
    }
    Ok(text.into_owned())
}

/// Resolves the `lstrlenA` byte length of one tagged string the way
/// `sub_470D50` dispatches the four tag domains. Tag 0x00 keys the loaded
/// length tables: the active frame's pool wins, every other loaded frame is
/// still readable and pools a root reload retired stay readable through the
/// retired length tables, mirroring the native cross-frame search.
pub(super) fn resolve_tagged_string_length(
    state: &CmvsPs2aVmState,
    word: u32,
) -> Result<u32, CoreError> {
    match word & 0xc000_0000 {
        0 => pool_string_length(state, word & 0x3fff_ffff).ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_VM_STRING_LENGTH",
                "string length is not bound",
            )
        }),
        0x4000_0000 | 0x8000_0000 => Ok(resolve_tagged_string_bytes(state, word)?.len() as u32),
        _ => Err(invalid(
            "SHINKI_EMU_CMVS_VM_STRING_TAG",
            "external string pool is not bound",
        )),
    }
}

/// Resolves one pool-relative string reference to its byte length the way
/// the original's raw pointer read does: an offset that names a pool entry
/// start uses the recorded length, and an offset inside an entry yields the
/// suffix up to the entry's terminator. The active frame's pool wins; every
/// other loaded frame and each pool a root reload retired stay readable.
fn pool_string_length(state: &CmvsPs2aVmState, offset: u32) -> Option<u32> {
    let active = state.frame_string_lengths.get(&state.current_frame)?;
    if let Some(length) = active.get(&offset) {
        return Some(*length);
    }
    let mut suffix = None;
    for (&start, &length) in active.iter() {
        if start < offset && offset < start + length {
            suffix = Some(length - (offset - start));
            break;
        }
    }
    if let Some(suffix) = suffix {
        return Some(suffix);
    }
    for (frame, table) in &state.frame_string_lengths {
        if *frame == state.current_frame {
            continue;
        }
        if let Some(length) = table.get(&offset) {
            return Some(*length);
        }
        for (&start, &length) in table.iter() {
            if start < offset && offset < start + length {
                return Some(length - (offset - start));
            }
        }
    }
    for table in &state.retired_string_lengths {
        if let Some(length) = table.get(&offset) {
            return Some(*length);
        }
        for (&start, &length) in table.iter() {
            if start < offset && offset < start + length {
                return Some(length - (offset - start));
            }
        }
    }
    None
}

fn data_segment_string(state: &CmvsPs2aVmState, start: u32) -> Result<Vec<u8>, CoreError> {
    let size = state
        .script_data_segment_sizes
        .get(&state.current_frame)
        .copied()
        .ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_VM_STRING_POOL",
                "script data segment is not bound",
            )
        })?;
    if start >= size || size as usize > MAX_SCRIPT_BYTES {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_STRING_BOUND",
            "data string offset is outside the segment",
        ));
    }
    let complete = size & !3;
    let words = state.script_data_segment_words.get(&state.current_frame);
    let tail = state.script_data_segment_tail.get(&state.current_frame);
    let mut bytes = Vec::new();
    for offset in start..size {
        let byte = if offset < complete {
            let word = words
                .and_then(|words| words.get(&(offset & !3)))
                .copied()
                .unwrap_or(0);
            word.to_le_bytes()[(offset & 3) as usize]
        } else {
            *tail
                .and_then(|tail| tail.get((offset - complete) as usize))
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_STRING_POOL",
                        "script data tail is not bound",
                    )
                })?
        };
        if byte == 0 {
            return Ok(bytes);
        }
        bytes.push(byte);
    }
    Err(invalid(
        "SHINKI_EMU_CMVS_VM_STRING_BOUND",
        "data string has no terminator inside its segment",
    ))
}
