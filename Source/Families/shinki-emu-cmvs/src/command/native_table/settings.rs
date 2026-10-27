use super::*;
use crate::{CmvsPs2aCommandEffectKind, CmvsPs2aCommandStackWord, CmvsPs2aCommandStackWordKind};

/// Micro programs owned by this cluster, indexed by the local
/// `table_index` of contract arms.
pub(super) const OPS: &[&[NativeMicroOp]] = &[
    // table_index 0 — case 5 (`sub_47BFF0`).
    &[
        NativeMicroOp::Move {
            dst: 0,
            src: NativeMicroSrc::Field(9648),
        },
        NativeMicroOp::Move {
            dst: 1,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 1,
        }),
        NativeMicroOp::SetResult(NativeMicroSrc::Reg(7)),
    ],
    // table_index 1 — case 6 (`sub_47C010`): body fully omitted.
    &[],
    // table_index 2 — case 7 (`sub_4846A0`).
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
    // table_index 3 — case 9 (`sub_47C290`).
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
    // table_index 4 — case 10 (`sub_486D70`).
    &[
        NativeMicroOp::Move {
            dst: 5,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::Move {
            dst: 5,
            src: NativeMicroSrc::Const(1),
        },
        NativeMicroOp::SetField {
            byte_offset: 9652,
            src: NativeMicroSrc::Reg(5),
        },
    ],
    // table_index 5 — case 13 (`sub_4870C0`).
    &[
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 81240,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 81244,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // table_index 6 — case 14 (`sub_47D550`).
    &[
        NativeMicroOp::Call(NativeMicroMacro::Timestamp),
        NativeMicroOp::SetField {
            byte_offset: 15148,
            src: NativeMicroSrc::Reg(7),
        },
    ],
    // table_index 7 — case 15 (`sub_47D570`).
    &[
        NativeMicroOp::Call(NativeMicroMacro::Timestamp),
        NativeMicroOp::BinOp {
            dst: 7,
            src: NativeMicroSrc::Field(15148),
            kind: NativeMicroBinKind::Sub,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Reg(7)),
    ],
    // table_index 8..=15 — cases 16-20/23/26/28: bodies fully omitted.
    &[],
    &[],
    &[],
    &[],
    &[],
    &[],
    &[],
    &[],
    // table_index 16 — case 37 (`sub_480470`).
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Lt,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x10,
        },
    ],
    // table_index 17 — case 79 (`sub_482FA0`), moved from the former tail.
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 7,
        },
        NativeMicroOp::SetField {
            byte_offset: 81252,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 81256,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 81224,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 81228,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 81232,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Lt,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x10,
        },
    ],
    // table_index 18 — case 45 (`sub_480510`).
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Lt,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x10,
        },
    ],
    // table_index 19 — case 49 (`sub_483F70`).
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Lt,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x10,
        },
    ],
    // table_index 20 — case 50 (`sub_483CD0`).
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Lt,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x10,
        },
    ],
    // table_index 21 — case 53 (`sub_483B80`): same shape as case 50.
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Lt,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x10,
        },
    ],
    // table_index 22 — case 55 (`sub_483E00`): same shape as case 50.
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Lt,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x10,
        },
    ],
    // table_index 23 — case 57 (`sub_480790`).
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Lt,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x10,
        },
    ],
    // table_index 24 — case 61 (`sub_483C40`).
    &[
        NativeMicroOp::Move {
            dst: 5,
            src: NativeMicroSrc::Const(1),
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 4,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 3,
        },
        NativeMicroOp::Move {
            dst: 5,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(5),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x10,
        },
    ],
    // table_index 25 — case 62 (`sub_484460`): same shape as case 61.
    &[
        NativeMicroOp::Move {
            dst: 5,
            src: NativeMicroSrc::Const(1),
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 4,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 3,
        },
        NativeMicroOp::Move {
            dst: 5,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(5),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x10,
        },
    ],
];

const fn word(offset: u8, kind: CmvsPs2aCommandStackWordKind) -> CmvsPs2aCommandStackWord {
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: offset,
        kind,
    }
}

const fn opaque(offset: u8) -> CmvsPs2aCommandStackWord {
    word(offset, CmvsPs2aCommandStackWordKind::OpaqueU32)
}
const fn boolean(offset: u8) -> CmvsPs2aCommandStackWord {
    word(offset, CmvsPs2aCommandStackWordKind::BooleanU32)
}
const fn string(offset: u8) -> CmvsPs2aCommandStackWord {
    word(offset, CmvsPs2aCommandStackWordKind::TaggedStringReference)
}

