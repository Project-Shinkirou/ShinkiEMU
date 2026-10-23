use super::*;

/// Rebuilds the per-frame entry queue (`sub_46DF10`).
///
/// At every frame boundary the original scheduler pushes the suspended
/// `(delay counter, pc, frame)` triple back onto the value stack, pushes
/// `(0, entry_pc, entry_frame)` for every active per-frame entry slot, and
/// finally switches `(13216, 13220, 13224)` to the topmost triple, which is
/// the last activated slot. The dispatch loop then consumes queued triples
/// through `0x410`/`0x411`/`0x414` transfers until `command 0` returns
/// `0xC000` and ends the frame. The whole re-queue is gated on the
/// scheduler-enable word that `command 137` sets; the 64 per-frame enable
/// flags (`command 137`'s companion table at the record base) are cleared
/// unconditionally afterwards, so a flag set in one frame never survives
/// into the next.
/// Applies one delivered control to the case-535 filter-chain selection and
/// returns the value the original `sub_468740` poll would report: the active
/// record id on confirm, all-ones while a directional edge only moves the
/// highlight or the poll stays idle, and -2 on escape.
pub(super) fn apply_filter_chain_control(
    state: &mut CmvsPs2aVmState,
    bank: u8,
    control: &str,
) -> u32 {
    let order = state
        .filter_chain_record_order
        .get(&bank)
        .cloned()
        .unwrap_or_default();
    let active = state.filter_chain_active_records.get(&bank).copied();
    let locked = state.filter_chain_locked_records.get(&bank).copied();
    // The keyboard branches of the native poll warp the cursor onto the
    // selected record's rectangle center (`sub_468740`'s `SetCursorPos`
    // block) before the hover pass runs, so a keyboard-driven selection
    // survives the next poll's pointer hit test. Mirror the warp here.
    let warp_to_record = |state: &mut CmvsPs2aVmState, record: u32| {
        if let Some([x, y, w, h]) = state
            .filter_chain_records
            .get(&bank)
            .and_then(|records| records.get(&record))
            .map(|record| record.words)
        {
            state.pointer_x = (x.wrapping_add(w / 2)) as i32;
            state.pointer_y = (y.wrapping_add(h / 2)) as i32;
        }
    };
    match control {
        "arrow_up" | "arrow_left" => {
            state.filter_chain_locked_records.remove(&bank);
            if let Some(next) = previous_selection(&order, active) {
                state.filter_chain_active_records.insert(bank, next);
                warp_to_record(state, next);
            }
            u32::MAX
        }
        "arrow_down" | "arrow_right" => {
            state.filter_chain_locked_records.remove(&bank);
            if let Some(next) = next_selection(&order, active) {
                state.filter_chain_active_records.insert(bank, next);
                warp_to_record(state, next);
            }
            u32::MAX
        }
        // `Menu_pump`: the release confirms the locked item, returning its
        // id; a release without a matching lock reports -10 — the press
        // edge latched the lock earlier in the same poll (`sub_468740`'s
        // dword-273 branch), so the confirm never re-locks here.
        "enter" | "space" => match (active, locked) {
            (Some(active), Some(locked)) if active == locked => {
                state.filter_chain_locked_records.remove(&bank);
                active
            }
            _ => (-10_i32) as u32,
        },
        "escape" => {
            state.filter_chain_locked_records.remove(&bank);
            (-2_i32) as u32
        }
        _ => u32::MAX,
    }
}

pub(super) fn previous_selection(order: &[u32], active: Option<u32>) -> Option<u32> {
    let Some(active) = active else {
        return order.last().copied();
    };
    let index = order.iter().position(|record| *record == active)?;
    index
        .checked_sub(1)
        .and_then(|index| order.get(index).copied())
        .or_else(|| order.last().copied())
}

pub(super) fn next_selection(order: &[u32], active: Option<u32>) -> Option<u32> {
    match active.and_then(|active| order.iter().position(|record| *record == active)) {
        Some(index) => order
            .get(index + 1)
            .copied()
            .or_else(|| order.first().copied()),
        None => order.first().copied(),
    }
}
