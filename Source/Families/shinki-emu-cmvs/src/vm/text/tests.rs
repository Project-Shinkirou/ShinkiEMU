use super::*;

fn global_string_operand(index: u32) -> crate::CmvsPs2aValueExpressionToken {
    let span = crate::CmvsPs2aSourceSpan {
        offset: 0,
        byte_length: 6,
    };
    crate::CmvsPs2aValueExpressionToken::NestedStackExpression {
        span,
        expression: Box::new(crate::CmvsPs2aStackExpression {
            span,
            nodes: vec![crate::CmvsPs2aStackExpressionNode::Value {
                span,
                opcode: 0x100,
                payload: index,
            }],
        }),
    }
}

fn assign_global_string(
    state: &mut CmvsPs2aVmState,
    source: crate::CmvsPs2aValueExpressionToken,
) -> Result<(), CoreError> {
    // 0x201 is the value-expression instruction; its 0x170 postfix assigns.
    let span = crate::CmvsPs2aSourceSpan {
        offset: 0,
        byte_length: 2,
    };
    execute_cmvs390_frame(
        state,
        &CmvsPs2aInstructionFrame::ValueExpression(crate::CmvsPs2aValueExpression {
            span,
            tokens: vec![
                global_string_operand(5),
                source,
                crate::CmvsPs2aValueExpressionToken::Operator {
                    span,
                    opcode: 0x170,
                },
            ],
        }),
    )
    .map(|_| ())
}

fn seed_pool_bytes(state: &mut CmvsPs2aVmState, frame: u16, entries: &[(u32, &[u8])]) {
    let mut pool = state
        .frame_string_pool_bytes
        .remove(&frame)
        .unwrap_or_default();
    for (offset, value) in entries {
        let start = *offset as usize;
        if pool.len() < start {
            pool.resize(start, 0);
        }
        let end = start + value.len() + 1;
        if pool.len() < end {
            pool.resize(end, 0);
        }
        pool[start..start + value.len()].copy_from_slice(value);
        pool[start + value.len()] = 0;
    }
    state.frame_string_pool_bytes.insert(frame, pool);
}

#[test]
fn value_assignment_resolves_source_tag_before_writing_global_target() {
    let span = crate::CmvsPs2aSourceSpan {
        offset: 0,
        byte_length: 6,
    };
    for (source, expected) in [
        (
            crate::CmvsPs2aValueExpressionToken::Value {
                span,
                opcode: 0x120,
                payload: 42,
            },
            b"pool".as_slice(),
        ),
        (
            crate::CmvsPs2aValueExpressionToken::Value {
                span,
                opcode: 0x125,
                payload: 42,
            },
            b"local".as_slice(),
        ),
        (global_string_operand(42), b"global".as_slice()),
    ] {
        let mut state = CmvsPs2aVmState::new(0);
        bind(&mut state, 0, b"pool");
        state.process_string_slots.insert(42, b"global".to_vec());
        state.process_string_slots.insert(5, b"old".to_vec());
        let mut data = vec![0; 42];
        data.extend_from_slice(b"local\0");
        state.install_script_data(0, &data).unwrap();
        assign_global_string(&mut state, source).unwrap();
        assert_eq!(state.process_string_slots[&5], expected);
        assert_eq!(state.process_string_slots[&42], b"global");
        let restored: CmvsPs2aVmState =
            postcard::from_bytes(&postcard::to_allocvec(&state).unwrap()).unwrap();
        assert_eq!(restored.process_string_slots[&5], expected);
    }
}

