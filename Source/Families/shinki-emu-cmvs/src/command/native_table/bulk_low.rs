//! Cluster 0 (bulk_low) native contracts: 79 dispatch handlers whose
//! bodies reduce to interpreter-word writes, error-mask raises and
//! fixed result codes.  Each arm cites the handler address from
//! `.tmp/cmvs-port/missing_handler_decomp.json`; register `N` in a
//! micro program is the `N`-th stack word of the contract (top first),
//! and register 7 is the macro scratch register.

use super::super::{
    CmvsPs2aCommandEffectKind, CmvsPs2aCommandStackWord, CmvsPs2aCommandStackWordKind,
};

use super::*;

/// `OpaqueU32` stack word `offset` bytes below the stack top.
const fn opaque(offset: u8) -> CmvsPs2aCommandStackWord {
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: offset,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    }
}

/// `TaggedStringReference` stack word `offset` bytes below the top
/// (the handler passes it through the `sub_470D50` resolver).
const fn tagged(offset: u8) -> CmvsPs2aCommandStackWord {
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: offset,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    }
}

/// `BooleanU32` stack word `offset` bytes below the top (the handler
/// only tests it against zero).
const fn boolean(offset: u8) -> CmvsPs2aCommandStackWord {
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: offset,
        kind: CmvsPs2aCommandStackWordKind::BooleanU32,
    }
}

/// Stack-word lists by shape, hoisted so the arms return `&'static`
/// slices.
const W_NONE: &[CmvsPs2aCommandStackWord] = &[];
const W_OPAQUE1: &[CmvsPs2aCommandStackWord] = &[opaque(4)];
const W_OPAQUE2: &[CmvsPs2aCommandStackWord] = &[opaque(4), opaque(8)];
const W_OPAQUE3: &[CmvsPs2aCommandStackWord] = &[opaque(4), opaque(8), opaque(12)];
const W_OPAQUE4: &[CmvsPs2aCommandStackWord] = &[opaque(4), opaque(8), opaque(12), opaque(16)];
const W_OPAQUE6: &[CmvsPs2aCommandStackWord] = &[
    opaque(4),
    opaque(8),
    opaque(12),
    opaque(16),
    opaque(20),
    opaque(24),
];
const W_BOOLEAN1: &[CmvsPs2aCommandStackWord] = &[boolean(4)];
const W_OPAQUE_TAGGED: &[CmvsPs2aCommandStackWord] = &[opaque(4), tagged(8)];
const W_OPAQUE_TAGGED2: &[CmvsPs2aCommandStackWord] = &[opaque(4), tagged(8), tagged(12)];
const W_OPAQUE2_TAGGED_BOOLEAN: &[CmvsPs2aCommandStackWord] =
    &[opaque(4), opaque(8), tagged(12), boolean(16)];
const W_OPAQUE_TAGGED_OPAQUE3: &[CmvsPs2aCommandStackWord] =
    &[opaque(4), tagged(8), opaque(12), opaque(16), opaque(20)];
const W_TAGGED1: &[CmvsPs2aCommandStackWord] = &[tagged(4)];
const W_TAGGED2: &[CmvsPs2aCommandStackWord] = &[tagged(4), tagged(8)];
const W_TAGGED3: &[CmvsPs2aCommandStackWord] = &[tagged(4), tagged(8), tagged(12)];
const W_TAGGED_OPAQUE: &[CmvsPs2aCommandStackWord] = &[tagged(4), opaque(8)];
const W_TAGGED2_OPAQUE: &[CmvsPs2aCommandStackWord] = &[tagged(4), tagged(8), opaque(12)];
const W_TAGGED3_OPAQUE_TAGGED_BOOLEAN: &[CmvsPs2aCommandStackWord] = &[
    tagged(4),
    tagged(8),
    tagged(12),
    opaque(16),
    tagged(20),
    boolean(24),
];

/// Every observable effect is an omitted deep worker call; the arm
/// comments list the omitted callee addresses.
const EMPTY_PROGRAM: &[NativeMicroOp] = &[];

