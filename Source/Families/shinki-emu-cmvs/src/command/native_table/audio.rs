use super::*;
use crate::{CmvsPs2aCommandEffectKind, CmvsPs2aCommandStackWord, CmvsPs2aCommandStackWordKind};

const fn word(offset: u8, kind: CmvsPs2aCommandStackWordKind) -> CmvsPs2aCommandStackWord {
    CmvsPs2aCommandStackWord {
        offset_from_top_bytes: offset,
        kind,
    }
}
use CmvsPs2aCommandStackWordKind::{BooleanU32, OpaqueU32, TaggedStringReference};

const VOICE_WORDS: [CmvsPs2aCommandStackWord; 5] = [
    word(4, OpaqueU32),
    word(8, TaggedStringReference),
    word(12, OpaqueU32),
    word(16, BooleanU32),
    word(20, OpaqueU32),
];
const FIXED_STREAM_WORDS: [CmvsPs2aCommandStackWord; 6] = [
    word(4, OpaqueU32),
    word(8, TaggedStringReference),
    word(12, OpaqueU32),
    word(16, OpaqueU32),
    word(20, BooleanU32),
    word(24, BooleanU32),
];
const STATIC_LOAD_WORDS: [CmvsPs2aCommandStackWord; 2] =
    [word(4, OpaqueU32), word(8, TaggedStringReference)];
const ONE_WORD: [CmvsPs2aCommandStackWord; 1] = [word(4, OpaqueU32)];
const TWO_WORDS: [CmvsPs2aCommandStackWord; 2] = [word(4, OpaqueU32), word(8, OpaqueU32)];
const STATIC_PLAY_WORDS: [CmvsPs2aCommandStackWord; 3] =
    [word(4, OpaqueU32), word(8, OpaqueU32), word(12, BooleanU32)];

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    // Native4844C0/4845D0/484610/484580/484660 use streaming bank592 slot15;
    //489E20/489F00/489E80/489F40 use the independent static bank608 slots0..23.
    let (bytes, words): (u8, &[CmvsPs2aCommandStackWord]) = match command_id {
        175 => (20, &VOICE_WORDS),        //489710→48DB70, voice slots5..8.
        576 => (24, &FIXED_STREAM_WORDS), //4844C0 returns0x4018.
        577..=579 | 585 | 587 => (4, &ONE_WORD),
        580 => (8, &TWO_WORDS),
        584 => (8, &STATIC_LOAD_WORDS),
        586 => (12, &STATIC_PLAY_WORDS),
        _ => return None,
    };
    Some((
        bytes,
        CmvsPs2aCommandEffectKind::NativeAudio { command_id },
        words,
    ))
}
