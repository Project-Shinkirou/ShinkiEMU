//! Executor for the mechanically recovered native micro programs
//! (`command::native_table`).  Ops run in order; guards skip forward by
//! op count.  All field offsets are byte offsets into
//! `interpreter_words`.

use super::*;

/// The interpreter's pinned result slot (`this[0x13D44]`).
const RESULT_FIELD: u32 = 0x13D44;

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    ops: &[crate::command::native_table::NativeMicroOp],
    values: &[(crate::CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    use crate::command::native_table::{NativeMicroMacro, NativeMicroOp, NativeMicroSrc};
    let mut regs = [0u32; 8];
    for (i, (_, v)) in values.iter().take(8).enumerate() {
        regs[i] = *v;
    }
    let mut pc = 0usize;
    while pc < ops.len() {
        let op = ops[pc];
        let val = |s: NativeMicroSrc, regs: &[u32; 8]| -> Result<u32, CoreError> {
            match s {
                NativeMicroSrc::Reg(i) => Ok(regs[(i as usize) & 7]),
                NativeMicroSrc::Field(off) => state.read_interpreter_word(off),
                NativeMicroSrc::Const(v) => Ok(v),
            }
        };
        match op {
            NativeMicroOp::SetField {
                byte_offset,
                src: s,
            } => {
                let v = val(s, &regs)?;
                state.write_interpreter_word(byte_offset, v);
            }
            NativeMicroOp::OrField { byte_offset, mask } => {
                let v = state.read_interpreter_word(byte_offset)? | mask;
                state.write_interpreter_word(byte_offset, v);
            }
            NativeMicroOp::SetResult(s) => {
                let v = val(s, &regs)?;
                state.write_interpreter_word(RESULT_FIELD, v);
            }
            NativeMicroOp::Move { dst, src: s } => regs[(dst as usize) & 7] = val(s, &regs)?,
            NativeMicroOp::BinOp { dst, src: s, kind } => {
                let a = regs[(dst as usize) & 7];
                let b = val(s, &regs)?;
                regs[(dst as usize) & 7] = bin_op(kind, a, b);
            }
            NativeMicroOp::Guard {
                lhs,
                kind,
                rhs,
                taken_ops,
            } => {
                let (l, r) = (val(lhs, &regs)?, val(rhs, &regs)?);
                if cmp_holds(kind, l, r) {
                    pc += usize::from(taken_ops);
                }
            }
            NativeMicroOp::Call(m) => match m {
                NativeMicroMacro::Timestamp => {
                    regs[7] = u32::try_from(state.tick).unwrap_or(u32::MAX)
                }
                NativeMicroMacro::ContextRead {
                    slot_reg,
                    index_reg,
                } => {
                    regs[7] = context_read(
                        state,
                        regs[(slot_reg as usize) & 7],
                        regs[(index_reg as usize) & 7],
                    );
                }
            },
        }
        pc += 1;
    }
    Ok(None)
}

fn bin_op(kind: crate::command::native_table::NativeMicroBinKind, a: u32, b: u32) -> u32 {
    match kind {
        crate::command::native_table::NativeMicroBinKind::Sub => a.wrapping_sub(b),
        crate::command::native_table::NativeMicroBinKind::Mul => a.wrapping_mul(b),
        crate::command::native_table::NativeMicroBinKind::Div => a.checked_div(b).unwrap_or(0),
        crate::command::native_table::NativeMicroBinKind::Or => a | b,
        crate::command::native_table::NativeMicroBinKind::Shl => a << (b & 31),
    }
}

fn cmp_holds(kind: crate::command::native_table::NativeMicroCmpKind, l: u32, r: u32) -> bool {
    match kind {
        crate::command::native_table::NativeMicroCmpKind::Eq => l == r,
        crate::command::native_table::NativeMicroCmpKind::Ne => l != r,
        crate::command::native_table::NativeMicroCmpKind::Lt => (l as i32) < (r as i32),
        crate::command::native_table::NativeMicroCmpKind::Le => (l as i32) <= (r as i32),
        crate::command::native_table::NativeMicroCmpKind::Ge => (l as i32) >= (r as i32),
        crate::command::native_table::NativeMicroCmpKind::GeU => l >= r,
        crate::command::native_table::NativeMicroCmpKind::LtU => l < r,
    }
}

/// `sub_444EF0` on `this[481 + slot]`: the slot's sub-object exposes
/// dword reads at `index + 1`; unbound slots read zero.  The context
/// table base is dword 481 (byte 0x784), matching the texture slot-object keys.
fn context_read(state: &CmvsPs2aVmState, slot: u32, index: u32) -> u32 {
    if slot >= 0x100 || index >= 0x400 {
        return 0;
    }
    let key = 0x784_u32 << 8 | slot;
    state
        .slot_object_seed_words
        .get(&key)
        .and_then(|words| words.get(index as usize + 1).copied())
        .unwrap_or(0)
}
