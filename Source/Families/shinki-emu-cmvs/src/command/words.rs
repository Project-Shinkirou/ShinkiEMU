use super::*;

/// One opaque stack word `offset` bytes below the command's first operand.
/// Operands are pushed in reverse order, so offset 4 is the last pushed word
/// and every following word sits one slot deeper.
const fn opaque_word(offset: u8) -> CmvsPs2aCommandStackWord {
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: offset,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    }
}

pub(super) const AUDIO_WORDS: [CmvsPs2aCommandStackWord; 3] = [
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
        kind: CmvsPs2aCommandStackWordKind::BooleanU32,
    },
];
pub(super) const PAIRED_AUDIO_WORDS: [CmvsPs2aCommandStackWord; 4] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
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
];
pub(super) const OPAQUE_WORD: [CmvsPs2aCommandStackWord; 1] = [CmvsPs2aCommandStackWord {
    offset_from_top_bytes: 4,
    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
}];
pub(super) const OPAQUE_BOOLEAN: [CmvsPs2aCommandStackWord; 1] = [CmvsPs2aCommandStackWord {
    offset_from_top_bytes: 4,
    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
}];
pub(super) const TWO_OPAQUE_WORDS: [CmvsPs2aCommandStackWord; 2] = [opaque_word(4), opaque_word(8)];

pub(super) const THREE_OPAQUE_WORDS: [CmvsPs2aCommandStackWord; 3] =
    [opaque_word(4), opaque_word(8), opaque_word(12)];

pub(super) const SEVEN_OPAQUE_WORDS: [CmvsPs2aCommandStackWord; 7] = [
    opaque_word(4),
    opaque_word(8),
    opaque_word(12),
    opaque_word(16),
    opaque_word(20),
    opaque_word(24),
    opaque_word(28),
];

pub(super) const EIGHT_OPAQUE_WORDS: [CmvsPs2aCommandStackWord; 8] = [
    opaque_word(4),
    opaque_word(8),
    opaque_word(12),
    opaque_word(16),
    opaque_word(20),
    opaque_word(24),
    opaque_word(28),
    opaque_word(32),
];

pub(super) const TEN_OPAQUE_WORDS: [CmvsPs2aCommandStackWord; 10] = [
    opaque_word(4),
    opaque_word(8),
    opaque_word(12),
    opaque_word(16),
    opaque_word(20),
    opaque_word(24),
    opaque_word(28),
    opaque_word(32),
    opaque_word(36),
    opaque_word(40),
];

pub(super) const FOUR_OPAQUE_WORDS: [CmvsPs2aCommandStackWord; 4] = [
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
pub(super) const TAGGED_STRING_WORD: [CmvsPs2aCommandStackWord; 1] = [CmvsPs2aCommandStackWord {
    offset_from_top_bytes: 4,
    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
}];
/// Case 307 (`sub_47CEE0`): the stack top carries the list index and the
/// word below it the tagged destination slot, read at offsets 4/8.
pub(super) const LIST_INDEX_AND_SLOT_WORDS: [CmvsPs2aCommandStackWord; 2] =
    [opaque_word(8), opaque_word(4)];
/// Case 166 (`sub_489630`): a selector, one tagged message string, a scale
/// and a bold flag, read at offsets 4/8/12/16.
pub(super) const MESSAGE_LAYOUT_WORDS: [CmvsPs2aCommandStackWord; 4] = [
    opaque_word(4),
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
    opaque_word(12),
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 16,
        kind: CmvsPs2aCommandStackWordKind::BooleanU32,
    },
];
pub(super) const FLAG_RANGE_WORDS: [CmvsPs2aCommandStackWord; 3] = [
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
pub(super) const INDEXED_RANGE_WORDS: [CmvsPs2aCommandStackWord; 3] = [
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
pub(super) const STRING_RANGE_WORDS: [CmvsPs2aCommandStackWord; 3] = [
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
pub(super) const RESOURCE_CHANNEL_WORDS: [CmvsPs2aCommandStackWord; 2] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
    },
];
pub(super) const SLOT_PAIR_WORDS: [CmvsPs2aCommandStackWord; 2] = [
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 4,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: 8,
        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
    },
];
