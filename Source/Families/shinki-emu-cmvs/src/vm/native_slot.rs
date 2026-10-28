//! 4782F0 installs the saved decoded root program, then475E40 overlays A.
use super::*;
use crate::system_save::slot::NativeInterpreter;
use crate::system_save::slot_io::{Reader, invalid as slot_error};
use crate::{CmvsScript, Ps2aHeader};

const SCALAR_FIELDS: [(u16, u32); 10] = [
    (274, 13200),
    (275, 13204),
    (276, 13208),
    (277, 13212),
    (262, 13216),
    (263, 13220),
    (264, 13224),
    (266, 15064),
    (513, 9652),
    (514, 10568),
];
const STRING_FIELDS: [u16; 6] = [10324, 12816, 13072, 13104, 13136, 13168];

/// 4712F0 stamps the slot with `GetLocalTime`. The local offset comes from
/// the platform zone database, which stays readable in multithreaded hosts.
pub(crate) fn native_save_local_timestamp() -> Result<[u16; 6], CoreError> {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    let year =
        u16::try_from(now.year()).map_err(|_| slot_error("native save year is out of range"))?;
    Ok([
        year,
        now.month() as u16,
        now.day() as u16,
        now.hour() as u16,
        now.minute() as u16,
        now.second() as u16,
    ])
}

fn timestamp_text(timestamp: [u16; 6]) -> Result<String, CoreError> {
    let [year, month, day, hour, minute, second] = timestamp;
    let month_value = time::Month::try_from(
        u8::try_from(month).map_err(|_| slot_error("native save month is invalid"))?,
    )
    .map_err(|_| slot_error("native save month is invalid"))?;
    time::Date::from_calendar_date(
        i32::from(year),
        month_value,
        u8::try_from(day).map_err(|_| slot_error("native save day is invalid"))?,
    )
    .map_err(|_| slot_error("native save date is invalid"))?;
    if year > 9999 || hour > 23 || minute > 59 || second > 59 {
        return Err(slot_error("native save timestamp is invalid"));
    }
    Ok(format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} "
    ))
}

pub(crate) fn capture_native_slot_header(vm: &CmvsPs2aVmState) -> Result<Vec<u8>, CoreError> {
    capture_native_slot_header_at(vm, native_save_local_timestamp()?)
}

/// 4712F0 captures the title and metadata; 4774D0 writes the fixed file version.
pub(crate) fn capture_native_slot_header_at(
    vm: &CmvsPs2aVmState,
    timestamp: [u16; 6],
) -> Result<Vec<u8>, CoreError> {
    let date = timestamp_text(timestamp)?;
    let caption = capture_string(vm, 10324)?;
    if date.len() + caption.len() >= 256 || caption.contains(&0) {
        return Err(slot_error(
            "native timestamp and save title exceed the 256-byte header field",
        ));
    }
    let mut header = vec![0; 600];
    header[..4].copy_from_slice(b"CSV2");
    header[4..8].copy_from_slice(&600u32.to_le_bytes());
    let word = |offset| vm.interpreter_words.get(&offset).copied().unwrap_or(0);
    header[8..12].copy_from_slice(&word(10556).to_le_bytes());
    header[16..16 + date.len()].copy_from_slice(date.as_bytes());
    header[16 + date.len()..16 + date.len() + caption.len()].copy_from_slice(&caption);
    for (offset, field) in [(528, 10312), (532, 10316), (536, 10320)] {
        header[offset..offset + 4].copy_from_slice(&word(field).to_le_bytes());
    }
    header[544..548].copy_from_slice(&[4, 0, 1, 0]);
    header[548..550].copy_from_slice(&(word(10292) as u16).to_le_bytes());
    Ok(header)
}

