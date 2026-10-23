//! Bulk-native command contracts recovered from the CMVS 3.90 dispatch
//! table handlers outside the semantic families.
//!
//! Each contract is mechanically derived from a decompiled handler
//! (`.tmp/cmvs-port/missing_handler_decomp.json`, produced from the
//! original `engine.exe`); the doc comment on every arm cites the
//! handler address so the derivation stays auditable.  Commands whose
//! handler bodies reduce to interpreter-word moves, error masks and
//! fixed result codes are expressed as `NativeMicroProgram` op lists;
//! handlers with subsystem side effects use dedicated effect kinds in
//! the owning module.

mod audio;
mod bulk_high;
mod bulk_low;
mod layers;
mod misc;
mod settings;
mod video;

use super::CommandFields;

/// Source of a value inside a native micro program.  Registers hold
/// stack operands resolved by the dispatcher before execution.
pub(crate) use super::{CmvsPs2aCommandEffectKind, CmvsPs2aCommandStackWord};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeMicroSrc {
    /// Stack operand at `values[index]` (top-down order of the contract).
    Reg(u8),
    /// `interpreter_words[byte_offset]`.
    Field(u32),
    /// A fixed dword from the handler body.
    Const(u32),
}

/// One recovered step of a native command handler.  Field offsets are
/// byte offsets into the interpreter word space, matching
/// `CmvsPs2aVmState::interpreter_words`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeMicroOp {
    /// `interpreter_words[byte_offset] = src`.
    SetField {
        byte_offset: u32,
        src: NativeMicroSrc,
    },
    /// `interpreter_words[byte_offset] |= mask` (native error-mask raise).
    OrField { byte_offset: u32, mask: u32 },
    /// Result slot `this[0x13D44]` = src.
    SetResult(NativeMicroSrc),
    /// `reg[a] = src` without touching interpreter state.
    Move { dst: u8, src: NativeMicroSrc },
    /// `reg[a] = reg[a] op src`.
    BinOp {
        dst: u8,
        src: NativeMicroSrc,
        kind: NativeMicroBinKind,
    },
    /// Skip the next `taken_ops` ops when the comparison holds.
    Guard {
        lhs: NativeMicroSrc,
        kind: NativeMicroCmpKind,
        rhs: NativeMicroSrc,
        taken_ops: u8,
    },
    /// Identified engine-side helper whose observable effect the
    /// executor reproduces (see `native_micro.rs`).
    Call(NativeMicroMacro),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeMicroBinKind {
    Sub,
    Mul,
    Div,
    Or,
    Shl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeMicroCmpKind {
    Eq,
    Ne,
    Lt,
    Le,
    Ge,
    GeU,
    LtU,
}

/// Helpers shared by more than one recovered handler, keyed by their
/// native address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeMicroMacro {
    /// `sub_41A000`: monotonic millisecond timestamp latch.
    Timestamp,
    /// `sub_444EF0` on the context table (`this[481 + slot]`): read
    /// `sub_object[index + 1]`; zero for an unbound slot.
    ContextRead { slot_reg: u8, index_reg: u8 },
}

/// Per-cluster micro-program registries.  A contract references its
/// program as `(cluster, table_index)` so each owning module appends
/// entries without touching a shared table.
pub(crate) fn micro_ops(cluster: u8, table_index: u8) -> &'static [NativeMicroOp] {
    let tables: &[&[NativeMicroOp]] = match cluster {
        0 => bulk_low::OPS,
        1 => bulk_high::OPS,
        2 => settings::OPS,
        3 => layers::OPS,
        4 => audio::OPS,
        5 => misc::OPS,
        _ => video::OPS,
    };
    tables.get(usize::from(table_index)).copied().unwrap_or(&[])
}

pub(crate) fn lookup(command_id: u16) -> Option<CommandFields> {
    bulk_low::lookup(command_id)
        .or_else(|| bulk_high::lookup(command_id))
        .or_else(|| settings::lookup(command_id))
        .or_else(|| layers::lookup(command_id))
        .or_else(|| audio::lookup(command_id))
        .or_else(|| misc::lookup(command_id))
        .or_else(|| video::lookup(command_id))
}
