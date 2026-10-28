use super::*;

/// Native byte offsets in the interpreter object.
pub(super) const SCRIPT_FRAME_COUNTER_FIELD: u32 = 0x33A8;
pub(super) const COROUTINE_RECORD_TABLE_BASE: u32 = 0x33DC;
pub(super) const COROUTINE_RECORD_COUNT: u32 = 64;
pub(super) const COROUTINE_RECORD_STRIDE: u32 = 28;
pub(super) const FRAME_ENTRY_QUEUE_SLOTS: u32 = 32;
pub(super) const EFFECT_CHANNEL_TABLE_DWORD_INDEX: u32 = 742;
pub(super) const FRAME_ENTRY_SCHEDULER_ENABLED_WORD: u32 = 0x33D4;
pub(super) const FRAME_ENTRY_REBUILD_GATE_FIELD: u32 = 0x29DC;

fn word(state: &CmvsPs2aVmState, offset: u32) -> u32 {
    state.interpreter_words.get(&offset).copied().unwrap_or(0)
}

fn record_pc(state: &CmvsPs2aVmState, slot: u32) -> Option<u32> {
    let base = COROUTINE_RECORD_TABLE_BASE + slot * COROUTINE_RECORD_STRIDE;
    state
        .interpreter_words
        .get(&(base + 4))
        .copied()
        .filter(|pc| *pc != u32::MAX)
}

fn push_context(
    state: &mut CmvsPs2aVmState,
    counter: u32,
    pc: u32,
    frame: u32,
) -> Result<(), CoreError> {
    push_word(state, counter)?;
    push_word(state, pc)?;
    push_word(state, frame)
}

fn push_suspended(state: &mut CmvsPs2aVmState) -> Result<(), CoreError> {
    push_context(
        state,
        word(state, SCRIPT_FRAME_COUNTER_FIELD),
        state.program_counter,
        u32::from(state.current_frame),
    )
}

fn push_record(state: &mut CmvsPs2aVmState, slot: u32, pc: u32) -> Result<(), CoreError> {
    let frame = word(
        state,
        COROUTINE_RECORD_TABLE_BASE + slot * COROUTINE_RECORD_STRIDE,
    );
    push_context(state, 0, pc, frame)
}