/// `sub_4806B0` (case 81): the context probe reads slot-object word
/// one (zero for an unbound slot in the emulated state), which acts
/// as the bound check — a zero word raises 0x10.  The probe uses a
/// zero index so the mask choice stays independent of the consumed
/// index operand (the executor returns zero for indices >= 0x400).
const CONTEXT_SLOT_BOUND_MASK: &[NativeMicroOp] = &[
    NativeMicroOp::Move {
        dst: 2,
        src: NativeMicroSrc::Const(0),
    },
    NativeMicroOp::Call(NativeMicroMacro::ContextRead {
        slot_reg: 0,
        index_reg: 2,
    }),
    NativeMicroOp::Guard {
        lhs: NativeMicroSrc::Reg(7),
        kind: NativeMicroCmpKind::Ne,
        rhs: NativeMicroSrc::Const(0),
        taken_ops: 1,
    },
    NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10,
    },
];

/// `sub_483970` &c.: the bound probe (word one of the slot object,
/// zero index) selects between the unbound raise (0x10) and the
/// index range check (`index < 1024`), whose failure raises 0x20.
const CONTEXT_INDEX_MASKS: &[NativeMicroOp] = &[
    NativeMicroOp::Move {
        dst: 2,
        src: NativeMicroSrc::Const(0),
    },
    NativeMicroOp::Call(NativeMicroMacro::ContextRead {
        slot_reg: 0,
        index_reg: 2,
    }),
    NativeMicroOp::Guard {
        lhs: NativeMicroSrc::Reg(7),
        kind: NativeMicroCmpKind::Eq,
        rhs: NativeMicroSrc::Const(0),
        taken_ops: 3,
    },
    NativeMicroOp::Guard {
        lhs: NativeMicroSrc::Reg(1),
        kind: NativeMicroCmpKind::LtU,
        rhs: NativeMicroSrc::Const(1024),
        taken_ops: 1,
    },
    NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x20,
    },
    NativeMicroOp::Guard {
        lhs: NativeMicroSrc::Reg(7),
        kind: NativeMicroCmpKind::Ne,
        rhs: NativeMicroSrc::Const(0),
        taken_ops: 1,
    },
    NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10,
    },
];

/// `sub_4879A0` family: the effect-playback table (`this[750+N]`)
/// lives in the executor's slot-object space, which micro field reads
/// cannot observe, so the unbound path (raise 0x1000) is encoded.
const PLAYBACK_TABLE_BOUND_MASK: &[NativeMicroOp] = &[NativeMicroOp::OrField {
    byte_offset: 10532,
    mask: 0x1000,
}];

/// `sub_47F870` family: same projection for the effect-channel table
/// (`this[742+N]`, raise 0x10000).
const EFFECT_CHANNEL_BOUND_MASK: &[NativeMicroOp] = &[NativeMicroOp::OrField {
    byte_offset: 10532,
    mask: 0x0001_0000,
}];

