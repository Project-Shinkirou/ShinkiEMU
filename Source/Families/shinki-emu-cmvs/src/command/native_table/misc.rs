use super::super::{
    CmvsPs2aCommandEffectKind, CmvsPs2aCommandStackWord, CmvsPs2aCommandStackWordKind,
};
use super::*;

/// Micro programs owned by this cluster, indexed by the local
/// `table_index` of contract arms.
pub(super) const OPS: &[&[NativeMicroOp]] = &[
    // 0 (cmd 131): reg0 = values[0] << 16 | values[1].
    &[
        NativeMicroOp::BinOp {
            dst: 0,
            src: NativeMicroSrc::Const(16),
            kind: NativeMicroBinKind::Shl,
        },
        NativeMicroOp::BinOp {
            dst: 0,
            src: NativeMicroSrc::Reg(1),
            kind: NativeMicroBinKind::Or,
        },
        NativeMicroOp::SetField {
            byte_offset: 10556,
            src: NativeMicroSrc::Reg(0),
        },
    ],
    // Reserved positional entry after the typed 280 cutover.
    &[],
    // 2 (cmd 495): container/slot-index gates; the context read and its
    // tail calls are omitted deep calls with no interpreter effect.
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Lt,
            rhs: NativeMicroSrc::Const(0x100),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x10,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(0x100),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Lt,
            rhs: NativeMicroSrc::Const(0x400),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x20,
        },
    ],
    // 3 (cmd 652): save-slot staging call only; no interpreter writes.
    &[],
    // 4 (cmd 653): save-slot staging call only; no interpreter writes.
    &[],
    // 5 (cmd 654): save-slot staging call only; no interpreter writes.
    &[],
    // 6 (cmd 786): string-table slot gate; the record construction tail
    // is an omitted deep call with no interpreter effect.
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Lt,
            rhs: NativeMicroSrc::Const(0x100),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x10,
        },
    ],
    // Reserved positional entry after the typed 816 cutover.
    &[],
    // Reserved positional entry after the typed 916 cutover.
    &[],
];

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    let fields: CommandFields = match command_id {
        // Dispatcher `sub_472F10` inline case body at 0x4733C3 (no
        // handler VA; the census lists it as a stub because it makes no
        // call): packs the two consumed words as
        // `this[10556] = values[0] << 16 | values[1]` (raw asm
        // `mov [esi+293Ch], edx`). Returns 0x4008.
        131 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 5,
                table_index: 0,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // 488960 -> 464980 -> 461F20/461790: per-text-owner bold,
        // false=400, true=600; only a changed bool clears the cache.
        280 => (
            8,
            CmvsPs2aCommandEffectKind::NativeText {
                operation: crate::CmvsNativeTextOperation::Bold,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_482680` (case 495; decompile failed, recovered
        // from the raw asm): slot/index gates around a context read.
        // values[0] at or above 0x100 (signed `jge`) raises 0x10; a null
        // `this[481+values[0]]` container raises 0x10; values[1] at or
        // above 0x400 raises 0x20 — each failure returns immediately, so
        // the slot gate re-test below skips the index gate after raising.
        // The success tail `sub_444EF0(container, index)` ->
        // `sub_445340` (`obj[1030]`) -> `sub_42BF50` (frees the +0x7C
        // scratch buffer) discards its result and writes no interpreter
        // state, so all three calls are omitted. The 481-table
        // occupancy gate is executor-side slot state (`slot_objects`,
        // byte 1924) that a micro program cannot read; it is treated as
        // bound here. Returns 0x4008.
        495 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 5,
                table_index: 2,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485580` (case 652): with both save-slot numbers
        // values[0] and values[1] at or below 0x3E7 (unsigned `ja` skips
        // the call), stages save-slot text through `sub_472BA0`: three
        // LocalAlloc(0x800) buffers, sprintf 0x48A8E0 of the slot ids
        // against the `this+0x151C` name buffer with formats
        // 0x4D2D0C/0x4D2D1C, and registry insert `sub_495690`. All
        // writes land in engine-owned staging memory, none in
        // `interpreter_words`, so the callee is omitted; the range gate
        // has no observable branch difference. Returns 0x4008.
        652 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 5,
                table_index: 3,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485550` (case 653): single-slot staging through
        // `sub_472AF0` when values[0] is at or below 0x3E7 (unsigned
        // `ja` skips) — LocalAlloc(0x800), sprintf against `this+0x151C`
        // (format 0x4D2D0C), frees the buffer, then the
        // `values[0] < 0xA` ten-slot menu rebuild reads `this+0xCEC`,
        // `this+0x28D4..0x28DC` and re-registers entries via
        // `sub_495690`. Reads only; every write is engine-side staging,
        // so the callee is omitted. Returns 0x4004.
        653 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 5,
                table_index: 4,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485510` (case 654): the twin of case 652 gating
        // on values[0]/values[1] at or below 0x3E7 (unsigned `ja`),
        // staging through `sub_472990` (three LocalAlloc(0x800) buffers,
        // sprintf formats 0x4D2D0C/0x4D2D2C over `this+0x151C`).
        // Engine-side writes only; callee omitted. Returns 0x4008.
        654 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 5,
                table_index: 5,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_484380` (case 786): string-table slot gate.
        // values[0] at or above 0x100 (signed `jge`) or a null
        // `this[481+values[0]]` slot raises 0x10 at byte 10532 and
        // returns; the success tail reads the string-table manager at
        // `this[480]` (`sub_432F10` returns its first dword) and calls
        // `sub_4470F0(slot_object, word)`, which dispatches through the
        // slot object's +0x1010 hook (`sub_43EFF0`). Those calls write
        // no interpreter state and are omitted; as in case 495, the
        // 481-table occupancy gate lives in executor-side slot state and
        // is treated as bound. Returns 0x4004.
        786 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 5,
                table_index: 6,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // 481BB0 -> 4458C0 constructs an empty native XYZ collision owner.
        816 => (
            4,
            CmvsPs2aCommandEffectKind::NativeHitPolygon { command_id },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // 488050 -> 43A370: independent eight-slot glyph owner weight.
        916 => (
            8,
            CmvsPs2aCommandEffectKind::NativeText {
                operation: crate::CmvsNativeTextOperation::GlyphBold,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // 47BF50 -> 413FC0: main+3164 bitmap manager, cell index at
        // stack-4 and character code truncated to u16 at stack-8.
        961 => (
            8,
            CmvsPs2aCommandEffectKind::NativeCustomGlyph { command_id },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    };
    Some(fields)
}