pub(crate) fn refresh_native_slot_header_time(
    header: &mut [u8],
    timestamp: [u16; 6],
) -> Result<(), CoreError> {
    if header.len() != 600 {
        return Err(slot_error("native save header has an invalid length"));
    }
    let date = timestamp_text(timestamp)?;
    // 4774D0 refreshes digits only, retaining the captured title/separators.
    for range in [0..4, 5..7, 8..10, 11..13, 14..16, 17..19] {
        header[16 + range.start..16 + range.end].copy_from_slice(&date.as_bytes()[range]);
    }
    Ok(())
}

/// C is already decoded PS2A, including its48-byte header; it is not a PS3 file.
pub(crate) fn native_program_header(bytes: &[u8]) -> Result<Ps2aHeader, CoreError> {
    if bytes.len() < 48 || bytes.len() > MAX_SCRIPT_BYTES || &bytes[..4] != b"PS2A" {
        return Err(slot_error("native C is not a bounded decoded PS2A program"));
    }
    let mut r = Reader::new(bytes);
    r.bytes(4)?;
    let header_length = r.u32()?;
    r.u32()?;
    let program_key = r.u32()?;
    let name_index_count = r.u32()?;
    let bytecode_size = r.u32()?;
    let metadata_size = r.u32()?;
    let name_index_size = r.u32()?;
    let initial_pc = r.u32()?;
    let compressed_size = r.u32()?;
    let uncompressed_size = r.u32()?;
    r.u32()?;
    let code_start = (header_length as usize)
        .checked_add(
            (name_index_count as usize)
                .checked_mul(4)
                .ok_or_else(|| slot_error("native program index overflowed"))?,
        )
        .ok_or_else(|| slot_error("native program layout overflowed"))?;
    let pool_start = code_start
        .checked_add(bytecode_size as usize)
        .and_then(|n| n.checked_add(metadata_size as usize))
        .ok_or_else(|| slot_error("native program layout overflowed"))?;
    if header_length != 48
        || code_start > bytes.len()
        || pool_start > bytes.len()
        || bytecode_size == 0
        || initial_pc >= bytecode_size
        || !initial_pc.is_multiple_of(2)
        || uncompressed_size as usize != bytes.len() - 48
    {
        return Err(slot_error("native decoded program sections are invalid"));
    }
    Ok(Ps2aHeader {
        header_length,
        program_key,
        name_index_count,
        bytecode_size,
        metadata_size,
        name_index_size,
        initial_pc,
        compressed_size,
        uncompressed_size,
    })
}

pub(crate) fn decode_native_program(bytes: &[u8]) -> Result<CmvsScript, CoreError> {
    crate::ps2a::parse_decoded_ps2a(bytes, native_program_header(bytes)?)
}

fn fill_data(vm: &CmvsPs2aVmState, frame: u16, bytes: &mut [u8]) -> Result<(), CoreError> {
    let size = bytes.len();
    if size > MAX_SCRIPT_BYTES
        || vm.script_data_segment_sizes.get(&frame).copied() != Some(size as u32)
    {
        return Err(slot_error(
            "native save has no matching runtime script data segment",
        ));
    }
    if let Some(words) = vm.script_data_segment_words.get(&frame) {
        for (&offset, &word) in words {
            let end = (offset as usize)
                .checked_add(4)
                .ok_or_else(|| slot_error("native script word overflowed"))?;
            if !offset.is_multiple_of(4) || end > size - size % 4 {
                return Err(slot_error("native script word is outside its segment"));
            }
            bytes[offset as usize..end].copy_from_slice(&word.to_le_bytes());
        }
    }
    let tail = vm
        .script_data_segment_tail
        .get(&frame)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if tail.len() != size % 4 {
        return Err(slot_error("native script data tail has an invalid length"));
    }
    bytes[size - tail.len()..].copy_from_slice(tail);
    Ok(())
}

fn capture_data(vm: &CmvsPs2aVmState, frame: u16, size: usize) -> Result<Vec<u8>, CoreError> {
    if size > MAX_SCRIPT_BYTES {
        return Err(slot_error("native script data exceeds its byte limit"));
    }
    let mut bytes = vec![0; size];
    fill_data(vm, frame, &mut bytes)?;
    Ok(bytes)
}