/// `sub_478BE0` &c.: the result slot receives the headless projection
/// (zero) of an unmodeled engine query.
const STORE_ZERO_RESULT: &[NativeMicroOp] = &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))];

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // Handler `sub_4806B0` (case 81): pops a context slot and an
        // index; a bound slot (`this[481+slot]`) forwards the context
        // word `sub_444EF0` to the render worker `sub_4469A0` (omitted),
        // otherwise the handler raises 0x10 at dword 2633.  The context
        // read doubles as the bound test: an unbound slot reads zero.
        // Returns 0x4008.
        81 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 0,
            },
            W_OPAQUE2,
        ),
        // Handler 4832E0: validates the surface and forwards word three
        // to 446B40 (source rectangle X).
        117 => (
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 1,
            },
            W_OPAQUE3,
        ),
        // Handler `sub_483160` (case 120): bound context slot and index
        // pre-check, then `sub_446A00` writes the triple at
        // `this[20313..20315]` through out-pointers (omitted — the
        // values are engine-side and unrecoverable from the op set).
        // Omitted callees: `sub_444EF0` (pure read), `sub_446A00`.
        // Returns 0x4008.
        120 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 2,
            },
            W_OPAQUE2,
        ),
        // Handler `sub_485EA0` (case 130): copies the interpreter's
        // inline string at byte offset 13072 over the resolved private
        // string (an `lstrcpyA` into the PS2A string pool, not
        // expressible in the micro op set).  Omitted callee:
        // `lstrcpyA` mutation of the referenced string.  Returns 0x4004.
        130 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 3,
            },
            W_TAGGED1,
        ),
        // Handler `sub_478BE0` (case 163): stores the result of the
        // query `sub_48E970` on `this[793]` into the result slot; the
        // query is an unmodeled engine call, encoded as zero.
        // Returns 0x4000.
        163 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 4,
            },
            W_NONE,
        ),
        // Handler `sub_486E30` (case 165): forwards the consumed word
        // to `sub_48CE10` (omitted).  No interpreter writes.
        // Returns 0x4004.
        165 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 5,
            },
            W_OPAQUE1,
        ),
        // Native ordinary voice controls and six resource-stream controls.
        // Worker48C950/48C0E0/48D700/48CD50 operate on real retained streams.
        167 | 169 | 172 | 173 | 180 | 184 | 185 => (
            8,
            CmvsPs2aCommandEffectKind::NativeAudio { command_id },
            W_OPAQUE2,
        ),
        //189 returns0xA000 until48CCF0 reports object absence, then0x4004.
        171 | 174 | 178 | 188 | 189 => (
            4,
            CmvsPs2aCommandEffectKind::NativeAudio { command_id },
            W_OPAQUE1,
        ),
        // Handler `sub_4893A0` (case 192): builds a prefixed movie
        // path and opens it through `sub_4410D0`; success stores one
        // into the result slot and keeps the stream object at
        // `this[3176]`, failure stores zero and frees it.  The archive
        // open is unmodeled, so the failure projection is encoded:
        // `this[3176]` cleared and result zero.  Omitted callees:
        // `sub_440E80`, `sub_4A9B8F`, `sub_432F30`, `sub_432F70`,
        // `sub_440E20`, `sub_432F10`, `sub_4410D0`.  Returns 0x4004.
        192 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 6,
            },
            W_TAGGED1,
        ),
        // Handler `sub_489550` (case 193): releases the movie object
        // at `this[794]` when present, clearing the field.  Omitted
        // callees: `sub_440E80`, `sub_4A9B8F`.  Returns 0x4000.
        193 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 7,
            },
            W_NONE,
        ),
        // Handler `sub_489520` (case 194): a bound movie object
        // publishes `sub_42E160()`'s frame query into the result slot
        // (unmodeled — no store on that path), otherwise the result is
        // -1.  Headless `this[794]` stays zero, so the -1 path is the
        // live one.  Returns 0x4000.
        194 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 8,
            },
            W_NONE,
        ),
        // Handler `sub_480030` (case 201): stores the boolean of the
        // query `sub_45CEC0(this[818])` into the result slot; the
        // query is unmodeled, encoded as zero (false).  Returns 0x4000.
        201 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 9,
            },
            W_NONE,
        ),
        // Handler `sub_480340` (case 204): converts the two consumed
        // coordinates to screen space and warps the OS cursor.  All
        // callees are window-system workers (omitted): `sub_432F40`
        // (twice), `sub_429A30`, `sub_429130`, `ClientToScreen`,
        // `SetCursorPos`.  No interpreter writes.  Returns 0x4008.
        204 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 10,
            },
            W_OPAQUE2,
        ),
        // Handler `sub_480310` (case 205): forwards the word's
        // zero-ness to `sub_41B160` (omitted).  No interpreter writes.
        // Returns 0x4004.
        205 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 11,
            },
            W_BOOLEAN1,
        ),
        // Handler `sub_478920` (case 208): the consumed word is
        // sign-extended to 64 bits and reduced with
        // `(hi ^ v) - hi`, which is the two's-complement absolute
        // value.  Returns 0x4004.
        208 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 12,
            },
            W_OPAQUE1,
        ),
        // Handler `sub_47CDC0` (case 209): three-word signed median —
        // the top word is raised to the second, then clamped to the
        // third.  Returns 0x400C.
        209 => (
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 13,
            },
            W_OPAQUE3,
        ),
        // Handler `sub_484F20` (case 213): stores
        // `low + rand() % (high - low)`; the CRT `rand` is not in the
        // op set, encoded as zero, so the result is the low bound.
        // Returns 0x4008.
        213 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 14,
            },
            W_OPAQUE2,
        ),
        // Handler `sub_4846F0` (case 214): stores `base` and, for an
        // exponent above one, its wrapping power via an unrolled
        // multiply loop.  The loop is not expressible in the op set;
        // exponents of zero and one are faithful, larger ones keep the
        // base (flagged for the integrator).  Returns 0x4008.
        214 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 15,
            },
            W_OPAQUE2,
        ),
        // Handler `sub_47CCE0` (case 215): mode word zero stores
        // `first * second / 1000`; modes one and two take mis-decompiled
        // `sqrt` float paths (omitted — no result store on those
        // paths).  Returns 0x400C.
        215 => (
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 16,
            },
            W_OPAQUE3,
        ),
        // Handler `sub_485390` (case 216): `sub_42EC50` converts
        // `100 * top` and the second word into the pair at
        // `this[20306]`/`this[20307]` through out-pointers (omitted —
        // values unrecoverable).  Returns 0x4008.
        216 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 17,
            },
            W_OPAQUE2,
        ),
        // Handler `sub_485360` (case 217): as case 216 without the
        // scale-by-100 (`sub_42EC50` omitted).  Returns 0x4008.
        217 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 18,
            },
            W_OPAQUE2,
        ),
        // Handler `sub_48A880` (case 218): `sub_42EF00` writes the
        // pair at `this[20309]`/`this[20310]`, then the handler divides
        // `this[20309]` by 100 (all omitted — out-pointer writes are
        // unrecoverable).  Returns 0x4010.
        218 => (
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 19,
            },
            W_OPAQUE4,
        ),
        // Handler `sub_48A840` (case 219): as case 218 without the
        // divide (`sub_42EF00` omitted).  Returns 0x4010.
        219 => (
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 20,
            },
            W_OPAQUE4,
        ),
        // Handler `sub_485330` (case 220): `sub_42EC50` converts the
        // top word against 4096 into `this[20306]`/`this[20307]`
        // (omitted — out-pointer writes unrecoverable).
        // Returns 0x4004.
        220 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 21,
            },
            W_OPAQUE1,
        ),
        // Handler `sub_4853C0` (case 224): `sub_42ED50` converts six
        // words into the triple at `this[20313..20315]` through
        // out-pointers (omitted — values unrecoverable).
        // Returns 0x4018.
        224 => (
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 22,
            },
            W_OPAQUE6,
        ),
        // Handler `sub_4879A0` (case 228): a bound playback slot
        // (`this[750+slot]`) would publish `sub_4630F0`'s triple into
        // `this[20306]`/`this[20307]`/`this[20309]` and set the result
        // to one; the table lives in the executor's slot-object space,
        // so the unbound path (raise 0x1000) is encoded.  Omitted
        // callees: `sub_4630F0`.  Returns 0x400C.
        228 => (
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 23,
            },
            W_OPAQUE_TAGGED2,
        ),
        // Handler `sub_489010` (case 231): a bound playback slot
        // clamps the float word to the 0.95..1.05 window and forwards
        // it to `sub_466240` (omitted); unbound raises 0x1000 (encoded,
        // see case 228).  Returns 0x4008.
        231 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 24,
            },
            W_OPAQUE2,
        ),
        // Handler `sub_486FE0` (case 232): stores the consumed word
        // into dword 401 (byte offset 1604).  Returns 0x4004.
        232 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 25,
            },
            W_OPAQUE1,
        ),
        // Handler `sub_47D1D0` (case 239): the string worker
        // `sub_41A500` consumes two resolved strings and a raw word
        // (omitted — string mutation is not expressible in the op
        // set).  Returns 0x400C.
        239 => (
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 26,
            },
            W_TAGGED2_OPAQUE,
        ),
        // Handler `sub_47D190` (case 240): `lstrcpyA` between two
        // resolved private strings (omitted — string mutation is not
        // expressible in the op set).  Returns 0x4008.
        240 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 27,
            },
            W_TAGGED2,
        ),
        // Handler `sub_47D0D0` (case 241): stores the boolean of
        // `sub_41A420` (string equality) into the result slot; the
        // compare is not expressible in the op set, so no store is
        // encoded (flagged for the integrator).  Returns 0x4008.
        241 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 28,
            },
            W_TAGGED2,
        ),
        // Handler `sub_47D090` (case 242): `lstrcatA` between two
        // resolved private strings (omitted — string mutation is not
        // expressible in the op set).  Returns 0x4008.
        242 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 29,
            },
            W_TAGGED2,
        ),
        // Handler `sub_47D360` (case 244): the token-copy loop with
        // the lead-byte test `sub_41A3A0` rewrites the first string up
        // to a delimiter from the second and stores the byte count in
        // the result slot (omitted — string content is not expressible
        // in the op set).  Returns 0x400C.
        244 => (
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 30,
            },
            W_TAGGED3,
        ),
        // Handler `sub_47D500` (case 245): stores the boolean of
        // `sub_41A490` (substring test) into the result slot; the
        // compare is not expressible in the op set, so no store is
        // encoded (flagged for the integrator).  Returns 0x4008.
        245 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 31,
            },
            W_TAGGED2,
        ),
        // Handler `sub_47D4D0` (case 246): `sub_419DC0` parses the
        // resolved string and writes the result slot through an
        // out-pointer (omitted — string content is not expressible in
        // the op set).  Returns 0x4004.
        246 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 32,
            },
            W_TAGGED1,
        ),
        // Handler `sub_47D120` (case 247): stores the index of the
        // first matching character between two resolved strings, or
        // -1 (omitted — string content is not expressible in the op
        // set).  Returns 0x4008.
        247 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 33,
            },
            W_TAGGED2,
        ),
        // Handler `sub_47D250` (case 249): stores the index of the
        // last matching character between two resolved strings, or -1
        // (omitted — string content is not expressible in the op set).
        // Returns 0x4008.
        249 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 34,
            },
            W_TAGGED2,
        ),
        // Handler `sub_47CE00` (case 250): `sub_418D70` appends
        // `"%d-"` with the raw word to the resolved buffer (omitted —
        // string mutation is not expressible in the op set).
        // Returns 0x4008.
        250 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 35,
            },
            W_TAGGED_OPAQUE,
        ),
        // Handler `sub_47D420` (case 251): folds the first ten bytes
        // of the resolved date string into a packed decimal in the
        // result slot (omitted — string content is not expressible in
        // the op set).  Returns 0x4004.
        251 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 36,
            },
            W_TAGGED1,
        ),
        // Handler `sub_486EE0` (case 253): `sub_418D70` formats the
        // resolved argument through the resolved format string into
        // the resolved buffer (omitted — string mutation is not
        // expressible in the op set).  Returns 0x400C.
        253 => (
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 37,
            },
            W_TAGGED3,
        ),
        // Handler `sub_478D20` (case 255): `sub_41A610` finds the
        // last '.' of the resolved path; the extension word replaces
        // the suffix or is appended with a new dot (omitted — string
        // mutation is not expressible in the op set).  Returns 0x4008.
        255 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 38,
            },
            W_TAGGED2,
        ),
        // Handler `sub_487910` (case 265): a bound playback slot would
        // store `sub_4630C0()`'s query into the result slot; the table
        // is unobservable to micro field reads, so the unbound path
        // (raise 0x1000) is encoded.  Omitted callee: `sub_4630C0`.
        // Returns 0x4004.
        265 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 39,
            },
            W_OPAQUE1,
        ),
        // Handler `sub_488B80` (case 266): a bound playback slot
        // forwards the resolved string to the effect worker
        // `loc_464D40` (omitted); unbound raises 0x1000 (encoded, see
        // case 265).  Returns 0x4008.
        266 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 40,
            },
            W_OPAQUE_TAGGED,
        ),
        // Handler `sub_487960` (case 273): a bound playback slot
        // forwards the slot object to `sub_464CC0` (omitted); unbound
        // raises 0x1000 (encoded, see case 265).  Returns 0x4004.
        273 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 41,
            },
            W_OPAQUE1,
        ),
        // Handler `sub_488AF0` (case 274): a bound playback slot
        // publishes `sub_4643F0`'s value pair into the result slot and
        // `this[20306]`/`this[20309]`; unbound raises 0x1000 (encoded,
        // see case 265).  Omitted callee: `sub_4643F0`.
        // Returns 0x4008.
        274 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 42,
            },
            W_OPAQUE_TAGGED,
        ),
        // Handler `sub_4890C0` (case 275): a bound playback slot
        // forwards the raw second word to `sub_466280` (omitted);
        // unbound raises 0x1000 (encoded, see case 265).
        // Returns 0x4008.
        275 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 43,
            },
            W_OPAQUE2,
        ),
        // Handler `sub_489100` (case 281): a bound playback slot would
        // store `sub_439C40`'s query on the resolved string into the
        // result slot; unbound raises 0x1000 (encoded, see case 265).
        // Omitted callee: `sub_439C40`.  Returns 0x4008.
        281 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 44,
            },
            W_OPAQUE_TAGGED,
        ),
        // Handler `sub_4876E0` (case 284): an index at or above 12
        // raises 0x1000; otherwise the handler (re)builds the playback
        // record through `sub_4629D0` (omitted — yields null headless),
        // stores it into `this[750+index]` (a no-op zero store in the
        // emulated state) and publishes its non-zero-ness.  Omitted
        // callees: `sub_432F40`, `sub_4290A0`, `sub_432F50`,
        // `sub_4292D0`, `sub_411E10`, `sub_461CD0`, `sub_4629D0`.
        // Returns 0x4014.
        284 => (
            20,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 45,
            },
            W_OPAQUE_TAGGED_OPAQUE3,
        ),
        // Handler `sub_487040` (case 288): stores the boolean of the
        // mode-0 query `sub_426F60` on the resolved string into the
        // result slot; the query is unmodeled, encoded as zero.
        // Returns 0x4004.
        288 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 46,
            },
            W_TAGGED1,
        ),
        // Handler `sub_487080` (case 289): as case 288 with the mode-1
        // query of `sub_426F60` (unmodeled, encoded as zero).
        // Returns 0x4004.
        289 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 47,
            },
            W_TAGGED1,
        ),
        // Handler `sub_47D060` (case 311): forwards the raw (unresolved)
        // word to `sub_457C90` (omitted).  No interpreter writes.
        // Returns 0x4004.
        311 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 48,
            },
            W_OPAQUE1,
        ),
        // Handler `sub_47A240` (case 314): stores the boolean of the
        // two-literal query `sub_470F60(&byte_4D2018, &byte_4D202C)`
        // into the result slot; the query is an unmodeled config read,
        // encoded as zero.  Returns 0x4000.
        314 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 49,
            },
            W_NONE,
        ),
        // Handler `sub_47A1D0` (case 315): clamps `abs32` of the top
        // word to one and stores the boolean of the message-box call
        // `sub_470EE0` into the result slot (unmodeled, encoded as
        // zero; the two resolved strings feed only the omitted call).
        // Returns 0x400C.
        315 => (
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 50,
            },
            W_OPAQUE_TAGGED2,
        ),
        // Handler `sub_47A130` (case 316): stores the boolean of the
        // six-word file dialog `sub_470DE0` into the result slot
        // (unmodeled, encoded as zero).  Returns 0x4018.
        316 => (
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 51,
            },
            W_TAGGED3_OPAQUE_TAGGED_BOOLEAN,
        ),
        // Handler `sub_478560` (case 317): constructs the modal
        // movie-dialog object, runs its vtable entry and flushes the
        // playback table — every observable effect is a deep worker
        // call (omitted: `sub_4278D0` twice, `sub_492310`,
        // `sub_419F50`, the vtable dispatch, `sub_462F70`, `sub_4649B0`,
        // `sub_493B00`, `sub_493050`, `sub_4A9B8F`).  The raw return is
        // 1 (pop width one), not the 0x4000 family — flagged for the
        // integrator.  Returns 1.
        317 => (
            1,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 52,
            },
            W_NONE,
        ),
        // Handler `sub_47F870` (case 340): a bound effect channel
        // (`this[742+channel]`) forwards the resolved string to
        // `sub_467930` (omitted); the table lives in the executor's
        // slot-object space, so the unbound path (raise 0x10000) is
        // encoded.  Returns 0x4008.
        340 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 53,
            },
            W_OPAQUE_TAGGED,
        ),
        // Handler `sub_47FB60` (case 342): a bound effect channel
        // releases it through `sub_467A20` (omitted); unbound raises
        // 0x10000 (encoded, see case 340).  Returns 0x4004.
        342 => (
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 54,
            },
            W_OPAQUE1,
        ),
        // Handler `sub_47E7B0` (case 348): a negative fade word skips
        // the fade reset; with `this[367]` set and `this[366]` set, the
        // unmodeled `sub_48A990(fade)` yields zero headless and clears
        // `this[366]`.  A bound effect channel would then publish the
        // boolean of `sub_466DD0` (omitted) into the result slot;
        // unbound raises 0x10000 (encoded, see case 340).  Omitted
        // callees: `sub_48A990`, `sub_466DD0`.  Returns 0x4010.
        348 => (
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 0,
                table_index: 55,
            },
            W_OPAQUE2_TAGGED_BOOLEAN,
        ),
        _ => return None,
    })
}

