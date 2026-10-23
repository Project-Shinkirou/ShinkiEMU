use super::*;

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // Handler `sub_47F0D0` matches case 322's shape: a channel and one
        // opaque word forwarded to the occupied channel's effect engine.
        // Returns 0x4008.
        323 => (
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
        // Handler `sub_47F110` consumes a channel and two opaque words,
        // forwarding both to the occupied channel's effect engine; the
        // usual occupancy fault otherwise. Returns 0x400c.
        // Handler `sub_47F080` forwards two opaque words to an occupied
        // effect channel (`sub_466FB0`), the case-324 shape. Returns
        // 0x400c.
        335 => (
            12,
            CmvsPs2aCommandEffectKind::ForwardEffectChannelPair {
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        324 => (
            12,
            CmvsPs2aCommandEffectKind::ForwardEffectChannelPair {
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47F030` consumes a channel and four opaque words
        // forwarded to the occupied channel's effect engine. Returns 0x4014.
        325 => (
            20,
            CmvsPs2aCommandEffectKind::ForwardEffectChannelQuad {
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
        // Handler `sub_47EF70` matches case 324's shape: a channel and two
        // opaque words forwarded to the occupied channel's effect engine.
        // Returns 0x400c.
        326 => (
            12,
            CmvsPs2aCommandEffectKind::ForwardEffectChannelPair {
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FBA0` matches case 322's shape: a channel and one
        // opaque word forwarded to the occupied channel's effect engine.
        // Returns 0x4008.
        327 => (
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
        // Handler `sub_47F690` matches case 324's shape: a channel and two
        // opaque words forwarded to the occupied channel. Returns 0x400c.
        328 => (
            12,
            CmvsPs2aCommandEffectKind::ForwardEffectChannelPair {
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47EA50` matches case 322's shape: a channel and one
        // opaque word forwarded to the occupied channel. Returns 0x4008.
        329 => (
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
        // Handler `sub_47FB20` (case 330) -> `sub_467A10`: consumes a channel.
        // Returns 0x4004.
        330 => (
            4,
            CmvsPs2aCommandEffectKind::ApplyEffectChannelOperation {
                error_mask: 0x0001_0000,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47E770` (case 331): stores whether the effect
        // channel slot carries an object into the interpreter word at byte
        // offset 81220; an out-of-range channel raises 0x10000 at byte
        // offset 10532. Returns 0x4004.
        331 => (
            4,
            CmvsPs2aCommandEffectKind::QueryChannelOccupancy {
                result_field_offset: 81220,
                channel_table_dword_index: 742,
                error_mask: 0x0001_0000,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47F270` (case 333): reads one occupied effect
        // channel object's transient activity field (+8, which
        // `sub_466F50` resets when the playback flag changes) into the
        // interpreter word at byte offset 81220; an out-of-range or empty
        // channel raises 0x10000 at byte offset 10532. Returns 0x4004.
        333 => (
            4,
            CmvsPs2aCommandEffectKind::QueryEffectChannelActivity {
                activity_state_key_base: 0x7420_0000,
                max_channel: 7,
                effect_table_dword_index: 742,
                error_mask: 0x0001_0000,
                result_field_offset: 81220,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47EFC0` (case 334): the stack top selects one of the
        // eight effect channels and the preceding word is the playback
        // activity flag; a changed flag is stored per channel and the
        // transient flag clears. Out-of-range selectors raise 0x10000 at
        // byte offset 10532. Returns 0x4008.
        334 => (
            8,
            CmvsPs2aCommandEffectKind::SetEffectChannelPlaybackFlag {
                effect_table_dword_index: 742,
                error_mask: 0x0001_0000,
                activity_state_key_base: 0x7420_0000,
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
        // Handler `sub_47FA40` (case 336) -> `sub_4678C0`: consumes a channel,
        // a mode word and two coordinates, setting the channel origin. An
        // out-of-range or empty channel raises 0x10000. Returns 0x4010.
        336 => (
            16,
            CmvsPs2aCommandEffectKind::SetEffectChannelOrigin {
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
        // Handler `sub_47F9A0` (case 337) -> `sub_4679B0`: consumes a channel,
        // a `__int16` playback mode and two `__int16` coordinates. Returns
        // 0x4010.
        337 => (
            16,
            CmvsPs2aCommandEffectKind::SetEffectChannelPlaybackMode {
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
        // Handler `sub_47E9E0` (case 343): the wait twin of case 345 —
        // same channel/name operands, but the handler returns `0xA000`
        // while the channel is unsettled and `0xC00C` (stop, pop 12) once
        // the queue drains. The headless channel settles immediately; the
        // pop matches the caller's three pushed words.
        343 => (
            12,
            CmvsPs2aCommandEffectKind::PrepareEffectChannelFromName {
                error_mask: 0x0001_0000,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47F7D0` (case 344): starts a playback on an
        // occupied channel and returns `0xA000` while `sub_467510` reports
        // the channel active, `0x400C` (pop 12) once it completes. The
        // headless channel completes immediately.
        344 => (
            12,
            CmvsPs2aCommandEffectKind::StartEffectChannelPlayback {
                error_mask: 0x0001_0000,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47E890` (case 345) -> `sub_466ED0`: on an occupied
        // channel, applies a named source configuration (flag word gates the
        // apply) and stores the idle report in the pinned result word.
        // Returns 0x4010.
        345 => (
            16,
            CmvsPs2aCommandEffectKind::ConfigureEffectChannelFromName {
                result_field_offset: 81220,
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
        // Handler `sub_47F1B0` (case 346) -> `sub_467CF0`: consumes a channel
        // and two values. Returns 0x400C.
        346 => (
            12,
            CmvsPs2aCommandEffectKind::SetEffectChannelValuePair {
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47F160` consumes a channel and a boolean, forwarding
        // the boolean to the effect engine only while the channel slot is
        // occupied; an unoccupied or out-of-range channel raises 0x10000.
        // Returns 0x4008.
        347 => (
            8,
            CmvsPs2aCommandEffectKind::SetEffectChannelEnabled {
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
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47ED00` consumes eight words: channel, effect id and
        // six opaque configuration words, then rebuilds the effect playback
        // objects. Out-of-range selectors raise 0x10000 / 0x1000 at byte
        // offset 10532. Returns 0x4020.
        349 => (
            32,
            CmvsPs2aCommandEffectKind::CreateEffectChannel {
                effect_table_dword_index: 742,
                playback_table_dword_index: 750,
                max_channel: 7,
                max_effect: 11,
                channel_error_mask: 0x0001_0000,
                effect_error_mask: 0x0000_1000,
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 32,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47F200` consumes a channel and stores the effect
        // engine's current two-word position into byte offsets 81224/81228.
        // The headless engine holds no animated position, so both stay zero.
        // Returns 0x4004.
        350 => (
            4,
            CmvsPs2aCommandEffectKind::QueryEffectPosition {
                result_x_field_offset: 81224,
                result_y_field_offset: 81228,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47F2C0` consumes a channel plus an x/y/width/height
        // rectangle and stores the pointer hit-test result into byte offset
        // 81220. The headless engine holds no persistent pointer, so the
        // canonical result is zero. Returns 0x4014.
        // Case 369 (`sub_47E720`): destroys the hit element on the
        // occupied channel (`sub_446E70`); the channel gate raises
        // 0x10000. Returns 0x4004.
        369 => (
            4,
            CmvsPs2aCommandEffectKind::DestroyEffectHitElement,
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        351 => (
            20,
            CmvsPs2aCommandEffectKind::QueryEffectPointerHit {
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_48A7C0` resolves the top stack word and stores it as
        // the raw window caption through the recovered caption component;
        // no prefix field participates. Returns 0x4004.
        352 => (
            4,
            CmvsPs2aCommandEffectKind::RequestWindowCaption,
            &TAGGED_STRING_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_48A710` assembles the window caption from the prefix
        // field at byte offset 288, a charset-dependent gap and the
        // resolved top stack word, copies the suffix at byte offset
        // 12816 and sets the main window text. Returns 0x4004.
        353 => (
            4,
            CmvsPs2aCommandEffectKind::BuildPrefixedWindowCaption {
                prefix_field_offset: 288,
                buffer_offset: 12816,
            },
            &TAGGED_STRING_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_48A670` resolves the top stack word and copies the
        // text into the byte-offset-10324 buffer when it stays below 128
        // bytes; longer text only ORs 8 into the error-flag word at
        // byte offset 10532. Returns 0x4004.
        354 => (
            4,
            CmvsPs2aCommandEffectKind::StoreBoundedInterpreterString {
                buffer_offset: 10324,
                max_text_bytes: 128,
                error_flag_field_offset: 10532,
                error_flag_mask: 8,
            },
            &TAGGED_STRING_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_48A550` (case 355): polls the chain-graph object's
        // busy counter (`[[this+0x780]+0x38]` via `sub_432F80`) and stores
        // its idle bit (1 when the counter reads zero) into the interpreter
        // status word at `this+0x13D44`. The headless chain graph is
        // synchronous, so the idle bit is the constant 1. Returns 0x4000.
        355 => (
            0,
            CmvsPs2aCommandEffectKind::StoreInterpreterConstant {
                field_offset: 81220,
                value: 1,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Inline dispatcher case 363: stores the status constant 7 into the
        // interpreter result word at `this+0x13D44`. This case is the
        // wait-loop timeout exit reached by the absolute jump at the
        // recovered scenario stream's `intproc` pc 511156. Returns 0x4000.
        363 => (
            0,
            CmvsPs2aCommandEffectKind::StoreInterpreterConstant {
                field_offset: 81220,
                value: 7,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_48A4A0` (case 364): publishes the chain-graph
        // object's raw `+0x54` field (`[[this+0x780]] -> +0x54` through
        // `sub_48A4C0`/`sub_490A00`) into the interpreter result word at
        // `this+0x13D44`. The headless chain graph keeps zero-initialized
        // fields. Returns 0x4000.
        364 => (
            0,
            CmvsPs2aCommandEffectKind::StoreInterpreterConstant {
                field_offset: 81220,
                value: 0,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_48A6C0` (case 356): runs the chain reset
        // controller (`sub_428920`), posts engine message 0x7FF7 and marks
        // the rebuild-request word before suspending the frame. The
        // stack word feeds the reset comparison only. Returns 0xC004.
        356 => (
            4,
            CmvsPs2aCommandEffectKind::RebuildChainGraphSuspend {
                clear_field_a_offset: 10548,
                clear_field_b_offset: 10552,
                request_field_offset: 10708,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_48A4D0` (case 365): clamps the stack top into the
        // layer range and stores it as the chain-graph object's current
        // layer (`sub_42B670` writes `+0x54`, the field case 364
        // publishes; out-of-range values collapse to 1, which the pushed
        // range-checked call sites never exercise). Returns 0x4004.
        365 => (
            4,
            CmvsPs2aCommandEffectKind::StoreComponentWord {
                component_field_offset: 1920,
                field_offset: 84,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    })
}
