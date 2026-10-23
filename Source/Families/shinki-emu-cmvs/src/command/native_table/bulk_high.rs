//! Mechanically recovered micro programs for the bulk-high command
//! cluster (dispatch handlers outside the semantic families).  Each arm
//! cites its native handler; derivation notes live with the arm, the
//! shared vocabulary in `super`.

use crate::{CmvsPs2aCommandStackWord, CmvsPs2aCommandStackWordKind};

const W_1O2O3T: [CmvsPs2aCommandStackWord; 3] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 12,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
];
const W_1O2O: [CmvsPs2aCommandStackWord; 2] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1O2O3O4O5O: [CmvsPs2aCommandStackWord; 5] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
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
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1O2O3O4O: [CmvsPs2aCommandStackWord; 4] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 12,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 16,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1O2O3O4O5O6O7O8O: [CmvsPs2aCommandStackWord; 8] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
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
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 24,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 28,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 32,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1O2O3O: [CmvsPs2aCommandStackWord; 3] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 12,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1O2O3O4O5O6O7O: [CmvsPs2aCommandStackWord; 7] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
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
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 24,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 28,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1O2O3O4O5O6O7O8O9OAOBO: [CmvsPs2aCommandStackWord; 11] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
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
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 24,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 28,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 32,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 36,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 40,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 44,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1O: [CmvsPs2aCommandStackWord; 1] = [CmvsPs2aCommandStackWord {
    offset_from_top_bytes: 4,
    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
}];
const W_1O2O3O4O5O6O: [CmvsPs2aCommandStackWord; 6] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
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
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 24,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1T2O3O: [CmvsPs2aCommandStackWord; 3] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 12,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1T2O3O4O5O: [CmvsPs2aCommandStackWord; 5] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
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
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1O2T3O4O5O6O: [CmvsPs2aCommandStackWord; 6] = [
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
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 20,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 24,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1B: [CmvsPs2aCommandStackWord; 1] = [CmvsPs2aCommandStackWord {
    offset_from_top_bytes: 4,
    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
}];
const W_1O2T3O: [CmvsPs2aCommandStackWord; 3] = [
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
];
const W_1O2T3T: [CmvsPs2aCommandStackWord; 3] = [
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
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
];
const W_1O2T3O4O5O6O7O8O: [CmvsPs2aCommandStackWord; 8] = [
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
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 20,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 24,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 28,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 32,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1B2O: [CmvsPs2aCommandStackWord; 2] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::BooleanU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1B2B: [CmvsPs2aCommandStackWord; 2] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::BooleanU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::BooleanU32,
    },
];
const W_1O2O3T5O6O: [CmvsPs2aCommandStackWord; 5] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 12,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 20,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 24,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1O2O3T4O5O: [CmvsPs2aCommandStackWord; 5] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 12,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 16,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 20,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1O2O3O4O5O6O7O8O9OAO: [CmvsPs2aCommandStackWord; 10] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
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
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 24,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 28,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 32,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 36,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 40,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1O2O3B: [CmvsPs2aCommandStackWord; 3] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 12,
        kind: CmvsPs2aCommandStackWordKind::BooleanU32,
    },
];
const W_1O2B: [CmvsPs2aCommandStackWord; 2] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::BooleanU32,
    },
];
const W_1O2T3O4O: [CmvsPs2aCommandStackWord; 4] = [
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
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1O2T: [CmvsPs2aCommandStackWord; 2] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
];
const W_1O2T3T4T: [CmvsPs2aCommandStackWord; 4] = [
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
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 16,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
];
const W_1O2T3T4O: [CmvsPs2aCommandStackWord; 4] = [
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
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 16,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
const W_1T2O: [CmvsPs2aCommandStackWord; 2] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
use super::*;

/// Micro programs owned by this cluster, indexed by the local
/// `table_index` of contract arms.
pub(super) const OPS: &[&[NativeMicroOp]] = &[
    // case 357 (sub_48A570)
    &[
        NativeMicroOp::SetField {
            byte_offset: 81224,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 81228,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 358 (sub_48A5F0)
    &[
        NativeMicroOp::SetField {
            byte_offset: 81224,
            src: NativeMicroSrc::Field(10516),
        },
        NativeMicroOp::SetField {
            byte_offset: 81228,
            src: NativeMicroSrc::Field(10520),
        },
    ],
    // case 359 (sub_48A520)
    &[
        NativeMicroOp::SetField {
            byte_offset: 81224,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 81228,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 361 (sub_48A5A0)
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
    // case 367 (sub_48A610)
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
    // case 374 (sub_47D680)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 375 (sub_47D720)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 393 (sub_47E1C0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 395 (sub_47E5A0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 407 (sub_47F590)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x20000,
    }],
    // case 412 (sub_479DF0)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 414 (sub_479DB0)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 434 (sub_479F70)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 436 (sub_479F30)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 438 (sub_479BE0)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 440 (sub_479B20)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 442 (sub_479B60)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 444 (sub_479BA0)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 448 (sub_47D8B0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 449 (sub_47D940)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 450 (sub_47DA60)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 451 (sub_47DA00)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 453 (sub_47E010)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 454 (sub_47E070)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 455 (sub_47E160)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 456 (sub_47E100)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 458 (sub_47DE60)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 459 (sub_47DEC0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 460 (sub_47DFB0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 461 (sub_47DF50)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x10000,
    }],
    // case 478 (sub_47A030)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 483 (sub_482040)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(1024),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 3,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 32,
        },
    ],
    // case 484 (sub_481EF0)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(1024),
            taken_ops: 4,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 3,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 32,
        },
    ],
    // case 485 (sub_482BF0)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
    ],
    // case 486 (sub_482CF0)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(1024),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 3,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 32,
        },
    ],
    // case 487 (sub_482E20)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(1024),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 3,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 32,
        },
    ],
    // case 488 (sub_482DB0)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(1024),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 3,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 32,
        },
    ],
    // case 489 (sub_482C70)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(1024),
            taken_ops: 4,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 3,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 32,
        },
    ],
    // case 496 (sub_482770)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(1024),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 3,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 32,
        },
    ],
    // case 497 (sub_4828C0)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(1024),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 3,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 32,
        },
    ],
    // case 498 (sub_482850)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(1024),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 3,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 32,
        },
    ],
    // case 499 (sub_4826F0)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(1024),
            taken_ops: 4,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 3,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 32,
        },
    ],
    // case 503 (sub_482590)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(1024),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 3,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 32,
        },
    ],
    // case 504 (sub_482450)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(1024),
            taken_ops: 4,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 3,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 32,
        },
    ],
    // case 536 (sub_486B20)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 537 (sub_486AC0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 546 (sub_47A660)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3252),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::SetField {
            byte_offset: 3252,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 547 (sub_47A630)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3252),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0xFFFFFFFF)),
    ],
    // case 556 (sub_47AED0)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3260),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0xFFFFFFFF)),
    ],
    // case 557 (sub_47AE00)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3260),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::SetField {
            byte_offset: 3260,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 558 (sub_47AA50)
    &[
        NativeMicroOp::SetField {
            byte_offset: 3260,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(1)),
        NativeMicroOp::SetField {
            byte_offset: 3260,
            src: NativeMicroSrc::Const(1),
        },
    ],
    // case 564 (sub_47A760)
    &[
        NativeMicroOp::SetField {
            byte_offset: 3268,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(1)),
        NativeMicroOp::SetField {
            byte_offset: 3268,
            src: NativeMicroSrc::Const(1),
        },
    ],
    // case 566 (sub_47A960)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3268),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0xFFFFFFFF)),
    ],
    // case 567 (sub_47A890)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(3268),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::SetField {
            byte_offset: 3268,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 572 (sub_489C40)
    &[],
    // case 578 (sub_484610)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 2,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 512,
        },
    ],
    // case 579 (sub_484580)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 3,
        },
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 81240,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 2,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 512,
        },
    ],
    // case 580 (sub_484660)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 512,
        },
    ],
    // case 583 (sub_47B2E0)
    &[
        NativeMicroOp::SetField {
            byte_offset: 3264,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(1)),
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 1,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::SetField {
            byte_offset: 3264,
            src: NativeMicroSrc::Const(1),
        },
    ],
    // case 585 (sub_489F00)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::LtU,
            rhs: NativeMicroSrc::Const(24),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 512,
        },
    ],
    // case 586 (sub_489E80)
    &[],
    // case 587 (sub_489F40)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::LtU,
            rhs: NativeMicroSrc::Const(24),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 512,
        },
    ],
    // case 591 (sub_489DC0)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::LtU,
            rhs: NativeMicroSrc::Const(24),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 512,
        },
    ],
    // case 608 (sub_4852E0)
    &[],
    // case 609 (sub_485000)
    &[],
    // case 610 (sub_484FD0)
    &[],
    // case 611 (sub_485060)
    &[],
    // case 612 (sub_485030)
    &[],
    // case 616 (sub_47BB80)
    &[],
    // case 617 (sub_47B670)
    &[],
    // case 618 (sub_47BE20)
    &[],
    // case 619 (sub_47B8D0)
    &[],
    // case 620 (sub_47BCF0)
    &[],
    // case 621 (sub_47BA00)
    &[],
    // case 622 (sub_47B7A0)
    &[],
    // case 624 (sub_4851E0)
    &[],
    // case 626 (sub_4851B0)
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
    // case 627 (sub_485090)
    &[],
    // case 628 (sub_485160)
    &[],
    // case 629 (sub_4850F0)
    &[],
    // case 651 (sub_47C0D0)
    &[],
    // case 656 (sub_481940)
    &[],
    // case 657 (sub_481A30)
    &[],
    // case 658 (sub_480CA0)
    &[],
    // case 659 (sub_480D70)
    &[],
    // case 660 (sub_480E30)
    &[],
    // case 661 (sub_480BD0)
    &[],
    // case 662 (sub_480F00)
    &[],
    // case 663 (sub_480FD0)
    &[],
    // case 668 (sub_485E40)
    &[
        NativeMicroOp::SetField {
            byte_offset: 10548,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetField {
            byte_offset: 10548,
            src: NativeMicroSrc::Const(1),
        },
    ],
    // case 669 (sub_485E10)
    &[
        NativeMicroOp::SetField {
            byte_offset: 10552,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetField {
            byte_offset: 10552,
            src: NativeMicroSrc::Const(1),
        },
    ],
    // case 673 (sub_4812F0)
    &[],
    // case 674 (sub_481440)
    &[],
    // case 675 (sub_481820)
    &[],
    // case 676 (sub_4811A0)
    &[],
    // case 677 (sub_480930)
    &[],
    // case 678 (sub_4815E0)
    &[],
    // case 679 (sub_481700)
    &[],
    // case 681 (sub_4810A0)
    &[],
    // case 687 (sub_480B90)
    &[],
    // case 712 (sub_478980)
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
    // case 714 (sub_478950)
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
    // case 722 (sub_479990)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Field(1448)),
    ],
    // case 738 (sub_479670)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Field(1480)),
    ],
    // case 742 (sub_4792C0)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 4,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Field(1520),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 6,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(1)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 4,
        },
        NativeMicroOp::SetField {
            byte_offset: 1520,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetField {
            byte_offset: 1520,
            src: NativeMicroSrc::Const(1),
        },
    ],
    // case 746 (sub_479790)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Field(1584)),
    ],
    // case 748 (sub_479A10)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
    ],
    // case 752 (sub_479EB0)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 754 (sub_479FF0)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 756 (sub_479CB0)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 757 (sub_461250)
    &[
        NativeMicroOp::SetField {
            byte_offset: 1312,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 1320,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 781 (sub_47C980)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::LtU,
            rhs: NativeMicroSrc::Const(3),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x1000000,
        },
    ],
    // case 782 (sub_47C760)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::LtU,
            rhs: NativeMicroSrc::Const(3),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x1000000,
        },
    ],
    // case 783 (sub_47C430)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::LtU,
            rhs: NativeMicroSrc::Const(3),
            taken_ops: 2,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x1000000,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0xFFFFFFFF)),
    ],
    // case 784 (sub_484160)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
    ],
    // case 785 (sub_484340)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
    ],
    // case 787 (sub_4841E0)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
    ],
    // case 788 (sub_4842F0)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
    ],
    // case 789 (sub_4840B0)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
    ],
    // case 790 (sub_484230)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 3,
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 2,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
    ],
    // case 791 (sub_484100)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
    ],
    // case 794 (sub_47C4B0)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::LtU,
            rhs: NativeMicroSrc::Const(3),
            taken_ops: 2,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x1000000,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
    ],
    // case 795 (sub_47C350)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::LtU,
            rhs: NativeMicroSrc::Const(3),
            taken_ops: 2,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x1000000,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
    ],
    // case 796 (sub_47C9C0)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::LtU,
            rhs: NativeMicroSrc::Const(3),
            taken_ops: 2,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x1000000,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
    ],
    // case 797 (sub_47CA40)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::LtU,
            rhs: NativeMicroSrc::Const(3),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x1000000,
        },
    ],
    // case 798 (sub_47C3D0)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::LtU,
            rhs: NativeMicroSrc::Const(3),
            taken_ops: 2,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x1000000,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
    ],
    // case 800 (sub_483590)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(1024),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 3,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 32,
        },
    ],
    // case 801 (sub_483780)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 2,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(1024),
            taken_ops: 3,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 3,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 32,
        },
    ],
    // case 808 (sub_481DB0)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
    ],
    // case 809 (sub_481E20)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
    ],
    // case 817 (sub_481BF0)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
    ],
    // case 818 (sub_481B40)
    &[
        NativeMicroOp::Move {
            dst: 3,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::Call(NativeMicroMacro::ContextRead {
            slot_reg: 0,
            index_reg: 3,
        }),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(7),
            kind: NativeMicroCmpKind::Ne,
            rhs: NativeMicroSrc::Const(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 16,
        },
    ],
    // case 824 (sub_4871F0)
    &[],
    // case 836 (sub_479D30)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 837 (sub_461290)
    &[
        NativeMicroOp::SetField {
            byte_offset: 1348,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 1356,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 838 (sub_479CF0)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 839 (sub_461270)
    &[
        NativeMicroOp::SetField {
            byte_offset: 1360,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 1368,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 840 (sub_479EF0)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 841 (sub_461370)
    &[
        NativeMicroOp::SetField {
            byte_offset: 1336,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 1344,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 842 (sub_479E70)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 843 (sub_461330)
    &[
        NativeMicroOp::SetField {
            byte_offset: 1324,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 1332,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 861 (sub_479AB0)
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
    // case 872 (sub_47FD60)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
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
    // case 873 (sub_47FDE0)
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
    // case 874 (sub_47FD10)
    &[
        NativeMicroOp::SetResult(NativeMicroSrc::Const(0)),
        NativeMicroOp::SetField {
            byte_offset: 81236,
            src: NativeMicroSrc::Const(0),
        },
        NativeMicroOp::SetField {
            byte_offset: 81240,
            src: NativeMicroSrc::Const(0),
        },
    ],
    // case 875 (sub_47FFD0)
    &[],
    // case 880 (sub_486320)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(8),
            taken_ops: 2,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x100000,
        },
    ],
    // case 881 (sub_486960)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Lt,
            rhs: NativeMicroSrc::Const(8),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x100000,
        },
    ],
    // case 882 (sub_4868A0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 883 (sub_486840)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 884 (sub_4868F0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 885 (sub_486A10)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 886 (sub_4869C0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 887 (sub_486640)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 888 (sub_486760)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 889 (sub_486700)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 890 (sub_4862A0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 891 (sub_486480)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(8),
            taken_ops: 2,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(1),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(256),
            taken_ops: 1,
        },
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 0x100000,
        },
    ],
    // case 892 (sub_4865F0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 893 (sub_4867A0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 894 (sub_4866B0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 895 (sub_4867F0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 0x100000,
    }],
    // case 896 (sub_487B30)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(8),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(1)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 2,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 4096,
        },
    ],
    // case 897 (sub_487CD0)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Ge,
            rhs: NativeMicroSrc::Const(8),
            taken_ops: 2,
        },
        NativeMicroOp::SetResult(NativeMicroSrc::Const(1)),
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Eq,
            rhs: NativeMicroSrc::Reg(0),
            taken_ops: 2,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 4096,
        },
    ],
    // case 898 (sub_4883B0)
    &[
        NativeMicroOp::Guard {
            lhs: NativeMicroSrc::Reg(0),
            kind: NativeMicroCmpKind::Lt,
            rhs: NativeMicroSrc::Const(8),
            taken_ops: 1,
        },
        NativeMicroOp::OrField {
            byte_offset: 10532,
            mask: 4096,
        },
    ],
    // case 899 (sub_488910)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 900 (sub_488190)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 901 (sub_487F00)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 902 (sub_487EC0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 903 (sub_488140)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 904 (sub_4880A0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 905 (sub_4880F0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 906 (sub_487AE0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 907 (sub_488320)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 908 (sub_4882D0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 909 (sub_487FF0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 910 (sub_487AA0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 911 (sub_488370)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 912 (sub_488240)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 913 (sub_4888B0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 914 (sub_488870)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 915 (sub_488830)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 918 (sub_487E70)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 919 (sub_487A40)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 920 (sub_4881F0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 922 (sub_4887E0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 923 (sub_4885B0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 924 (sub_488600)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 925 (sub_488770)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 926 (sub_4886B0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 927 (sub_488650)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 928 (sub_488730)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 933 (sub_487400)
    &[],
    // case 935 (sub_487340)
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
    // case 936 (sub_487240)
    &[],
    // case 952 (sub_488530)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 953 (sub_488410)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 954 (sub_487F50)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 955 (sub_4884A0)
    &[NativeMicroOp::OrField {
        byte_offset: 10532,
        mask: 4096,
    }],
    // case 960 (sub_47BF80)
    &[],
    // case 962 (sub_47BFC0)
    &[],
    // case 965 (sub_479030)
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
    // case 967 (sub_4790F0)
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
    // Case 565 (`sub_47A990`): clears the result word, then the 3268
    // gate (scene object absent headless) takes the 0x400C path — result
    // zero, pop 12, continue. The live branch would wait on input via
    // 0xC004; unreachable without a bound object. Returns 0x400C.
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
    // Case 625 (`sub_485220`): same shape over the renderer object
    // (sub_411C60/sub_458370 absent headless) — result zero, 0x400C.
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
    // Case 711 (`sub_47A3D0`): channel-transition busy query; the
    // unbound channel table takes the <= 0 path — result zero, 0x4008.
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(0))],
];

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    match command_id {
        // Handler `sub_48A570` (case 357). Returns 0x4000.
        // publishes the screen metric pair into dwords 20306/20307 via the
        // 480-object getters, which default to zero headless. Omitted callees:
        // sub_432F70, sub_432F30.
        357 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 0,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_48A5F0` (case 358). Returns 0x4000.
        // copies the stored screen metrics (dwords 2629/2630) into the publish
        // pair at dwords 20306/20307.
        358 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 1,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_48A520` (case 359). Returns 0x4000.
        // publishes GetSystemMetrics(SM_CXSCREEN/CYSCREEN); the headless desktop
        // reports no metrics, so both dwords stay zero. Omitted callees:
        // GetSystemMetrics(x2).
        359 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 2,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_48A5A0` (case 361). Returns void.
        // window-state poll: the result word is pre-set zero, then takes 2 when
        // iconic or 1 when zoomed; the headless window is neither. Omitted
        // callees: IsIconic, IsZoomed.
        361 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 3,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_48A610` (case 367). Returns void.
        // clears the result word, then sets 1 when the screen object reports a
        // live mode; the child-absent default keeps zero. Omitted callees:
        // sub_432F40, sub_4291D0.
        367 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 4,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_47D680` (case 374). Returns 0x400C.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        374 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 5,
            },
            &W_1O2O3T[..],
        )),
        // Handler `sub_47D720` (case 375). Returns 0x4008.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        375 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 6,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_47E1C0` (case 393). Returns 0x4014.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        393 => Some((
            20,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 7,
            },
            &W_1O2O3O4O5O[..],
        )),
        // Handler `sub_47E5A0` (case 395). Returns 0x4010.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        395 => Some((
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 8,
            },
            &W_1O2O3O4O[..],
        )),
        // Handler `sub_47F590` (case 407). Returns 0x4008.
        // quad-state query on the occupied channel (`ch < 8 && this[742+ch]` and
        // quad < 32); the channel table stays unbound headless so the else path
        // raises mask 0x20000. Omitted callees: sub_401000, the ushort read at
        // +0x70 + 76*quad.
        407 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 9,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_479DF0` (case 412). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45CDF0; both default to the
        // child-absent zero. Omitted callee: sub_45CDF0.
        412 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 10,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_479DB0` (case 414). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45CDD0; both default to the
        // child-absent zero. Omitted callee: sub_45CDD0.
        414 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 11,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_479F70` (case 434). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45CFC0; both default to the
        // child-absent zero. Omitted callee: sub_45CFC0.
        434 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 12,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_479F30` (case 436). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45CFA0; both default to the
        // child-absent zero. Omitted callee: sub_45CFA0.
        436 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 13,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_479BE0` (case 438). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45CC90; both default to the
        // child-absent zero. Omitted callee: sub_45CC90.
        438 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 14,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_479B20` (case 440). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45CC30; both default to the
        // child-absent zero. Omitted callee: sub_45CC30.
        440 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 15,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_479B60` (case 442). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45CC50; both default to the
        // child-absent zero. Omitted callee: sub_45CC50.
        442 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 16,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_479BA0` (case 444). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45CC70; both default to the
        // child-absent zero. Omitted callee: sub_45CC70.
        444 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 17,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_47D8B0` (case 448). Returns 0x4010.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        448 => Some((
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 18,
            },
            &W_1O2O3O4O[..],
        )),
        // Handler `sub_47D940` (case 449). Returns 0x4020.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        449 => Some((
            32,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 19,
            },
            &W_1O2O3O4O5O6O7O8O[..],
        )),
        // Handler `sub_47DA60` (case 450). Returns 0x400C.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        450 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 20,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_47DA00` (case 451). Returns 0x4008.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        451 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 21,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_47E010` (case 453). Returns 0x400C.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        453 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 22,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_47E070` (case 454). Returns 0x401C.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        454 => Some((
            28,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 23,
            },
            &W_1O2O3O4O5O6O7O[..],
        )),
        // Handler `sub_47E160` (case 455). Returns 0x400C.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        455 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 24,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_47E100` (case 456). Returns 0x4008.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        456 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 25,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_47DE60` (case 458). Returns 0x400C.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        458 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 26,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_47DEC0` (case 459). Returns 0x401C.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        459 => Some((
            28,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 27,
            },
            &W_1O2O3O4O5O6O7O[..],
        )),
        // Handler `sub_47DFB0` (case 460). Returns 0x400C.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        460 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 28,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_47DF50` (case 461). Returns 0x4008.
        // layer-object call guarded by the channel/layer gate (ch < 8 && this[742+ch] && sub_466AE0(layer)); sub_466AE0 reads the channel child object, which the
        // recovered model never binds, so the else path raises mask 0x10000.
        // Omitted callees: sub_466AE0, sub_445340 and the layer worker (see
        // the decompiled body).
        461 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 29,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_47A030` (case 478). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45D080; both default to the
        // child-absent zero. Omitted callee: sub_45D080.
        478 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 30,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_482040` (case 483). Returns 0x4008.
        // context-table slot call (slot < 256, word index < 1024): the bound-slot
        // probe reads the first seed word via ContextRead (a bound slot whose
        // first context word is zero routes to the 0x10 mask); the indexed read
        // feeds the omitted workers. Omitted callees: sub_444EF0, sub_445340,
        // sub_42BA80.
        483 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 31,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_481EF0` (case 484). Returns 0x4008.
        // context-table slot query: same gate as case 483; the then-branch
        // publishes the 1030-object word (sub_42BA30, child-absent zero).
        // Omitted callees: sub_444EF0, sub_445340, sub_42BA30.
        484 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 32,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_482BF0` (case 485). Returns 0x400C.
        // context-table slot call with one payload word. Omitted callees:
        // sub_444EF0, sub_445340, sub_42C0A0.
        485 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 33,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_482CF0` (case 486). Returns 0x401C.
        // context-table slot call with five payload words. Omitted callees:
        // sub_444EF0, sub_445340, sub_42C150.
        486 => Some((
            28,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 34,
            },
            &W_1O2O3O4O5O6O7O[..],
        )),
        // Handler `sub_482E20` (case 487). Returns 0x400C.
        // context-table slot call with one payload word. Omitted callees:
        // sub_444EF0, sub_445340, sub_42C1B0.
        487 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 35,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_482DB0` (case 488). Returns 0x4008.
        // context-table slot call, no payload. Omitted callees: sub_444EF0,
        // sub_445340, sub_42C170.
        488 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 36,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_482C70` (case 489). Returns 0x4008.
        // context-table slot query (child-absent zero). Omitted callees:
        // sub_444EF0, sub_445340, sub_42C140.
        489 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 37,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_482770` (case 496). Returns 0x402C.
        // nine-word context-table geometry call. Omitted callees: sub_444EF0,
        // sub_445340, sub_42C000.
        496 => Some((
            44,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 38,
            },
            &W_1O2O3O4O5O6O7O8O9OAOBO[..],
        )),
        // Handler `sub_4828C0` (case 497). Returns 0x400C.
        // context-table slot call with one payload word. Omitted callees:
        // sub_444EF0, sub_445340, sub_42C070.
        497 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 39,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_482850` (case 498). Returns 0x4008.
        // context-table slot call, no payload. Omitted callees: sub_444EF0,
        // sub_445340, sub_42C020.
        498 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 40,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_4826F0` (case 499). Returns 0x4008.
        // context-table slot query (child-absent zero). Omitted callees:
        // sub_444EF0, sub_445340, sub_42BFF0.
        499 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 41,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_482590` (case 503). Returns 0x4008.
        // context-table slot call, no payload. Omitted callees: sub_444EF0,
        // sub_445340, sub_42BD80.
        503 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 42,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_482450` (case 504). Returns 0x4008.
        // context-table slot query (child-absent zero). Omitted callees:
        // sub_444EF0, sub_445340, sub_42BD40.
        504 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 43,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_486B20` (case 536). Returns 0x4004.
        // slot call on the 799-table (slot < 6); the table stays unbound headless
        // so the else path raises mask 0x100000. Omitted callee: sub_416BD0.
        536 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 44,
            },
            &W_1O[..],
        )),
        // Handler `sub_486AC0` (case 537). Returns 0x4018.
        // five-word 799-table call; unbound headless -> mask 0x100000. Omitted
        // callee: sub_4685F0.
        537 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 45,
            },
            &W_1O2O3O4O5O6O[..],
        )),
        // Handler `sub_47A660` (case 546). Returns 0x4000.
        // frees the filter-graph owner held at dword 813 when present; the
        // handle word ends zeroed either way. The sub_458C30 liveness gate is
        // omitted (assumed live). Omitted callees: sub_458C30, sub_458C00,
        // sub_4A9B8F.
        546 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 46,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_47A630` (case 547). Returns 0x4000.
        // singleton query: publishes sub_458E50 when the handle word at byte 3252 is
        // live, else -1; the child-absent default is zero. Omitted callee:
        // sub_458E50.
        547 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 47,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_47AED0` (case 556). Returns 0x4000.
        // singleton query: publishes sub_4590C0 when the handle word at byte 3260 is
        // live, else -1; the child-absent default is zero. Omitted callee:
        // sub_4590C0.
        556 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 48,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_47AE00` (case 557). Returns 0x4000.
        // destroys the singleton at byte 3260 when live: the handle word is
        // nulled; the nested teardown (sub_4590A0/sub_443E70) and the dynamic this[481+sub]
        // release are unmodelled. Omitted callees: sub_432F40, sub_432F10,
        // sub_4A9B8F, sub_4590A0/sub_443E70.
        557 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 49,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_47AA50` (case 558). Returns 0x400C.
        // rebuilds the 815-singleton: the prior handle is nulled, the result word
        // takes 1, then the fresh constructor handle is stored as a non-zero
        // marker (native stores the heap pointer). The this[481] master table
        // write and the v12[297] failure path stay unmodelled. Omitted callees:
        // sub_459540, sub_4590A0, sub_443E70, sub_443D90, sub_458F10.
        558 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 50,
            },
            &W_1T2O3O[..],
        )),
        // Handler `sub_47A760` (case 564). Returns 0x4014.
        // rebuilds the 817-singleton (sub_459570 constructor): prior handle
        // nulled, result 1, fresh handle marker stored; the v8[298] failure path
        // stays unmodelled. Omitted callees: sub_459710, sub_459570, sub_4A9B8F.
        564 => Some((
            20,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 51,
            },
            &W_1T2O3O4O5O[..],
        )),
        // Handler `sub_47A960` (case 566). Returns 0x4000.
        // singleton query: publishes sub_459750 when the handle word at byte 3268 is
        // live, else -1; the child-absent default is zero. Omitted callee:
        // sub_459750.
        566 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 52,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_47A890` (case 567). Returns 0x4000.
        // destroys the singleton at byte 3268 when live: the handle word is
        // nulled; the nested teardown (sub_459710/sub_443E70) and the dynamic this[481+sub]
        // release are unmodelled. Omitted callees: sub_432F40, sub_432F10,
        // sub_4A9B8F, sub_459710/sub_443E70.
        567 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 53,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_489C40` (case 572). Returns 0x4004.
        // clamps the operand to 3000 and applies it as the master volume;
        // the audio path is omitted. Omitted callees: sub_48C3B0, sub_48D720.
        572 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 54,
            },
            &W_1O[..],
        )),
        // Handler `sub_484610` (case 578). Returns 0x4004.
        // audio-device query: a non-zero operand raises mask 0x200; the zero path
        // publishes sub_48C230(15) (child-absent zero). Omitted callee:
        // sub_48C230.
        578 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 55,
            },
            &W_1O[..],
        )),
        // Handler `sub_484580` (case 579). Returns 0x4004.
        // audio-device query: a non-zero operand raises mask 0x200; the zero path
        // stores the sub_48C2A0(15) out-pair into dwords 20309/20310 as the
        // child-absent zero default. Omitted callee: sub_48C2A0.
        579 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 56,
            },
            &W_1O[..],
        )),
        // Handler `sub_484660` (case 580). Returns 0x4008.
        // audio-device store: a non-zero first operand raises mask 0x200; the
        // zero path forwards the second word. Omitted callee: sub_48D700.
        580 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 57,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_47B2E0` (case 583). Returns 0x4014.
        // rebuilds the 816-singleton: prior handle nulled, result 1; the fresh
        // handle marker is stored only when the 481-slot (values[1], offset 8)
        // passes the seed probe. The this[481+slot] unbind write and the
        // constructor workers stay unmodelled. Omitted callees: sub_45A0A0,
        // sub_45A050, sub_45AA50, sub_432EF0, sub_432F00.
        583 => Some((
            20,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 58,
            },
            &W_1O2O3O4O5O[..],
        )),
        // Handler `sub_489F00` (case 585). Returns 0x4004.
        // audio-channel selector: an operand above 23 raises mask 0x200; the
        // in-range path calls sub_48CC40, omitted.
        585 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 59,
            },
            &W_1O[..],
        )),
        // Handler `sub_489E80` (case 586). Returns 0x400C.
        // three-word audio-channel mix: scales dwords 390/392 into the forwarded
        // volume; no interpreter writes. Omitted callee: sub_48C9E0.
        586 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 60,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_489F40` (case 587). Returns 0x4004.
        // audio-channel selector: an operand above 23 raises mask 0x200; the
        // in-range path calls sub_48DB50, omitted.
        587 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 61,
            },
            &W_1O[..],
        )),
        // Handler `sub_489DC0` (case 591). Returns 0x4004.
        // audio-channel selector: an operand above 23 raises mask 0x200; the
        // in-range path calls sub_48C920 (gated on dword 389), omitted.
        591 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 62,
            },
            &W_1O[..],
        )),
        // Handler `sub_4852E0` (case 608). Returns 0x400C.
        // three-word screen-object call. Omitted callees: sub_411C60, sub_458AF0.
        608 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 63,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_485000` (case 609). Returns 0x4004.
        // one-word screen-object call. Omitted callees: sub_411C60, sub_458AD0.
        609 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 64,
            },
            &W_1O[..],
        )),
        // Handler `sub_484FD0` (case 610). Returns 0x4004.
        // one-word screen-object call. Omitted callees: sub_411C60, sub_458AB0.
        610 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 65,
            },
            &W_1O[..],
        )),
        // Handler `sub_485060` (case 611). Returns 0x4004.
        // one-word screen-object call. Omitted callees: sub_411C60, sub_458B40.
        611 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 66,
            },
            &W_1O[..],
        )),
        // Handler `sub_485030` (case 612). Returns 0x4004.
        // one-word screen-object call. Omitted callees: sub_411C60, sub_458B20.
        612 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 67,
            },
            &W_1O[..],
        )),
        // Handler `sub_47BB80` (case 616). Returns 0x4018.
        // builds the 10764-prefixed resource name (LocalAlloc + sub_427280 +
        // lstrcatA) and hands it to sub_48AE50 with the four payload words and
        // dwords 3785/3786; no interpreter writes. Omitted callees:
        // LocalAlloc, sub_427280, lstrcatA, sub_48AE50, LocalFree.
        616 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 68,
            },
            &W_1O2T3O4O5O6O[..],
        )),
        // Handler `sub_47B670` (case 617). Returns 0x4018.
        // builds the 10764-prefixed resource name (LocalAlloc + sub_427280 +
        // lstrcatA) and hands it to sub_4405F0 with the four payload words and
        // dwords 3785/3786; no interpreter writes. Omitted callees:
        // LocalAlloc, sub_427280, lstrcatA, sub_4405F0, LocalFree.
        617 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 69,
            },
            &W_1O2T3O4O5O6O[..],
        )),
        // Handler `sub_47BE20` (case 618). Returns 0x4018.
        // builds the 10764-prefixed resource name (LocalAlloc + sub_427280 +
        // lstrcatA) and hands it to sub_48AE50 with the four payload words and
        // dwords 3785/3786; no interpreter writes. Omitted callees:
        // LocalAlloc, sub_427280, lstrcatA, sub_48AE50, LocalFree.
        618 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 70,
            },
            &W_1O2T3O4O5O6O[..],
        )),
        // Handler `sub_47B8D0` (case 619). Returns 0x4018.
        // builds the 10764-prefixed resource name (LocalAlloc + sub_427280 +
        // lstrcatA) and hands it to sub_48AE50 with the four payload words and
        // dwords 3785/3786; no interpreter writes. Omitted callees:
        // LocalAlloc, sub_427280, lstrcatA, sub_48AE50, LocalFree.
        619 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 71,
            },
            &W_1O2T3O4O5O6O[..],
        )),
        // Handler `sub_47BCF0` (case 620). Returns 0x4018.
        // builds the 10764-prefixed resource name (LocalAlloc + sub_427280 +
        // lstrcatA) and hands it to sub_48AE50 with the four payload words and
        // dwords 3785/3786; no interpreter writes. Omitted callees:
        // LocalAlloc, sub_427280, lstrcatA, sub_48AE50, LocalFree.
        620 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 72,
            },
            &W_1O2T3O4O5O6O[..],
        )),
        // Handler `sub_47BA00` (case 621). Returns 0x4018.
        // builds the 10764-prefixed resource name (LocalAlloc + sub_427280 +
        // lstrcatA) and hands it to sub_4331B0 with the four payload words and
        // dwords 3785/3786; no interpreter writes. Omitted callees:
        // LocalAlloc, sub_427280, lstrcatA, sub_4331B0, LocalFree.
        621 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 73,
            },
            &W_1O2T3O4O5O6O[..],
        )),
        // Handler `sub_47B7A0` (case 622). Returns 0x4018.
        // builds the 10764-prefixed resource name (LocalAlloc + sub_427280 +
        // lstrcatA) and hands it to sub_441A70 with the four payload words and
        // dwords 3785/3786; no interpreter writes. Omitted callees:
        // LocalAlloc, sub_427280, lstrcatA, sub_441A70, LocalFree.
        622 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 74,
            },
            &W_1O2T3O4O5O6O[..],
        )),
        // Handler `sub_4851E0` (case 624). Returns 0x4004.
        // boolean screen-object call. Omitted callees: sub_411C60, sub_458A10.
        624 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 75,
            },
            &W_1B[..],
        )),
        // Handler `sub_4851B0` (case 626). Returns 0x4000.
        // publishes the screen-object count (sub_458370, child-absent zero).
        // Omitted callees: sub_411C60, sub_458370.
        626 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 76,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_485090` (case 627). Returns 0x4014.
        // five-word screen-object call. Omitted callees: sub_411C60,
        // sub_4588C0.
        627 => Some((
            20,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 77,
            },
            &W_1O2O3O4O5O[..],
        )),
        // Handler `sub_485160` (case 628). Returns 0x4010.
        // four-word screen-object call. Omitted callees: sub_411C60,
        // sub_4589B0.
        628 => Some((
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 78,
            },
            &W_1O2O3O4O[..],
        )),
        // Handler `sub_4850F0` (case 629). Returns 0x401C.
        // seven-word screen-object call. Omitted callees: sub_411C60,
        // sub_458930.
        629 => Some((
            28,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 79,
            },
            &W_1O2O3O4O5O6O7O[..],
        )),
        // Handler `sub_47C0D0` (case 651). Returns 0x400C.
        // timeline registration with the resolved name. Omitted callee:
        // sub_470D50, sub_4243F0.
        651 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 80,
            },
            &W_1O2T3O[..],
        )),
        // Handler `sub_481940` (case 656). Returns 0x400C.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_448D70 runs with two resolved names. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_448D70.
        656 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 81,
            },
            &W_1O2T3T[..],
        )),
        // Handler `sub_481A30` (case 657). Returns 0x4004.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_448E20 runs with dwords 792/835. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_448E20.
        657 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 82,
            },
            &W_1O[..],
        )),
        // Handler `sub_480CA0` (case 658). Returns 0x4004.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_4483E0 runs with dwords 792/835. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_4483E0.
        658 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 83,
            },
            &W_1O[..],
        )),
        // Handler `sub_480D70` (case 659). Returns 0x4004.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_4484A0 runs with no args. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_4484A0.
        659 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 84,
            },
            &W_1O[..],
        )),
        // Handler `sub_480E30` (case 660). Returns 0x4004.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_448550 runs with dwords 792/835. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_448550.
        660 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 85,
            },
            &W_1O[..],
        )),
        // Handler `sub_480BD0` (case 661). Returns 0x4004.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_448320 runs with dwords 792/835. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_448320.
        661 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 86,
            },
            &W_1O[..],
        )),
        // Handler `sub_480F00` (case 662). Returns 0x4008.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_448600 runs with dwords 792/835. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_448600.
        662 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 87,
            },
            &W_1O[..],
        )),
        // Handler `sub_480FD0` (case 663). Returns 0x4004.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_4486C0 runs with dwords 792/835. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_4486C0.
        663 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 88,
            },
            &W_1O[..],
        )),
        // Handler `sub_485E40` (case 668). Returns 0x4004.
        // stores `operand != 0` into dword 2637 (byte 10548).
        668 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 89,
            },
            &W_1B[..],
        )),
        // Handler `sub_485E10` (case 669). Returns 0x4004.
        // stores `operand != 0` into dword 2638 (byte 10552).
        669 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 90,
            },
            &W_1B[..],
        )),
        // Handler `sub_4812F0` (case 673). Returns 0x4020.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_448C80 runs with a name and six words. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_448C80.
        673 => Some((
            32,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 91,
            },
            &W_1O2T3O4O5O6O7O8O[..],
        )),
        // Handler `sub_481440` (case 674). Returns 0x4020.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_448ED0 runs with a name and six words. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_448ED0.
        674 => Some((
            32,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 92,
            },
            &W_1O2T3O4O5O6O7O8O[..],
        )),
        // Handler `sub_481820` (case 675). Returns 0x4018.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_448AC0 runs with a name and four words. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_448AC0.
        675 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 93,
            },
            &W_1O2T3O4O5O6O[..],
        )),
        // Handler `sub_4811A0` (case 676). Returns 0x4020.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_448B90 runs with a name and six words. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_448B90.
        676 => Some((
            32,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 94,
            },
            &W_1O2T3O4O5O6O7O8O[..],
        )),
        // Handler `sub_480930` (case 677). Returns 0x4020.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_448840 runs with a name and six words. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_448840.
        677 => Some((
            32,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 95,
            },
            &W_1O2T3O4O5O6O7O8O[..],
        )),
        // Handler `sub_4815E0` (case 678). Returns 0x4018.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_448920 runs with a name and four words. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_448920.
        678 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 96,
            },
            &W_1O2T3O4O5O6O[..],
        )),
        // Handler `sub_481700` (case 679). Returns 0x4018.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_4489F0 runs with a name and four words. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_4489F0.
        679 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 97,
            },
            &W_1O2T3O4O5O6O[..],
        )),
        // Handler `sub_4810A0` (case 681). Returns 0x4018.
        // r
        // slot-table churn (free sub_447E30/sub_4A9B8F, ctor
        // sub_447D90, store pointer) is outside the recovered slot-object
        // model, and sub_448780 runs with a name and four words. No interpreter writes. Omitted
        // callees: sub_447E30, sub_4A9B8F, sub_447D90, sub_448780.
        681 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 98,
            },
            &W_1O2T3O4O5O6O[..],
        )),
        // Handler `sub_480B90` (case 687). Returns 0x4010.
        // three-word filter call gated on slot <= 3; the gate routes only to the
        // omitted worker. Omitted callee: sub_448090.
        687 => Some((
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 99,
            },
            &W_1O2O3O4O[..],
        )),
        // Handler `sub_478980` (case 712). Returns 0x4000.
        // publishes the boolean/word state of the 480-object through sub_42AB30;
        // child-absent default zero. Omitted callees: sub_432F10, sub_42AB30.
        712 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 100,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_478950` (case 714). Returns 0x4000.
        // publishes the boolean/word state of the 480-object through sub_42AF50;
        // child-absent default zero. Omitted callees: sub_432F10, sub_42AF50.
        714 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 101,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_479990` (case 722). Returns 0x4008.
        // dual-mode control: a non-zero first operand drives the omitted
        // window-message call; the zero path publishes the stored dword at
        // byte 1448 into the result word. Omitted callees: sub_496730-class
        // worker, sub_41A200.
        722 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 102,
            },
            &W_1B2O[..],
        )),
        // Handler `sub_479670` (case 738). Returns 0x4008.
        // dual-mode control: a non-zero first operand drives the omitted
        // window-message call; the zero path publishes the stored dword at
        // byte 1480 into the result word. Omitted callees: sub_496730-class
        // worker, sub_41A200.
        738 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 103,
            },
            &W_1B2O[..],
        )),
        // Handler `sub_4792C0` (case 742). Returns 0x4008.
        // boolean toggle at dword 380 (byte 1520): a non-zero first operand
        // stores `second operand != 0`, the zero path publishes the stored
        // boolean into the result word.
        742 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 104,
            },
            &W_1B2B[..],
        )),
        // Handler `sub_479790` (case 746). Returns 0x4008.
        // dual-mode control: a non-zero first operand drives the omitted
        // window-message call; the zero path publishes the stored dword at
        // byte 1584 into the result word. Omitted callees: sub_496730-class
        // worker, sub_41A200.
        746 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 105,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_479A10` (case 748). Returns 0x4008.
        // dual-mode control: a non-zero first operand drives the omitted setter;
        // the zero path publishes the deep query boolean (sub_4292C0,
        // child-absent zero). Omitted callees: sub_432F40, sub_429B20,
        // sub_4292C0.
        748 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 106,
            },
            &W_1B2B[..],
        )),
        // Handler `sub_479EB0` (case 752). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45CF60; both default to the
        // child-absent zero. Omitted callee: sub_45CF60.
        752 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 107,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_479FF0` (case 754). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45D000; both default to the
        // child-absent zero. Omitted callee: sub_45D000.
        754 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 108,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_479CB0` (case 756). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45CD50; both default to the
        // child-absent zero. Omitted callee: sub_45CD50.
        756 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 109,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_461250` (case 757). Returns void.
        // void handler: clears the dword pair 328/330.
        757 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 110,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_47C980` (case 781). Returns 0x4008.
        // bounded-slot call: a slot above 2 raises mask 0x1000000; the occupied
        // path runs the omitted worker. Occupancy lives in the slot-object map
        // (unreachable here), so the unbound path falls through without the
        // worker. Omitted callee: sub_442BE0.
        781 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 111,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_47C760` (case 782). Returns 0x4018.
        // bounded-slot creator: a slot above 2 raises mask 0x1000000; the create
        // path rebuilds the dword-796 and dword-481 entries (slot-object churn
        // outside the micro model). Omitted callees: sub_441E10, sub_443E70,
        // sub_443D90, sub_441D90, sub_4430E0, sub_4A9B8F.
        782 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 112,
            },
            &W_1O2O3T5O6O[..],
        )),
        // Handler `sub_47C430` (case 783). Returns 0x4004.
        // bounded-slot query: a slot above 2 raises mask 0x1000000; the occupied
        // path publishes the sub_442B40 pair into dwords 20305/20309. With the
        // 796-occupancy unmodelled the unbound path stores dword 20309 = 0 and
        // result -1. Omitted callee: sub_442B40.
        783 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 113,
            },
            &W_1O[..],
        )),
        // Handler `sub_484160` (case 784). Returns 0x4014.
        // context-table slot call; worker omitted. Omitted callees:
        // sub_444EF0, sub_445340, sub_446F60.
        784 => Some((
            20,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 114,
            },
            &W_1O2O3T4O5O[..],
        )),
        // Handler `sub_484340` (case 785). Returns 0x4008.
        // context-table slot call; worker omitted. Omitted callees:
        // sub_444EF0, sub_445340, sub_4470D0.
        785 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 115,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_4841E0` (case 787). Returns 0x400C.
        // context-table slot call; worker omitted. Omitted callees:
        // sub_444EF0, sub_445340, sub_446FF0.
        787 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 116,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_4842F0` (case 788). Returns 0x400C.
        // context-table slot call; worker omitted. Omitted callees:
        // sub_444EF0, sub_445340, sub_4470B0.
        788 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 117,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_4840B0` (case 789). Returns 0x4010.
        // context-table slot call; worker omitted. Omitted callees:
        // sub_444EF0, sub_445340, sub_446EB0.
        789 => Some((
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 118,
            },
            &W_1O2O3O4O[..],
        )),
        // Handler `sub_484230` (case 790). Returns 0x4018.
        // two-slot context-table call: both operands (values[0] at offset 4,
        // values[2] at offset 12) must be bound; either unbound probe raises
        // mask 0x10. Omitted callees: sub_444EF0, sub_445340, sub_447010.
        790 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 119,
            },
            &W_1O2O3O4O5O6O[..],
        )),
        // Handler `sub_484100` (case 791). Returns 0x4018.
        // context-table slot call; worker omitted. Omitted callees:
        // sub_444EF0, sub_445340, sub_446ED0.
        791 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 120,
            },
            &W_1O2O3O4O5O6O[..],
        )),
        // Handler `sub_47C4B0` (case 794). Returns 0x4004.
        // bounded-slot query: a slot above 2 raises mask 0x1000000; the
        // occupied path publishes sub_442B30 (child-absent zero) and the unbound
        // path stores zero. Omitted callee: sub_442B30.
        794 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 121,
            },
            &W_1O[..],
        )),
        // Handler `sub_47C350` (case 795). Returns 0x4008.
        // bounded-slot query: a slot above 2 raises mask 0x1000000; the
        // occupied path publishes sub_442A80 (child-absent zero) and the unbound
        // path stores zero. Omitted callee: sub_442A80.
        795 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 122,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_47C9C0` (case 796). Returns 0x4004.
        // bounded-slot query: a slot above 2 raises mask 0x1000000; the
        // occupied path publishes sub_442BF0 (child-absent zero) and the unbound
        // path stores zero. Omitted callee: sub_442BF0.
        796 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 123,
            },
            &W_1O[..],
        )),
        // Handler `sub_47CA40` (case 797). Returns 0x4008.
        // bounded-slot call: a slot above 2 raises mask 0x1000000; the occupied
        // path runs the omitted worker. Omitted callee: sub_42EA10.
        797 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 124,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_47C3D0` (case 798). Returns 0x4004.
        // bounded-slot query: a slot above 2 raises mask 0x1000000; the
        // occupied path publishes sub_442B20 (child-absent zero) and the unbound
        // path stores zero. Omitted callee: sub_442B20.
        798 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 125,
            },
            &W_1O[..],
        )),
        // Handler `sub_483590` (case 800). Returns 0x400C.
        // context-table slot call with the index guard; worker omitted.
        // Omitted callees: sub_444EF0, sub_445340, sub_446A60.
        800 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 126,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_483780` (case 801). Returns 0x400C.
        // context-table slot call with the index guard; worker omitted.
        // Omitted callees: sub_444EF0, sub_445340, sub_446B00.
        801 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 127,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_481DB0` (case 808). Returns 0x4028.
        // context-table slot call; worker omitted. Omitted callees:
        // sub_444EF0, sub_445340, sub_446250.
        808 => Some((
            40,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 128,
            },
            &W_1O2O3O4O5O6O7O8O9OAO[..],
        )),
        // Handler `sub_481E20` (case 809). Returns 0x4004.
        // context-table slot call; worker omitted. Omitted callees:
        // sub_444EF0, sub_445340, sub_4466F0.
        809 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 129,
            },
            &W_1O[..],
        )),
        // Handler `sub_481BF0` (case 817). Returns 0x4004.
        // context-table slot call; worker omitted. Omitted callees:
        // sub_444EF0, sub_445340, sub_445940.
        817 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 130,
            },
            &W_1O[..],
        )),
        // Handler `sub_481B40` (case 818). Returns 0x4010.
        // context-table slot call; worker omitted. Omitted callees:
        // sub_444EF0, sub_445340, sub_445880.
        818 => Some((
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 131,
            },
            &W_1O2O3O4O[..],
        )),
        // Handler `sub_4871F0` (case 824). Returns 0x4004.
        // resolves the name and forwards it to the archive scanner; no
        // interpreter writes. Omitted callees: sub_470D50, sub_43F720.
        824 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 132,
            },
            &W_1O[..],
        )),
        // Handler `sub_479D30` (case 836). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45CD90; both default to the
        // child-absent zero. Omitted callee: sub_45CD90.
        836 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 133,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_461290` (case 837). Returns void.
        // void handler: clears the dword pair 337/339.
        837 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 134,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_479CF0` (case 838). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45CD70; both default to the
        // child-absent zero. Omitted callee: sub_45CD70.
        838 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 135,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_461270` (case 839). Returns void.
        // void handler: clears the dword pair 340/342.
        839 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 136,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_479EF0` (case 840). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45CF80; both default to the
        // child-absent zero. Omitted callee: sub_45CF80.
        840 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 137,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_461370` (case 841). Returns void.
        // void handler: clears the dword pair 334/336.
        841 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 138,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_479E70` (case 842). Returns 0x4000.
        // device state poll: the result word takes the boolean return and
        // dword 20309 the boolean out-param of sub_45CE30; both default to the
        // child-absent zero. Omitted callee: sub_45CE30.
        842 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 139,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_461330` (case 843). Returns void.
        // void handler: clears the dword pair 331/333.
        843 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 140,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_479AB0` (case 861). Returns 0x4000.
        // publishes the boolean return of sub_45CBF0 on the 818-object
        // (child-absent zero). Omitted callee: sub_45CBF0.
        861 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 141,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_47FD60` (case 872). Returns 0x4000.
        // four-word device query: the sub_45AF70 out-params land in dwords
        // 20305/20309/20310/20311 as the child-absent zero default. Omitted
        // callee: sub_45AF70.
        872 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 142,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_47FDE0` (case 873). Returns 0x4004.
        // publishes the boolean return of sub_45AFB0 (child-absent zero).
        // Omitted callee: sub_45AFB0.
        873 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 143,
            },
            &W_1B[..],
        )),
        // Handler `sub_47FD10` (case 874). Returns 0x4000.
        // three-word device query: the sub_45AF20 out-params land in dwords
        // 20305/20309/20310 as the child-absent zero default. Omitted callee:
        // sub_45AF20.
        874 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 144,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_47FFD0` (case 875). Returns 0x4004.
        // one-word device call. Omitted callee: sub_45C490.
        875 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 145,
            },
            &W_1O[..],
        )),
        // Handler `sub_486320` (case 880). Returns 0x4008.
        // creates the 805-table object: an out-of-range pair (index >= 8 or
        // slot >= 256) raises mask 0x100000; the create path is slot-object
        // churn outside the micro model. Omitted callees: sub_432F40,
        // sub_4290A0, sub_432F50, sub_4292D0, sub_411E10, sub_4161F0,
        // sub_4A9B8F, sub_416160.
        880 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 146,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_486960` (case 881). Returns 0x4004.
        // destroys the 805-table object for the slot (index >= 8 raises mask
        // 0x100000); the free itself is slot-object churn. Omitted callees:
        // sub_4161F0, sub_4A9B8F.
        881 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 147,
            },
            &W_1O[..],
        )),
        // Handler `sub_4868A0` (case 882). Returns 0x4008.
        // slot call gated on the 805-table occupancy (`slot < 8 &&
        // this[805+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x100000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_416410.
        882 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 148,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_486840` (case 883). Returns 0x4018.
        // slot call gated on the 805-table occupancy (`slot < 8 &&
        // this[805+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x100000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_416520.
        883 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 149,
            },
            &W_1O2O3O4O5O6O[..],
        )),
        // Handler `sub_4868F0` (case 884). Returns 0x4028.
        // slot call gated on the 805-table occupancy (`slot < 8 &&
        // this[805+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x100000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_416A70.
        884 => Some((
            40,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 150,
            },
            &W_1O2O3O4O5O6O7O8O9OAO[..],
        )),
        // Handler `sub_486A10` (case 885). Returns 0x4004.
        // slot call gated on the 805-table occupancy (`slot < 8 &&
        // this[805+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x100000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_4165A0.
        885 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 151,
            },
            &W_1O[..],
        )),
        // Handler `sub_4869C0` (case 886). Returns 0x4004.
        // slot call gated on the 805-table occupancy (`slot < 8 &&
        // this[805+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x100000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_416590.
        886 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 152,
            },
            &W_1O[..],
        )),
        // Handler `sub_486640` (case 887). Returns 0x4004.
        // slot call gated on the 805-table occupancy (`slot < 8 &&
        // this[805+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x100000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_416640.
        887 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 153,
            },
            &W_1O[..],
        )),
        // Handler `sub_486760` (case 888). Returns 0x4004.
        // slot call gated on the 805-table occupancy (`slot < 8 &&
        // this[805+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x100000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_416BD0.
        888 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 154,
            },
            &W_1O[..],
        )),
        // Handler `sub_486700` (case 889). Returns 0x4018.
        // slot call gated on the 805-table occupancy (`slot < 8 &&
        // this[805+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x100000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_4164A0.
        889 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 155,
            },
            &W_1O2O3O4O5O6O[..],
        )),
        // Handler `sub_4862A0` (case 890). Returns 0x400C.
        // slot call gated on the 805-table occupancy (`slot < 8 &&
        // this[805+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x100000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_416220.
        890 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 156,
            },
            &W_1O2O3B[..],
        )),
        // Handler `sub_486480` (case 891). Returns 0x4010.
        // creates the 805-table object (case-880 shape with two extra payload
        // words folded into the omitted constructor). Omitted callees:
        // sub_432F40, sub_4290A0, sub_432F50, sub_4292D0, sub_411E10,
        // sub_4161F0, sub_4A9B8F, sub_416160.
        891 => Some((
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 157,
            },
            &W_1O2O3O4O[..],
        )),
        // Handler `sub_4865F0` (case 892). Returns 0x4008.
        // slot call gated on the 805-table occupancy (`slot < 8 &&
        // this[805+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x100000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_433AC0.
        892 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 158,
            },
            &W_1O2B[..],
        )),
        // Handler `sub_4867A0` (case 893). Returns 0x4014.
        // slot call gated on the 805-table occupancy (`slot < 8 &&
        // this[805+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x100000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_416500.
        893 => Some((
            20,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 159,
            },
            &W_1O2O3O4O5O[..],
        )),
        // Handler `sub_4866B0` (case 894). Returns 0x4008.
        // slot call gated on the 805-table occupancy (`slot < 8 &&
        // this[805+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x100000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_4164F0.
        894 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 160,
            },
            &W_1O2B[..],
        )),
        // Handler `sub_4867F0` (case 895). Returns 0x4004.
        // slot call gated on the 805-table occupancy (`slot < 8 &&
        // this[805+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x100000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_416570.
        895 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 161,
            },
            &W_1O[..],
        )),
        // Handler `sub_487B30` (case 896). Returns 0x4010.
        // creates the 775-table console object: an index >= 8 raises mask
        // 0x1000; the create path also lazily builds the 783-parent (churn
        // outside the micro model) and publishes `constructor != 0`, so the
        // good path stores the non-zero marker. Omitted callees: sub_432F40,
        // sub_4290A0, sub_432F50, sub_4292D0, sub_411E10, sub_461CD0,
        // sub_437AF0.
        896 => Some((
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 162,
            },
            &W_1O2T3O4O[..],
        )),
        // Handler `sub_487CD0` (case 897). Returns 0x4018.
        // creates the 775-table console object (case-896 shape with two more
        // payload words). Omitted callees: same as case 896.
        897 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 163,
            },
            &W_1O2T3O4O5O6O[..],
        )),
        // Handler `sub_4883B0` (case 898). Returns 0x4004.
        // destroys the 775-table console object (index >= 8 raises mask 0x1000);
        // the free itself is slot-object churn. Omitted callees: sub_437D30,
        // sub_4A9B8F.
        898 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 164,
            },
            &W_1O[..],
        )),
        // Handler `sub_488910` (case 899). Returns 0x400C.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43B200.
        899 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 165,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_488190` (case 900). Returns 0x400C.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43B2B0.
        900 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 166,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_487F00` (case 901). Returns 0x4014.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A700.
        901 => Some((
            20,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 167,
            },
            &W_1O2O3O4O5O[..],
        )),
        // Handler `sub_487EC0` (case 902). Returns 0x4004.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A6D0.
        902 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 168,
            },
            &W_1O[..],
        )),
        // Handler `sub_488140` (case 903). Returns 0x4010.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A420.
        903 => Some((
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 169,
            },
            &W_1O2O3O4O[..],
        )),
        // Handler `sub_4880A0` (case 904). Returns 0x4008.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A3A0.
        904 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 170,
            },
            &W_1O2T[..],
        )),
        // Handler `sub_4880F0` (case 905). Returns 0x400C.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A3E0.
        905 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 171,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_487AE0` (case 906). Returns 0x4008.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_466270.
        906 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 172,
            },
            &W_1O2B[..],
        )),
        // Handler `sub_488320` (case 907). Returns 0x4008.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A790.
        907 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 173,
            },
            &W_1O2T[..],
        )),
        // Handler `sub_4882D0` (case 908). Returns 0x400C.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A670.
        908 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 174,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_487FF0` (case 909). Returns 0x400C.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A6A0.
        909 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 175,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_487AA0` (case 910). Returns 0x4008.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A410.
        910 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 176,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_488370` (case 911). Returns 0x4008.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43AEF0.
        911 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 177,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_488240` (case 912). Returns 0x4008.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_439D90.
        912 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 178,
            },
            &W_1O2T[..],
        )),
        // Handler `sub_4888B0` (case 913). Returns 0x4008.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_439C40.
        913 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 179,
            },
            &W_1O2T[..],
        )),
        // Handler `sub_488870` (case 914). Returns 0x4008.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43AF20.
        914 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 180,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_488830` (case 915). Returns 0x4008.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43AF10.
        915 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 181,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_487E70` (case 918). Returns 0x4014.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A750.
        918 => Some((
            20,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 182,
            },
            &W_1O2O3O4O5O[..],
        )),
        // Handler `sub_487A40` (case 919). Returns 0x4008.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_437D60.
        919 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 183,
            },
            &W_1O2T[..],
        )),
        // Handler `sub_4881F0` (case 920). Returns 0x4004.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43B1E0.
        920 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 184,
            },
            &W_1O[..],
        )),
        // Handler `sub_4887E0` (case 922). Returns 0x4014.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A640.
        922 => Some((
            20,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 185,
            },
            &W_1O2O3O4O5O[..],
        )),
        // Handler `sub_4885B0` (case 923). Returns 0x4004.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_438110.
        923 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 186,
            },
            &W_1O[..],
        )),
        // Handler `sub_488600` (case 924). Returns 0x4010.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A460.
        924 => Some((
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 187,
            },
            &W_1O2O3O4O[..],
        )),
        // Handler `sub_488770` (case 925). Returns 0x4008.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_438120.
        925 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 188,
            },
            &W_1O2O[..],
        )),
        // Handler `sub_4886B0` (case 926). Returns 0x400C.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A5E0.
        926 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 189,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_488650` (case 927). Returns 0x4018.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A520.
        927 => Some((
            24,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 190,
            },
            &W_1O2O3O4O5O6O[..],
        )),
        // Handler `sub_488730` (case 928). Returns 0x4004.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_43A630.
        928 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 191,
            },
            &W_1O[..],
        )),
        // Handler `sub_487400` (case 933). Returns 0x4010.
        // resolves three names and strips the extension from the 13072-prefixed
        // path before the sub_4154E0 call; no interpreter writes. Omitted
        // callees: sub_470D50(x3), lstrcpyA, sub_41A610, sub_4154E0.
        933 => Some((
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 192,
            },
            &W_1O2T3T4T[..],
        )),
        // Handler `sub_487340` (case 935). Returns 0x4010.
        // publishes 1 when sub_4157B0 finds the object and copies its three
        // name fields; the child-absent lookup fails, so the result word takes
        // zero. Omitted callees: sub_470D50(x3), sub_4157B0, lstrcpyA(x3).
        935 => Some((
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 193,
            },
            &W_1O2T3T4T[..],
        )),
        // Handler `sub_487240` (case 936). Returns 0x4004.
        // one-word archive call. Omitted callee: sub_415690.
        936 => Some((
            4,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 194,
            },
            &W_1O[..],
        )),
        // Handler `sub_488530` (case 952). Returns 0x4010.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_437D90.
        952 => Some((
            16,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 195,
            },
            &W_1O2T3T4O[..],
        )),
        // Handler `sub_488410` (case 953). Returns 0x4008.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_438C40.
        953 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 196,
            },
            &W_1O2T[..],
        )),
        // Handler `sub_487F50` (case 954). Returns 0x400C.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_438520.
        954 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 197,
            },
            &W_1O2T3T[..],
        )),
        // Handler `sub_4884A0` (case 955). Returns 0x4008.
        // console call gated on the 775-table occupancy (`slot < 8 &&
        // this[775+slot]`); the table stays unbound headless, so the else
        // path raises mask 0x1000 and the then-branch result writes (if
        // any) stay unreachable. Omitted callee: sub_439360.
        955 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 198,
            },
            &W_1O2T[..],
        )),
        // Handler `sub_47BF80` (case 960). Returns 0x4008.
        // resolves the name and forwards it with dwords 831 to the loader;
        // no interpreter writes. Omitted callees: sub_470D50, sub_4140C0.
        960 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 199,
            },
            &W_1T2O[..],
        )),
        // Handler `sub_47BFC0` (case 962). Returns 0x400C.
        // three-word call (the top word read as a 16-bit selector); no
        // interpreter writes. Omitted callee: sub_414160.
        962 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 200,
            },
            &W_1O2O3O[..],
        )),
        // Handler `sub_479030` (case 965). Returns 0x4000.
        // publishes the boolean/word state of the 480-object through sub_42AEE0;
        // child-absent default zero. Omitted callees: sub_432F10, sub_42AEE0.
        965 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 201,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Handler `sub_4790F0` (case 967). Returns 0x4000.
        // publishes the boolean/word state of the 480-object through sub_42AF70;
        // child-absent default zero. Omitted callees: sub_432F10, sub_42AF70.
        967 => Some((
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 202,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Case 360 (`sub_48A7F0`): pops the X/Y pair and repositions the
        // engine window through SetWindowPos; no interpreter state.
        // Returns 0xC008 (pop 8; the window call is presentation-side).
        360 => Some((
            8,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Case 366 (`sub_48A650`): returns 0xC000 when sub_428D20 is
        // non-zero, else 0x4000. The polled engine state is zero in the
        // headless session, so the continue arm is the observable one.
        366 => Some((
            0,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Case 565 (`sub_47A990`): result-zero path per the 3268 gate.
        565 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 203,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Case 625 (`sub_485220`): result-zero path per the renderer gate.
        625 => Some((
            12,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 204,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Case 711 (`sub_47A3D0`): result-zero path per the channel gate.
        711 => Some((
            8,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 1,
                table_index: 205,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Case 713 (`sub_4789B0`): compares the polled boolean against
        // the 480-object state and mutates only object-side records; the
        // 0x400C return is the observable boundary. Interpreter state is
        // untouched.
        713 => Some((
            12,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        // Case 825 (`sub_487220`): returns 0xA000 while the 831-object
        // reports progress, else 0x4000; the object is unbound headless,
        // so the continue arm is the observable one.
        825 => Some((
            0,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        )),
        _ => None,
    }
}
