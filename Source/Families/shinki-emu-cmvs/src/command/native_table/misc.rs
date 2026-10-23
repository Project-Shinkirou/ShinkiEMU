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
    // 1 (cmd 280): font-cache slot gate; the then-branch only calls the
    // omitted deep callee, so the program reduces to the 0x1000 raise.
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(12),
            taken_ops: 24,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3000),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 23,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(1),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3004),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 21,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(2),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3008),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 19,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(3),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3012),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 17,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(4),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3016),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 15,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(5),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3020),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 13,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(6),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3024),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 11,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(7),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3028),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 9,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(8),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3032),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 7,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(9),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3036),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 5,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(10),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3040),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(11),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3044),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x1000,
        },
    ],
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
    // 7 (cmd 816): string-table slot gate; the 0x608-byte table build on
    // the slot object is an omitted deep call.
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
    // 8 (cmd 916): glyph-cache slot gate, mirroring program 1 over the
    // eight-entry table at byte 3100.
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(8),
            taken_ops: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3100),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 15,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(1),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3104),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 13,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(2),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3108),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 11,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(3),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3112),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 9,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(4),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3116),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 7,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(5),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3120),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 5,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(6),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3124),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(7),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3128),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x1000,
        },
    ],
    // 9 (cmd 961): string-table record construction only; no
    // interpreter writes.
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
        // Handler `sub_488960` (case 280): font-cache selector. With
        // font index values[0] below twelve and the font-object table
        // `this[750..761]` (bytes 3000..3044) bound at that index, it
        // calls `sub_464980(font_object, values[1] != 0)` — an omitted
        // deep call that only refreshes the font manager's +0x68 cached
        // id through `sub_461F20`/`sub_461790`; otherwise it raises
        // mask 0x1000 at byte 10532. The unrolled slot gates read the
        // live interpreter words, so font-loading contracts that publish
        // those words keep this faithful; a negative index (native reads
        // `this[750+index]` below the table) always raises here.
        // Returns 0x4008.
        280 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 5,
                table_index: 1,
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
        // Handler `sub_481BB0` (case 816; decompile failed, recovered
        // from the raw asm): the 0x10 gate mirrors case 786 — values[0]
        // at or above 0x100 (signed `jge`) or a null
        // `this[481+values[0]]` slot raises 0x10 at byte 10532. The
        // success tail calls `sub_4458C0(slot_object)`: allocates a
        // 0x608-byte block (`sub_4A9BD1`), constructs it via
        // `sub_447C30` and stores the result at slot_object+0x101C — an
        // omitted deep call with no interpreter writes. The 481-table
        // occupancy gate is executor-side slot state, treated as bound.
        // Returns 0x4004.
        816 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 5,
                table_index: 7,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_488050` (case 916): glyph-cache selector, the
        // eight-entry twin of case 280 over `this[775..782]` (bytes
        // 3100..3128). With values[0] below eight and the glyph object
        // bound, calls `sub_43A370(glyph_object, values[1] != 0)` —
        // omitted deep call refreshing the +0x78 cached id through
        // `sub_461F20`/`sub_461790`; otherwise raises 0x1000. A negative
        // index always raises here (native reads below the table).
        // Returns 0x4008.
        916 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 5,
                table_index: 8,
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
        // Handler `sub_47BF50` (case 961): string-table record
        // construction. Reads the string-table manager at `this[790]`
        // (byte 3160) and calls `sub_413FC0(manager, values[0],
        // values[1] as u16)`: LocalAlloc(0x88) per record, per-entry
        // string buffers, linked through the manager's list with the
        // head parked at manager+4. All writes land in the engine-side
        // manager object, none in `interpreter_words`; the callee is
        // omitted. Returns 0x4008.
        961 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 5,
                table_index: 9,
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
        _ => return None,
    };
    Some(fields)
}