#[test]
fn string_reductions_build_owned_cp932_names_for_ordinary_and_reload_consumers() {
    let span = crate::CmvsPs2aSourceSpan {
        offset: 0,
        byte_length: 2,
    };
    let value = |opcode, payload| crate::CmvsPs2aValueExpressionToken::Value {
        span,
        opcode,
        payload,
    };
    let operator = |opcode| crate::CmvsPs2aValueExpressionToken::Operator { span, opcode };
    for binary in [0x160, 0x163, 0x172] {
        let mut state = CmvsPs2aVmState::new(0);
        state.current_frame = 2;
        let (prefix, _, malformed) = encoding_rs::SHIFT_JIS.encode("画像-");
        assert!(!malformed);
        state.process_string_slots.insert(11, prefix.into_owned());
        state.process_string_slots.insert(12, b"variant-".to_vec());
        state.install_script_data(2, b"01\0").unwrap();
        seed_pool_bytes(&mut state, 0, &[(42, b"wrong.pb3")]);
        seed_pool_bytes(&mut state, 2, &[(42, b".pb3")]);
        execute_cmvs390_frame(
            &mut state,
            &CmvsPs2aInstructionFrame::ValueExpression(crate::CmvsPs2aValueExpression {
                span,
                tokens: vec![
                    global_string_operand(11),
                    global_string_operand(11),
                    global_string_operand(12),
                    operator(binary),
                    value(0x125, 0),
                    operator(binary),
                    value(0x120, 42),
                    operator(binary),
                    operator(0x170),
                ],
            }),
        )
        .unwrap();
        state.process_string_slots.insert(12, b"changed".to_vec());
        push_word(&mut state, 20).unwrap();
        command(&mut state, 32).unwrap();
        push_word(&mut state, 0x8000_000b).unwrap();
        push_word(&mut state, 20).unwrap();
        assert_eq!(
            command(&mut state, 48).unwrap(),
            Some(CmvsPs2aVmAction::LoadTextureParentResource {
                parent_slot: 20,
                resource: "画像-variant-01.pb3".into(),
            }),
        );
        for word in [0, 250, 0x8000_000b, 20] {
            push_word(&mut state, word).unwrap();
        }
        assert!(matches!(
            command(&mut state, 708).unwrap(),
            Some(CmvsPs2aVmAction::ReloadTextureParentImage {
                parent_slot: 20,
                resource,
                bank: CmvsResourceSearchBank::Texture,
                ..
            }) if resource == "画像-variant-01.pb3"
        ));
    }
}

#[test]
fn chained_string_assignments_preserve_the_reduced_value_for_resource_consumers() {
    let mut state = CmvsPs2aVmState::new(0);
    let span = crate::CmvsPs2aSourceSpan {
        offset: 0,
        byte_length: 2,
    };
    state.process_string_slots.insert(11, b"image-".to_vec());
    state
        .process_string_slots
        .insert(12, b"variant.pb3".to_vec());
    execute_cmvs390_frame(
        &mut state,
        &CmvsPs2aInstructionFrame::ValueExpression(crate::CmvsPs2aValueExpression {
            span,
            tokens: vec![
                global_string_operand(6),
                global_string_operand(5),
                global_string_operand(11),
                global_string_operand(12),
                crate::CmvsPs2aValueExpressionToken::Operator {
                    span,
                    opcode: 0x163,
                },
                crate::CmvsPs2aValueExpressionToken::Operator {
                    span,
                    opcode: 0x170,
                },
                crate::CmvsPs2aValueExpressionToken::Operator {
                    span,
                    opcode: 0x170,
                },
            ],
        }),
    )
    .unwrap();
    push_word(&mut state, 20).unwrap();
    command(&mut state, 32).unwrap();
    for slot in [5, 6] {
        push_word(&mut state, 0x8000_0000 | slot).unwrap();
        push_word(&mut state, 20).unwrap();
        assert_eq!(
            command(&mut state, 48).unwrap(),
            Some(CmvsPs2aVmAction::LoadTextureParentResource {
                parent_slot: 20,
                resource: "image-variant.pb3".into(),
            }),
        );
    }
}