// Shared stack-word suffixes. Named consts are required because rvalue
// static promotion does not apply to `const fn` call expressions.
const BOOLEAN_TOP: [CmvsPs2aCommandStackWord; 1] = [boolean(4)];
const SLOT_TOP: [CmvsPs2aCommandStackWord; 1] = [opaque(4)];
const SLOT_INDEX: [CmvsPs2aCommandStackWord; 2] = [opaque(4), opaque(8)];
const SLOT_NAME: [CmvsPs2aCommandStackWord; 2] = [opaque(4), string(8)];
const SLOT_NAME_NAME: [CmvsPs2aCommandStackWord; 3] = [opaque(4), string(8), string(12)];
const SLOT_NAME_BOOL_BOOL: [CmvsPs2aCommandStackWord; 4] =
    [opaque(4), string(8), boolean(12), boolean(16)];
const NAME_NAME_NAME_SELECTOR: [CmvsPs2aCommandStackWord; 4] =
    [string(4), string(8), string(12), opaque(16)];

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // Handler `sub_47BFF0` (case 5): `this[20305]` = `sub_428F40`
        // (`this[2412]`), a context query on the dword-2412 sub-object;
        // modeled as `ContextRead` on the dword-2412 slot key. Returns
        // 0x4000.
        5 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 0,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47C010` (case 6): passes the zero/non-zero top word
        // to `sub_41A200` (engine-side setter with no interpreter-word
        // writes — omitted). Returns 0x4004.
        6 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 1,
            },
            &BOOLEAN_TOP as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4846A0` (case 7): `this[20305]` = `sub_41A0F0()`,
        // a no-argument engine getter whose value is unrecoverable from
        // the corpus; the recovered subset publishes zero. Returns 0x4000.
        7 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 2,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47C290` (case 9): `this[20305]` = `sub_419AA0()`,
        // a no-argument engine getter whose value is unrecoverable from
        // the corpus; the recovered subset publishes zero. Returns 0x4000.
        9 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 3,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486D70` (case 10): `this[2413]` = (top word != 0),
        // a normalized boolean store. Returns 0x4004.
        10 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 4,
            },
            &BOOLEAN_TOP as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4870C0` (case 13): `GetLocalTime` snapshot into
        // `this[20309..=20311]` (date, time, day-of-week). The headless
        // runtime has no wall-clock source, so the recovered subset
        // publishes the zero default while keeping the three writes.
        // Returns 0x4000.
        13 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 5,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47D550` (case 14): `this[3787]` = `sub_41A000()`
        // millisecond timestamp latch. Returns 0x4000.
        14 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 6,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47D570` (case 15): `this[20305]` =
        // `sub_41A000()` - `this[3787]` (elapsed-ms delta). Returns 0x4000.
        15 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 7,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47BC30` (case 16): builds "<settings prefix at
        // this+10764><resolved name>" via `sub_427280` (`lstrcpyA`, a
        // read of the interpreter string buffer) and hands it to
        // `sub_48AE50` (five-channel name load — engine-side, omitted).
        // No interpreter-word writes. Returns 0x4008. Omitted callees:
        // sub_427280, sub_48AE50.
        16 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 8,
            },
            &SLOT_NAME as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47B720` (case 17): same settings-prefix name build
        // as case 16, delivered to `sub_4405F0` (ten-channel name load —
        // engine-side, omitted). Returns 0x4008. Omitted callees:
        // sub_427280, sub_4405F0.
        17 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 9,
            },
            &SLOT_NAME as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47BED0` (case 18): settings-prefix name build
        // delivered to `sub_48AE50` (engine-side, omitted). Returns
        // 0x4008. Omitted callees: sub_427280, sub_48AE50.
        18 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 10,
            },
            &SLOT_NAME as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47B980` (case 19): settings-prefix name build
        // delivered to `sub_48AE50` (engine-side, omitted). Returns
        // 0x4008. Omitted callees: sub_427280, sub_48AE50.
        19 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 11,
            },
            &SLOT_NAME as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47BDA0` (case 20): settings-prefix name build
        // delivered to `sub_48AE50` (engine-side, omitted). Returns
        // 0x4008. Omitted callees: sub_427280, sub_48AE50.
        20 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 12,
            },
            &SLOT_NAME as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47B5F0` (case 23): settings-prefix name build
        // delivered to `sub_4405F0` (engine-side, omitted). Returns
        // 0x4008. Omitted callees: sub_427280, sub_4405F0.
        23 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 13,
            },
            &SLOT_NAME as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47B850` (case 26): settings-prefix name build
        // delivered to `sub_441A70` (engine-side, omitted). Returns
        // 0x4008. Omitted callees: sub_427280, sub_441A70.
        26 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 14,
            },
            &SLOT_NAME as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486DA0` (case 28): when the fourth word equals -1,
        // `ShellExecuteA` launches the resolved file with optional
        // parameters/directory (OS-side, omitted); otherwise nothing
        // happens. No interpreter-word writes on either path. Returns
        // 0x4010. Omitted callees: ShellExecuteA.
        28 => (
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 15,
            },
            &NAME_NAME_NAME_SELECTOR as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_480470` (case 37): a slot word at or above 256
        // raises the 0x10 error mask; otherwise the dword-481 context
        // object resolves (`sub_444EF0` for a non-negative index) and is
        // passed to `sub_445E50` (engine-side, omitted). The dynamic
        // slot-table occupancy read (`this[481+slot] != 0`) is beyond the
        // micro vocabulary; in-range slots take the success path.
        // Returns 0x4008. Omitted callees: sub_444EF0, sub_445E50.
        37 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 16,
            },
            &SLOT_INDEX as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_480510` (case 45): a slot word at or above 256
        // raises 0x10; the success path resolves `sub_444EF0` and calls
        // `sub_445E70` (engine-side, omitted). Occupancy read beyond the
        // micro vocabulary as in case 37. Returns 0x4008. Omitted
        // callees: sub_444EF0, sub_445E70.
        45 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 18,
            },
            &SLOT_INDEX as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_483F70` (case 49): raises 0x10 unless the slot
        // word is below 256 and the dword-481 slot object exists; the
        // success path releases the object through `sub_446E70`
        // (engine-side, omitted). Occupancy read beyond the micro
        // vocabulary as in case 37. Returns 0x4004. Omitted callee:
        // sub_446E70.
        49 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 19,
            },
            &SLOT_TOP as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_483CD0` (case 50): on a valid bound slot, plays
        // the resolved name through `sub_446C20` (engine-side, omitted)
        // and stores its success boolean into the result word (the
        // engine result is unrecoverable; zero in the recovered subset);
        // an invalid slot raises 0x10 without touching the result word.
        // `sub_427790` (settings-string conditional append) is
        // engine-side and omitted. Occupancy read beyond the micro
        // vocabulary as in case 37. Returns 0x400C. Omitted callees:
        // sub_4405A0, sub_432F10, sub_446C20, sub_427790.
        50 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 20,
            },
            &SLOT_NAME as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_483B80` (case 53): same slot gate and result-word
        // shape as case 50, with two extra boolean words forwarded to
        // `sub_443FF0` (engine-side, omitted). Returns 0x4010. Omitted
        // callees: sub_432F10, sub_443FF0, sub_427790.
        53 => (
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 21,
            },
            &SLOT_NAME_BOOL_BOOL as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_483E00` (case 55): same slot gate and result-word
        // shape as case 50, with a second resolved name forwarded to
        // `sub_446CD0` (engine-side, omitted). Returns 0x400C. Omitted
        // callees: sub_432F10, sub_446CD0, sub_427790.
        55 => (
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 22,
            },
            &SLOT_NAME_NAME as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_480790` (case 57): raises 0x10 unless the slot
        // word is below 256 and bound; the success path resolves
        // `sub_444EF0` on the index word and releases through
        // `sub_446E70` (engine-side, omitted). Returns 0x4008. Omitted
        // callees: sub_444EF0, sub_446E70.
        57 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 23,
            },
            &SLOT_INDEX as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_483C40` (case 61): on a valid bound slot with a
        // negative second word, stores `sub_444210(...) != 0` into the
        // result word (engine query unrecoverable; zero in the recovered
        // subset); otherwise raises 0x10 without touching the result
        // word. Returns 0x4008. Omitted callees: sub_432EF0, sub_432F00,
        // sub_444210.
        61 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 24,
            },
            &SLOT_INDEX as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_484460` (case 62): on a valid bound slot with a
        // negative second word, stores `sub_444160(...) != 0` into the
        // result word (engine query unrecoverable; zero in the recovered
        // subset); otherwise raises 0x10. Returns 0x4008. Omitted
        // callee: sub_444160.
        62 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 25,
            },
            &SLOT_INDEX as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_482FA0` (case 79): on a valid bound slot, queries
        // the object graph (`sub_444EF0` for a non-negative index,
        // `sub_445340` reads object dword 1030 — beyond the context-read
        // range, resolving to zero; `sub_401000` omitted) and copies six
        // dwords into `this[20313/20314/20306..=20308]` and the result
        // word; an invalid slot raises 0x10. Returns 0x4008. Omitted
        // callees: sub_444EF0, sub_445340, sub_401000.
        79 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 2,
                table_index: 17,
            },
            &SLOT_INDEX as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    })
}