/// sub_46DF10 uses the same value stack as commands 129/138 and opcode
/// 0x414. Candidates are pushed in ascending slot order; only the final
/// triple is popped here. ScriptReturn consumes the remaining triples in
/// this same dispatch, ultimately resuming the original suspension.
pub fn rebuild_frame_entry_queue(state: &mut CmvsPs2aVmState) -> Result<(), CoreError> {
    let mut loaded = false;
    if word(state, FRAME_ENTRY_REBUILD_GATE_FIELD) == 0 {
        if word(state, FRAME_ENTRY_SCHEDULER_ENABLED_WORD) != 0 {
            push_suspended(state)?;
            for slot in 0..FRAME_ENTRY_QUEUE_SLOTS {
                let Some(pc) = record_pc(state, slot) else {
                    continue;
                };
                let scan_index = slot as i32 - 3;
                let active = match scan_index {
                    0..=3 | 25 => true,
                    7..=14 => {
                        let channel = (scan_index - 7) as u32;
                        state
                            .slot_objects
                            .contains_key(&(EFFECT_CHANNEL_TABLE_DWORD_INDEX << 8 | channel))
                            && state
                                .effect_channels
                                .get(&(channel as u8))
                                .is_some_and(|object| object.read_dword(2) != 0)
                    }
                    23 => {
                        word(
                            state,
                            COROUTINE_RECORD_TABLE_BASE + slot * COROUTINE_RECORD_STRIDE - 4,
                        ) != 0
                    }
                    _ => false,
                };
                if active {
                    // While a queued dialog dispatch is winding down, the
                    // shared continuation stack must not interleave more
                    // record contexts: hold actives until the dispatch
                    // completes its cleanup.
                    if state.dialog_dispatch_base_depth.is_none() {
                        push_record(state, slot, pc)?;
                    }
                }
            }
            if let Some((frame, pc, _)) = state.forced_dialog_record.take()
                && pc != u32::MAX
            {
                // Native record resume, `sub_477F60` + the 0x410 leg
                // of the dispatch loop: push the suspended triple
                // [counter][pc][script] for the eventual unwind, then
                // the record params [0][paramB][paramA], register the
                // new call depth's frame base after paramA (so nested
                // calls' returns land above the params), and switch
                // straight into the record entry — no extra context
                // triple above the params.
                let counter_now = word(state, SCRIPT_FRAME_COUNTER_FIELD);
                let pc_now = state.program_counter;
                let script_now = u32::from(state.current_frame);
                super::stack::push_word_public(state, counter_now)?;
                super::stack::push_word_public(state, pc_now)?;
                super::stack::push_word_public(state, script_now)?;
                let rec_base = COROUTINE_RECORD_TABLE_BASE + 28 * 29;
                let param_a = word(state, rec_base);
                let param_b = word(state, rec_base + 4);
                super::stack::push_word_public(state, 0)?;
                super::stack::push_word_public(state, param_b)?;
                super::stack::push_word_public(state, param_a)?;
                push_call_frame_base(state)?;
                state.program_counter = pc;
                state.current_frame = u16::try_from(frame).unwrap_or(1);
            }

            loaded = true;
        } else {
            // Disabled-scheduler fallback: record 26 (flag+pc) takes
            // priority over record 28 (native dwords 3500/3502 and
            // 3515/3516, checked before the activity-flag clear below).
            // `sub_46DF10`'s disabled branch first honors the engine's
            // raw resume-request dwords (gate 3500, frame 3501, pc 3502,
            // then the 3515/3516 bank) before falling back to records —
            // the menu keeps its record-17 poll alive through these while
            // the script scheduler is off.
            let req_gate = if state.engine_request_armed {
                word(state, 3500 * 4)
            } else {
                0
            };
            let mut req_pc = word(state, 3502 * 4);
            let mut req_frame = word(state, 3501 * 4);
            // A coroutine resumed through an earlier request may have
            // parked mid-body (the wait commands' suspension): the record
            // table's park slot holds that pc, and the next request
            // resumes the park rather than replaying the record head.
            if req_gate != 0 {
                state.engine_request_armed = false;
                state.engine_request_live = false;
                if let Some((park_pc, park_frame)) = state.engine_request_park.take() {
                    req_pc = park_pc;
                    req_frame = park_frame;
                }
            }
            let alt_pc = word(state, 3516 * 4);
            let alt_frame = word(state, 3515 * 4);
            if req_gate != 0 && req_pc != u32::MAX {
                push_suspended(state)?;
                push_context(state, 0, req_pc, req_frame)?;
                // `sub_46DF10`'s tail reads the pushed param triple back
                // into the live registers — the stack keeps both triples
                // (the suspension and the params) exactly as pushed; the
                // return chain consumes them through the record-30
                // handshake after the write.
                state.program_counter = req_pc;
                if let Ok(frame_id) = u16::try_from(req_frame) {
                    state.current_frame = frame_id;
                }
                // The engine re-arms the request per interaction; consume
                // the gate after one resume so the record's own wait gates
                // pace the loop instead of a resume storm.
                state.write_interpreter_word(3500 * 4, 0);
                state.engine_request_live = true;
                loaded = true;
            } else if alt_pc != u32::MAX && alt_pc != 0 {
                push_suspended(state)?;
                push_context(state, 0, alt_pc, alt_frame)?;
                state.program_counter = alt_pc;
                if let Ok(frame_id) = u16::try_from(alt_frame) {
                    state.current_frame = frame_id;
                }
                loaded = true;
            }
            let selected = if loaded {
                None
            } else {
                record_pc(state, 26)
                    .filter(|_| {
                        word(
                            state,
                            COROUTINE_RECORD_TABLE_BASE + 26 * COROUTINE_RECORD_STRIDE - 4,
                        ) != 0
                    })
                    .map(|pc| (26, pc))
                    .or_else(|| record_pc(state, 28).map(|pc| (28, pc)))
            };
            if let Some((slot, pc)) = selected {
                push_suspended(state)?;
                push_record(state, slot, pc)?;
                loaded = true;
            }
        }
    }
    if loaded {
        // The gate stays script-owned: the ring closes it with cmd137(0)
        // and reopens with cmd137(1), exactly as the native loop does.
        // Closing it here deadlocks a ring that parks mid-cycle (its own
        // reopen site never runs) and the poll loop dies with it.
        let frame = pop_word(state)?;
        let pc = pop_word(state)?;
        let counter = pop_word(state)?;
        state.current_frame = u16::try_from(frame).map_err(|_| {
            invalid(
                "SHINKI_EMU_CMVS_VM_COROUTINE",
                "scheduler restored an out-of-range frame id",
            )
        })?;
        state.program_counter = pc;
        state.write_interpreter_word(SCRIPT_FRAME_COUNTER_FIELD, counter);
    }
    // This loop runs even with the rebuild gate closed. Only the activity
    // flag is reset; registration frame, PC, arguments and timestamp survive.
    for slot in 0..COROUTINE_RECORD_COUNT {
        state.write_interpreter_word(
            COROUTINE_RECORD_TABLE_BASE + slot * COROUTINE_RECORD_STRIDE - 4,
            0,
        );
    }
    Ok(())
}