#[test]
fn value_assignment_past_pool_reads_empty_and_replaces_target() {
    let span = crate::CmvsPs2aSourceSpan {
        offset: 0,
        byte_length: 6,
    };
    // `sub_470D50` tag 0 never bounds-checks: an offset past the pool
    // reads empty bytes and the assignment writes the empty value.
    {
        let (tag, payload) = (0x120, 0x3fff_ffff_u32);
        let mut state = CmvsPs2aVmState::new(0);
        bind(&mut state, 0, b"pool");
        state.install_script_data(0, b"short\0").unwrap();
        state.process_string_slots.insert(5, b"keep".to_vec());
        assign_global_string(
            &mut state,
            crate::CmvsPs2aValueExpressionToken::Value {
                span,
                opcode: tag,
                payload,
            },
        )
        .unwrap();
        assert_eq!(state.process_string_slots[&5], b"");
        assert!(!state.execution_failed);
    }
}

#[test]
fn value_assignment_reads_mid_pool_offsets_without_string_starts() {
    let span = crate::CmvsPs2aSourceSpan {
        offset: 0,
        byte_length: 6,
    };
    let mut state = CmvsPs2aVmState::new(0);
    bind(&mut state, 0, b"pool");
    state.install_script_data(0, b"short\0").unwrap();
    state.process_string_slots.insert(5, b"keep".to_vec());
    assign_global_string(
        &mut state,
        crate::CmvsPs2aValueExpressionToken::Value {
            span,
            opcode: 0x120,
            payload: 43,
        },
    )
    .unwrap();
    assert_eq!(state.process_string_slots[&5], b"ool");
}

fn bind(state: &mut CmvsPs2aVmState, frame: u16, bytes: &[u8]) {
    state
        .frame_string_values
        .insert(frame, BTreeMap::from([(42, bytes.to_vec())]));
    seed_pool_bytes(state, frame, &[(42, bytes)]);
}
fn command(state: &mut CmvsPs2aVmState, id: u16) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    execute_cmvs390_frame(
        state,
        &CmvsPs2aInstructionFrame::Command {
            span: crate::CmvsPs2aSourceSpan {
                offset: 0,
                byte_length: 2,
            },
            command_id: id,
        },
    )
}

#[test]
fn native_save_title_uses_cp932_byte_bounds_and_preserves_errors_and_old_title() {
    let mut state = CmvsPs2aVmState::new(0);
    state.write_interpreter_word(10532, 0x20);
    for text in [
        String::new(),
        "a".repeat(127),
        format!("{}a", "あ".repeat(63)),
    ] {
        let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(&text);
        assert!(!invalid);
        bind(&mut state, 0, &bytes);
        push_word(&mut state, 42).unwrap();
        assert_eq!(command(&mut state, 354).unwrap(), None);
        assert_eq!(
            state.interpreter_string_buffers[&10324],
            vec![CmvsStringSegment::OwnedString(text)]
        );
        assert_eq!(state.interpreter_words[&10532], 0x20);
    }
    let previous = state.interpreter_string_buffers[&10324].clone();
    for text in ["a".repeat(128), "あ".repeat(64)] {
        let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(&text);
        assert!(!invalid);
        state.process_string_slots.insert(2, bytes.into_owned());
        push_word(&mut state, 0x8000_0002).unwrap();
        assert_eq!(command(&mut state, 354).unwrap(), None);
        assert_eq!(state.interpreter_string_buffers[&10324], previous);
        assert_eq!(state.interpreter_words[&10532], 0x28);
    }
    let restored: CmvsPs2aVmState =
        postcard::from_bytes(&postcard::to_allocvec(&state).unwrap()).unwrap();
    assert_eq!(restored.interpreter_string_buffers[&10324], previous);
    assert_eq!(restored.interpreter_words[&10532], 0x28);
}
#[test]
fn global_assignment_copies_bytes_before_slot_and_script_replacement() {
    let mut state = CmvsPs2aVmState::new(0);
    bind(&mut state, 0, b"first.ps3");
    append_value_entry_to_buffer(&mut state, 0x120, 42).unwrap();
    materialize_message_buffer(&mut state, 0x121, 0).unwrap();
    state.message_buffer.clear();
    append_value_entry_to_buffer(&mut state, 0x121, 0).unwrap();
    materialize_message_buffer(&mut state, 0x121, 1).unwrap();
    bind(&mut state, 0, b"replacement.ps3");
    state
        .process_string_slots
        .insert(0, b"changed.ps3".to_vec());
    state.current_frame = 3;
    push_word(&mut state, 0x8000_0001).unwrap();
    assert_eq!(
        command(&mut state, 128).unwrap(),
        Some(CmvsPs2aVmAction::ReloadRootScript {
            name: "first.ps3".into()
        })
    );
    assert_eq!(state.current_frame, 0);
}

