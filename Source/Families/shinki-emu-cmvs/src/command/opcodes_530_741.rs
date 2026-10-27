use super::*;

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // 4852E0/485000/484FD0/485060/485030 address the primary
        // independent view, not either of the 4566B0 plane cameras.
        608 => (
            12,
            CmvsPs2aCommandEffectKind::NativeIndependentView { command_id },
            &THREE_OPAQUE_WORDS,
        ),
        609..=612 | 624..=625 => (
            4,
            CmvsPs2aCommandEffectKind::NativeIndependentView { command_id },
            &OPAQUE_WORD,
        ),
        626 => (
            0,
            CmvsPs2aCommandEffectKind::NativeIndependentView { command_id },
            &[],
        ),
        627 => (
            20,
            CmvsPs2aCommandEffectKind::NativeIndependentView { command_id },
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
            ],
        ),
        628 => (
            16,
            CmvsPs2aCommandEffectKind::NativeIndependentView { command_id },
            &FOUR_OPAQUE_WORDS,
        ),
        629 => (
            28,
            CmvsPs2aCommandEffectKind::NativeIndependentView { command_id },
            &SEVEN_OPAQUE_WORDS,
        ),
        // 4789B0 updates the retained graphics policy only when the boolean
        // differs; fullscreen changes rebuild the window and yield C004.
        713 => (
            4,
            CmvsPs2aCommandEffectKind::ConfigureNativeViewPolicy { command_id: 713 },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // 478950 -> 42AF50 reads policy DWORD24, not renderer DWORD43.
        714 => (
            0,
            CmvsPs2aCommandEffectKind::QueryComponentWordBooleanToResult {
                component_field_offset: 1920,
                field_offset: 96,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Case 559 (`sub_47B1E0`): tears down and rebuilds the native
        // graphics context from four stack words; the headless pipeline
        // keeps the observable sysreg0 success store. The native return
        // 0x4014 pops twenty bytes (four stack words plus the return
        // linkage the interpreter recycles for the frame hand-off).
        559 => (
            20,
            CmvsPs2aCommandEffectKind::RebuildGraphicsContext {
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
        // Case 563 (`sub_47B4C0`): skippable present wait; the non-zero
        // mode breaks out with sysreg0 = 1 through the skip latch family.
        // Returns 0xA000 or 0xC004.
        563 => (
            4,
            CmvsPs2aCommandEffectKind::PresentSkippableWait {
                result_field_offset: 81220,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // 47C040 skips queued delays, completes replacements and resets.
        644 => (
            0,
            CmvsPs2aCommandEffectKind::RunEffectTimeline,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Case 647 (`sub_47C1F0` -> `sub_424630`): five-word timeline
        // step append. Returns 0x4014 (pop 20).
        647 => (
            20,
            CmvsPs2aCommandEffectKind::PushEffectTimelineCamera,
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
        // 481B00 -> 4490B0 returns -1 for a live snow manager.
        // Out-of-range slots preserve the result; missing owners fail closed.
        686 => (
            4,
            CmvsPs2aCommandEffectKind::QueryNativeSceneWorkerStatus,
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Case 672 (`sub_481590`): destroys the scene-layer worker at
        // the 737-table slot (<= 3). Returns 0x4004.
        672 => (
            4,
            CmvsPs2aCommandEffectKind::DestroySceneLayerWorker737,
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // 481440 -> 448ED0 -> 456350 constructs real D3D billboards.
        // Top-first: slot, resource, span, height, fall time, fade distance,
        // particle count, follow-camera boolean. Returns 0x4020.
        674 => (
            32,
            CmvsPs2aCommandEffectKind::NativeSceneWorker { command_id },
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
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 28,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 32,
                    kind: CmvsPs2aCommandStackWordKind::BooleanU32,
                },
            ],
        ),
        // Case 680 (`sub_480A80`): rebuilds the scene-layer worker at
        // the 737-table slot (<= 3) and destroys the 481-table child;
        // the native return 0x4010 pops sixteen bytes.
        680 => (
            16,
            CmvsPs2aCommandEffectKind::RebuildSceneLayerChild,
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
        // Cases 649/650 (`sub_47C160`/`sub_47C110` -> `sub_424510`/
        // `sub_424480`): six-word timeline step appends. Both return
        // 0x4018 (pop 24).
        649 => (
            24,
            CmvsPs2aCommandEffectKind::PushEffectTimelineTransform { mode: 1 },
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
        650 => (
            24,
            CmvsPs2aCommandEffectKind::PushEffectTimelineTransform { mode: 2 },
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
        // Case 560 (`sub_47B470`): destroys the graphics context when the
        // render state allows. Returns 0x4000.
        560 => (
            0,
            CmvsPs2aCommandEffectKind::ReleaseGraphicsContext {
                require_render_state: true,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Case 561 (`sub_47B400`): unconditionally destroys the graphics
        // context. Returns 0x4000.
        561 => (
            0,
            CmvsPs2aCommandEffectKind::ReleaseGraphicsContext {
                require_render_state: false,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Case 562 (`sub_47B440`): sysreg0 becomes the render state, or -1
        // without a context. Returns 0x4000.
        562 => (
            0,
            CmvsPs2aCommandEffectKind::QueryGraphicsContext {
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486BC0` queries the six-slot filter-chain table at
        // byte offset 3196: a missing bank raises the 0x100000 mask, a live
        // bank appends one opaque queue node and stores one at byte offset
        // 81220. Returns 0x4008 either way.
        // Handler `sub_485F50` (case 538): delivers one click to the
        // interactive chain picked by the stack-top bank index (0..=5) and
        // reports whether a record was committed (sub_468390). The
        // headless host has no pointer; the chain's hover walk stands in
        // for it, so the commit follows the chain's hover state. Returns
        // 0x400c.
        538 => (
            12,
            CmvsPs2aCommandEffectKind::ClickCommitFilterChain {
                max_bank: 5,
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486130` (case 539): frees the bank's previous
        // hotspot record and creates a fresh one bound to the channel with
        // the stack position pair (bank < 6, channel < 0x100, else the
        // 0x100000 mask at byte offset 10532). The headless graph retains
        // only the registration id. Returns 0x4010.
        539 => (
            16,
            CmvsPs2aCommandEffectKind::CreateChainHotspotRecord {
                max_bank: 5,
                max_channel: 255,
                error_flag_field_offset: 10532,
                error_mask: 0x0010_0000,
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
        530 => (
            8,
            CmvsPs2aCommandEffectKind::QueryFilterChainSlot {
                table_byte_offset: 3196,
                max_bank: 5,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x100000,
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
        // Handler `sub_486B60` updates four opaque words in the keyed list
        // record of one live filter-chain bank through `sub_468640`. A
        // missing record is ignored; a missing bank raises 0x100000 at the
        // interpreter error field. Returns 0x4018.
        531 => (
            24,
            CmvsPs2aCommandEffectKind::UpdateFilterChainRecord {
                table_byte_offset: 3196,
                max_bank: 5,
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
        // Handler `sub_486C10` forwards nine record words plus the bank to
        // `sub_468AD0`. The helper updates one keyed parameter block; its
        // layout is selected by process-global backend mode 0x4F057C. A
        // missing bank raises the 0x100000 error mask. Returns 0x4028.
        532 => (
            40,
            CmvsPs2aCommandEffectKind::UpdateFilterChainParameterBlock {
                table_byte_offset: 3196,
                max_bank: 5,
                backend_mode_address: 0x004f_057c,
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486D30` validates one of the six filter-chain banks
        // and applies its complete queued record list through `sub_4686A0`.
        // A missing bank raises 0x100000 at the interpreter error field.
        // Returns 0x4004.
        533 => (
            4,
            CmvsPs2aCommandEffectKind::ApplyFilterChainRecords {
                table_byte_offset: 3196,
                max_bank: 5,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x100000,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486CE0` returns the active record id from the selected
        // bank through `sub_468690`, or -1 when no record is active.
        534 => (
            4,
            CmvsPs2aCommandEffectKind::ReadActiveFilterChainSelection {
                table_byte_offset: 3196,
                max_bank: 5,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x100000,
                result_field_offset: 81220,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_486A50` validates a six-bank interactive chain and
        // forwards one input poll to `sub_468740`. The helper walks records
        // in insertion order, changes the active record on directional
        // input, and returns the active record id on confirm. Returns 0x4004.
        535 => (
            4,
            CmvsPs2aCommandEffectKind::PollFilterChainSelection {
                table_byte_offset: 3196,
                max_bank: 5,
                error_flag_field_offset: 10532,
                error_flag_mask: 0x100000,
                result_field_offset: 81220,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47A4F0` consumes four words to rebuild the singleton
        // filter-graph owner behind slot-table byte offset 3252 (replacing
        // any previous occupant) and stores one at byte offset 81220; the
        // consumed words themselves are not inspected. Returns 0x4010.
        544 => (
            16,
            CmvsPs2aCommandEffectKind::CreateSingletonSlotObject {
                table_byte_offset: 3252,
                slot: 0,
                field_offset: 81220,
                value: 1,
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
        // Handler `sub_47A5F0` frees the singleton filter-graph owner at
        // slot-table byte offset 3252 when present. Returns 0x4000.
        545 => (
            0,
            CmvsPs2aCommandEffectKind::DestroySingletonSlotObject {
                table_byte_offset: 3252,
                slot: 0,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47A6B0` reads the control word, clears byte offset
        // 81220 and branches on the singleton filter-graph owner created by
        // case 544; the healthy zero-control path returns 0xA000 (no pop),
        // which the nucleus reproduces by restoring the contract pop.
        548 => (
            4,
            CmvsPs2aCommandEffectKind::RunFilterGraphControl {
                table_byte_offset: 3252,
                slot: 0,
                field_offset: 81220,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47AFF0` frees any previous owner, builds a fresh
        // 0x28-byte owner from the four consumed words (flag and progress
        // fields zeroed by `sub_459BE0`) and publishes the handle.
        // Returns 0x4010.
        549 => (
            16,
            CmvsPs2aCommandEffectKind::CreateScreenShake {
                handle_field_offset: 3256,
            },
            &FOUR_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // 459CC0 returns remaining shake periods, or all ones for absent/infinite.
        550 => (
            0,
            CmvsPs2aCommandEffectKind::QueryScreenShakeRemaining {
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // 47B0F0 stores the consumed paused flag only when the owner exists.
        551 => (
            4,
            CmvsPs2aCommandEffectKind::SetScreenShakePaused,
            &OPAQUE_BOOLEAN as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47B160` distinguishes a missing owner (writes one,
        // 0x4004), a present owner with a set boolean (writes zero and
        // waits, 0xA000) and a present owner with a clear boolean
        // (destroy, writes one, 0xC004).
        552 => (
            4,
            CmvsPs2aCommandEffectKind::WaitOrDestroyScreenShake {
                result_field_offset: 81220,
            },
            &OPAQUE_BOOLEAN as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47B120` destroys a present owner without reading the
        // stack. Returns 0x4000.
        553 => (
            0,
            CmvsPs2aCommandEffectKind::DestroyScreenShake,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // `sub_47ACA0`: mask name at top, parent at -8, then total
        // milliseconds, software feather duration, and show/hide mode.
        // The mask belongs to the 815 singleton; the parent's image stays.
        554 => (
            20,
            CmvsPs2aCommandEffectKind::StartTexturePresentation {
                clear_background: false,
            },
            &[
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 8,
                    kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 4,
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
        // Handler `sub_47AF00` (case 555): the texture presentation commit.
        // Without a live presentation object it stores zero (0x4004); with
        // one and a zero mode it returns 0xA000 (frame wait); otherwise it
        // stores success and returns 0xC004 (show and end the frame).
        555 => (
            4,
            CmvsPs2aCommandEffectKind::CommitTexturePresentation {
                result_field_offset: 81220,
                present_object_field_offset: 3260,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        556 => (
            0,
            CmvsPs2aCommandEffectKind::QueryTexturePresentation,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        557 => (
            0,
            CmvsPs2aCommandEffectKind::FinishTexturePresentation,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        558 => (
            12,
            CmvsPs2aCommandEffectKind::StartTexturePresentation {
                clear_background: true,
            },
            &[
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
            ],
        ),
        // 484F90 -> 432F10 -> 4164F0 writes graphics DWORD18, the same
        // owner read by native save tag 515 through 42B010. Returns 0x4004.
        592 => (
            4,
            CmvsPs2aCommandEffectKind::StoreComponentBoolean {
                component_field_offset: 1920,
                field_offset: 72,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485A10` stores the top and second stack words into
        // the interpreter fields at byte offsets 10296 and 10300, then
        // writes one or zero to byte offset 10292 depending on whether both
        // are non-zero. Returns 0x4008.
        688 => (
            8,
            CmvsPs2aCommandEffectKind::StorePresentationFrameFields {
                top_field_offset: 10296,
                second_field_offset: 10300,
                active_field_offset: 10292,
            },
            &SLOT_PAIR_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_471030` captures the presented framebuffer,
        // downsamples to the case-688 dimensions and stores a bottom-up
        // 24-bpp BMP as the save snapshot. Pops nothing, pushes nothing and
        // returns 0 in both success and readback failure.
        696 => (
            0,
            CmvsPs2aCommandEffectKind::CaptureSaveSnapshot,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485950` stores the consumed word at byte offset
        // 10304 (the save preparation field). Returns 0x400C.
        689 => (
            4,
            CmvsPs2aCommandEffectKind::StoreInterpreterWord {
                field_offset: 10304,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4859D0` loads the named save slot and switches the
        // running program. Returns 0x400C for out-of-range slots.
        693 => (
            4,
            CmvsPs2aCommandEffectKind::LoadNativeSaveSlot,
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485BB0` writes the running program state into the
        // named save slot. Returns 0x400C.
        694 => (
            4,
            CmvsPs2aCommandEffectKind::SaveNativeGameSlot,
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_474626` stores the two top stack words at byte
        // offsets 10316 (top) and 10320 (next). Returns 0x4008.
        698 => (
            8,
            CmvsPs2aCommandEffectKind::StoreInterpreterWordPair {
                top_field_offset: 10_316,
                second_field_offset: 10_320,
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
        // Handler `sub_485440` probes `save%03d.dat` for the `CSV` magic
        // and publishes the boolean into the result word. Returns 0x400C.
        699 => (
            4,
            CmvsPs2aCommandEffectKind::ProbeNativeSaveSlot {
                result_field_offset: 81220,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485A60` rewrites the save header's title field at
        // +272 and publishes the write outcome. Returns 0x4008.
        700 => (
            8,
            CmvsPs2aCommandEffectKind::SetNativeSaveTitle {
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4855C0` reads the save slot's timestamp, title,
        // metadata and thumbnail. Returns 0x4014.
        701 => (
            20,
            CmvsPs2aCommandEffectKind::LoadNativeSaveInfo,
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
                    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
                },
                CmvsPs2aCommandStackWord {
                    offset_from_top_bytes: 20,
                    kind: CmvsPs2aCommandStackWordKind::TaggedStringReference,
                },
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485BE0` rewrites the save header metadata pair at
        // dwords 133/134 and publishes the write outcome. Returns 0x400C.
        702 => (
            12,
            CmvsPs2aCommandEffectKind::SetNativeSaveMeta {
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Inline thunk `mov dword [this+81220], 1`.
        704 | 705 => (
            0,
            CmvsPs2aCommandEffectKind::StoreInterpreterConstant {
                field_offset: 81220,
                value: 1,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Inline thunk `mov dword [this+10572], 1`.
        719 => (
            0,
            CmvsPs2aCommandEffectKind::StoreInterpreterConstant {
                field_offset: 10572,
                value: 1,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Native save-menu slot count: negative becomes 10, above 256 is 256.
        // Scans saveNNN.dat headers and selects the newest populated slot.
        691 => (
            4,
            CmvsPs2aCommandEffectKind::ScanNativeSaveSlots,
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // sub_4959A0 sets native save permission (1596), the saved flag
        // (15064), and the save submenu's enabled state. No GPU operation.
        692 => (
            4,
            CmvsPs2aCommandEffectKind::SetNativeSaveEnabled,
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_485970` (case 695) scans the save-slot range
        // `[start, start + count)` for the newest `saveNN.dat` and writes
        // its slot index, or -1 when the range is invalid. The headless
        // host exposes no writable save files, so the canonical result is
        // -1. Returns 0x4008.
        695 => (
            8,
            CmvsPs2aCommandEffectKind::QueryNewestSaveSlot {
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
        // Handler `sub_485B90` (case 697): copies the top stack word into
        // the interpreter word at byte offset 10312. Returns 0x4004.
        697 => (
            4,
            CmvsPs2aCommandEffectKind::StoreInterpreterWord {
                field_offset: 10312,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47A340` stores the top stack word into byte offset
        // 76 of the component referenced by interpreter byte offset 1920;
        // the companion getter result is discarded. Returns 0x4004.
        715 => (
            4,
            CmvsPs2aCommandEffectKind::StoreComponentWord {
                component_field_offset: 1920,
                field_offset: 76,
            },
            &OPAQUE_WORD as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47A370` (case 708) -> `sub_425B60`: recreates the
        // UI text object in the paired slot table (`this+481+slot`) and
        // loads the named resource through it, publishing success in the
        // pinned result word. Returns 0x4010 (pop 16).
        708 => (
            16,
            CmvsPs2aCommandEffectKind::ReloadTextureParentImage {
                result_field_offset: 81220,
                table_byte_offset: 1924,
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
            ] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47A490` (case 709) -> `sub_444BE0`: releases the
        // channel's transition sub-objects; no interpreter state changes.
        // Returns 0x4004.
        709 => (
            4,
            CmvsPs2aCommandEffectKind::ReleaseChannelTransition,
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_47A4C0` (case 710) -> `sub_444CB0`: publishes
        // whether the channel's transition words (dwords 3100/3101) are
        // active into the pinned result word. Returns 0x4004.
        710 => (
            4,
            CmvsPs2aCommandEffectKind::QueryChannelTransitionBusy {
                slot_table_dword_index: 481,
                error_mask: 0x0001_0000,
                result_field_offset: 81220,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_489CB0` (case 568): clamps the consumed volume
        // (0-100) and applies it system-side; no interpreter state.
        // Returns 0x4004.
        568 => (
            4,
            CmvsPs2aCommandEffectKind::SetSystemVolume,
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4846C0` stores `stack_top != 0` into the interpreter
        // word at byte offset 10568 and returns 0x4004.
        717 => (
            4,
            CmvsPs2aCommandEffectKind::StoreInterpreterWord {
                field_offset: 10568,
            },
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::BooleanU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // Dispatcher case 718 natively copies the interpreter word at
        // byte offset 10568 (`mov eax, [esi+2948h]`, the setup loop's
        // per-pass store) into the result field at 81220.  The port
        // wires the query to `LOADER_PROGRESS_FIELD` (11792) instead:
        // the session seeds it and the save-menu chain polls it
        // (runtime-proven 2026-10-21; the 10568 read left the walk
        // without its save). Returns 0x4000.
        718 => (
            0,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordToResult {
                source_field_offset: 11792,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_478EC0` (case 721): the stack top is a config key and
        // the next word its boolean value, stored into the interpreter config
        // word. A zero key instead stores the skip flag into the result.
        // Returns 0x4008.
        721 => (
            8,
            CmvsPs2aCommandEffectKind::StoreSystemConfigBoolean {
                config_field_offset: 1440,
                skip_flag_field_offset: 1452,
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
        // Handler `sub_479270` (case 741): a non-zero selector stores the
        // second word's boolean at byte offset 1500; a zero selector copies
        // that flag back into the result field at 81220. Returns 0x4008.
        741 => (
            8,
            CmvsPs2aCommandEffectKind::InterpreterFlagRoundTrip {
                flag_field_offset: 1500,
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
        // 47C090 publishes the actual queued timeline count.
        645 => (
            0,
            CmvsPs2aCommandEffectKind::ReadEffectTimelineCount {
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Case 646 (`sub_47C240` -> `sub_4246C0`): appends a timed delay
        // step (type 1) with the stack-top duration to the effect timeline
        // (sixteen records). Returns 0x4004.
        646 => (
            4,
            CmvsPs2aCommandEffectKind::PushEffectTimelineDelay,
            &[CmvsPs2aCommandStackWord {
                offset_from_top_bytes: 4,
                kind: CmvsPs2aCommandStackWordKind::OpaqueU32,
            }] as &[CmvsPs2aCommandStackWord],
        ),
        // 4245A0 queues a parent-image replacement with a transition duration.
        648 => (
            12,
            CmvsPs2aCommandEffectKind::PushEffectTimelineChannel,
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
        // 424790 clears all five timeline header words.
        640 => (
            0,
            CmvsPs2aCommandEffectKind::ResetEffectTimeline,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // 424B00 returns the active timeline cursor, or -1.
        641 => (
            0,
            CmvsPs2aCommandEffectKind::ReadEffectTimelineCursor {
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479860` (case 720): a non-zero selector applies
        // the cursor style engine-side; zero publishes the setting word.
        // Returns 0x4008.
        720 => (
            8,
            CmvsPs2aCommandEffectKind::StoreWindowStyleSetting {
                setting_field_offset: 1492,
                result_field_offset: 81220,
            },
            &TWO_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // 485D10 captures 4712F0 at PC+2 with live counter 13224 reset
        // to zero; the host owns the complete native slot transaction.
        690 => (
            0,
            CmvsPs2aCommandEffectKind::BuildSaveSnapshot,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_478D90` (case 725): the zero-top path publishes the
        // pinned config word at byte offset 1460 as a boolean into the
        // result field at 81220; a non-zero top instead posts engine
        // message 0x8006 (only the zero-top call sites appear in the
        // recovered scenario streams). The stack words are read only for
        // the branch test, so the contract declares the full pop width
        // with no words. Returns 0x4008.
        725 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordBooleanToResult {
                source_field_offset: 1460,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_478F20` (case 723): the case-724 shape with the
        // pinned config word at byte offset 1440 as the polled source. The
        // stack words feed only the branch test, so the contract declares
        // the full pop width with no words. Returns 0x4008.
        723 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordBooleanToResult {
                source_field_offset: 1440,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_478F80` (case 724): the case-725 shape with the
        // pinned config word at byte offset 1444 as the polled source. The
        // stack words feed only the branch test, so the contract declares
        // the full pop width with no words. Returns 0x4008.
        724 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordBooleanToResult {
                source_field_offset: 1444,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479450` (case 745): the zero-top path publishes the
        // pinned interpreter word at byte offset 1580 into the result word
        // at 81220 unchanged; a non-zero top posts the engine message
        // instead (no such call site in the recovered scenario streams).
        // The stack words feed only the branch test. Returns 0x4008.
        745 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordToResult {
                source_field_offset: 1580,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4795C0` (case 736): the case-723/724 shape with the
        // pinned config word at byte offset 1476 as the polled source; the
        // stack words feed only the branch test, so the contract declares
        // the full pop width with no words. Returns 0x4008.
        736 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordBooleanToResult {
                source_field_offset: 1476,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4796D0` (case 735): the case-745 shape with the
        // pinned config word at byte offset 1472 as the polled source; the
        // raw word publishes into the result word at 81220 and the stack
        // words feed only the branch test. Returns 0x4008.
        735 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordToResult {
                source_field_offset: 1472,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479350` (case 740): the case-736 shape with the
        // pinned config word at byte offset 1468 as the polled source; the
        // stack words feed only the branch test. Returns 0x4008.
        740 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordBooleanToResult {
                source_field_offset: 1468,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479730` (case 737): the case-735 shape with the
        // pinned config word at byte offset 1488 as the polled source.
        // Returns 0x4008.
        737 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordToResult {
                source_field_offset: 1488,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4791F0` (case 743): the case-735 shape with the
        // pinned config word at byte offset 1504 as the polled source.
        // Returns 0x4008.
        743 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordToResult {
                source_field_offset: 1504,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479230` (case 744): the case-735 shape with the
        // pinned config word at byte offset 1508 as the polled source.
        // Returns 0x4008.
        744 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordToResult {
                source_field_offset: 1508,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_479310` (case 760): the case-735 shape with the
        // pinned config word at byte offset 1516 as the polled source.
        // Returns 0x4008.
        760 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordToResult {
                source_field_offset: 1516,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // 478FE0: selector zero polls TEXT_REVISION_ALPHA; otherwise
        // 478670 updates every font manager and invalidates changed caches.
        761 => (
            8,
            CmvsPs2aCommandEffectKind::NativeText {
                operation: crate::CmvsNativeTextOperation::AlternateCoverage,
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
        // Handler `sub_478DF0` (case 726): the case-736 shape with the
        // pinned config word at byte offset 1548 as the polled source.
        // Returns 0x4008.
        726 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordBooleanToResult {
                source_field_offset: 1548,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4794A0` (case 727): the case-736 shape with the
        // pinned config word at byte offset 1556 as the polled source.
        // Returns 0x4008.
        727 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordBooleanToResult {
                source_field_offset: 1556,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4798C0` (case 728): the case-736 shape with the
        // pinned config word at byte offset 1552 as the polled source.
        // Returns 0x4008.
        728 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordBooleanToResult {
                source_field_offset: 1552,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // 479180/478E50/479500/479920/4797F0: top words [mode, value].
        // Mode zero reads config 1560..1576; setters clamp signed gain
        // to 0..256 and route synchronously through 428280. Return 0x4008.
        729..=733 => (
            8,
            CmvsPs2aCommandEffectKind::NativeAudioGainSetting { command_id },
            &TWO_OPAQUE_WORDS as &[CmvsPs2aCommandStackWord],
        ),
        // Handler (case 739): the case-735 shape with the pinned config
        // word at byte offset 1484 as the polled source. Returns 0x4008.
        739 => (
            8,
            CmvsPs2aCommandEffectKind::LoadInterpreterWordToResult {
                source_field_offset: 1484,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_489B30` (case 569): publishes the guarded field
        // `[[this+3168]+0x910]` on the interpreter-referenced component
        // through `sub_48C3B0` into the result word at 81220 unchanged.
        // Returns 0x4000.
        569 => (
            0,
            CmvsPs2aCommandEffectKind::QueryComponentWordToResult {
                component_field_offset: 3168,
                field_offset: 2320,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_489A70` (case 571): the case-569 shape; the
        // argument order selects the guarded field `[[this+3168]+0x914]`
        // (2320+4) as the raw published value. Returns 0x4000.
        571 => (
            0,
            CmvsPs2aCommandEffectKind::QueryComponentWordToResult {
                component_field_offset: 3168,
                field_offset: 2324,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_489B70` (case 570): clamps the stack top to 2000 and
        // feeds the guarded component quad through `sub_48D720`; the
        // headless component table keeps the field writes internal.
        // Returns 0x4004.
        570 => (
            4,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_489AF0` (case 573): the case-569 shape; the
        // argument order selects the guarded field `[[this+3168]+0x918]`
        // (2328) as the raw published value. Returns 0x4000.
        573 => (
            0,
            CmvsPs2aCommandEffectKind::QueryComponentWordToResult {
                component_field_offset: 3168,
                field_offset: 2328,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_489BE0` (case 574): the guarded component quad plus
        // the booleanized stack top feed `sub_48D720`; the headless
        // component table keeps the writes internal. Returns 0x4004.
        574 => (
            4,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_489AB0` (case 575): the case-569 shape; the argument
        // order selects the guarded field `[[this+3168]+0x91C]` (2332),
        // booleanized into the result word. Returns 0x4000.
        575 => (
            0,
            CmvsPs2aCommandEffectKind::QueryComponentWordBooleanToResult {
                component_field_offset: 3168,
                field_offset: 2332,
                result_field_offset: 81220,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    })
}
