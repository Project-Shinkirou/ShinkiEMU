use super::*;

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // 480900 configures queue2964 equal-depth insertion, not a view.
        41 => (
            4,
            CmvsPs2aCommandEffectKind::SetNativeQueuePolicy,
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }],
        ),
        // 483710 -> 446AF0 -> 42E8A0: absolute rotation in integer degrees.
        74 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureTextureRotation,
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
        // 483530 -> 446A40: selects camera-projected basic kind 2 or 3.
        76 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureTextureBasicKind { command_id },
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
        // Case 60 (`sub_4843E0`): hit query on a table-1924 slot object
        // guarded by a negative second word; consumes the present-complete
        // word (9644) and stores the test result in sysreg0. Returns
        // 0x4008.
        60 => (
            8,
            CmvsPs2aCommandEffectKind::QueryPresentSlotHit {
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
        // Handler `sub_47C2B0` (case 8): publishes the selected system
        // statistic (`sub_41A020`, a boot-global with no explicit seed)
        // into the result field. The headless fresh boot reads zero.
        // Returns 0x4000.
        8 => (
            0,
            CmvsPs2aCommandEffectKind::StoreInterpreterConstant {
                field_offset: 81220,
                value: 0,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Dispatcher case 0 returns 0xc000. The high bit terminates its
        // current loop, while the 0x4000 bit advances the opcode by two.
        0 => (
            0,
            CmvsPs2aCommandEffectKind::StopDispatch,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Dispatcher case 3 (`sub_477AC0`) tears down every engine-side
        // object table — the 256-slot channel table, the six filter-chain
        // banks, texture parents/children, native text, effect channels and
        // playback — then reloads `main.ps3` through `sub_4781B0` and
        // clears the record-28 fallback words (flag/frame 0, pc -1). It
        // reads nothing off the stack and returns 0.
        3 => (
            0,
            CmvsPs2aCommandEffectKind::ResetEngineToMain,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_478B50` zeroes the interpreter word selected by the
        // stack-top dword index into `this+33AC` (the wait-state family)
        // and returns 0x4004.
        11 => (
            4,
            CmvsPs2aCommandEffectKind::ClearInterpreterTableWord {
                table_base_offset: 13228,
                table_word_count: 10,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_481CD0` (case 42): validates the stack slot index
        // against 0x100, binds the stack child element when its id is
        // non-negative, and stores reference dimensions on that graphics
        // object (`sub_445E80`). Its width scales both coordinate axes.
        // Returns 0x4010.
        42 => (
            16,
            CmvsPs2aCommandEffectKind::SetResourceSlotRegion { max_slot: 255 },
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
        // Handler `sub_478B80` copies the table word selected by the
        // stack-top dword index from `this+33AC` (byte offset 81220 is
        // the destination) and returns 0x4004.
        12 => (
            4,
            CmvsPs2aCommandEffectKind::LoadInterpreterTableWord {
                table_base_offset: 13228,
                table_word_count: 10,
                field_offset: 81220,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Identical setup helpers receive different interpreter-owned tables.
        16 | 17 | 18 | 19 | 20 | 23 | 24 | 26 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureResourceSearch {
                bank: match command_id {
                    16 => CmvsResourceSearchBank::Script,
                    17 => CmvsResourceSearchBank::Texture,
                    18 => CmvsResourceSearchBank::Native0d0c,
                    19 => CmvsResourceSearchBank::CrossfadeAudio,
                    20 => CmvsResourceSearchBank::Native0d14,
                    23 => CmvsResourceSearchBank::Native0d00,
                    24 => CmvsResourceSearchBank::Native0d04,
                    26 => CmvsResourceSearchBank::Native0d18,
                    _ => unreachable!(),
                },
            },
            &RESOURCE_CHANNEL_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47BCB0` resolves the top stack word through the
        // tagged-string resolver, copies the interpreter prefix field at
        // byte offset 10764 into the string buffer at byte offset 3356,
        // appends the resolved string, and returns 0x4004. Both segments
        // retain the copied game-relative value and a symbolic root; absolute
        // paths stay outside the VM snapshot.
        21 => (
            4,
            CmvsPs2aCommandEffectKind::StorePrefixedInterpreterString {
                buffer_offset: 3356,
                prefix_field_offset: 10764,
                ensure_directories: false,
            },
            &TAGGED_STRING_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47BB30` behaves like case 21 for the buffer at byte
        // offset 5404 with the prefix field at byte offset 7452, then walks
        // the assembled path creating missing directories, and returns
        // 0x4004.
        22 => (
            4,
            CmvsPs2aCommandEffectKind::StorePrefixedInterpreterString {
                buffer_offset: 5404,
                prefix_field_offset: 7452,
                ensure_directories: true,
            },
            &TAGGED_STRING_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47C270` copies the top stack word into `this+400`
        // (dword index; byte offset 1600) and returns 0x4004. Byte offset
        // 1600 is the interpreter identity field the system-save loader
        // (`sub_427B30`) compares against the container identity word.
        29 => (
            4,
            CmvsPs2aCommandEffectKind::StoreInterpreterWord { field_offset: 1600 },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_487190` copies the top stack word into `this+2678`
        // (dword index; byte offset 10712) and returns 0x4004. The
        // destination field has no recovered public behavior, so retain it
        // as a version-pinned opaque interpreter word.
        30 => (
            4,
            CmvsPs2aCommandEffectKind::StoreInterpreterWord {
                field_offset: 10712,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_487140` forwards the top stack word to `sub_41A310`.
        // That setter writes `dword_4F0578` and the handler returns 0x4004.
        31 => (
            4,
            CmvsPs2aCommandEffectKind::StoreProcessGlobalWord {
                address: 0x004f_0578,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4807E0` reads the top slot index, rejects indices at
        // or above 256 by OR-ing 0x10 into `this+10532`, and otherwise
        // replaces the slot's object with a fresh 0x3088-byte instance.
        // Returns 0x4004.
        32 => (
            4,
            CmvsPs2aCommandEffectKind::CreateSlotObject {
                table_byte_offset: 1924,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4808A0` bounds-checks the top slot index the same way
        // and destroys the object held there. Returns 0x4004.
        33 => (
            4,
            CmvsPs2aCommandEffectKind::DestroySlotObject {
                table_byte_offset: 1924,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4805E0` selects the top-level texture container with
        // the stack-top word and recreates the child selected by word two.
        34 => (
            8,
            CmvsPs2aCommandEffectKind::ResetTextureChild {
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
        // 480620 → 4443B0 destroys one child of the selected live parent.
        35 => (
            8,
            CmvsPs2aCommandEffectKind::DestroyTextureChild {
                texture_table_byte_offset: 1924,
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
        // Handler `sub_480560` (case 39): the stack top selects a script
        // slot and the next word is a selector; the slot occupancy lands in
        // the result field. An out-of-range slot raises the error mask.
        // Returns 0x4008.
        39 => (
            8,
            CmvsPs2aCommandEffectKind::QueryScriptSlotOccupancy {
                slot_table_dword_index: 481,
                max_slot: 255,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x10,
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
        // Handler `sub_481C30` reads destination/source slots at top/8,
        // rejects either index at or above 256 via the same error flag, and
        // otherwise moves the source object into the destination slot.
        // Returns 0x4008.
        40 => (
            8,
            CmvsPs2aCommandEffectKind::MoveSlotObject {
                table_byte_offset: 1924,
            },
            &SLOT_PAIR_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // 4804C0 clears header bit0x100; it does not choose a presentation target.
        46 => (
            8,
            CmvsPs2aCommandEffectKind::DisableTextureShake,
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
        // Handler `sub_4803D0` tears down every live entry of the
        // 256-slot texture channel table at byte offset 1924 without reading
        // the stack. Returns 0x4000.
        47 => (
            0,
            CmvsPs2aCommandEffectKind::ClearTextureChannelTable {
                channel_table_base_offset: 1924,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_483D70` resolves stack/8 as the parent PB resource,
        // selects the stack-top container and replaces its resource loader.
        // The loader success flag is stored at byte offset 81220.
        48 => (
            8,
            CmvsPs2aCommandEffectKind::LoadTextureParentResource {
                texture_table_base_offset: 1924,
                max_parent_slot: 255,
                result_field_offset: 81220,
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
                    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_484020` (case 51): reads one script slot selector and
        // stores the slot's presentation state into the interpreter words at
        // byte offsets 81220/81224/81228/81232. An unoccupied slot raises
        // 0x10 at byte offset 10532. Returns 0x4004.
        51 => (
            4,
            CmvsPs2aCommandEffectKind::QueryTextureParentImageInfo {
                slot_table_dword_index: 1924,
                error_mask: 0x10,
                result_field_offset: 81220,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // `sub_483EB0`: name at -8, parent at -4, then three image
        // processing words. The native guard is this[481+parent].
        59 => (
            20,
            CmvsPs2aCommandEffectKind::LoadTextureParentFromName {
                texture_table_byte_offset: 1924,
                error_mask: 0x10,
                result_field_offset: 81220,
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
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 20,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // 480420 -> 445E40: detach a live child's parent basic context.
        44 => (
            8,
            CmvsPs2aCommandEffectKind::DetachTextureChildContext,
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
        // Handler `sub_483FB0` -> `sub_444F10`: on a live channel slot the
        // child query's vtable result lands in the pinned result word (the
        // recovered subset reports the child-absent default 0); the optional
        // `sub_444EF0` select stays in the renderer. An out-of-range or
        // unbound slot raises 0x10. Returns 0x4010.
        52 => (
            16,
            CmvsPs2aCommandEffectKind::QueryChannelChildState {
                slot_table_dword_index: 481,
                error_mask: 0x10,
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_480700` resolves the third stack word as the
        // resource name and plays it through the stack-top channel when the
        // channel table entry is live; otherwise it only raises the error
        // flag at byte offset 10532. Returns 0x400c.
        56 => (
            12,
            CmvsPs2aCommandEffectKind::LoadTextureChildResource {
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 12,
                    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler (case 63, dispatch thunk at 0x473188): loads the engine
        // object pointer from interpreter dword 831 (byte offset 3324) and
        // sets the object's dword 24720 through `sub_4405A0` — a refresh
        // request on an engine-owned object with no stack operands,
        // returning 0x4000. The headless graph owns presentation pacing, so
        // the flag write has no observable interpreter state and the
        // command reduces to the no-op contract.
        63 => (
            0,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // 482F20/482F60 create or free the basic surface, not the image loader.
        64 | 65 => (
            4,
            CmvsPs2aCommandEffectKind::SetTextureParentSurface {
                initialized: command_id == 64,
                texture_table_base_offset: 1924,
                max_parent_slot: 255,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x10,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // 4838D0 -> 446B30 -> 42E950: source rectangle (x, y, width, height).
        // Every negative surface selector addresses the parent's basic surface.
        68 => (
            24,
            CmvsPs2aCommandEffectKind::ConfigureTextureChildRect {
                texture_table_base_offset: 1924,
                max_parent_slot: 255,
                max_child_id: 1023,
                error_flag_field_offset: 10532,
                parent_error_mask: 0x10,
                child_error_mask: 0x20,
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
        // 483A00 -> 446B90 -> 42E9B0: destination offset (DWORD8/9).
        69 => (
            16,
            CmvsPs2aCommandEffectKind::ConfigureTextureAuxiliaryPair {
                texture_table_base_offset: 1924,
                max_parent_slot: 255,
                max_child_id: 1023,
                error_flag_field_offset: 10532,
                parent_error_mask: 0x10,
                child_error_mask: 0x20,
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
        // 483600 -> 446AD0 -> 42E870: source offset (DWORD6/7).
        70 => (
            16,
            CmvsPs2aCommandEffectKind::ConfigureTextureChildPosition {
                texture_table_base_offset: 1924,
                max_parent_slot: 255,
                max_child_id: 1023,
                error_flag_field_offset: 10532,
                parent_error_mask: 0x10,
                child_error_mask: 0x20,
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
        // 483B10 -> 446BE0 -> 42EA10: draw order (DWORD10).
        71 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureTextureAuxiliaryWord {
                texture_table_base_offset: 1924,
                max_parent_slot: 255,
                max_child_id: 1023,
                error_flag_field_offset: 10532,
                parent_error_mask: 0x10,
                child_error_mask: 0x20,
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
        // 4837F0 -> 446B10 -> 42E8E0: signed alpha, upper-clamped to 255.
        72 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureTextureAlpha {
                texture_table_base_offset: 1924,
                max_parent_slot: 255,
                max_child_id: 1023,
                error_flag_field_offset: 10532,
                parent_error_mask: 0x10,
                child_error_mask: 0x20,
            },
            &THREE_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // 483A80 forwards two raw IEEE754 scales to the selected basic surface.
        73 => (
            16,
            CmvsPs2aCommandEffectKind::ConfigureTextureScale,
            &FOUR_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // 483860 -> 446B20 -> 42E900: basic blend state, not audio.
        75 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureTextureBlend,
            &THREE_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // 4834E0 -> 446A30: selects basic kind 2 without a child bound check.
        66 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureTextureBasicKind { command_id },
            &TWO_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // 483030 -> 446A50/446A70: flat pass selection.
        78 => (
            12,
            CmvsPs2aCommandEffectKind::ConfigureTextureBasicKind { command_id },
            &THREE_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // 482EA0 -> 4443F0 -> 42C300: width, height and packed solid color.
        // Unlike 68-72 this handler has no child-bound error-mask branch.
        67 => (
            20,
            CmvsPs2aCommandEffectKind::ConfigureTextureSolidSurface {
                texture_table_base_offset: 1924,
                max_parent_slot: 255,
                error_flag_field_offset: 10532,
                parent_error_mask: 0x10,
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
        // 481D30 → 445EA0 sets visibility on the root (negative selector) or one child.
        38 => (
            12,
            CmvsPs2aCommandEffectKind::SetTextureVisibility,
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
        // Handler `sub_47B570` (case 1): tears down the message/presentation
        // engine objects held at this+0xC60..0xC6C (frees the two heap
        // blocks, nulls the pointers, and resets the two engines through
        // `sub_48EC20`/`sub_48CBC0`). No stack operands and no
        // interpreter-visible words. Returns 0x4000.
        1 => (
            0,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    })
}