#[test]
fn caption_composes_native_owned_prefix_gap_and_suffix_after_restore() {
    let mut state = CmvsPs2aVmState::new(0);
    state.process_string_slots.insert(1, b"Title".to_vec());
    push_word(&mut state, 0x8000_0001).unwrap();
    command(&mut state, 352).unwrap();
    for (charset, gap) in [(1, "\u{3000}"), (0, "  ")] {
        state
            .process_global_words
            .insert(NATIVE_FONT_SHIFT_JIS, charset);
        for suffix in ["chapter", ""] {
            state
                .process_string_slots
                .insert(2, suffix.as_bytes().to_vec());
            push_word(&mut state, 0x8000_0002).unwrap();
            assert_eq!(
                command(&mut state, 353).unwrap(),
                Some(CmvsPs2aVmAction::SetWindowCaption {
                    segments: vec![CmvsStringSegment::OwnedString(format!(
                        "Title{gap}{suffix}"
                    ))]
                })
            );
            assert_eq!(
                state.interpreter_string_buffers[&12816],
                vec![CmvsStringSegment::OwnedString(suffix.into())]
            );
            state = postcard::from_bytes(&postcard::to_allocvec(&state).unwrap()).unwrap();
            assert_eq!(state.process_global_words[&NATIVE_FONT_SHIFT_JIS], charset);
        }
    }
    assert_eq!(
        state.interpreter_string_buffers[&288],
        vec![CmvsStringSegment::OwnedString("Title".into())]
    );
}