/// Micro programs in arm order: entry `N` is the program referenced by
/// the arm with `table_index: N` above.
pub(super) const OPS: &[&[NativeMicroOp]] = &[
    // 81 (`sub_4806B0`).
    CONTEXT_SLOT_BOUND_MASK,
    // 117 (`sub_4832E0`), 120 (`sub_483160`).
    CONTEXT_INDEX_MASKS,
    CONTEXT_INDEX_MASKS,
    // 130 (`sub_485EA0`).
    EMPTY_PROGRAM,
    // 163 (`sub_478BE0`).
    STORE_ZERO_RESULT,
    // 165 (`sub_486E30`).
    EMPTY_PROGRAM,
    // 192 (`sub_4893A0`): failure projection of the movie open — the
    // stream field `this[3176]` (byte 12704) is cleared and the result
    // slot stores zero.
    &[
        NativeMicroOp::SetField {
            byte_offset: 12704,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
    ],
    // 193 (`sub_489550`): clear `this[794]` (byte 3176) only when the
    // movie object is present.
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3176),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::SetField {
            byte_offset: 3176,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // 194 (`sub_489520`): an unbound movie object stores -1; the
    // bound-path query `sub_42E160` is unmodeled (no store).
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3176),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0xFFFF_FFFF)),
    ],
    // 201 (`sub_480030`).
    STORE_ZERO_RESULT,
    // 204 (`sub_480340`), 205 (`sub_480310`).
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    // 208 (`sub_478920`): absolute value of the sign-extended word —
    // non-negative words pass through, negative ones negate.
    &[
        NativeMicroOp::Move {
            dst: 2,
            src: NativeMicroSrc::Reg(0),
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::Move {
            dst: 2,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::BinOp {
            dst: 2,
            src: NativeMicroSrc::Reg(0),
            kind: NativeMicroBinKind::Sub,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Reg(2)),
    ],
    // 209 (`sub_47CDC0`): signed median — raise the top word to the
    // second, then clamp it to the third.
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Reg(0),
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Reg(1),
            taken_ops: 1,
        },
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Reg(1),
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(3),
            kind: NativeMicroCmpKind::Le,
            rhs: NativeMicroSrc::Reg(2),
            taken_ops: 1,
        },
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Reg(2),
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Reg(3)),
    ],
    // 213 (`sub_484F20`): rand encoded as zero — result is the low
    // bound.
    &[NativeMicroOp::SetResult(NativeMicroSrc::Reg(0))],
    // 214 (`sub_4846F0`): pow loop unexpressible — base stored.
    &[NativeMicroOp::SetResult(NativeMicroSrc::Reg(0))],
    // 215 (`sub_47CCE0`): mode-zero integer path
    // `first * second / 1000`; the sqrt float paths are omitted.
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(2),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 4,
        },
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Reg(0),
        },
        NativeMicroOp::BinOp {
            dst: 3,
            src: NativeMicroSrc::Reg(1),
            kind: NativeMicroBinKind::Mul,
        },
        NativeMicroOp::BinOp {
            dst: 3,
            src: NativeMicroSrc::Const(1000),
            kind: NativeMicroBinKind::Div,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Reg(3)),
    ],
    // 216 (`sub_485390`), 217 (`sub_485360`), 218 (`sub_48A880`),
    // 219 (`sub_48A840`), 220 (`sub_485330`), 224 (`sub_4853C0`).
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    // 228 (`sub_4879A0`), 231 (`sub_489010`).
    PLAYBACK_TABLE_BOUND_MASK,
    PLAYBACK_TABLE_BOUND_MASK,
    // 232 (`sub_486FE0`): store the word into dword 401.
    &[NativeMicroOp::SetField {
        byte_offset: 1604,
        src: NativeMicroSrc::Reg(0),
    }],
    // 239 (`sub_47D1D0`), 240 (`sub_47D190`), 241 (`sub_47D0D0`),
    // 242 (`sub_47D090`), 244 (`sub_47D360`), 245 (`sub_47D500`),
    // 246 (`sub_47D4D0`), 247 (`sub_47D120`), 249 (`sub_47D250`),
    // 250 (`sub_47CE00`), 251 (`sub_47D420`), 253 (`sub_486EE0`),
    // 255 (`sub_478D20`).
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    EMPTY_PROGRAM,
    // 265 (`sub_487910`), 266 (`sub_488B80`), 273 (`sub_487960`),
    // 274 (`sub_488AF0`), 275 (`sub_4890C0`), 281 (`sub_489100`).
    PLAYBACK_TABLE_BOUND_MASK,
    PLAYBACK_TABLE_BOUND_MASK,
    PLAYBACK_TABLE_BOUND_MASK,
    PLAYBACK_TABLE_BOUND_MASK,
    PLAYBACK_TABLE_BOUND_MASK,
    PLAYBACK_TABLE_BOUND_MASK,
    // 284 (`sub_4876E0`): the unmodeled record build yields null, so
    // the in-range path publishes zero; at or above 12 raises 0x1000.
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::GeU,
            rhs: NativeMicroSrc::Const(12),
            taken_ops: 1,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::LtU,
            rhs: NativeMicroSrc::Const(12),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x1000,
        },
    ],
    // 288 (`sub_487040`), 289 (`sub_487080`).
    STORE_ZERO_RESULT,
    STORE_ZERO_RESULT,
    // 311 (`sub_47D060`).
    EMPTY_PROGRAM,
    // 314 (`sub_47A240`), 315 (`sub_47A1D0`), 316 (`sub_47A130`).
    STORE_ZERO_RESULT,
    STORE_ZERO_RESULT,
    STORE_ZERO_RESULT,
    // 317 (`sub_478560`).
    EMPTY_PROGRAM,
    // 340 (`sub_47F870`), 342 (`sub_47FB60`).
    EFFECT_CHANNEL_BOUND_MASK,
    EFFECT_CHANNEL_BOUND_MASK,
    // 348 (`sub_47E7B0`): fade gate and the `this[366]` clear, then
    // the unbound effect-channel raise.
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Lt,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(1468),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(1464),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::SetField {
            byte_offset: 1464,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x0001_0000,
        },
    ],
];
