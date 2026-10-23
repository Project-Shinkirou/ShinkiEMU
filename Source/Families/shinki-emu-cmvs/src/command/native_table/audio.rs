use super::*;
use crate::{CmvsPs2aCommandEffectKind, CmvsPs2aCommandStackWord, CmvsPs2aCommandStackWordKind};

// The three handlers in this cluster share the named-stream audio
// worker family (`sub_48DB70` for block/BGM starts, `sub_48BE90` for
// immediate SE starts).  The headless audio graph holds no stream
// objects — the same regime as the integrated case-182/186 contracts
// (`RegisterAudioStreamNameSlot`/`PlayNamedAudioStream`) — and the
// synchronous archive resolves named resources immediately, so a
// reached worker call models as success exactly like the integrated
// case-166 text layout: the pinned result word takes 1 and a worker
// out-param stays 0.

/// Case 175 (`sub_489710`) operand set, top-down: block selector
/// (0..=3), tagged stream name, volume scale, loop flag, playback-rate
/// float bits.
const LOAD_NAMED_STREAM_WORDS: [CmvsPs2aCommandStackWord; 5] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 12,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 16,
        kind: CmvsPs2aCommandStackWordKind::BooleanU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 20,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];

/// Case 576 (`sub_4844C0`) operand set, top-down: non-start flag,
/// tagged stream name, volume, and three raw worker arguments (two of
/// them boolean-predicated at the call boundary).
const CHANNEL_CONTROL_WORDS: [CmvsPs2aCommandStackWord; 6] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::BooleanU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 12,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 16,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 20,
        kind: CmvsPs2aCommandStackWordKind::BooleanU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 24,
        kind: CmvsPs2aCommandStackWordKind::BooleanU32,
    },
];

/// Case 584 (`sub_489E20`) operand set, top-down: SE block index
/// (0..=0x17) and the tagged effect name.
const SOUND_EFFECT_WORDS: [CmvsPs2aCommandStackWord; 2] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
];

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    let fields: CommandFields = match command_id {
        // Handler `sub_489710` (case 175): loads and starts the named
        // BGM/SE stream through the audio worker `sub_48DB70` on block
        // `selector + 5`, with the resolved name, the search path from
        // `this+3348`, the volume chain `((this+1560 * this+1572) >> 8)`
        // optionally scaled by operand 2, the loop flag (operand 3) and
        // the operand-4 rate float clamped to 1.0..=3.0 — all consumed
        // by the worker and unobservable in the headless model, as is
        // the worker's channel-descriptor table (0xC60).  A selector
        // above 3 raises 0x400 at byte offset 10532; a zero audio
        // gate (`this+1552`) skips the worker and stores 0 into the
        // result slot instead; the reached worker publishes success 1
        // into the pinned result slot and its out-param at 81236 stays
        // 0.  Returns 0x4014.
        175 => (
            20,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 4,
                table_index: 0,
            },
            &LOAD_NAMED_STREAM_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4844C0` (case 576): starts the named stream on
        // the fixed block 0xF through `sub_48DB70`, with the search
        // path from `this+3340`, the volume `(operand 2 * this+1560)
        // >> 8` and operands 3..5 forwarded raw (two boolean-predicated
        // at the call boundary) — all call-internal and unobservable
        // headlessly.  A non-zero top operand raises 0x200 at byte
        // offset 10532 and skips the start; the reached worker
        // publishes success 1 into the pinned result slot.  Returns
        // 0x4008.
        576 => (
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 4,
                table_index: 1,
            },
            &CHANNEL_CONTROL_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_489E20` (case 584): starts the named sound
        // effect immediately through `sub_48BE90` with the block index
        // and the search-path dword at `this[835]` (byte 3340) — the
        // call is the whole effect and publishes its success into the
        // pinned result slot (`this[20305]`, headless success 1 like
        // the sibling audio contracts).  A block index above 0x17
        // raises 0x200 at byte offset 10532 (`this[2633] |= 0x200`)
        // and leaves the result slot untouched.  Returns 0x4008.
        584 => (
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 4,
                table_index: 2,
            },
            &SOUND_EFFECT_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    };
    Some(fields)
}

/// Micro programs owned by this cluster, indexed by the local
/// `table_index` of contract arms.
pub(super) const OPS: &[&[NativeMicroOp]] = &[
    // `sub_489710` (case 175): selector gate, audio-enable gate, and
    // the three write shapes (error mask / worker success + out-param /
    // disabled zero).
    &[
        // Selector below 4: skip the error raise and the error return.
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::LtU,
            rhs: NativeMicroSrc::Const(4),
            taken_ops: 2,
        },
        // Selector above 3: raise 0x400 and return without touching
        // the result slot.
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x400,
        },
        // Error return: skip the whole load body.
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Const(1),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(1),
            taken_ops: 5,
        },
        // Audio gate zero (`this+1552`): jump to the disabled store.
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(1552),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 3,
        },
        // Reached worker (`sub_48DB70`): success into the result slot.
        NativeMicroOp::SetResult(NativeMicroSrc::Const(1)),
        // Worker out-param (loop/extent word) stays zero.
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
        // Enabled return: skip the disabled store.
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Const(1),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(1),
            taken_ops: 1,
        },
        // Disabled gate: result slot takes 0, no worker out-param.
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
    ],
    // `sub_4844C0` (case 576): non-start flag gate and the worker
    // success store.
    &[
        // Top operand zero: skip the error raise and the error return.
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        // Non-zero flag: raise 0x200 and return without a start.
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x200,
        },
        // Error return: skip the success store.
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Const(1),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(1),
            taken_ops: 1,
        },
        // Reached worker (`sub_48DB70` on block 0xF): success into the
        // result slot.
        NativeMicroOp::SetResult(NativeMicroSrc::Const(1)),
    ],
    // `sub_489E20` (case 584): block-index gate and the worker success
    // store.
    &[
        // Block index at most 0x17: skip the error raise and the error
        // return.
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::LtU,
            rhs: NativeMicroSrc::Const(0x18),
            taken_ops: 2,
        },
        // Index above 0x17: raise 0x200 and return, result untouched.
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x200,
        },
        // Error return: skip the success store.
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Const(1),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(1),
            taken_ops: 1,
        },
        // Reached worker (`sub_48BE90`): success into the result slot.
        NativeMicroOp::SetResult(NativeMicroSrc::Const(1)),
    ],
];
