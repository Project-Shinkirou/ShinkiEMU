use super::*;

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // Handler `sub_47F5F0` (case 402) -> `sub_466D50`: shows one of 32
        // quads on the occupied channel by setting the quad record's
        // visibility flag and the element's visible flag. Returns 0x4008.
        402 => (
            8,
            CmvsPs2aCommandEffectKind::SelectEffectQuad {
                error_mask: 0x0002_0000,
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
        // Handler `sub_47F640` (case 403) -> `sub_466D90`: hides one of 32
        // quads on the occupied channel by clearing the quad record's
        // visibility flag and the element's visible flag. Returns 0x4008.
        403 => (
            8,
            CmvsPs2aCommandEffectKind::DeselectEffectQuad {
                error_mask: 0x0002_0000,
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
        // Handler `sub_47F4D0` (case 404): queries three state words from
        // the effect engine into byte offsets 81236/81240/81244. The
        // headless engine holds no animated state, so they stay zero.
        // Returns 0x4004.
        404 => (
            4,
            CmvsPs2aCommandEffectKind::QueryEffectState {
                first_field_offset: 81236,
                second_field_offset: 81240,
                third_field_offset: 81244,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47F470` (case 405): stores whether the selected quad
        // exists on the occupied channel into byte offset 81220. The
        // headless engine never registers quads, so the canonical result is
        // zero. Returns 0x4008.
        405 => (
            8,
            CmvsPs2aCommandEffectKind::QueryEffectQuadActive {
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47F330` (case 406) selects/activates one quad slot on        // an occupied effect channel with a value word; the recovered graph
        // keeps only occupancy. Returns 0x400C.
        406 => (
            12,
            CmvsPs2aCommandEffectKind::ActivateEffectQuad {
                error_mask: 0x0002_0000,
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
        // Handler `sub_47ACA0`-adjacent case 410 via `sub_45C5A0`: clears
        // four internal presentation manager counters and re-derives its
        // state; nothing is observable in the headless presentation. Takes
        // no operands and returns 0x4000.
        410 => (
            0,
            CmvsPs2aCommandEffectKind::ResetPresentationBuffers,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479C20` (case 416): copies the input manager's
        // advance-group latches (`sub_45CCB0` reads manager dwords 271/272)
        // into the interpreter words at byte offsets 81220/81236 and takes
        // no stack operands. Returns 0x4000.
        416 => (
            0,
            CmvsPs2aCommandEffectKind::ReadInputLatches {
                release_field_offset: 81220,
                held_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // sub_45CB80 consumes the advance latch (dwords 271/273).
        417 => (
            0,
            CmvsPs2aCommandEffectKind::ConsumeAdvanceLatch,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47A0B0` (case 424): writes the scene layer group at
        // base word 289 (`sub_45D290` reads words 289/290) into byte offsets
        // 81220 and 81236. Returns 0x4000.
        424 => (
            0,
            CmvsPs2aCommandEffectKind::QuerySceneLayer {
                base_word: 289,
                result_field_offset: 81220,
                aux_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4616E0` (case 425): clears scene words 289 and 291.
        // Returns 0x4000.
        425 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 289 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479D70` (case 426): writes the scene layer group at
        // base word 298 (`sub_45CDB0` reads words 298/299) into byte offsets
        // 81220 and 81236. Returns 0x4000.
        426 => (
            0,
            CmvsPs2aCommandEffectKind::QuerySceneLayer {
                base_word: 298,
                result_field_offset: 81220,
                aux_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4612B0` (case 427): clears scene words 298 and 300.
        // Returns 0x4000.
        427 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 298 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479E30` (case 428): writes the scene layer group at
        // base word 301 (`sub_45CE10` reads words 301/302). Returns 0x4000.
        428 => (
            0,
            CmvsPs2aCommandEffectKind::QuerySceneLayer {
                base_word: 301,
                result_field_offset: 81220,
                aux_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_461310` (case 429): clears scene words 301 and 303.
        // Returns 0x4000.
        429 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 301 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47A070` (case 430): writes the scene layer group at
        // base word 304 (`sub_45D270` reads words 304/305) into byte offsets
        // 81220 and 81236. Returns 0x4000.
        430 => (
            0,
            CmvsPs2aCommandEffectKind::QuerySceneLayer {
                base_word: 304,
                result_field_offset: 81220,
                aux_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4616C0` (case 431): clears scene words 304 and 306.
        // Returns 0x4000.
        431 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 304 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479FB0` (case 432): writes the scene layer group at
        // base word 307 (`sub_45CFE0` reads words 307/308). Returns 0x4000.
        432 => (
            0,
            CmvsPs2aCommandEffectKind::QuerySceneLayer {
                base_word: 307,
                result_field_offset: 81220,
                aux_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4613D0` (case 433): clears scene words 307 and 309.
        // Returns 0x4000.
        433 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 307 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47DC80` (case 463): consumes a channel and a child
        // selector and forwards the element's texture object. Returns 0x4008.
        463 => (
            8,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
        // Handler `sub_47DCE0` (case 464): consumes a channel, a child
        // selector and nine configuration words. Returns 0x402C.
        464 => (
            44,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47DE00` (case 465): consumes a channel, a child selector and 1 configuration word. Returns 0x400C.
        465 => (
            12,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
        // Handler `sub_47DDA0` (case 466): consumes a channel and a child selector. Returns 0x4008.
        466 => (
            8,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
        // Handler `sub_47DAC0` (case 468): consumes a channel and a child selector. Returns 0x4008.
        468 => (
            8,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
        // Handler `sub_47DB20` (case 469): consumes a channel, a child selector and 6 configuration words. Returns 0x4020.
        469 => (
            32,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
        // Handler `sub_47DC20` (case 470): consumes a channel, a child selector and 1 configuration word. Returns 0x400C.
        470 => (
            12,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
        // Handler `sub_47DBC0` (case 471): consumes a channel and a child selector. Returns 0x4008.
        471 => (
            8,
            CmvsPs2aCommandEffectKind::ApplyEffectElementOperation {
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
        // Handler `sub_47B0B0` writes `owner != 0 && owner->field5 != 0`
        // into the version-pinned result field without touching the stack.
        // Returns 0x4000.
        524 => (
            0,
            CmvsPs2aCommandEffectKind::QuerySaveImageEnabled {
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485FD0` replaces the occupant of the six-slot
        // filter-chain table at byte offset 3196, bounded by bank six and
        // channel 256 (0x100000 error mask at byte offset 10532 otherwise).
        // Returns 0x4008.
        528 => (
            8,
            CmvsPs2aCommandEffectKind::ReplaceFilterChainSlot {
                table_byte_offset: 3196,
                max_bank: 5,
                max_channel_id: 255,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x100000,
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
        // Handler `sub_486C80` destroys one of the six filter-chain banks at
        // byte offset 3196. Banks above five raise 0x100000 at byte offset
        // 10532; an already-empty valid bank is a no-op. Returns 0x4004.
        529 => (
            4,
            CmvsPs2aCommandEffectKind::DestroyFilterChainSlot {
                table_byte_offset: 3196,
                max_bank: 5,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x100000,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_482940` shares `sub_4837F0`'s validation with the
        // case-72 audio rebind: the container liveness and child bound
        // Handler `sub_482940` (case 490): pops (value, child, channel),
        // resolves the this+0x784 channel child (channel < 0x100, child
        // < 0x400) and installs the value payload on the child presenter
        // through `sub_42C1D0`/`sub_432AB0`. Returns 0x400c.
        490 => (
            12,
            CmvsPs2aCommandEffectKind::InstallChannelChildPayload {
                selector: 490,
                channel_table_field_offset: 0x784,
            },
            &THREE_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_482A40` (case 491): the seven-word payload install
        // forwarding five engine values through `sub_42C280`. Returns
        // 0x401c.
        491 => (
            28,
            CmvsPs2aCommandEffectKind::InstallChannelChildPayload {
                selector: 491,
                channel_table_field_offset: 0x784,
            },
            &SEVEN_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_482B70` (case 492): the three-word state install
        // writing (1, value, value, 0) through `sub_432820`. Returns
        // 0x400c.
        492 => (
            12,
            CmvsPs2aCommandEffectKind::InstallChannelChildPayload {
                selector: 492,
                channel_table_field_offset: 0x784,
            },
            &THREE_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Case 493 (`sub_482B00`): destroys one texture container child's
        // load state. The container slot must be live (`< 0x100`, raising
        // 0x10) and the child selector below 0x400 (raising 0x20) at byte
        // offset 10532. Returns 0x4008.
        493 => (
            8,
            CmvsPs2aCommandEffectKind::ReleaseTextureChildState {
                texture_table_base_offset: 1924,
                max_parent_slot: 255,
                max_child_id: 1023,
                container_error_flag_field_offset: 10532,
                container_error_mask: 0x10,
                child_error_flag_field_offset: 10532,
                child_error_mask: 0x20,
            },
            &TWO_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Case 494 (`sub_4829C0`): publishes one texture container child's
        // load state into the result field at byte offset 81220. The
        // container slot must be live (`< 0x100`, raising 0x10) and the
        // child selector below 0x400 (raising 0x20) at byte offset 10532.
        // Returns 0x4008.
        494 => (
            8,
            CmvsPs2aCommandEffectKind::QueryTextureChildState {
                texture_table_base_offset: 1924,
                max_parent_slot: 255,
                max_child_id: 1023,
                container_error_flag_field_offset: 10532,
                container_error_mask: 0x10,
                child_error_flag_field_offset: 10532,
                child_error_mask: 0x20,
                result_field_offset: 81220,
            },
            &TWO_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Case 500 (`sub_4823E0`): draws one texture container child onto
        // the parent surface. Validation mirrors the case-490 family: the
        // container slot must be live (`< 0x100`, raising 0x10) and the
        // child selector must be below 0x400 (raising 0x20) at byte offset
        // 10532. The engine draw chain stays outside the recovered subset.
        // Handler `sub_4821A0` (case 509): like case 494 but on the 481
        // channel table; publishes the child's draw state (idle zero in
        // the recovered subset) at byte offset 81220. Returns 0x4008.
        509 => (
            8,
            CmvsPs2aCommandEffectKind::QueryTextureChildState {
                texture_table_base_offset: 481,
                max_parent_slot: 255,
                max_child_id: 1023,
                container_error_flag_field_offset: 10532,
                container_error_mask: 0x10,
                child_error_flag_field_offset: 10532,
                child_error_mask: 0x20,
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4822F0` (case 508): advances the channel child's
        // draw state (void, no result word); the same validation as 509.
        // Case 482 (`sub_4820B0`): stores the value word through the
        // 481-table child (`sub_42BAC0`); the slot gate raises 0x10 and
        // an index at or above 1024 raises 0x20. Returns 0x400C.
        482 => (
            12,
            CmvsPs2aCommandEffectKind::StoreTextureChildByIndex,
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
        // Case 481 (`sub_481F70`): passes the eight-word float block to
        // the 481-table child (`sub_42BA40`); the slot gate raises 0x10
        // and an index at or above 1024 raises 0x20. Returns 0x4020.
        481 => (
            32,
            CmvsPs2aCommandEffectKind::StoreTextureChildByIndex,
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
        // Case 480 (`sub_481E60`): passes the float pair to the
        // 481-table child (`sub_42B970`); the slot gate raises 0x10 and
        // an index at or above 1024 raises 0x20. Returns 0x4010.
        480 => (
            16,
            CmvsPs2aCommandEffectKind::StoreTextureChildByIndex,
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
        // Handler `sub_482130` (case 505): releases the channel child at
        // the stack index (a negative index releases the object itself);
        // a missing slot raises 0x10 and an out-of-range index raises
        // 0x20. Returns 0x4008.
        505 => (
            8,
            CmvsPs2aCommandEffectKind::ReleaseTextureChildByIndex,
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
        // Handler `sub_482220` (case 506): passes nine stack words to the
        // child geometry call (native returns 0x4024, pop 36); the
        // 481-table slot gate raises 0x10 and an index at or above 1024
        // raises 0x20.
        506 => (
            36,
            CmvsPs2aCommandEffectKind::ConfigureTextureChildGeometry,
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 36,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_482360` (case 507): stores the value word through
        // the 481-table child; the slot gate raises 0x10 and an index at
        // or above 1024 raises 0x20. Returns 0x400C.
        507 => (
            12,
            CmvsPs2aCommandEffectKind::StoreTextureChildWord,
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
        // Handler `sub_4822F0` (case 508): advances the channel child's
        // draw state (void, no result word); the same validation as 509.
        508 => (
            8,
            CmvsPs2aCommandEffectKind::KickTextureChildDraw,
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
        500 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureChannelChildPresenter {
                selector: 500,
                channel_table_field_offset: 0x784,
            },
            &TWO_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Case 501 (`sub_4824D0`): the eight-word variant of the case-500
        // child draw, forwarding six engine values after the container and
        // child selectors. Same validation and masks. Returns 0x4020.
        501 => (
            32,
            CmvsPs2aCommandEffectKind::ConfigureChannelChildPresenter {
                selector: 501,
                channel_table_field_offset: 0x784,
            },
            &EIGHT_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Case 502 (`sub_482600`): the three-word variant of the case-500
        // child draw, forwarding one engine value after the selectors.
        // Same validation and masks. Returns 0x400c.
        502 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureChannelChildPresenter {
                selector: 502,
                channel_table_field_offset: 0x784,
            },
            &THREE_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Case 860 (`sub_479C60`): stores whether the advance-group press
        // latch (`sub_45CCD0`, manager dword 273) reads non-zero. Takes no
        // stack operands. Returns 0x4000.
        860 => (
            0,
            CmvsPs2aCommandEffectKind::StorePressLatch {
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Case 418 (`sub_479A70`): publishes the cancel-group latches.
        // Returns 0x4000.
        418 => (
            0,
            CmvsPs2aCommandEffectKind::ReadCancelLatches {
                held_field_offset: 81220,
                edge_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Case 420 (`sub_479AE0`): publishes the skip-group latches.
        // Returns 0x4000.
        420 => (
            0,
            CmvsPs2aCommandEffectKind::ReadAuxLatches {
                held_field_offset: 81220,
                edge_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Case 422 (`sub_47A0F0`): publishes the pointer-click group
        // latches (zero without a click). Returns 0x4000.
        422 => (
            0,
            CmvsPs2aCommandEffectKind::ReadAuxLatches {
                held_field_offset: 81220,
                edge_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    })
}
