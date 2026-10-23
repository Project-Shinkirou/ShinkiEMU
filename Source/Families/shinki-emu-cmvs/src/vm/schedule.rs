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
    let pc = state
        .interpreter_words
        .get(&(base + 4))
        .copied()
        .filter(|pc| *pc != u32::MAX);
    if let Some(pc) = pc {
        // A loaded record table can carry a continuation captured while
        // the frame sat inside an expression (an interior byte offset).
        // The native scheduler only ever resumes at decoded instruction
        // starts; snap an interior offset forward to the next valid one.
        if let Some(starts) = state.script_valid_starts.get(
            &u16::try_from(state.interpreter_words.get(&base).copied().unwrap_or(0))
                .unwrap_or(state.current_frame),
        ) {
            if !starts.contains(&pc) {
                if let Some(next) = starts.iter().copied().filter(|p| *p > pc).min() {
                    if tracing::enabled!(tracing::Level::INFO) {
                        tracing::info!(
                            event = "shinki.emu.cmvs.vm.record_pc_snapped",
                            slot,
                            pc,
                            next,
                            tick = state.tick
                        );
                    }
                    return Some(next);
                }
            }
        }
        if pc >= 0x0100_0000 && tracing::enabled!(tracing::Level::INFO) {
            tracing::info!(
                event = "shinki.emu.cmvs.vm.record_pc_anomaly",
                slot,
                base,
                pc,
                w0 = state.interpreter_words.get(&base).copied().unwrap_or(0),
                w8 = state
                    .interpreter_words
                    .get(&(base + 8))
                    .copied()
                    .unwrap_or(0),
                tick = state.tick
            );
        }
    }
    pc
}

fn push_context(
    state: &mut CmvsPs2aVmState,
    counter: u32,
    pc: u32,
    frame: u32,
) -> Result<(), CoreError> {
    {
        super::stack::set_push_origin("sched");
    }
    push_word(state, counter)?;
    push_word(state, pc)?;
    let result = push_word(state, frame);
    super::stack::set_push_origin("push");
    result
}

