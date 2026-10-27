use super::*;

pub(super) fn push_word(state: &mut CmvsPs2aVmState, value: u32) -> Result<(), CoreError> {
    push_immediate(state, value, 4)
}

pub(super) fn push_immediate(
    state: &mut CmvsPs2aVmState,
    value: u32,
    advance_bytes: u16,
) -> Result<(), CoreError> {
    if advance_bytes < 4 {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_STACK_WIDTH",
            "CMVS PS2A immediate push is smaller than its written word",
        ));
    }
    let advance = usize::from(advance_bytes);
    let start = usize::try_from(state.stack_cursor_bytes).map_err(|_| {
        invalid(
            "SHINKI_EMU_CMVS_VM_STACK_LIMIT",
            "CMVS PS2A stack cursor exceeds platform bounds",
        )
    })?;
    let end = start.checked_add(advance).ok_or_else(|| {
        invalid(
            "SHINKI_EMU_CMVS_VM_STACK_LIMIT",
            "CMVS PS2A stack size overflowed",
        )
    })?;
    if end > MAX_STACK_BYTES {
        return Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_VM_STACK_LIMIT",
            format!(
                "CMVS PS2A stack exceeds the recovered execution budget at pc {} frame {} cursor {}",
                state.program_counter, state.current_frame, state.stack_cursor_bytes
            ),
        ));
    }
    ensure_stack_memory(state, end)?;
    state.stack_bytes[start..start + 4].copy_from_slice(&value.to_le_bytes());
    state.stack_initialized[start..start + 4].fill(true);
    state.stack_bytes[start + 4..end].fill(0);
    state.stack_initialized[start + 4..end].fill(false);
    state.stack_cursor_bytes = u32::try_from(end).map_err(|_| {
        invalid(
            "SHINKI_EMU_CMVS_VM_STACK_LIMIT",
            "CMVS PS2A stack cursor exceeds the recovered execution budget",
        )
    })?;
    Ok(())
}

pub(super) fn ensure_stack_memory(
    state: &mut CmvsPs2aVmState,
    end: usize,
) -> Result<(), CoreError> {
    if end > MAX_STACK_BYTES {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_STACK_LIMIT",
            "CMVS PS2A variable buffer exceeds the recovered execution budget",
        ));
    }
    state
        .stack_bytes
        .resize(end.max(state.stack_bytes.len()), 0);
    state
        .stack_initialized
        .resize(end.max(state.stack_initialized.len()), false);
    Ok(())
}

pub(super) fn push_call_frame_base(state: &mut CmvsPs2aVmState) -> Result<(), CoreError> {
    if state.call_frame_bases.len() >= MAX_STACK_BYTES / 4 {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_CALL_DEPTH",
            "CMVS PS2A call depth exceeds the recovered execution budget",
        ));
    }
    state.call_frame_bases.push(state.stack_cursor_bytes);
    Ok(())
}

pub(super) fn pop_word(state: &mut CmvsPs2aVmState) -> Result<u32, CoreError> {
    let value = read_stack_word_from_top(state, 4)?;
    drop_stack_bytes(state, 4)?;
    Ok(value)
}

pub(super) fn drop_stack_bytes(state: &mut CmvsPs2aVmState, bytes: u16) -> Result<(), CoreError> {
    let cursor = usize::try_from(state.stack_cursor_bytes).map_err(|_| {
        invalid(
            "SHINKI_EMU_CMVS_VM_STACK_LIMIT",
            "CMVS PS2A stack cursor exceeds platform bounds",
        )
    })?;
    let retained = cursor.checked_sub(usize::from(bytes)).ok_or_else(|| {
        invalid(
            "SHINKI_EMU_CMVS_VM_STACK_UNDERFLOW",
            "CMVS PS2A stack adjustment underflowed",
        )
    })?;
    state.stack_initialized[retained..cursor].fill(false);
    state.stack_cursor_bytes = u32::try_from(retained).map_err(|_| {
        invalid(
            "SHINKI_EMU_CMVS_VM_STACK_LIMIT",
            "CMVS PS2A stack cursor exceeds the recovered execution budget",
        )
    })?;
    Ok(())
}

