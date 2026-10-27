use super::*;

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // Case 282 (`sub_489080`): stores the value word into the
        // 750-table playback record (`sub_466270`); slots at or above
        // twelve raise 0x1000. Returns 0x4008.
        282 => (
            8,
            CmvsPs2aCommandEffectKind::StorePlaybackRecordWord,
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

        // Handler `sub_47CB00` masks the stack-top label with 0x3F, advances
        // the program counter past the two-byte opcode, pushes the frame-
        // counter field, the advanced PC and the current frame index as the
        // nested-call return record, clears the frame-counter field and
        // transfers dispatch to the record's registered frame and PC words
        // (byte offsets 13276/13280 + 28*label). The dispatcher result is 0:
        // the handler pops the label itself and the interpreter loop must
        // not advance the PC again. A record without a registered PC blocks
        // because the original would execute at an unregistered address.
        138 => (
            4,
            CmvsPs2aCommandEffectKind::ResumeInterpreterCoroutineRecord {
                record_table_base_offset: 13276,
                record_stride_bytes: 28,
                max_index_mask: 63,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47CC80` writes -1 into the first word of the
        // 28-byte record selected by the stack-top index (64 records at
        // byte offset 13280); indices above 63 are ignored. Returns 0x4004.
        139 => (
            4,
            CmvsPs2aCommandEffectKind::MarkInterpreterRecordUnused {
                record_table_base_offset: 13280,
                record_count: 64,
                record_stride_bytes: 28,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47C2D0` resolves the top stack word, builds the
        // prefix_10764 + relative icon filename in a local buffer. 496C80
        // calls LoadImageA(IMAGE_ICON, LR_LOADFROMFILE), then WM_SETICON
        // with ICON_SMALL only when loading succeeds. Missing icons do not
        // fault dispatch.
        // Returns 0x4004.
        143 => (
            4,
            CmvsPs2aCommandEffectKind::RequestWindowIcon {
                prefix_field_offset: 10764,
            },
            &TAGGED_STRING_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_489160` (case 258): requires an occupied playback
        // record at the stack-top index (0..=11) and forwards the tagged
        // name and the opaque word into it; an unoccupied or out-of-range
        // index only ORs 0x1000 into the pinned error-flag word.
        // Returns 0x400c.
        258 => (
            12,
            CmvsPs2aCommandEffectKind::ForwardEffectPlaybackSetup {
                max_index: 11,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0000_1000,
            },
            &[
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4878C0` (case 259): the case-258 shape with three
        // more opaque words (pop 20); an unoccupied or out-of-range index
        // only ORs the playback error mask. Returns 0x4014.
        259 => (
            20,
            CmvsPs2aCommandEffectKind::ConfigureEffectPlaybackSlots {
                max_index: 11,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0000_1000,
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_488A50` (case 261): the case-259 shape with four
        // words (pop 16); the occupied playback record consumes the three
        // opaque operands, an unoccupied or out-of-range index only ORs the
        // playback error mask. Returns 0x4010.
        261 => (
            16,
            CmvsPs2aCommandEffectKind::ConfigureEffectPlaybackSlots {
                max_index: 11,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0000_1000,
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 16,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_488A00` (case 263): the case-258 shape against a
        // different engine entry point (`sub_4649F0`); the occupancy check,
        // error mask and pop width match. Returns 0x400c.
        263 => (
            12,
            CmvsPs2aCommandEffectKind::ForwardEffectPlaybackSetup {
                max_index: 11,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0000_1000,
            },
            &[
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4874C0` (case 271): forwards one opaque word to the
        // occupied playback record at the stack-top index (`sub_464A20`);
        // an unoccupied or out-of-range index only raises the playback
        // error mask. Returns 0x4008.
        271 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureEffectPlaybackSlots {
                max_index: 11,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0000_1000,
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
        // Handler `sub_488BD0` (case 270): the case-271 shape against a
        // different engine entry point (`sub_43AF10`); occupancy check,
        // error mask and pop width match. Returns 0x4008.
        270 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureEffectPlaybackSlots {
                max_index: 11,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0000_1000,
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
        // Handler `sub_488AA0` (case 269): the case-258 shape against
        // engine entry `sub_464C80`; occupancy check, error mask and pop
        // width match. Returns 0x400c.
        269 => (
            12,
            CmvsPs2aCommandEffectKind::ForwardEffectPlaybackSetup {
                max_index: 11,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0000_1000,
            },
            &[
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_488CB0` (case 272): the case-271 shape against
        // engine entry `sub_466220`; occupancy check, error mask and pop
        // width match. Returns 0x4008.
        272 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureEffectPlaybackSlots {
                max_index: 11,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0000_1000,
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
        // Handler `sub_487880` (case 260): pokes the occupied playback
        // record at the stack-top index with no operand words
        // (`sub_464C00`); an unoccupied or out-of-range index only raises
        // the playback error mask. Returns 0x4004.
        260 => (
            4,
            CmvsPs2aCommandEffectKind::ConfigureEffectPlaybackSlots {
                max_index: 11,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0000_1000,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47D2B0` (case 243): the string-copy counter. The
        // stack carries the destination tag, the source tag, one opaque
        // scratch word and a byte bound; the DBCS walk stores the display
        // unit count into the pinned result word at 81220. Returns 0x4010.
        243 => (
            16,
            CmvsPs2aCommandEffectKind::CopyTaggedStringLengthToResult {
                result_field_offset: 81220,
            },
            &[
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
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4889B0` (case 262): an occupied playback record at
        // the stack-top index consumes the resolved tagged name
        // (`sub_4649B0`); an unoccupied or out-of-range index only raises
        // the playback error mask. Returns 0x4008.
        262 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureEffectPlaybackSlots {
                max_index: 11,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0000_1000,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_488C10` (case 268): the case-258 shape with two
        // opaque operand words into engine entry `sub_464A60`; the
        // occupancy check, error mask and pop width match. Returns 0x400c.
        268 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureEffectPlaybackSlots {
                max_index: 11,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0000_1000,
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_488C60` (case 267): an occupied playback record at
        // the stack-top index consumes the resolved tagged name with a
        // constant zero second argument (`sub_465C10`); an unoccupied or
        // out-of-range index only raises the playback error mask.
        // Returns 0x4008.
        267 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureEffectPlaybackSlots {
                max_index: 11,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0000_1000,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_487500` (case 264): an occupied playback record at
        // the stack-top index consumes the second word's boolean
        // (`sub_464BF0`); the same occupancy check and error mask apply.
        // Returns 0x4008.
        264 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureEffectPlaybackSlots {
                max_index: 11,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0000_1000,
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
        // `sub_47EA90` (case 320) rebuilds the channel and text owner in
        // current-stage coordinates. Channel/index bounds raise separate
        // masks; the remaining words supply position and size. Returns 0x4018.
        320 => (
            24,
            CmvsPs2aCommandEffectKind::CreateStageEffectChannel {
                max_channel: 7,
                max_index: 11,
                channel_error_mask: 0x0001_0000,
                index_error_mask: 0x0000_1000,
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_487550` (case 256): validates the stack-top
        // playback index (0..=11) and creates that playback record from the
        // resolved tagged name and one opaque word (the per-index tables at
        // `+0xBB8`/`+0xBE8`). The headless record keeps only the occupancy.
        // Returns 0x400c.
        256 => (
            12,
            CmvsPs2aCommandEffectKind::CreateEffectPlayback {
                max_index: 11,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x0001_0000,
            },
            &[
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_488CF0` (case 257): validates the stack-top
        // playback index (0..=11) and destroys the playback record at that
        // index (freeing the native record and clearing the table entry).
        // The headless playback record carries no native resources, so only
        // the table entry is removed. Returns 0x4004.
        257 => (
            4,
            CmvsPs2aCommandEffectKind::DestroyEffectPlayback { max_index: 11 },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Inline dispatcher case 144 natively stores one at byte offset
        // 10560 (`mov [esi+2940h], 1`).  The port instead seeds the
        // story-stream work gate at byte offset 2940 that intproc
        // (pc 370590) polls before each batch — runtime-proven
        // 2026-10-21: the 10560-only store left the prefix stalled.
        // Returns 0x4000.
        144 => (
            0,
            CmvsPs2aCommandEffectKind::StoreInterpreterConstant {
                field_offset: 2940,
                value: 1,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486F40` reads start/count/enabled at stack top/8/12,
        // updates each bit through `sub_48ABC0`, and returns 0x400c.
        145 => (
            12,
            CmvsPs2aCommandEffectKind::MutateProcessFlagRange,
            &FLAG_RANGE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_487000` reads start/count/value at stack top/8/12,
        // writes each index through `sub_48AC50`, and returns 0x400c.
        146 => (
            12,
            CmvsPs2aCommandEffectKind::StoreProcessIndexedRange,
            &INDEXED_RANGE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486F80` reads start/count/f32 bits at stack top/8/12,
        // writes each index through `sub_48AC30`, and returns 0x400c.
        147 => (
            12,
            CmvsPs2aCommandEffectKind::StoreProcessFloatRange,
            &INDEXED_RANGE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4871B0` resolves the top/12 string, copies it to each
        // stack top/8 range slot through `sub_48AC90`, and returns 0x400c.
        148 => (
            12,
            CmvsPs2aCommandEffectKind::StoreProcessStringRange,
            &STRING_RANGE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4802A0` resolves the top stack word, appends it to
        // the recovered case-152 string list through `sub_491470`, and
        // stores the new length at byte offset 81220. Returns 0x4004.
        // Handlers `sub_47CF60` (305), `sub_47CF20` (306),
        // `sub_47CEE0` (307) and `sub_47CE80` (308): each resolves the
        // deeper stack word through `sub_470D50`, walks the this+0xC18
        // list with the stack-top index, and lstrcpyA's the node's string
        // at the handler-specific offset (+0x24/+0x64/+0x04/+0x44), or the
        // empty fallback constant when the list runs short. 0x4008 each.
        305 => (
            8,
            CmvsPs2aCommandEffectKind::CopyPrivateListStringToSlot {
                list_id: 305,
                node_string_offset: 0x24,
            },
            &LIST_INDEX_AND_SLOT_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        306 => (
            8,
            CmvsPs2aCommandEffectKind::CopyPrivateListStringToSlot {
                list_id: 306,
                node_string_offset: 0x64,
            },
            &LIST_INDEX_AND_SLOT_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        307 => (
            8,
            CmvsPs2aCommandEffectKind::CopyPrivateListStringToSlot {
                list_id: 307,
                node_string_offset: 0x04,
            },
            &LIST_INDEX_AND_SLOT_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        308 => (
            8,
            CmvsPs2aCommandEffectKind::CopyPrivateListStringToSlot {
                list_id: 308,
                node_string_offset: 0x44,
            },
            &LIST_INDEX_AND_SLOT_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47CEC0` (case 302): stores the this+0xC18 manager's
        // count field into the pinned result word through `sub_432F10`.
        // Returns 0x4000.
        302 => (
            0,
            CmvsPs2aCommandEffectKind::StoreListCountToResult { list_id: 302 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47CE50` (case 309): walks the this+0xC18 list
        // counting nodes below the stack-top index and stores the walked
        // length (-1 for an empty list) into the pinned result word.
        // Returns 0x4008.
        309 => (
            8,
            CmvsPs2aCommandEffectKind::StoreListWalkLengthToResult { list_id: 309 },
            &LIST_INDEX_AND_SLOT_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        152 => (
            4,
            CmvsPs2aCommandEffectKind::AppendPrivateStringToList {
                list_id: 152,
                length_field_offset: 81220,
            },
            &TAGGED_STRING_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4802E0` passes the top word as a cursor index to the
        // cursor manager referenced by interpreter byte offset 32; the
        // manager skips work when its current-index field (byte offset 4 of
        // the manager object) already matches. Returns 0x4004.
        153 => (
            4,
            CmvsPs2aCommandEffectKind::SelectSystemCursor {
                manager_field_offset: 32,
                current_index_field_offset: 4,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Case 156: the recovered dispatcher body is empty (`LABEL_119` sets
        // the result to 0x4008 and returns); two stack words are popped and
        // never read.
        156 => (
            8,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        160 => (
            12,
            CmvsPs2aCommandEffectKind::PlayAudio,
            &AUDIO_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_489630` measures one tagged message string through
        // the window engine's text layout with a selector (0..=3, raising
        // 0x400 at byte offset 10532), a scale and a bold flag. The
        // headless page renderer lays the text out itself, so the layout
        // reports success and leaves the measured extent at zero.
        // Returns 0x4010.
        166 => (
            16,
            CmvsPs2aCommandEffectKind::LayoutMessageText {
                max_selector: 3,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x400,
                result_field_offset: 81220,
                extent_field_offset: 81236,
            },
            &MESSAGE_LAYOUT_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        //4898C0 disposes voice selector+5;4899D0 queries actual playback ms.
        168 | 170 => (
            4,
            CmvsPs2aCommandEffectKind::NativeAudio { command_id },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        //48A0B0 starts fade-out and clears only the descriptor source name.
        182 => (
            8,
            CmvsPs2aCommandEffectKind::NativeAudio { command_id },
            &TWO_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        //489F80 consumes slot/name/volume/fade-in-ms/loop/start;0x4018.
        186 => (
            24,
            CmvsPs2aCommandEffectKind::NativeAudio { command_id },
            &[
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // `sub_486E90` -> `sub_418D70`: native type-before-width formatting
        // of (destination, format, word), not CRT printf.
        // formatted bytes land NUL-terminated in the destination storage.
        // Returns 0x400C (pop 12).
        252 => (
            12,
            CmvsPs2aCommandEffectKind::FormatStringIntoSlot,
            &[
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler (case 297) -> `sub_427E80`: writes the system state to
        // save/system.dat. No stack words; returns 0x4000.
        297 => (
            0,
            CmvsPs2aCommandEffectKind::SaveSystemState,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Stop both alternating audio channels; no stack words.
        161 => (
            0,
            CmvsPs2aCommandEffectKind::StopAudio,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Fade out the active audio channel using the consumed duration.
        162 => (
            4,
            CmvsPs2aCommandEffectKind::FadeOutAudio,
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        164 => (
            16,
            CmvsPs2aCommandEffectKind::PlayPairedAudio,
            &PAIRED_AUDIO_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_48A110` registers one resource channel slot record
        // (slot/name/volume/loop/start) in the fixed table at byte
        // offset 1608 and returns 0x4014. Slots above 5 only raise the
        // error flag at byte offset 10532.
        176 => (
            20,
            CmvsPs2aCommandEffectKind::RegisterResourceChannelSlot {
                max_slot: 5,
                table_base_offset: 1608,
                record_stride_bytes: 52,
                gate_field_offset: 1556,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x200,
            },
            &[
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
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        //48A230 starts a retained decoder;48A2B0 joins/disposes it.
        177 => (
            8,
            CmvsPs2aCommandEffectKind::NativeAudio { command_id },
            &TWO_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        179 => (
            4,
            CmvsPs2aCommandEffectKind::NativeAudio { command_id },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_480280` reads the input-manager object's persistent
        // pointer fields 180/181 through `sub_45CF40` and writes them into
        // system registers 1 and 2. Returns 0x4000.
        200 => (
            0,
            CmvsPs2aCommandEffectKind::QueryPointerPosition {
                x_field_offset: 81224,
                y_field_offset: 81228,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_480060` reads the persistent pointer and tests it
        // against the stack-provided rectangle (x, y, width, height) with
        // inclusive bounds, storing the boolean into the version-pinned
        // result field. Returns 0x4010.
        202 => (
            16,
            CmvsPs2aCommandEffectKind::HitTestPointerRect {
                result_field_offset: 81220,
            },
            &FOUR_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // `sub_4800F0`: 1=CreateRectRgnIndirect, 2=CreateEllipticRgnIndirect,
        // 3=CreateRoundRectRgn(x1,y1,x2,y2,width,height), 4=CreatePolygonRgn
        // with WINDING fill. For type 4, x1 indexes the process word array
        // and y1 counts interleaved XY pairs (native local capacity 256).
        // PtInRegion publishes 0/1 at 81220; unknown types and failed GDI
        // creation publish zero. Returns 0x401C, popping all seven words.
        203 => (
            28,
            CmvsPs2aCommandEffectKind::HitTestPointerRegion {
                result_field_offset: 81220,
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_484F60` stores `rand() % stack_top` into the
        // interpreter word at byte offset 81220 and returns 0x4004.
        // Handler `sub_4891B0` (case 196): reads the string at offset 4,
        // concatenates it onto the base path and opens a movie stream through
        // `sub_412800`; the open result lands in the interpreter word at byte
        // offset 81220. Returns 0x4004.
        196 => (
            4,
            CmvsPs2aCommandEffectKind::OpenMovieStream {
                result_field_offset: 81220,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_489360` (case 197): releases the movie object held at
        // dword index 795 and returns 0x4000, so it consumes nothing.
        197 => (
            0,
            CmvsPs2aCommandEffectKind::ReleaseMovieObject,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_489330` (case 198): publishes the movie object's state
        // into the interpreter word at byte offset 81220, or -1 when the
        // object is gone. Returns 0x4000.
        198 => (
            0,
            CmvsPs2aCommandEffectKind::QueryMovieState {
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        212 => (
            4,
            CmvsPs2aCommandEffectKind::StoreRandomModulo {
                target_field_offset: 81220,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47D220` stores `lstrlenA(resolved_top)` into the
        // interpreter word at byte offset 81220 and returns 0x4004.
        248 => (
            4,
            CmvsPs2aCommandEffectKind::StoreStringLength {
                target_field_offset: 81220,
            },
            &TAGGED_STRING_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_488FC0` (case 276) -> `sub_464AA0`: consumes a playback
        // index below 12 and a value, storing the record's dword 41. Faults
        // raise 0x1000. Returns 0x4008.
        276 => (
            8,
            CmvsPs2aCommandEffectKind::SetEffectPlaybackField {
                error_mask: 0x0000_1000,
                field_offset: 41,
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
        // Handler `sub_488F80` (case 277) -> `sub_464A90`: as case 276 but the
        // record's dword 44. Returns 0x4008.
        277 => (
            8,
            CmvsPs2aCommandEffectKind::SetEffectPlaybackField {
                error_mask: 0x0000_1000,
                field_offset: 44,
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
        // Handler `sub_488F40` (case 278) -> `sub_464A80`: as case 276 but the
        // record's dword 45. Returns 0x4008.
        278 => (
            8,
            CmvsPs2aCommandEffectKind::SetEffectPlaybackField {
                error_mask: 0x0000_1000,
                field_offset: 45,
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
        // `488EF0` validates the playback owner, then `429850` writes the
        // renderer-global DWORD43, not a field of that playback record.
        // Returns 0x4008.
        279 => (
            8,
            CmvsPs2aCommandEffectKind::SetNativeViewSuppression {
                error_mask: 0x0000_1000,
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
        // Handler `sub_488E70` (case 286) -> `sub_462C10`: consumes a playback
        // index below 12, two script strings and a value; stores them on the
        // record's text surface. Faults raise 0x1000. Returns 0x4010.
        286 => (
            16,
            CmvsPs2aCommandEffectKind::SetEffectTextSurface {
                error_mask: 0x0000_1000,
            },
            &[
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_48A570` publishes the render object's inner
        // display-mode fields 6 and 7 into system registers 1 and 2. The
        // renderer object model is not recovered, so the effect blocks.
        // Returns 0x4000.
        294 => (
            0,
            CmvsPs2aCommandEffectKind::QueryRendererDisplayMode {
                renderer_field_offset: 1920,
                result_field_offset: 81224,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Inline dispatcher case 296 calls `sub_427B30`, which opens
        // `system.dat` under the case-22 path buffer, verifies, decrypts
        // and decompresses it, and restores the recovered system tables.
        // The result depends entirely on host storage, so the nucleus turns
        // it into a storage request and waits for the host. Returns 0x4000.
        296 => (
            0,
            CmvsPs2aCommandEffectKind::RequestSystemStateLoad {
                path_buffer_offset: 5404,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // 47CE30/467E70 counts actual native message-history nodes.
        304 => (
            0,
            CmvsPs2aCommandEffectKind::StoreListCountToResult { list_id: 304 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47F8D0` clears the resource slot named by the
        // stack-top index (0..=7) and returns 0x4004; indices at or above
        // 8 raise the 0x10000 error mask at byte offset 10532.
        321 => (
            4,
            CmvsPs2aCommandEffectKind::ClearResourceSlot {
                max_slot: 7,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x10000,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FA90` forwards one opaque word to an occupied
        // effect channel (`sub_467A50`), the same observable shape as
        // case 322. Returns 0x4008.
        236 => (
            8,
            CmvsPs2aCommandEffectKind::ForwardEffectChannelWord {
                effect_table_dword_index: 742,
                max_channel: 7,
                error_mask: 0x0001_0000,
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
        // Handler `sub_47FAE0` forwards one opaque word to an occupied
        // effect channel (`sub_467A00`), the case-322 shape. Returns
        // 0x4008.
        341 => (
            8,
            CmvsPs2aCommandEffectKind::ForwardEffectChannelWord {
                effect_table_dword_index: 742,
                max_channel: 7,
                error_mask: 0x0001_0000,
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
        // Handler `sub_47F9F0` is the boolean variant of case 322: the
        // forwarded word is `stack[-8] != 0` (`sub_467000`). The same
        // channel liveness and bounds apply. Returns 0x4008.
        332 => (
            8,
            CmvsPs2aCommandEffectKind::ForwardEffectChannelWord {
                effect_table_dword_index: 742,
                max_channel: 7,
                error_mask: 0x0001_0000,
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
        // Handler `sub_47F830` consumes a channel and one opaque word,
        // forwarding the word to the effect engine only while the channel
        // slot is occupied; otherwise it raises 0x10000. Returns 0x4008.
        322 => (
            8,
            CmvsPs2aCommandEffectKind::ForwardEffectChannelWord {
                effect_table_dword_index: 742,
                max_channel: 7,
                error_mask: 0x0001_0000,
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
        // `sub_47E910` / `sub_466DD0`: channel, read-flag id, text.
        // Pending glyphs return 0xA000 without popping; completion is 0xC00C.
        338 => (
            12,
            CmvsPs2aCommandEffectKind::DisplayChannelText,
            &[
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // 47CFA0 owns four resolved strings and six words in the history list.
        310 => (
            40,
            CmvsPs2aCommandEffectKind::AppendNativeMessageHistory,
            &TEN_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // `47F6E0`: channel, read flag, voice id. Pending 0xA000 keeps all
        // operands; completion 0x400C pops twelve bytes and continues.
        339 => (
            12,
            CmvsPs2aCommandEffectKind::WaitChannelAdvance,
            &THREE_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_489D20` (case 181): a six-iteration engine loop
        // that re-arms each message-engine channel's display record from a
        // static empty template (`sub_48CAB0` re-binds channel 9+i, then
        // `lstrcpy` copies the 0x4C863C template into the 52-byte record).
        // No stack operands and no interpreter-visible words: the headless
        // graph keeps the channels unbound, so this reduces to the no-op
        // contract. Returns 0x4000.
        181 => (
            0,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47CBB0` (case 140): stores `stack_top != 0` into
        // the scheduler rebuild gate at this+0x29DC (byte offset 10716) —
        // the word `rebuild_frame_entry_queue` tests to skip refilling the
        // entry queue. Returns 0x4004.
        140 => (
            4,
            CmvsPs2aCommandEffectKind::StoreInterpreterWord {
                field_offset: 10716,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // 467E20 frees history nodes, retaining the configured capacity.
        303 => (
            0,
            CmvsPs2aCommandEffectKind::ClearNativeMessageHistory,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    })
}
