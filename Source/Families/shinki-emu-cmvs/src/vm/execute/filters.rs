use super::*;

/// Reverse-engineering aid: report filter-chain state only when it changes,
/// so a long run shows each registration/selection transition once.
fn log_chain_state_transition(state: &mut CmvsPs2aVmState) {
    if !tracing::enabled!(tracing::Level::INFO) {
        return;
    }
    let mut signature = String::new();
    for bank in 0..6_u8 {
        let order = state
            .filter_chain_record_order
            .get(&bank)
            .cloned()
            .unwrap_or_default();
        let active = state.filter_chain_active_records.get(&bank).copied();
        signature.push_str(&format!(
            "{}:[{}]|{};",
            bank,
            order
                .iter()
                .map(|record| record.to_string())
                .collect::<Vec<_>>()
                .join(","),
            active.map(|record| record.to_string()).unwrap_or_default()
        ));
    }
    if state.last_filter_chain_signature.as_deref() != Some(signature.as_str()) {
        state.last_filter_chain_signature = Some(signature.clone());
        tracing::info!(
            event = "shinki.emu.cmvs.vm.chain_state",
            pc = state.program_counter,
            frame = state.current_frame,
            state = signature
        );
    }
}

/// The native poll's pointer hover pass (`sub_468740`, LABEL_22 loop): the
/// first record in registration order whose rectangle contains the pointer
/// becomes the chain's active record. The rectangle is the record's `cmd
/// 531` word quad (`sub_468640` stores them at node bytes +12..+24 as x, y,
/// w, h) and the comparison is half-open on both axes. A moved hover keeps
/// any previously locked record: the native release path then fails the
/// `active == locked` test and reports -10, exactly like a real click that
/// slid off its target.
fn pointer_hover_pass(state: &mut CmvsPs2aVmState, bank: u8) {
    let order = state
        .filter_chain_record_order
        .get(&bank)
        .cloned()
        .unwrap_or_default();
    let records = state.filter_chain_records.get(&bank);
    let mut hovered = None;
    if let Some(records) = records {
        for key in &order {
            let Some(record) = records.get(key) else {
                continue;
            };
            let [x, y, w, h] = record.words;
            let (x, y, w, h) = (x as i32, y as i32, w as i32, h as i32);
            if state.pointer_x >= x
                && state.pointer_y >= y
                && state.pointer_x < x.wrapping_add(w)
                && state.pointer_y < y.wrapping_add(h)
            {
                hovered = Some(*key);
                break;
            }
        }
    }
    match hovered {
        Some(key) => {
            if state.filter_chain_active_records.get(&bank) != Some(&key) {
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.chain_pointer_hover",
                        pc = state.program_counter,
                        frame = state.current_frame,
                        bank,
                        record = key
                    );
                }
                state.filter_chain_active_records.insert(bank, key);
            }
        }
        None => {
            // A pointer off every rectangle clears the active record
            // (`v5` stays 0 and `v4[2] = v5`); any lock survives, so a
            // later release fails the `active == locked` test.
            state.filter_chain_active_records.remove(&bank);
        }
    }
}

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    effect_kind: CmvsPs2aCommandEffectKind,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (effect_kind, values) {
        (
            CmvsPs2aCommandEffectKind::RunFilterGraphControl {
                table_byte_offset: _,
                slot: _,
                field_offset: _,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, _)],
        ) => {
            // Entirely resolved by the recovered 0xA000 wait gate in
            // `execute_command`; this arm only serves the continue path
            // (missing owner handle), which pops and advances normally.
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReplaceFilterChainSlot {
                table_byte_offset,
                max_bank,
                max_channel_id,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank), (CmvsPs2aCommandStackWordKind::OpaqueU32, channel)],
        ) => {
            if *bank > max_bank || *channel > max_channel_id {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            // The previous occupant is freed and a fresh chain object takes
            // the slot; only occupancy and identity are observable.
            let key = slot_object_key(table_byte_offset, *bank)?;
            state.slot_object_seed_words.remove(&key);
            let bank_index = u8::try_from(*bank).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_FILTER_CHAIN",
                    "CMVS filter-chain bank exceeds the recovered bound",
                )
            })?;
            state.filter_chain_records.remove(&bank_index);
            state.filter_chain_record_order.remove(&bank_index);
            state.filter_chain_active_records.remove(&bank_index);
            state.filter_chain_channels.remove(&bank_index);
            let identity = state.next_slot_object_id;
            state.next_slot_object_id = identity.checked_add(1).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_SLOT_OBJECTS",
                    "CMVS slot object identity space overflowed",
                )
            })?;
            state.slot_objects.insert(key, identity);
            state.filter_chain_channels.insert(
                bank_index,
                u8::try_from(*channel).map_err(|_| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_FILTER_CHAIN",
                        "CMVS filter-chain channel exceeds the recovered bound",
                    )
                })?,
            );
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::DestroyFilterChainSlot {
                table_byte_offset,
                max_bank,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank)],
        ) => {
            if *bank > u32::from(max_bank) {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            let bank = u8::try_from(*bank).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_FILTER_CHAIN",
                    "CMVS filter-chain bank exceeds the recovered bound",
                )
            })?;
            let key = slot_object_key(table_byte_offset, u32::from(bank))?;
            state.slot_objects.remove(&key);
            state.slot_object_seed_words.remove(&key);
            state.filter_chain_records.remove(&bank);
            state.filter_chain_record_order.remove(&bank);
            state.filter_chain_active_records.remove(&bank);
            state.filter_chain_channels.remove(&bank);
            state.filter_chain_apply_revisions.remove(&bank);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::QueryFilterChainSlot {
                table_byte_offset,
                max_bank,
                error_flag_field_offset,
                error_flag_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank), (CmvsPs2aCommandStackWordKind::OpaqueU32, record_key)],
        ) => {
            // `sub_468560` allocates a keyed queue node on a live bank. Host
            // allocation exhaustion is represented by the deterministic
            // record budget rather than ambient allocator behavior.
            let key = slot_object_key(table_byte_offset, *bank);
            let live =
                *bank <= max_bank && key.is_ok() && state.slot_objects.contains_key(&key.unwrap());
            if live {
                let bank = u8::try_from(*bank).map_err(|_| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_FILTER_CHAIN",
                        "CMVS filter-chain bank exceeds the recovered bound",
                    )
                })?;
                let records = state.filter_chain_records.entry(bank).or_default();
                if !records.contains_key(record_key)
                    && records.len() >= MAX_FILTER_CHAIN_RECORDS_PER_BANK
                {
                    return Err(invalid(
                        "SHINKI_EMU_CMVS_VM_FILTER_CHAIN_BUDGET",
                        "CMVS filter-chain record budget is exhausted",
                    ));
                }
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    records.entry(*record_key)
                {
                    entry.insert(CmvsFilterChainRecord::default());
                    state
                        .filter_chain_record_order
                        .entry(bank)
                        .or_default()
                        .push(*record_key);
                    if tracing::enabled!(tracing::Level::INFO) {
                        tracing::info!(
                            event = "shinki.emu.cmvs.vm.chain_record_appended",
                            pc = state.program_counter,
                            frame = state.current_frame,
                            bank,
                            record = *record_key
                        );
                    }
                }
                state.interpreter_words.insert(result_field_offset, 1);
            } else {
                let flags = state
                    .interpreter_words
                    .get(&u32::from(error_flag_field_offset))
                    .copied()
                    .unwrap_or(0);
                state
                    .interpreter_words
                    .insert(u32::from(error_flag_field_offset), flags | error_flag_mask);
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::UpdateFilterChainRecord {
                table_byte_offset,
                max_bank,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank), (CmvsPs2aCommandStackWordKind::OpaqueU32, record_key), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth)],
        ) => {
            let slot_key = slot_object_key(table_byte_offset, *bank);
            let live = *bank <= u32::from(max_bank)
                && slot_key.is_ok()
                && state.slot_objects.contains_key(&slot_key.unwrap());
            if !live {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            let bank = u8::try_from(*bank).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_FILTER_CHAIN",
                    "CMVS filter-chain bank exceeds the recovered bound",
                )
            })?;
            if let Some(record) = state
                .filter_chain_records
                .get_mut(&bank)
                .and_then(|records| records.get_mut(record_key))
            {
                record.words = [*first, *second, *third, *fourth];
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.chain_record_rect",
                        pc = state.program_counter,
                        frame = state.current_frame,
                        bank,
                        record = *record_key,
                        x = *first,
                        y = *second,
                        w = *third,
                        h = *fourth
                    );
                }
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::UpdateFilterChainParameterBlock {
                table_byte_offset,
                max_bank,
                backend_mode_address,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank), (CmvsPs2aCommandStackWordKind::OpaqueU32, record_key), (CmvsPs2aCommandStackWordKind::OpaqueU32, selector), (CmvsPs2aCommandStackWordKind::OpaqueU32, word), (CmvsPs2aCommandStackWordKind::OpaqueU32, first), (CmvsPs2aCommandStackWordKind::OpaqueU32, second), (CmvsPs2aCommandStackWordKind::OpaqueU32, third), (CmvsPs2aCommandStackWordKind::OpaqueU32, fourth), (CmvsPs2aCommandStackWordKind::OpaqueU32, fifth), (CmvsPs2aCommandStackWordKind::OpaqueU32, sixth)],
        ) => {
            let slot_key = slot_object_key(table_byte_offset, *bank);
            let live = *bank <= u32::from(max_bank)
                && slot_key.is_ok()
                && state.slot_objects.contains_key(&slot_key.unwrap());
            if !live {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            let bank = u8::try_from(*bank).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_FILTER_CHAIN",
                    "CMVS filter-chain bank exceeds the recovered bound",
                )
            })?;
            let Some(record) = state
                .filter_chain_records
                .get_mut(&bank)
                .and_then(|records| records.get_mut(record_key))
            else {
                return Ok(None);
            };
            if *selector >= 4 {
                return Ok(None);
            }
            let block = CmvsFilterChainParameterBlock {
                word: *word,
                short_words: [
                    *first as u16,
                    *second as u16,
                    *third as u16,
                    *fourth as u16,
                    *fifth as u16,
                    *sixth as u16,
                ],
            };
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.chain_param_update",
                    tick = state.tick,
                    pc = state.program_counter,
                    bank = u32::from(bank),
                    record_key = *record_key,
                    selector = *selector,
                    word = *word,
                    first = *first,
                    second = *second,
                    third = *third,
                    fourth = *fourth
                );
            }
            let alternate_backend = state
                .process_global_words
                .get(&backend_mode_address)
                .copied()
                .unwrap_or(0)
                != 0;
            if alternate_backend {
                match *selector {
                    0 => {
                        record.parameter_blocks.insert(0, block);
                    }
                    1 => {
                        record.parameter_blocks.insert(1, block);
                        record.parameter_blocks.insert(2, block);
                    }
                    2 => {
                        record.parameter_blocks.insert(3, block);
                    }
                    _ => {}
                }
            } else {
                record.parameter_blocks.insert(
                    u8::try_from(*selector).map_err(|_| {
                        invalid(
                            "SHINKI_EMU_CMVS_VM_FILTER_CHAIN",
                            "CMVS filter-chain selector exceeds the recovered bound",
                        )
                    })?,
                    block,
                );
            }
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ApplyFilterChainRecords {
                table_byte_offset,
                max_bank,
                error_flag_field_offset,
                error_flag_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank)],
        ) => {
            let slot_key = slot_object_key(table_byte_offset, *bank);
            let live = *bank <= u32::from(max_bank)
                && slot_key.is_ok()
                && state.slot_objects.contains_key(&slot_key.unwrap());
            if !live {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            let bank = u8::try_from(*bank).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_FILTER_BANK",
                    "CMVS filter-chain bank exceeded the recovered range",
                )
            })?;
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.chain_apply",
                    tick = state.tick,
                    pc = state.program_counter,
                    bank = u32::from(bank)
                );
            }
            let revision = state
                .filter_chain_apply_revisions
                .get(&bank)
                .copied()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_FILTER_REVISION",
                        "CMVS filter-chain apply revision overflowed",
                    )
                })?;
            state.filter_chain_apply_revisions.insert(bank, revision);
            // Applying the chain renders the registered entries; the active
            // selection survives the apply so a confirm key returns it.
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::PollFilterChainSelection {
                table_byte_offset,
                max_bank,
                error_flag_field_offset,
                error_flag_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank)],
        ) => {
            if tracing::enabled!(tracing::Level::TRACE) {
                let bank_u8 = *bank as u8;
                tracing::trace!(
                    event = "shinki.emu.cmvs.vm.poll535_probe",
                    bank = bank_u8,
                    records = state
                        .filter_chain_record_order
                        .get(&bank_u8)
                        .map(|order| order.len())
                        .unwrap_or(0),
                    active = state
                        .filter_chain_active_records
                        .get(&bank_u8)
                        .copied()
                        .unwrap_or(u32::MAX)
                );
            }
            let slot_key = slot_object_key(table_byte_offset, *bank);
            let live = *bank <= u32::from(max_bank)
                && slot_key.is_ok()
                && state.slot_objects.contains_key(&slot_key.unwrap());
            if tracing::enabled!(tracing::Level::TRACE) && *bank != 0 {
                tracing::trace!(
                    event = "shinki.emu.cmvs.vm.chain_poll_live",
                    pc = state.program_counter,
                    bank,
                    live
                );
            }
            if !live {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            let bank = u8::try_from(*bank).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_FILTER_CHAIN",
                    "CMVS filter-chain bank exceeds the recovered bound",
                )
            })?;
            // The original poll (`sub_468740`) is synchronous: every call
            // first runs the pointer hover pass — the first record in
            // registration order whose rectangle (`cmd 531` words, node
            // bytes +12..+24) contains the pointer becomes the chain's
            // active record — and then reads the live input latches:
            // directionals move the highlight, a press locks the hovered
            // record, its release confirms the lock (returning the record
            // key), and cancel aborts with -2. An idle poll returns -1;
            // a confirm on a different record returns -10.
            log_chain_state_transition(state);
            pointer_hover_pass(state, bank);
            // The native poll sequences its input branches (`sub_468740`):
            // after the hover pass, the PRESS edge only latches the
            // hovered record (manager dword 273, manager field 12) and
            // falls through to the idle return; the RELEASE edge then
            // confirms the lock, returning the record key when the hover
            // still matches it and -10 otherwise, consuming both advance
            // latches (`sub_45CB80`). A held button drives nothing.
            if state.input_advance_press {
                if let Some(active) = state.filter_chain_active_records.get(&bank).copied() {
                    state.filter_chain_locked_records.insert(bank, active);
                }
            }
            let control: Option<&'static str> = if state.input_cancel_held {
                Some("escape")
            } else if state.input_up_edge {
                Some("arrow_up")
            } else if state.input_down_edge {
                Some("arrow_down")
            } else if state.input_advance_release {
                Some("enter")
            } else {
                None
            };
            if let Some(control) = control {
                let result = apply_filter_chain_control(state, bank, control);
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.chain_control",
                        pc = state.program_counter,
                        frame = state.current_frame,
                        bank,
                        control,
                        result,
                        confirm_held = state.input_confirm_held,
                        advance_release = state.input_advance_release,
                        active = state
                            .filter_chain_active_records
                            .get(&bank)
                            .copied()
                            .unwrap_or(u32::MAX)
                    );
                }
                // The native hit-test consumes a directional edge exactly
                // once (manager dword 271 clears on delivery); without this
                // the script's poll loop re-applies the same edge once per
                // command execution and walks the highlight several items
                // per physical press. The release branch consumes the
                // press latch alongside the release (`sub_45CB80`).
                match control {
                    "arrow_up" => state.input_up_edge = false,
                    "arrow_down" => state.input_down_edge = false,
                    "enter"
                        // `sub_45CB80` clears the advance latches only
                        // when the confirm resolves a record lock. With no
                        // locked record on this bank the release edge must
                        // survive for the menu's latch-driven path
                        // (`cmd 416 -> 640 -> 344`), which the settings
                        // apply depends on.
                        // A poll that resolved a lock (record key) or a
                        // mismatch (-10) owns the edge; an idle bank poll
                        // (-1, nothing registered) leaves it for the
                        // menu's latch-driven `cmd 416` path.
                        if result != u32::MAX => {
                            state.input_advance_release = false;
                            state.input_advance_press = false;
                        }
                    _ => {}
                }
                state.interpreter_words.insert(result_field_offset, result);
                return Ok(None);
            }
            // Idle polls report -1 (`sub_468740`'s no-input branch): the
            // hover pass above already moved the active record, and the
            // dispatcher's message gate only consumes confirmed keys.
            state
                .interpreter_words
                .insert(result_field_offset, u32::MAX);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ReadActiveFilterChainSelection {
                table_byte_offset,
                max_bank,
                error_flag_field_offset,
                error_flag_mask,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank)],
        ) => {
            let slot_key = slot_object_key(table_byte_offset, *bank);
            let live = *bank <= u32::from(max_bank)
                && slot_key.is_ok()
                && state.slot_objects.contains_key(&slot_key.unwrap());
            if !live {
                let field = u32::from(error_flag_field_offset);
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state
                    .interpreter_words
                    .insert(field, flags | error_flag_mask);
                return Ok(None);
            }
            let bank = u8::try_from(*bank).map_err(|_| {
                invalid(
                    "SHINKI_EMU_CMVS_VM_FILTER_CHAIN",
                    "CMVS filter-chain bank exceeds the recovered bound",
                )
            })?;
            // `sub_468690` is a pure read: the chain's active record key,
            // or -1 when nothing is hovered. The save-screen loop's -2
            // exit comes from the poll command's escape path, not this
            // read.
            log_chain_state_transition(state);
            let result = state
                .filter_chain_active_records
                .get(&bank)
                .copied()
                .unwrap_or(u32::MAX);
            if tracing::enabled!(tracing::Level::INFO) && state.program_counter == 410_128 {
                tracing::info!(
                    event = "shinki.emu.cmvs.vm.loop_cmd534",
                    result,
                    bank,
                    active = state.filter_chain_active_records.get(&bank).copied(),
                    tick = state.tick
                );
            }
            state.interpreter_words.insert(result_field_offset, result);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::ClickCommitFilterChain {
                max_bank,
                result_field_offset,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank), (CmvsPs2aCommandStackWordKind::OpaqueU32, key), (CmvsPs2aCommandStackWordKind::OpaqueU32, warp)],
        ) => {
            // `sub_485F50` → `sub_468390`: look the record up by key in
            // the bank's chain and commit it: the record becomes the
            // chain's active selection, and when the warp flag is set the
            // pointer is moved onto the record's rectangle — the native
            // warps to the rectangle center (`SetCursorPos`), which keeps
            // the following poll's hover pass on the committed record.
            // A miss (unknown key, empty chain) reports 0; a commit
            // reports 1. The result lands in the same message word 81220
            // the poll feeds.
            if *bank > max_bank {
                state.interpreter_words.insert(result_field_offset, 0);
                return Ok(None);
            }
            let Ok(bank) = u8::try_from(*bank) else {
                state.interpreter_words.insert(result_field_offset, 0);
                return Ok(None);
            };
            let record = state
                .filter_chain_records
                .get(&bank)
                .and_then(|records| records.get(key));
            let Some(record) = record else {
                state.interpreter_words.insert(result_field_offset, 0);
                return Ok(None);
            };
            if *warp != 0 {
                let [x, y, w, h] = record.words;
                state.pointer_x = x.wrapping_add(w / 2) as i32;
                state.pointer_y = y.wrapping_add(h / 2) as i32;
            }
            if state.filter_chain_active_records.get(&bank) != Some(key) {
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "shinki.emu.cmvs.vm.chain_click_commit",
                        pc = state.program_counter,
                        frame = state.current_frame,
                        bank,
                        record = *key,
                        warp = *warp
                    );
                }
                state.filter_chain_active_records.insert(bank, *key);
            }
            state.interpreter_words.insert(result_field_offset, 1);
            Ok(None)
        }
        (
            CmvsPs2aCommandEffectKind::CreateChainHotspotRecord {
                max_bank,
                max_channel,
                error_flag_field_offset,
                error_mask,
            },
            [(CmvsPs2aCommandStackWordKind::OpaqueU32, bank), (CmvsPs2aCommandStackWordKind::OpaqueU32, channel), (CmvsPs2aCommandStackWordKind::OpaqueU32, _extra), (CmvsPs2aCommandStackWordKind::OpaqueU32, _name)],
        ) => {
            // `sub_486130`: free the bank's previous chain and install a
            // fresh single-record chain (`sub_468330`): record key 0, and
            // the rectangle read from the channel's base quad (origin +
            // quad 0's x/y/w/h). The trailing stack words only feed the
            // node's auxiliary fields (sound binding), which nothing reads
            // back. The poll over this chain confirms key 0 — no engine
            // message — while `cmd 538` commits the hotspot by that key.
            if *bank > max_bank || *channel > max_channel {
                let field = error_flag_field_offset;
                let flags = state.interpreter_words.get(&field).copied().unwrap_or(0);
                state.interpreter_words.insert(field, flags | error_mask);
                return Ok(None);
            }
            let Ok(bank) = u8::try_from(*bank) else {
                return Ok(None);
            };
            // The hotspot installs into the bank's chain slot table; the
            // native creator materializes the slot the same way the
            // chain-allocation command (`cmd 528`) does.
            let bank_key = slot_object_key(3196, u32::from(bank))?;
            if !state.slot_objects.contains_key(&bank_key) {
                let identity = state.next_slot_object_id;
                state.next_slot_object_id = identity.checked_add(1).ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_VM_SLOT_OBJECTS",
                        "CMVS slot object identity space overflowed",
                    )
                })?;
                state.slot_objects.insert(bank_key, identity);
            }
            let rect = state
                .effect_channels
                .get(&u8::try_from(*channel).unwrap_or(u8::MAX))
                .map(|object| {
                    let (origin_x, origin_y) = object.origin();
                    let [x, y, w, h] = object.quad_rect(0);
                    [
                        (origin_x.wrapping_add(x)) as u32,
                        (origin_y.wrapping_add(y)) as u32,
                        w as u32,
                        h as u32,
                    ]
                })
                .unwrap_or([0, 0, 0, 0]);
            let record = CmvsFilterChainRecord {
                words: rect,
                ..Default::default()
            };
            state
                .filter_chain_records
                .insert(bank, [(0_u32, record)].into());
            state.filter_chain_record_order.insert(bank, vec![0_u32]);
            state.filter_chain_active_records.remove(&bank);
            state.filter_chain_locked_records.remove(&bank);
            // The layout-completion callback: the native engine raises
            // process word 2212 once the stage's layout render finishes
            // (`sub_486130` closes the layout pass; the next scenario pass
            // takes the `proc[2212] != 0` branch at `intproc` 345790). The
            // headless renderer is synchronous, so the completion is
            // modeled here at the callback's native trigger point.
            state.process_indexed_words.insert(2212, 1);
            log_chain_state_transition(state);
            Ok(None)
        }
        _ => Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
            format!(
                "filters contract mismatch at pc {} frame {}: {:?}",
                state.program_counter, state.current_frame, effect_kind
            ),
        )),
    }
}