pub(super) fn read_stack_word_from_top(
    state: &CmvsPs2aVmState,
    offset_from_top_bytes: u8,
) -> Result<u32, CoreError> {
    let offset = usize::from(offset_from_top_bytes);
    if offset < 4 {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_STACK_WIDTH",
            "CMVS PS2A stack word offset is smaller than one word",
        ));
    }
    let cursor = usize::try_from(state.stack_cursor_bytes).map_err(|_| {
        invalid(
            "SHINKI_EMU_CMVS_VM_STACK_LIMIT",
            "CMVS PS2A stack cursor exceeds platform bounds",
        )
    })?;
    let start = cursor.checked_sub(offset).ok_or_else(|| {
        CoreError::invalid(
            "SHINKI_EMU_CMVS_VM_STACK_UNDERFLOW",
            format!(
                "CMVS PS2A command stack underflowed at pc {} frame {} cursor {}",
                state.program_counter, state.current_frame, state.stack_cursor_bytes
            ),
        )
    })?;
    let end = start.checked_add(4).ok_or_else(|| {
        invalid(
            "SHINKI_EMU_CMVS_VM_STACK_UNDERFLOW",
            "CMVS PS2A command stack underflowed",
        )
    })?;
    if state.stack_initialized.get(start..end) != Some(&[true, true, true, true]) {
        return Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_VM_STACK_UNINITIALIZED",
            format!(
                "CMVS PS2A command reads an uninitialized stack word at pc {} frame {} cursor {} offset {}",
                state.program_counter, state.current_frame, state.stack_cursor_bytes, offset
            ),
        ));
    }
    let bytes: [u8; 4] = state.stack_bytes[start..end].try_into().map_err(|_| {
        invalid(
            "SHINKI_EMU_CMVS_VM_STACK_UNDERFLOW",
            "CMVS PS2A command stack underflowed",
        )
    })?;
    Ok(u32::from_le_bytes(bytes))
}

pub(super) fn advance(
    state: &mut CmvsPs2aVmState,
    frame: &CmvsPs2aInstructionFrame,
) -> Result<(), CoreError> {
    state.program_counter = state
        .program_counter
        .checked_add(u32::from(frame.span().byte_length))
        .ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_VM_PC",
                "CMVS PS2A program counter overflowed",
            )
        })?;
    if !state.program_counter.is_multiple_of(2) {
        return Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_VM_PC_UNALIGNED",
            "CMVS PS2A program counter advanced to an odd offset",
        ));
    }
    Ok(())
}

/// The recovered `sub_46D270` case 0x122 addressing: the operand indexes
/// DOWN from the active call frame's base (the frame-base table entry at
/// `this + 4*frame + 80952`), reaching the caller's argument words below
/// the callee's reserved locals.
pub(super) fn read_frame_relative_word(
    state: &CmvsPs2aVmState,
    offset_bytes: u8,
) -> Result<u32, CoreError> {
    let offset = usize::from(offset_bytes);
    let base = usize::try_from(
        state
            .call_frame_bases
            .last()
            .copied()
            .unwrap_or(state.stack_cursor_bytes),
    )
    .map_err(|_| {
        invalid(
            "SHINKI_EMU_CMVS_VM_STACK_LIMIT",
            "CMVS PS2A frame base exceeds platform bounds",
        )
    })?;
    let start = base.checked_sub(offset).ok_or_else(|| {
        CoreError::invalid(
            "SHINKI_EMU_CMVS_VM_STACK_UNDERFLOW",
            format!(
                "CMVS PS2A frame-relative read underflowed at pc {} frame {} base {} offset {}",
                state.program_counter, state.current_frame, base, offset
            ),
        )
    })?;
    let end = start + 4;
    if state.stack_initialized.get(start..end) != Some(&[true, true, true, true]) {
        return Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_VM_STACK_UNINITIALIZED",
            format!(
                "CMVS PS2A frame-relative read hits an uninitialized word at pc {} frame {}",
                state.program_counter, state.current_frame
            ),
        ));
    }
    Ok(u32::from_le_bytes(
        state.stack_bytes[start..end]
            .try_into()
            .expect("four bytes"),
    ))
}

/// Session-facing push used by the dialog dispatch queue.
pub(crate) fn push_word_public(state: &mut CmvsPs2aVmState, value: u32) -> Result<(), CoreError> {
    push_word(state, value)
}