#[test]
fn caption_rejects_native_byte_overflow_without_replacing_suffix() {
    for suffix in ["a".repeat(125), format!("{}a", "あ".repeat(62))] {
        let mut state = CmvsPs2aVmState::new(0);
        state
            .interpreter_string_buffers
            .insert(288, vec![CmvsStringSegment::OwnedString(String::new())]);
        let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(&suffix);
        assert!(!invalid);
        bind(&mut state, 0, &bytes);
        push_word(&mut state, 42).unwrap();
        assert!(command(&mut state, 353).unwrap().is_some());
        let previous = state.interpreter_string_buffers[&12816].clone();
        let mut too_long = bytes.into_owned();
        too_long.push(b'x');
        bind(&mut state, 0, &too_long);
        push_word(&mut state, 42).unwrap();
        assert_eq!(
            command(&mut state, 353).unwrap_err().code(),
            "SHINKI_EMU_CMVS_WINDOW_CAPTION"
        );
        assert_eq!(state.interpreter_string_buffers[&12816], previous);
    }
}
#[test]
fn call_copies_name_before_switching_to_destination_frame() {
    let mut state = CmvsPs2aVmState::new(0);
    bind(&mut state, 3, b"caller.ps3");
    bind(&mut state, 1, b"wrong.ps3");
    state.current_frame = 3;
    push_word(&mut state, 42).unwrap();
    push_word(&mut state, 0).unwrap();
    assert_eq!(
        command(&mut state, 129).unwrap(),
        Some(CmvsPs2aVmAction::CallScript {
            frame: 1,
            name: "caller.ps3".into()
        })
    );
    assert_eq!(state.current_frame, 1);
}
#[test]
fn native_slots_reject_overflow_nul_and_invalid_encoding_without_overwrite() {
    for bytes in [vec![b'x'; 1024], vec![0], vec![0x81]] {
        let mut state = CmvsPs2aVmState::new(0);
        state.process_string_slots.insert(0, b"safe".to_vec());
        state.message_buffer = bytes;
        assert!(materialize_message_buffer(&mut state, 0x121, 0).is_err());
        assert_eq!(state.process_string_slots[&0], b"safe");
    }
}
#[test]
fn slot_concatenation_and_length_use_copied_native_bytes() {
    let mut state = CmvsPs2aVmState::new(0);
    state.process_string_slots.insert(0, b"chapter".to_vec());
    bind(&mut state, 0, b".ps3");
    append_value_entry_to_buffer(&mut state, 0x121, 0).unwrap();
    append_value_entry_to_buffer(&mut state, 0x120, 42).unwrap();
    materialize_message_buffer(&mut state, 0x121, 1).unwrap();
    assert_eq!(
        resolve_resource_name(&state, 0x8000_0001).unwrap(),
        "chapter.ps3"
    );
    assert_eq!(
        resolve_tagged_string_length(&state, 0x8000_0001).unwrap(),
        11
    );
}
#[test]
fn restore_retains_slot_values_without_serializing_script_pool() {
    let mut state = CmvsPs2aVmState::new(0);
    bind(&mut state, 0, b"private-pool.ps3");
    state.process_string_slots.insert(0, b"saved.ps3".to_vec());
    let encoded = postcard::to_allocvec(&state).unwrap();
    assert!(
        !encoded
            .windows(b"private-pool.ps3".len())
            .any(|s| s == b"private-pool.ps3")
    );
    let restored: CmvsPs2aVmState = postcard::from_bytes(&encoded).unwrap();
    assert_eq!(
        resolve_resource_name(&restored, 0x8000_0000).unwrap(),
        "saved.ps3"
    );
    assert!(resolve_resource_name(&restored, 42).is_err());
}

#[test]
fn legacy_and_unknown_snapshot_versions_are_rejected_at_the_prefix() {
    let mut payload = postcard::to_allocvec(&CmvsPs2aVmState::new(0)).unwrap();
    // Use a complete valid payload, so this cannot pass merely because
    // the state following an otherwise accepted version is truncated.
    for version in (0..=38).chain([40, 127]) {
        payload[0] = version;
        assert!(postcard::from_bytes::<CmvsPs2aVmState>(&payload).is_err());
    }
    let mut state = CmvsPs2aVmState::new(0);
    state.process_string_slots.insert(128, b"invalid".to_vec());
    assert!(validate_cmvs390_vm_state(&state).is_err());
    state.process_string_slots.clear();
    state.process_string_slots.insert(0, vec![0x81]);
    assert!(validate_cmvs390_vm_state(&state).is_err());
}

#[test]
fn configured_path_is_owned_across_script_and_global_replacement() {
    let mut state = CmvsPs2aVmState::new(0);
    bind(&mut state, 0, b"save\\");
    state.process_string_slots.insert(1, b"save\\".to_vec());
    push_word(&mut state, 0x8000_0001).unwrap();
    command(&mut state, 22).unwrap();
    bind(&mut state, 0, b"different\\");
    state.process_string_slots.insert(1, b"other\\".to_vec());
    state.current_frame = 2;
    let restored: CmvsPs2aVmState =
        postcard::from_bytes(&postcard::to_allocvec(&state).unwrap()).unwrap();
    assert_eq!(
        restored.interpreter_string_buffers[&5404],
        vec![
            CmvsStringSegment::InterpreterPrefixField { field_offset: 7452 },
            CmvsStringSegment::OwnedString("save\\".into()),
        ]
    );
}