fn push_suspended(state: &mut CmvsPs2aVmState) -> Result<(), CoreError> {
    if tracing::enabled!(tracing::Level::INFO) {
        tracing::info!(
            event = "shinki.emu.cmvs.vm.suspend_capture",
            pc = state.program_counter,
            frame = state.current_frame,
            tick = state.tick,
            valid = state
                .script_valid_starts
                .get(&state.current_frame)
                .is_some_and(|s| s.contains(&state.program_counter))
        );
    }
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
    // Drain the session-queued dialog dispatches the way the native
    // input callback arms the interpreter: the identity word goes under
    // the (counter, pc, frame) context so the dispatcher head's frame
    // locals read it as the call argument.
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
                            && word(
                                state,
                                super::execute::effect_channel_activity_field(channel),
                            ) != 0
                    }
                    23 => {
                        word(
                            state,
                            COROUTINE_RECORD_TABLE_BASE + slot * COROUTINE_RECORD_STRIDE - 4,
                        ) != 0
                    }
                    _ => false,
                };
                if active && tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.entry_armed",
                        slot,
                        scan_index,
                        pc,
                        frame = word(
                            state,
                            COROUTINE_RECORD_TABLE_BASE + slot * COROUTINE_RECORD_STRIDE,
                        ),
                        tick = state.tick
                    );
                }
                if active {
                    // While a queued dialog dispatch is winding down, the
                    // shared continuation stack must not interleave more
                    // record contexts: hold actives until the dispatch
                    // completes its cleanup.
                    if state.dialog_dispatch_base_depth.is_some() {
                        if tracing::enabled!(tracing::Level::INFO) {
                            tracing::info!(
                                event = "shinki.emu.cmvs.vm.entry_held",
                                slot,
                                pc,
                                tick = state.tick
                            );
                        }
                    } else {
                        push_record(state, slot, pc)?;
                    }
                }
            }
            if let Some((frame, pc, id)) = state.forced_dialog_record.take() {
                if pc != u32::MAX {
                    if tracing::enabled!(tracing::Level::INFO) {
                        tracing::info!(
                            event = "shinki.emu.cmvs.vm.forced_record_resume",
                            pc,
                            frame,
                            id,
                            tick = state.tick
                        );
                    }
                    // Native record resume, `sub_477F60` + the 0x410 leg
                    // of the dispatch loop: push the suspended triple
                    // [counter][pc][script] for the eventual unwind, then
                    // the record params [0][paramB][paramA], register the
                    // new call depth's frame base after paramA (so nested
                    // calls' returns land above the params), and switch
                    // straight into the record entry — no extra context
                    // triple above the params.
                    super::stack::set_push_origin("menu");
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
                    super::stack::set_push_origin("push");
                }
            }
            if state.pending_dialog_dispatches.len() > 1
                && state.dialog_dispatch_base_depth.is_some()
            {
                // A dispatch is still winding down; hold the remaining queued
                // entries for later engine frames so each completes its cleanup
                // before the next lays its own locals.
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.dialog_dispatch_held",
                        tick = state.tick,
                        queued = state.pending_dialog_dispatches.len()
                    );
                }
            } else if let Some((entry_pc, frame, id)) = state.pending_dialog_dispatches.pop() {
                // The dispatcher chain runs on top of the untouched suspension:
                // its tail returns into the suspended pc (like the native
                // callback returning to the engine loop) and the scheduler's
                // own suspension triple stays intact below.
                super::stack::set_push_origin("dialog");
                // Native `sub_46D270` case 0x103 reads frame_base - payload: the
                // dispatcher head's fl12/fl8 come from the words the caller
                // pushed BENEATH the frame base. Lay [fl12][fl8 = id][return]
                // then record the base.
                let suspended_pc = state.program_counter;
                // The native callback lays the action parameters beneath
                // the identity pair: the jve1 branch (intproc 25932) reads
                // them as its frame locals at base-16/-12. Take them from
                // the armed record's argument words (record table +8/+12).
                let rec_base = COROUTINE_RECORD_TABLE_BASE + 28 * 28;
                let param_a = word(state, rec_base + 8);
                let param_b = word(state, rec_base + 12);

                super::stack::push_word_public(state, param_a)?;
                super::stack::push_word_public(state, param_b)?;
                super::stack::push_word_public(state, suspended_pc)?;
                super::stack::push_word_public(state, id)?;
                super::stack::push_word_public(state, id)?;
                push_call_frame_base(state)?;
                // The item dispatcher reads its input through the
                // positive 0x108 offsets (base+4); after the adopt pops
                // the top triple the stale word at base+4 is the triple's
                // pc slot. Lay one id word above the base so base+4
                // carries the selection id instead.
                super::stack::push_word_public(state, id)?;
                push_context(state, 0, entry_pc, u32::from(frame))?;
                super::stack::set_push_origin("push");
                state.dialog_dispatch_consumed = true;
                state.dialog_dispatch_base_depth = Some(state.call_frame_bases.len());
                state.dialog_dispatch_cursor = Some(state.stack_cursor_bytes);
                // Single-shot: the dispatcher tail republishes the identity into
                // the polled word; parking the sentinel keeps the overlay ring
                // from re-arming the dispatch loop within the same tick chain.
                // The native marker is 1000: the intproc idle loop compares
                // word 17 against 1000 (jfc@370590) and processes the queued
                // work only on equality — any other value takes the early
                // return at 371834.
                state.process_indexed_words.insert(17, 1000);
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.dialog_dispatch_queued",
                        tick = state.tick,
                        entry_pc,
                        frame,
                        id
                    );
                }
            }

            loaded = true;
        } else if let Some((entry_pc, frame, id)) = state.pending_dialog_dispatches.pop() {
            // The post-save menu runs through the disabled-scheduler
            // fallback (the engine request dwords), and the queued quad
            // dispatch — the input callback's item dispatcher — must fire
            // there too. Lay the same frame layout the enabled branch
            // uses; the shared adopt tail below pops the triple and
            // resumes the dispatcher entry.
            super::stack::set_push_origin("dialog");
            let rec_base = COROUTINE_RECORD_TABLE_BASE + 28 * 28;
            let param_a = word(state, rec_base + 8);
            let param_b = word(state, rec_base + 12);
            let suspended_pc = state.program_counter;
            super::stack::push_word_public(state, param_a)?;
            super::stack::push_word_public(state, param_b)?;
            super::stack::push_word_public(state, suspended_pc)?;
            super::stack::push_word_public(state, id)?;
            super::stack::push_word_public(state, id)?;
            push_call_frame_base(state)?;
            push_context(state, 0, entry_pc, u32::from(frame))?;
            super::stack::set_push_origin("push");
            state.dialog_dispatch_consumed = true;
            state.dialog_dispatch_base_depth = Some(state.call_frame_bases.len());
            state.dialog_dispatch_cursor = Some(state.stack_cursor_bytes);
            state.process_indexed_words.insert(17, 1000);
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.dialog_dispatch_queued",
                    tick = state.tick,
                    entry_pc,
                    frame,
                    id
                );
            }
            loaded = true;
        } else {
            // Disabled-scheduler fallback: record 26 (flag+pc) takes
            // priority over record 28 (native dwords 3500/3502 and
            // 3515/3516, checked before the activity-flag clear below).
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.scheduler_fallback_probe",
                    slot26_flag = word(
                        state,
                        COROUTINE_RECORD_TABLE_BASE + 26 * COROUTINE_RECORD_STRIDE - 4,
                    ),
                    slot26_pc = record_pc(state, 26),
                    slot28_pc = record_pc(state, 28),
                    tick = state.tick
                );
            }
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
                    if tracing::enabled!(tracing::Level::INFO) {
                        tracing::info!(
                            event = "shinki.emu.cmvs.vm.engine_request_park_resume",
                            pc = park_pc,
                            frame = park_frame,
                            tick = state.tick
                        );
                    }
                }
            }
            let alt_pc = word(state, 3516 * 4);
            let alt_frame = word(state, 3515 * 4);
            if req_gate != 0 && req_pc != u32::MAX {
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.engine_request_resume",
                        pc = req_pc,
                        frame = req_frame,
                        tick = state.tick
                    );
                }
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
                state.interpreter_words.insert(3500 * 4, 0);
                state.engine_request_live = true;
                loaded = true;
            } else if alt_pc != u32::MAX && alt_pc != 0 {
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.engine_request_resume_alt",
                        pc = alt_pc,
                        frame = alt_frame,
                        tick = state.tick
                    );
                }
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
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.scheduler_fallback_resume",
                        slot,
                        pc,
                        tick = state.tick
                    );
                }
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
        state
            .interpreter_words
            .insert(SCRIPT_FRAME_COUNTER_FIELD, counter);
    }
    // This loop runs even with the rebuild gate closed. Only the activity
    // flag is reset; registration frame, PC, arguments and timestamp survive.
    for slot in 0..COROUTINE_RECORD_COUNT {
        state.interpreter_words.insert(
            COROUTINE_RECORD_TABLE_BASE + slot * COROUTINE_RECORD_STRIDE - 4,
            0,
        );
    }
    Ok(())
}