pub(crate) fn capture_native_program(
    script: &CmvsScript,
    vm: &CmvsPs2aVmState,
) -> Result<Vec<u8>, CoreError> {
    let data_size = script.data_segment.len();
    let h = script.header;
    let pool = vm
        .frame_string_pool_bytes
        .get(&0)
        .map(Vec::as_slice)
        .ok_or_else(|| slot_error("native root string pool is not installed"))?;
    if pool.len() != script.raw_string_pool().len() {
        return Err(slot_error(
            "native root string pool changed its allocated extent",
        ));
    }
    let size = 48usize
        .checked_add(script.name_index.len() * 4)
        .and_then(|n| n.checked_add(script.bytecode.len()))
        .and_then(|n| n.checked_add(data_size))
        .and_then(|n| n.checked_add(pool.len()))
        .ok_or_else(|| slot_error("native program capture overflowed"))?;
    if size > MAX_SCRIPT_BYTES
        || h.header_length != 48
        || h.name_index_count as usize != script.name_index.len()
        || h.bytecode_size as usize != script.bytecode.len()
        || h.metadata_size as usize != data_size
    {
        return Err(slot_error("native root program has inconsistent sections"));
    }
    let mut bytes = Vec::with_capacity(size);
    bytes.extend_from_slice(&script.raw_header);
    bytes[40..44].copy_from_slice(&((size - 48) as u32).to_le_bytes());
    for word in &script.name_index {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(&script.bytecode);
    let data_start = bytes.len();
    bytes.resize(data_start + data_size, 0);
    fill_data(vm, 0, &mut bytes[data_start..])?;
    bytes.extend_from_slice(pool);
    Ok(bytes)
}

/// All parsing and URI validation completes before any live VM field changes.
pub(crate) fn install_native_program(
    bytes: &[u8],
    uri: &str,
    vm: &mut CmvsPs2aVmState,
) -> Result<CmvsScript, CoreError> {
    let script = decode_native_program(bytes)?;
    // install_script_frame validates the URI before publishing any state.
    vm.install_script_frame(0, uri, roukaku_core::Hash256::from_sha256(bytes), &script)?;
    vm.current_frame = 0;
    vm.stack_cursor_bytes = 0;
    vm.call_frame_bases.clear();
    vm.call_frame_bases.push(0);
    vm.write_interpreter_word(13200, bytes.len() as u32);
    vm.write_interpreter_word(13216, 0);
    vm.write_interpreter_word(13220, script.header.initial_pc);
    vm.write_interpreter_word(80948, 0);
    vm.write_interpreter_word(81208, 0);
    vm.write_interpreter_word(80952, 0);
    Ok(script)
}

fn capture_string(vm: &CmvsPs2aVmState, field: u16) -> Result<Vec<u8>, CoreError> {
    let Some(segments) = vm.interpreter_string_buffers.get(&field) else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for segment in segments {
        match segment {
            CmvsStringSegment::OwnedString(text) => {
                let (bytes, _, malformed) = encoding_rs::SHIFT_JIS.encode(text);
                if malformed {
                    return Err(slot_error(
                        "native interpreter string cannot encode as Shift-JIS",
                    ));
                }
                result.extend_from_slice(&bytes);
            }
            _ => {
                return Err(slot_error(
                    "native interpreter string contains an unresolved prefix",
                ));
            }
        }
    }
    Ok(result)
}

pub(crate) fn capture_native_interpreter(
    vm: &CmvsPs2aVmState,
    program_image: &[u8],
) -> Result<NativeInterpreter, CoreError> {
    let program = native_program_header(program_image)?;
    if vm.stack_bytes.len() > 65536
        || vm.call_frame_bases.is_empty()
        || vm.call_frame_bases.len() > 64
    {
        return Err(slot_error("native interpreter exceeds native stack limits"));
    }
    let mut state = NativeInterpreter::default();
    vm.message_history.validate()?;
    state.message_history = Some(
        vm.message_history
            .capture(vm.interpreter_words.get(&10304).copied().unwrap_or(0)),
    );
    state.globals.timeline = super::native_timeline::capture(vm)?;
    let (projection, projection_scalars) = super::native_slot_projection::capture(vm)?;
    state.projection = projection;
    state.scalars.insert(512, projection_scalars[0]);
    state.scalars.insert(515, projection_scalars[1]);
    for (index, field) in STRING_FIELDS.into_iter().enumerate() {
        state.strings[index] = capture_string(vm, field)?;
    }
    for (tag, field) in SCALAR_FIELDS {
        state
            .scalars
            .insert(tag, vm.interpreter_words.get(&field).copied().unwrap_or(0));
    }
    state.scalars.insert(274, program_image.len() as u32);
    state.scalars.insert(262, u32::from(vm.current_frame));
    state.scalars.insert(263, vm.program_counter);
    state.scalars.insert(
        264,
        vm.interpreter_words
            .get(&schedule::SCRIPT_FRAME_COUNTER_FIELD)
            .copied()
            .unwrap_or(0),
    );
    state.scalars.insert(576, vm.stack_cursor_bytes);
    state
        .scalars
        .insert(577, (vm.call_frame_bases.len() - 1) as u32);
    state.frame_bases[..vm.call_frame_bases.len()].copy_from_slice(&vm.call_frame_bases);
    state.value_stack[..vm.stack_bytes.len()].copy_from_slice(&vm.stack_bytes);
    for (index, word) in state.suspend_words.iter_mut().enumerate() {
        *word = vm
            .interpreter_words
            .get(&(13228 + index as u32 * 4))
            .copied()
            .unwrap_or(0);
    }
    state.record_prefix = vm.interpreter_words.get(&13268).copied().unwrap_or(0);
    for (index, row) in state.records.iter_mut().enumerate() {
        for (field, word) in row.iter_mut().enumerate() {
            *word = vm
                .interpreter_words
                .get(&(13272 + index as u32 * 28 + field as u32 * 4))
                .copied()
                .unwrap_or(if field == 2 { u32::MAX } else { 0 });
        }
    }
    for (index, word) in state.text_state.iter_mut().enumerate() {
        *word = vm.read_interpreter_word(81220 + index as u32 * 4)?;
    }
    for (index, word) in state.indexed_words.iter_mut().enumerate() {
        *word = vm
            .process_indexed_words
            .get(&(index as u32))
            .copied()
            .unwrap_or(0);
    }
    for (index, word) in state.float_words.iter_mut().enumerate() {
        *word = vm
            .process_float_words
            .get(&(index as u32))
            .copied()
            .unwrap_or(0);
    }
    for &bit in vm.process_flag_bits.range(..2048) {
        state.flags[bit as usize / 8] |= 1 << (bit % 8);
    }
    // 4718AB marks the saved image without changing the live flag table.
    state.flags[0] |= 1;
    for (index, string) in state.global_strings.iter_mut().enumerate() {
        *string = vm
            .process_string_slots
            .get(&(index as u32))
            .cloned()
            .unwrap_or_default();
    }
    state.script_data = capture_data(vm, 0, program.metadata_size as usize)?;
    state.validate()?;
    Ok(state)
}

/// Restores only regions present in native A; process system-save ranges and
/// live expression flag/result words81212/81216 deliberately remain live.
pub(crate) fn apply_native_interpreter(
    vm: &mut CmvsPs2aVmState,
    state: &NativeInterpreter,
) -> Result<(), CoreError> {
    state.validate()?;
    let frame = state.scalars[&262] as u16;
    if !vm.script_frames.contains_key(&frame)
        || vm.script_data_segment_sizes.get(&0).copied() != Some(state.script_data.len() as u32)
    {
        return Err(slot_error(
            "native interpreter references an uninstalled script frame",
        ));
    }
    let decoded_strings: Vec<String> = state
        .strings
        .iter()
        .map(|bytes| {
            encoding_rs::SHIFT_JIS
                .decode_without_bom_handling(bytes)
                .0
                .into_owned()
        })
        .collect();
    super::native_timeline::validate(&state.globals.timeline)?;
    super::native_slot_projection::apply(
        vm,
        &state.projection,
        [state.scalars[&512], state.scalars[&515]],
    )?;
    super::native_timeline::apply(vm, &state.globals.timeline)?;
    vm.message_history.entries.clear();
    if let Some(history) = &state.message_history {
        history.validate()?;
        vm.message_history = history.clone();
    }
    vm.install_script_data(0, &state.script_data)?;
    for (tag, field) in SCALAR_FIELDS {
        // 475E40 consumes these three lengths without restoring live frames.
        if !matches!(tag, 275..=277) {
            vm.write_interpreter_word(field, state.scalars[&tag]);
        }
    }
    vm.current_frame = frame;
    vm.program_counter = state.scalars[&263];
    vm.current_instruction_start = vm.program_counter;
    vm.write_interpreter_word(schedule::SCRIPT_FRAME_COUNTER_FIELD, state.scalars[&264]);
    vm.stack_cursor_bytes = state.scalars[&576];
    vm.write_interpreter_word(80948, state.scalars[&576]);
    vm.write_interpreter_word(81208, state.scalars[&577]);
    vm.call_frame_bases.clear();
    vm.call_frame_bases
        .extend_from_slice(&state.frame_bases[..=state.scalars[&577] as usize]);
    vm.stack_bytes.clone_from(&state.value_stack);
    vm.stack_initialized.resize(65536, true);
    vm.stack_initialized.fill(true);
    for (field, text) in STRING_FIELDS.into_iter().zip(decoded_strings) {
        vm.interpreter_string_buffers
            .insert(field, vec![CmvsStringSegment::OwnedString(text)]);
    }
    for (index, &word) in state.suspend_words.iter().enumerate() {
        vm.write_interpreter_word(13228 + index as u32 * 4, word);
    }
    vm.write_interpreter_word(13268, state.record_prefix);
    for (index, row) in state.records.iter().enumerate() {
        for (field, &word) in row.iter().enumerate() {
            vm.write_interpreter_word(13272 + index as u32 * 28 + field as u32 * 4, word);
        }
    }
    for (index, &word) in state.text_state.iter().enumerate() {
        vm.write_interpreter_word(81220 + index as u32 * 4, word);
    }
    vm.process_indexed_words.retain(|index, _| *index >= 2048);
    vm.process_float_words.retain(|index, _| *index >= 1024);
    vm.process_flag_bits.retain(|bit| *bit >= 2048);
    vm.process_string_slots.retain(|index, _| *index >= 64);
    for (index, &word) in state.indexed_words.iter().enumerate() {
        if word != 0 {
            vm.process_indexed_words.insert(index as u32, word);
        }
    }
    for (index, &word) in state.float_words.iter().enumerate() {
        if word != 0 {
            vm.process_float_words.insert(index as u32, word);
        }
    }
    for (index, &byte) in state.flags.iter().enumerate() {
        for bit in 0..8 {
            if byte & (1 << bit) != 0 {
                vm.process_flag_bits.insert(index as u32 * 8 + bit);
            }
        }
    }
    for (index, bytes) in state.global_strings.iter().enumerate() {
        if !bytes.is_empty() {
            vm.process_string_slots.insert(index as u32, bytes.clone());
        }
    }
    Ok(())
}

/// Exact 471F50 record reconciliation followed by 477F60(29). The temporary
/// [0, record-PC, record-frame] triple is consumed immediately; only the
/// suspended [counter, PC, frame] triple remains live on the value stack.
pub(crate) fn reconcile_native_records_and_request29(
    vm: &mut CmvsPs2aVmState,
    prior: &[[u32; 7]],
) -> Result<bool, CoreError> {
    if prior.len() != 64 {
        return Err(slot_error(
            "native pre-load record table must contain 64 rows",
        ));
    }
    let mut records = [[0u32; 7]; 64];
    for (index, row) in records.iter_mut().enumerate() {
        for (field, word) in row.iter_mut().enumerate() {
            *word = vm
                .interpreter_words
                .get(&(13272 + index as u32 * 28 + field as u32 * 4))
                .copied()
                .unwrap_or(if field == 2 { u32::MAX } else { 0 });
        }
        if row[1] != 0 {
            row[0] = 0;
            row[2] = u32::MAX;
        }
        if prior[index][1] != 0 {
            *row = prior[index];
        }
    }
    let request = records[29];
    let cursor = checked_native_request(vm, request[1], request[2])?;
    for (index, row) in records.iter().enumerate() {
        for (field, &word) in row.iter().enumerate() {
            vm.write_interpreter_word(13272 + index as u32 * 28 + field as u32 * 4, word);
        }
    }
    vm.write_interpreter_word(14060, 0);
    vm.write_interpreter_word(14064, u32::MAX);
    vm.write_interpreter_word(14056, 0);
    vm.write_interpreter_word(10564, 1);
    vm.write_interpreter_word(10576, 0);
    if let Some(cursor) = cursor {
        push_native_request(vm, cursor, request[1], request[2]);
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Native 477F60 is also the save-completion record30 entry.
pub(crate) fn request_native_record(
    vm: &mut CmvsPs2aVmState,
    record: u32,
) -> Result<bool, CoreError> {
    if record >= 64 {
        return Err(slot_error("native record index exceeds the table"));
    }
    let base = 13272 + record * 28;
    let frame = vm.interpreter_words.get(&(base + 4)).copied().unwrap_or(0);
    let pc = vm
        .interpreter_words
        .get(&(base + 8))
        .copied()
        .unwrap_or(u32::MAX);
    if let Some(cursor) = checked_native_request(vm, frame, pc)? {
        push_native_request(vm, cursor, frame, pc);
        Ok(true)
    } else {
        Ok(false)
    }
}

fn checked_native_request(
    vm: &CmvsPs2aVmState,
    frame: u32,
    pc: u32,
) -> Result<Option<usize>, CoreError> {
    if pc == u32::MAX {
        return Ok(None);
    }
    if frame > 3 || !vm.script_frames.contains_key(&(frame as u16)) {
        return Err(slot_error(
            "native record references an uninstalled script frame",
        ));
    }
    let cursor = vm.stack_cursor_bytes as usize;
    if cursor > 65536 - 24
        || vm.stack_bytes.len() > 65536
        || vm.stack_initialized.len() != vm.stack_bytes.len()
    {
        return Err(slot_error("native record context exceeds the value stack"));
    }
    Ok(Some(cursor))
}

fn push_native_request(vm: &mut CmvsPs2aVmState, cursor: usize, frame: u32, pc: u32) {
    let counter = vm
        .interpreter_words
        .get(&schedule::SCRIPT_FRAME_COUNTER_FIELD)
        .copied()
        .unwrap_or(0);
    let values = [
        counter,
        vm.program_counter,
        u32::from(vm.current_frame),
        0,
        pc,
        frame,
    ];
    if vm.stack_bytes.len() < cursor + 24 {
        vm.stack_bytes.resize(cursor + 24, 0);
        vm.stack_initialized.resize(cursor + 24, false);
    }
    for (index, word) in values.into_iter().enumerate() {
        vm.stack_bytes[cursor + index * 4..cursor + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    vm.stack_initialized[cursor..cursor + 24].fill(true);
    vm.stack_cursor_bytes = (cursor + 12) as u32;
    vm.current_frame = frame as u16;
    vm.program_counter = pc;
    vm.current_instruction_start = pc;
    vm.write_interpreter_word(13216, frame);
    vm.write_interpreter_word(13220, pc);
    vm.write_interpreter_word(schedule::SCRIPT_FRAME_COUNTER_FIELD, 0);
    vm.write_interpreter_word(80948, vm.stack_cursor_bytes);
}