#[test]
fn unsafe_path_does_not_replace_the_previous_owned_buffer() {
    let mut state = CmvsPs2aVmState::new(0);
    bind(&mut state, 0, b"save/");
    push_word(&mut state, 42).unwrap();
    command(&mut state, 22).unwrap();
    let previous = state.interpreter_string_buffers[&5404].clone();
    bind(&mut state, 0, b"../outside/");
    push_word(&mut state, 42).unwrap();
    assert_eq!(
        command(&mut state, 22).unwrap_err().code(),
        "SHINKI_EMU_CMVS_RESOURCE_PATH"
    );
    assert_eq!(state.interpreter_string_buffers[&5404], previous);
}
#[test]
fn missing_or_external_pool_never_falls_back_to_a_different_frame() {
    let mut state = CmvsPs2aVmState::new(0);
    bind(&mut state, 1, b"wrong.ps3");
    assert!(resolve_resource_name(&state, 42).is_err());
    assert!(resolve_resource_name(&state, 0xc000_002a).is_err());
    assert!(resolve_resource_name(&state, 0x8000_0000).is_err());
    assert!(resolve_tagged_string_bytes(&state, 0x8000_0080).is_err());
}

#[test]
fn native_range_copy_reads_source_once_even_when_overwriting_it() {
    let mut state = CmvsPs2aVmState::new(0);
    state.process_string_slots.insert(0, b"copied".to_vec());
    store_process_string_range(&mut state, 0, 3, 0x8000_0000).unwrap();
    state.process_string_slots.insert(0, b"changed".to_vec());
    assert_eq!(state.process_string_slots[&1], b"copied");
    assert_eq!(state.process_string_slots[&2], b"copied");
}

#[test]
fn data_string_domain_reads_mutable_segment_and_preserves_partial_tail() {
    let mut state = CmvsPs2aVmState::new(0);
    state.install_script_data(0, b"name\0x\0").unwrap();
    assert_eq!(resolve_resource_name(&state, 0x4000_0000).unwrap(), "name");
    assert_eq!(resolve_resource_name(&state, 0x4000_0005).unwrap(), "x");
    append_value_entry_to_buffer(&mut state, 0x125, 0).unwrap();
    materialize_message_buffer(&mut state, 0x121, 0).unwrap();
    state
        .script_data_segment_words
        .get_mut(&0)
        .unwrap()
        .insert(0, u32::from_le_bytes(*b"edit"));
    assert_eq!(resolve_resource_name(&state, 0x4000_0000).unwrap(), "edit");
    assert_eq!(resolve_resource_name(&state, 0x8000_0000).unwrap(), "name");
    let restored: CmvsPs2aVmState =
        postcard::from_bytes(&postcard::to_allocvec(&state).unwrap()).unwrap();
    assert_eq!(resolve_resource_name(&restored, 0x4000_0005).unwrap(), "x");
    state.install_script_data(0, b"unterminated").unwrap();
    assert!(resolve_resource_name(&state, 0x4000_0000).is_err());
}

#[test]
fn native_search_configuration_is_owned_isolated_and_restorable() {
    let mut state = CmvsPs2aVmState::new(0);
    for (id, bank, path) in [
        (16, CmvsResourceSearchBank::Script, "scripts.cpz"),
        (17, CmvsResourceSearchBank::Texture, "image.cpz"),
        (18, CmvsResourceSearchBank::Native0d0c, "voice.cpz"),
        (19, CmvsResourceSearchBank::CrossfadeAudio, "audio.cpz"),
        (20, CmvsResourceSearchBank::Native0d14, "other.cpz"),
        (23, CmvsResourceSearchBank::Native0d00, "second.cpz"),
        (24, CmvsResourceSearchBank::Native0d04, "third.cpz"),
        (26, CmvsResourceSearchBank::Native0d18, "fourth.cpz"),
    ] {
        state
            .process_string_slots
            .insert(0, path.as_bytes().to_vec());
        push_word(&mut state, 0x8000_0000).unwrap();
        push_word(&mut state, 0).unwrap();
        assert_eq!(command(&mut state, id).unwrap(), None);
        assert_eq!(state.resource_search_paths[&bank][&0], path);
    }
    state.process_string_slots.clear();
    state.current_frame = 1;
    let bytes = postcard::to_allocvec(&state).unwrap();
    let restored: CmvsPs2aVmState = postcard::from_bytes(&bytes).unwrap();
    validate_cmvs390_vm_state(&restored).unwrap();
    assert_eq!(restored.resource_search_paths, state.resource_search_paths);
    assert_eq!(restored.resource_search_paths.len(), 8);
}

