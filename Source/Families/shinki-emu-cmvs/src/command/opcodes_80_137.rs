use super::*;

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // Handler `sub_48A3A0` (case 183): publishes the slot-status
        // word `sub_48C230(selector + 9)` for selectors 0-5 into sysreg0;
        // above five raises 0x200. The headless table reads canonical
        // zero. Returns 0x4004.
        183 => (
            4,
            CmvsPs2aCommandEffectKind::ReadSlotStatusWord {
                result_field_offset: 81220,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),

        // Handler `sub_480660` applies the second stack word to the
        // stack-top channel when the channel table entry is live;
        // otherwise it only raises the error flag at byte offset 10532.
        // Returns 0x4008.
        80 => (
            8,
            CmvsPs2aCommandEffectKind::InitializeTextureChildSurface {
                texture_table_base_offset: 1924,
                max_parent_slot: 0xff,
                max_child_id: 0x3ff,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x10,
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
        // Handler `sub_484A20` (case 88) -> `sub_456DC0`: consumes a target
        // Handler `sub_484890` (case 86): reads seven dwords — object
        // index, camera handle, three floats (z, y, x) and two scalar
        // tail words — projects the point through the camera object
        // (`sub_456C40`, kind 3) and stores the truncated coordinates
        // into dwords 20306..20308. The camera object graph is not
        // recovered, so the identity branch passes the coordinates
        // through unchanged. Returns 0x401C (28 bytes popped).
        86 => (
            28,
            CmvsPs2aCommandEffectKind::ProjectPointToScreenWords { result_base: 81224 },
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
        // Handler `sub_484A20` (case 88) -> `sub_456DC0`: consumes a target
        // selector and three values. Returns 0x4010.
        88 => (
            16,
            CmvsPs2aCommandEffectKind::ConfigureScreenOffset {
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
        // Handler `sub_4849D0` (case 89) -> `sub_456D80`: consumes a target
        // selector and two values. Returns 0x400C.
        89 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureScreenScale {
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
        // Handler `sub_484A70` (case 90) -> `sub_456DF0`: consumes a target
        // selector and three values. Returns 0x4010.
        90 => (
            16,
            CmvsPs2aCommandEffectKind::ConfigureScreenRgb {
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
        // Handler `sub_484820` (case 91) -> `sub_456C00`: consumes a target
        // selector and one value. Returns 0x4008.
        91 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureScreenRotation {
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
        // Handler `sub_484760` (case 92) -> `sub_456690`: consumes a target
        // selector and one value; stores the screen object's field 11.
        // Returns 0x4008.
        92 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureScreenField {
                error_mask: 0x0001_0000,
                field_offset: 11,
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
        // Handler `sub_484980` (case 93) -> `sub_456D60`: consumes a target
        // selector and two values. Returns 0x400C.
        93 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureScreenScalePair {
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
        // Handler `sub_484940` (case 94) -> `sub_457CA0`: consumes a target
        // selector and one boolean. Returns 0x4008.
        94 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureScreenFlag {
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
        // Handler `sub_4847F0` (case 95) -> `sub_457C90`: consumes a target
        // selector and one value; stores the screen object's field 0.
        // Returns 0x4008.
        95 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureScreenField {
                error_mask: 0x0001_0000,
                field_offset: 0,
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
        // Handler `sub_484E80` (case 96) -> `sub_456DC0`: consumes three
        // values. Returns 0x400C.
        96 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureScreenOffsetDirect {
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
        // Handler `sub_484E40` (case 97) -> `sub_456D80`: consumes two
        // values. Returns 0x4008.
        97 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureScreenScaleDirect {
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
        // Handler `sub_484ED0` (case 98) -> `sub_456DF0` without a target
        // selector: consumes three values. Returns 0x400C.
        98 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureScreenRgbDirect {
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
        // Handler `sub_484AC0` (case 99) -> `sub_456C00` without a target
        // selector: consumes one value. Returns 0x4004.
        99 => (
            4,
            CmvsPs2aCommandEffectKind::ConfigureScreenRotationDirect {
                error_mask: 0x0001_0000,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_484730` (case 100) -> `sub_456690` without a target
        // selector: consumes one value; stores field 11. Returns 0x4004.
        100 => (
            4,
            CmvsPs2aCommandEffectKind::ConfigureScreenFieldDirect {
                error_mask: 0x0001_0000,
                field_offset: 11,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_484E00` (case 101) -> `sub_456D60` without a target
        // selector: consumes two values. Returns 0x4008.
        101 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureScreenScalePairDirect {
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
        // Handler `sub_484860` (case 102) -> `sub_429B10`: consumes a target
        // selector and one value; stores the screen object's field 38.
        // Returns 0x4008.
        102 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureScreenField {
                error_mask: 0x0001_0000,
                field_offset: 38,
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
        // Handler `sub_4847A0` (case 103) -> `sub_456BE0`: consumes a target
        // selector and two values. Returns 0x400C.
        103 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureScreenPair {
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
        // Handler `sub_484D00` (case 104) -> `sub_457B90`: consumes one
        // boolean. Returns 0x4004.
        104 => (
            4,
            CmvsPs2aCommandEffectKind::CommitScreenParams {
                error_mask: 0x0001_0000,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_484B30` (case 108) -> `sub_4576F0`: stores the
        // screen transition's mode/rates/flag into the render component;
        // no interpreter state changes. Returns 0x4014.
        108 => (
            20,
            CmvsPs2aCommandEffectKind::StoreTransitionParams,
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
        // Handler `sub_484D40` (case 105): consumes one selector and stores
        // the commit result at byte offset 81220. Returns 0x4004.
        105 => (
            4,
            CmvsPs2aCommandEffectKind::WaitScreenCommit {
                error_mask: 0x0001_0000,
                result_field_offset: 81220,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_484CD0` (case 106): no stack operands; stores the
        // pending flag at byte offset 81220. Returns 0x4000.
        106 => (
            0,
            CmvsPs2aCommandEffectKind::QueryScreenPending {
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485F20` resolves the top stack word and passes it to
        // `sub_4781B0`, which loads the named script as the root frame-0
        // script, resets the stack and frame table and installs the entry
        // PC. The loader consumes the name operand itself — it pops the
        // operand off the stack cursor before pushing the return record —
        // and its dispatcher return code 0 adds no further pop.
        128 => (
            0,
            CmvsPs2aCommandEffectKind::ReloadRootScript,
            &TAGGED_STRING_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485ED0` passes the stack-top frame slot and the
        // resolved stack/8 script name to `sub_478080`, which loads the
        // named script into frame slot+1, pushes the frame counter, the
        // advanced PC and the old frame index, and transfers dispatch. The
        // loader itself pops both operands; the dispatcher result 0 adds no
        // further pop.
        129 => (
            8,
            CmvsPs2aCommandEffectKind::CallScript,
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
        // Handler `sub_478CF0` stores `stack_top != 0` through
        // `sub_41A110` at process-global backend-mode address 0x4F057C and
        // returns 0x4004.
        135 => (
            4,
            CmvsPs2aCommandEffectKind::StoreProcessGlobalBoolean {
                address: 0x004f_057c,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47CBE0` bounds the top stack word to 63 and writes a
        // 28-byte record at byte offset 13272 + 28*index: zero, the current
        // frame index, the four remaining stack words and a host clock word
        // from `sub_41A000`. Out-of-range indices skip the write. Returns
        // 0x4014.
        136 => (
            20,
            CmvsPs2aCommandEffectKind::StoreInterpreterTimestampedRecord {
                table_base_offset: 13276,
                record_stride_bytes: 28,
                max_index: 63,
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
        // Handler `sub_47CCB0` stores `stack_top != 0` into the
        // interpreter dword 3317 (byte offset 0x33D4) and returns 0x4004.
        137 => (
            4,
            CmvsPs2aCommandEffectKind::StoreInterpreterWord {
                field_offset: 0x33D4,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4831E0` is the case-112 twin (the same container
        // and child validation, a different engine-side effect id).
        // Returns 0x400c.
        113 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureTextureChildAudio {
                texture_table_base_offset: 1924,
                max_parent_slot: 255,
                max_child_id: 1023,
                container_error_flag_field_offset: 10532,
                container_error_flag_mask: 0x10,
                child_error_flag_field_offset: 10532,
                child_error_flag_mask: 0x20,
            },
            &THREE_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handlers `sub_483360`/`sub_4833E0`/`sub_483460` (cases 114-116)
        // are the case-112 twins: the same container (mask 0x10) and child
        // bound (mask 0x20) validation with different engine-side effect
        // ids. Returns 0x400c.
        114..=116 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureTextureChildAudio {
                texture_table_base_offset: 1924,
                max_parent_slot: 255,
                max_child_id: 1023,
                container_error_flag_field_offset: 10532,
                container_error_flag_mask: 0x10,
                child_error_flag_field_offset: 10532,
                child_error_flag_mask: 0x20,
            },
            &THREE_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_483260` joins the case-72 child audio rebind
        // family: container liveness (mask 0x10) and child bound (mask
        // 0x20) at byte offset 10532. Returns 0x400c.
        112 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureTextureChildAudio {
                texture_table_base_offset: 1924,
                max_parent_slot: 255,
                max_child_id: 1023,
                container_error_flag_field_offset: 10532,
                container_error_flag_mask: 0x10,
                child_error_flag_field_offset: 10532,
                child_error_flag_mask: 0x20,
            },
            &THREE_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    })
}
