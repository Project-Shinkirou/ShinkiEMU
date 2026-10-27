use super::*;

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // Case 772 (`sub_426670`): sets interpreter dword 2408 to one.
        // Returns 0x4000.
        772 => (
            0,
            CmvsPs2aCommandEffectKind::SetPresentAllFlag { field_offset: 9632 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Case 774 (`sub_485E70`): sysreg0 becomes `this[2411] != 0` (the
        // present-complete flag the renderer raises after the present-all
        // request). Returns 0x4000.
        774 => (
            0,
            CmvsPs2aCommandEffectKind::ReadPresentCompleteFlag {
                result_field_offset: 81220,
                complete_field_offset: 9644,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Case 773 (`sub_426680`): clears the four-word present block
        // (dwords 2408..2411). Returns 0x4000.
        773 => (
            0,
            CmvsPs2aCommandEffectKind::ClearPresentBlock,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479400` reads two words: with a non-zero selector it
        // stores the second word as the boolean flag at byte offset 1464,
        // otherwise it copies that flag into byte offset 81220. Returns
        // 0x4008.
        747 => (
            8,
            CmvsPs2aCommandEffectKind::InterpreterFlagRoundTrip {
                flag_field_offset: 1464,
                result_field_offset: 81220,
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
        // `479A10`: the setter writes renderer DWORD47 through `429B20`;
        // the query reads that flag through `4292C0`, not `429290`'s
        // effective fullscreen predicate. Returns 0x4008.
        748 => (
            8,
            CmvsPs2aCommandEffectKind::ConfigureNativeAlternateView {
                result_field_offset: 81220,
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
        // 479570 exchanges the global default font family with the tagged
        // string destination/source; zero reads, nonzero writes.
        734 => (
            8,
            CmvsPs2aCommandEffectKind::NativeText {
                operation: crate::CmvsNativeTextOperation::DefaultFont,
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
        // Handler `sub_479120` (case 749): the zero-top path polls the
        // guarded field `[[this+3168]+0x9E4]` on the interpreter-referenced
        // component through `sub_48C290` and publishes its boolean into the
        // result word at 81220; a non-zero top instead posts engine message
        // 0x8006 through `sub_48CDD0` (no such call site appears in the
        // recovered scenario streams). The stack words feed only the branch
        // test, so the contract declares the full pop width with no words.
        // Returns 0x4008.
        749 => (
            8,
            CmvsPs2aCommandEffectKind::QueryComponentWordBooleanToResult {
                component_field_offset: 3168,
                field_offset: 2532,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // 478A90 -> 42AF40 publishes retained graphics-policy DWORD25.
        764 => (
            0,
            CmvsPs2aCommandEffectKind::QueryComponentWordToResult {
                component_field_offset: 1920,
                field_offset: 100,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Dispatcher case 750 writes one to byte offset 81220 when the
        // interpreter word at dword index 399 (byte offset 1596) is
        // non-zero, and zero otherwise. Returns 0x4000.
        750 => (
            0,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordBooleanToResult {
                source_field_offset: 1596,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Native input modes 293/296, or interpreter override 1464.
        751 => (
            0,
            CmvsPs2aCommandEffectKind::ReadAutomaticAdvance {
                result_field_offset: 81220,
                override_field_offset: 1464,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // 776/782 open CStreamVideo; 777 configures a range on the existing
        // controller. Stack offsets are relative to the top, not push order.
        776 | 782 => (
            if command_id == 776 { 20 } else { 24 },
            CmvsPs2aCommandEffectKind::CmvController { command_id },
            if command_id == 776 {
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
                    CmvsPs2aCommandStackWord {
                        offset_from_top_bytes: 20,
                        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                    },
                ] as &[CmvsPs2aCommandStackWord]
            } else {
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
                    CmvsPs2aCommandStackWord {
                        offset_from_top_bytes: 20,
                        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                    },
                    CmvsPs2aCommandStackWord {
                        offset_from_top_bytes: 24,
                        kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                    },
                ] as &[CmvsPs2aCommandStackWord]
            },
        ),
        777 => (
            20,
            CmvsPs2aCommandEffectKind::CmvController { command_id },
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
        781 | 795 | 797 => (
            8,
            CmvsPs2aCommandEffectKind::CmvController { command_id },
            &TWO_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        778 | 779 | 780 | 783 | 794 | 796 | 798 => (
            4,
            CmvsPs2aCommandEffectKind::CmvController { command_id },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_483680` (case 802): reads the container selector at
        // offset 4, the child id at 8 and a boolean at 12, then toggles the
        // child's audio (`sub_444EF0`/`sub_446AE0` stay outside the recovered
        // subset). A dead container raises 0x10 and an out-of-range child
        // 0x20 at byte offset 10532. Returns 0x400c, so it pops all twelve
        // bytes it reads.
        802 => (
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
        // Dispatcher case 845 invokes `sub_45CBC0` on `this+0xCC8`; that
        // method writes one to its field at byte offset 0x568.
        845 => (
            0,
            CmvsPs2aCommandEffectKind::StoreComponentConstant {
                component_offset: 0x0cc8,
                field_offset: 0x0568,
                value: 1,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479C90` (case 846) writes `sub_45CBB0`'s value —
        // the input manager's selection id (field 347: set on the press
        // edge of one of the twelve key bytes, kept until an explicit
        // clear) — into the result field at 81220. The port mirrors the
        // confirmed quad id at interpreter word 1388 on the same press
        // edge (see the cmd-404 poll). Returns 0x4000.
        // Handler `sub_479C90` (case 846) publishes the input manager
        // selection id (`sub_45CBB0`: field 347). Live values route the
        // menu poll's item dispatch into native branch families whose
        // resource sequences are not yet recovered (the first one
        // resolves dialogue text as an audio name); the query keeps
        // publishing zero, the form every verified flow was proven
        // against. The mirror (VM field, menu-gated press edge) and the
        // sel846 telemetry stay armed for the recovery rounds.
        // Handler `sub_479C90` (case 846) publishes the input manager
        // selection id (`sub_45CBB0`: field 347). The live value chain is
        // proven (sel846 telemetry: mirror -> query -> result slot, all
        // menu poll sites fire), but a live value changes the save
        // orchestrator's path after its 369756 dispatch (the second
        // record-29 resume stops before cmd 694) and the menu item
        // compare (`sysreg0 == local16`) needs the item-argument layer
        // (local16 comes from the dispatcher's frame params — the
        // concurrent session's active domain) to match. Keep zero, the
        // form every green flow was proven against.
        846 => (
            0,
            CmvsPs2aCommandEffectKind::PublishInputSelection {
                field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_480000` forwards `top != 0` to the settings setter
        // `sub_45C2C0`, which resets the recovered settings fields and
        // stores the boolean at field offset 0. Returns 0x4004.
        848 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSystemSettingBoolean {
                setting_field_offset: 0,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FC50` forwards `top != 0` and `second != 0` to
        // `sub_45AEA0`, which resets the recovered settings fields and
        // stores the booleans at field offsets 24 and 28. Returns 0x4008.
        849 => (
            8,
            CmvsPs2aCommandEffectKind::StoreSystemSettingBooleanPair {
                first_field_offset: 24,
                second_field_offset: 28,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FC20` forwards `top != 0` to the settings setter
        // `sub_45AE80`, which resets the recovered settings fields and
        // stores the boolean at field offset 0. Returns 0x4004.
        850 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSystemSettingBoolean {
                setting_field_offset: 0,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FDC0` copies settings field 44 (`sub_45A0E0`) into
        // the interpreter word at byte offset 81220 without a stack pop.
        // Returns 0x4000.
        851 => (
            0,
            CmvsPs2aCommandEffectKind::CopySettingsFieldToInterpreterWord {
                settings_field_offset: 44,
                target_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FBE0` reads the settings field selected by the top
        // stack word through `sub_45C300` (negative indices fall back to
        // field 0, indices above five resolve to zero) and stores its
        // truthiness at byte offset 81220. Returns 0x4004.
        853 => (
            4,
            CmvsPs2aCommandEffectKind::StoreIndexedSettingBoolean {
                target_field_offset: 81220,
                max_index: 5,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FCE0` forwards `top != 0` to `sub_45AF00`
        // (reset + store at field offset 12). Returns 0x4004.
        854 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSystemSettingBoolean {
                setting_field_offset: 12,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FC80` forwards `top != 0` to `sub_45AEC0`
        // (reset + store at field offset 16). Returns 0x4004.
        855 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSystemSettingBoolean {
                setting_field_offset: 16,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FCB0` forwards `top != 0` to `sub_45AEE0`
        // (reset + store at field offset 20). Returns 0x4004.
        856 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSystemSettingBoolean {
                setting_field_offset: 20,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FEC0` derives a result through `sub_45C390` from
        // settings fields 72/76/80 and stores the result at byte offset
        // 81220 and the second selector's truthiness at 81236, without a
        // stack pop. Returns 0x4000.
        857 => (
            0,
            CmvsPs2aCommandEffectKind::StoreDerivedSettingsPair {
                first_selector_field_offset: 72,
                second_selector_field_offset: 76,
                result_field_offset: 80,
                result_target_field_offset: 81220,
                flag_target_field_offset: 81236,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FF00` forwards the raw top word to `sub_45C430`,
        // which stores it at field offset 36 only when it lies in 1..=90.
        // No reset path. Returns 0x4004.
        858 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSystemSettingBoundedWord {
                setting_field_offset: 36,
                minimum: 1,
                maximum: 90,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FF30` forwards the raw top word to `sub_45C450`,
        // which stores it at field offset 32 only when non-negative.
        // No reset path. Returns 0x4004.
        859 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSystemSettingNonNegativeWord {
                setting_field_offset: 32,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FE20` copies settings field 88 (`sub_45C360`) into
        // the interpreter word at byte offset 81220 without a stack pop.
        // Returns 0x4000.
        864 => (
            0,
            CmvsPs2aCommandEffectKind::CopySettingsFieldToInterpreterWord {
                settings_field_offset: 88,
                target_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FE90` stores `top == 1 && settings[88] >= 0`
        // (`sub_45C370`, signed compare) at byte offset 81220. Returns
        // 0x4004.
        866 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSettingsPredicate {
                settings_field_offset: 88,
                target_field_offset: 81220,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FF60` forwards the top stack word to the settings
        // setter `sub_45C470`, which resets the recovered settings fields
        // and stores the word at field offset 4. Returns 0x4004.
        868 => (
            4,
            CmvsPs2aCommandEffectKind::StoreSettingsWordReset {
                setting_field_offset: 4,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FE40` derives the case-869 snapshot through
        // `sub_45C3E0`: settings gate field 120 copies fields 100/104/124/128
        // into interpreter words 81236/81240/81224/81228; settings field 176
        // is always copied to 81244, and the gate's truthiness is stored at
        // 81220. No stack pop. Returns 0x4000.
        869 => (
            0,
            CmvsPs2aCommandEffectKind::DeriveSettingsSnapshot {
                gate_field_offset: 120,
                copy_source_field_offsets: [100, 104, 124, 128],
                copy_target_field_offsets: [81236, 81240, 81224, 81228],
                always_source_field_offset: 176,
                always_target_field_offset: 81244,
                gate_target_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47FF90` forwards six stack words to `sub_45B260`,
        // which bounds the top index to five and stores the remaining five
        // words as a 20-byte record at settings field 188 + 20*index.
        // Returns 0x4018.
        870 => (
            24,
            CmvsPs2aCommandEffectKind::StoreSettingsRecord {
                record_base_field_offset: 188,
                record_stride_bytes: 20,
                max_index: 5,
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
        // Handler `sub_487160` forwards `stack_top != 0` to `sub_41A0D0`.
        // The setter writes `dword_4F0580`; the handler returns 0x4004.
        940 => (
            4,
            CmvsPs2aCommandEffectKind::StoreProcessGlobalBoolean {
                address: 0x004f_0580,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479060` (case 966): resolves the stack-top resource
        // name, opens the scenario stream, marks the loading flag (interpreter
        // dword 2677) and suspends the calling coroutine until the load
        // completes. The headless archive resolves synchronously, so only
        // the suspension is observable. Returns 0xC004 on success, 0x4004
        // on a failed or already-busy open.
        966 => (
            4,
            CmvsPs2aCommandEffectKind::OpenScenarioStream {
                loading_field_offset: 2677,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486E60` (case 832): stores `stack_top != 0` into
        // interpreter byte offset 10720, the text skip-mode flag the
        // channel waits consult. Returns 0x4004.
        832 => (
            4,
            CmvsPs2aCommandEffectKind::StoreInterpreterWord {
                field_offset: 10720,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // 478AC0 compares the raw operand with policy DWORD25, then
        // 42B3D0 clamps and reapplies policy DWORD24 to renderer DWORD43.
        // This handler always returns 4004, including fullscreen mode.
        765 => (
            4,
            CmvsPs2aCommandEffectKind::ConfigureNativeViewPolicy { command_id: 765 },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Inline dispatcher case 847 (`sub_45CBA0` on the `this[818]`
        // input manager): clears the selection field 347 — the consume
        // side of the `cmd 846` read / `cmd 847` clear pair. The mirrored
        // selection lives in the process-word table at 2200; until `cmd
        // 846` publishes real values the clear stays a no-op, the form
        // the verified menu flows were proven against. Returns 0x4000.
        847 => (
            0,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    })
}