#[test]
fn invalid_search_path_preserves_previous_binding_and_invalid_slot_is_native_noop() {
    let mut state = CmvsPs2aVmState::new(0);
    state.resource_search_paths.insert(
        CmvsResourceSearchBank::Script,
        BTreeMap::from([(0, "first.cpz".into())]),
    );
    state
        .process_string_slots
        .insert(0, b"../escape.cpz".to_vec());
    push_word(&mut state, 0x8000_0000).unwrap();
    push_word(&mut state, 0).unwrap();
    assert_eq!(
        command(&mut state, 16).unwrap_err().code(),
        "SHINKI_EMU_CMVS_RESOURCE_PATH"
    );
    assert_eq!(
        state.resource_search_paths[&CmvsResourceSearchBank::Script][&0],
        "first.cpz"
    );
    let mut state = CmvsPs2aVmState::new(0);
    push_word(&mut state, 0xc000_0000).unwrap();
    push_word(&mut state, 5).unwrap();
    assert_eq!(command(&mut state, 16).unwrap(), None);
    assert!(state.resource_search_paths.is_empty());
    state.resource_search_paths.insert(
        CmvsResourceSearchBank::Script,
        BTreeMap::from([(5, "a.cpz".into())]),
    );
    assert_eq!(
        validate_cmvs390_vm_state(&state).unwrap_err().code(),
        "SHINKI_EMU_CMVS_RESOURCE_SLOT"
    );
}

#[test]
fn native_header_destinations_update_owned_slots_pools_and_unaligned_data_without_aliasing() {
    let mut vm = CmvsPs2aVmState::new(0);
    vm.frame_string_pool_bytes.insert(0, vec![0; 32]);
    vm.frame_string_lengths
        .insert(0, std::collections::BTreeMap::from([(0, 0)]));
    vm.frame_string_values
        .insert(0, std::collections::BTreeMap::from([(0, Vec::new())]));
    vm.install_script_data(0, &[b'a'; 11]).unwrap();
    vm.write_native_string_reference(0x8000_0002, b"owned")
        .unwrap();
    vm.write_native_string_reference(0, b"pool title").unwrap();
    vm.write_native_string_reference(0x4000_0003, b"data")
        .unwrap();
    assert_eq!(
        resolve_tagged_string_bytes(&vm, 0x8000_0002)
            .unwrap()
            .as_ref(),
        b"owned"
    );
    assert_eq!(
        resolve_tagged_string_bytes(&vm, 0).unwrap().as_ref(),
        b"pool title"
    );
    assert_eq!(resolve_tagged_string_length(&vm, 0).unwrap(), 10);
    assert_eq!(
        resolve_tagged_string_bytes(&vm, 0x4000_0003)
            .unwrap()
            .as_ref(),
        b"data"
    );
    assert_eq!(
        resolve_tagged_string_bytes(&vm, 0x4000_0000)
            .unwrap()
            .as_ref(),
        b"aaadata"
    );
    let previous = vm.clone();
    assert!(
        vm.write_native_string_reference(0x4000_0009, b"overflow")
            .is_err()
    );
    assert_eq!(vm, previous);
}
