use super::*;

fn command(
    state: &mut CmvsPs2aVmState,
    id: u16,
    top_first: &[u32],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    for value in top_first.iter().rev() {
        push_word(state, *value)?;
    }
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

fn fixture() -> CmvsPs2aVmState {
    let mut state = CmvsPs2aVmState::new(0);
    state.native_stage_size = [1280, 720];
    state.native_render_size = [1280, 720];
    state.frame_string_values.insert(
        0,
        BTreeMap::from([(10, b"Example Font".to_vec()), (20, b"plain text".to_vec())]),
    );
    seed_pool_bytes(&mut state, 0, &[(10, b"Example Font"), (23, b"plain text")]);
    state
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
fn text_objects_are_independent_and_font_owner_survives_surface_destruction() {
    let mut state = fixture();
    command(&mut state, 256, &[3, 10, 196]).unwrap();
    command(&mut state, 256, &[8, 10, 0]).unwrap();
    let font_owner = state.slot_objects[&(762 << 8 | 3)];
    let surface_owner = state.slot_objects[&(750 << 8 | 3)];
    command(&mut state, 261, &[3, 30, 2, 7]).unwrap();
    assert_eq!(state.native_text[&8].size, 0);
    assert_eq!(state.native_text[&3].character_spacing, 2);
    assert_eq!(state.native_text[&3].capacity_hint, 196);
    assert_eq!(
        command(&mut state, 257, &[3]).unwrap(),
        Some(CmvsPs2aVmAction::NativeText(
            CmvsNativeTextAction::Destroy { slot: 3 }
        ))
    );
    assert!(!state.slot_objects.contains_key(&(750 << 8 | 3)));
    assert_eq!(state.slot_objects[&(762 << 8 | 3)], font_owner);
    command(&mut state, 256, &[3, 10, 0]).unwrap();
    assert_ne!(state.slot_objects[&(750 << 8 | 3)], surface_owner);
    assert_eq!(state.slot_objects[&(762 << 8 | 3)], font_owner);
    assert_eq!(state.native_text[&3].size, 0);
    assert_eq!(state.native_text[&3].style, 2);
    assert_eq!(state.interpreter_words[&81220], 1);
}

#[test]
fn invalid_slots_raise_native_flag_and_missing_destroy_is_a_noop() {
    let mut state = fixture();
    assert_eq!(command(&mut state, 257, &[4]).unwrap(), None);
    command(&mut state, 261, &[4, 30, 0, 0]).unwrap();
    assert_eq!(state.interpreter_flag_words[&10532] & 0x1000, 0x1000);
    assert!(state.native_text.is_empty());
    command(&mut state, 256, &[12, 10, 0]).unwrap();
    assert!(state.native_text.is_empty());
    assert!(state.native_text_fonts.is_empty());
    assert_eq!(state.stack_cursor_bytes, 0);
}

#[test]
fn metrics_and_colors_preserve_native_signed_sentinels_and_cursor_rules() {
    let mut state = fixture();
    command(&mut state, 256, &[0, 10, 0]).unwrap();
    command(&mut state, 259, &[0, 44, 468, 512, 30]).unwrap();
    command(&mut state, 261, &[0, 30, 2, 4]).unwrap();
    command(&mut state, 263, &[0, 0x00112233, 0x00445566]).unwrap();
    command(&mut state, 261, &[0, 0, u32::MAX, u32::MAX]).unwrap();
    command(&mut state, 263, &[0, u32::MAX, 0x00102030]).unwrap();
    assert_eq!(state.native_text[&0].size, 30);
    assert_eq!(state.native_text[&0].character_spacing, 2);
    assert_eq!(state.native_text[&0].line_spacing, 4);
    assert_eq!(state.native_text[&0].colors, [0x00112233, 0x00102030]);
    command(&mut state, 268, &[0, 90, 510]).unwrap();
    assert_eq!(state.native_text[&0].cursor, [90, 510]);
    command(&mut state, 260, &[0]).unwrap();
    assert_eq!(state.native_text[&0].cursor, [44, 468]);
    command(&mut state, 264, &[0, 0]).unwrap();
    assert!(!state.native_text[&0].kinsoku);
}

#[test]
fn text_and_font_values_are_copied_before_script_or_global_changes() {
    let mut state = fixture();
    command(&mut state, 256, &[5, 10, 0]).unwrap();
    command(&mut state, 259, &[5, 0, 0, 512, 23]).unwrap();
    command(&mut state, 261, &[5, 23, 0, 0]).unwrap();
    let action = command(&mut state, 267, &[5, 23]).unwrap();
    state.process_string_slots.insert(2, b"Other Font".to_vec());
    command(&mut state, 262, &[5, 0x80000002]).unwrap();
    state.process_string_slots.clear();
    state.frame_string_values.clear();
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::NativeText(
            CmvsNativeTextAction::DrawStatic {
                slot: 5,
                source: "plain text".into()
            }
        ))
    );
    let restored: CmvsPs2aVmState =
        postcard::from_bytes(&postcard::to_allocvec(&state).unwrap()).unwrap();
    assert_eq!(restored.native_text[&5].font, "Other Font");
    assert_eq!(restored.native_text_fonts[&5], "Other Font");
}

#[test]
fn overlong_font_does_not_replace_existing_object() {
    let mut state = fixture();
    command(&mut state, 256, &[1, 10, 0]).unwrap();
    let original = state.slot_objects[&(750 << 8 | 1)];
    state
        .frame_string_values
        .get_mut(&0)
        .unwrap()
        .insert(10, vec![b'x'; 64]);
    state.frame_string_pool_bytes.remove(&0);
    seed_pool_bytes(&mut state, 0, &[(10, &[b'x'; 64])]);
    assert_eq!(
        command(&mut state, 256, &[1, 10, 0]).unwrap_err().code(),
        "SHINKI_EMU_CMVS_TEXT_FONT"
    );
    assert_eq!(state.slot_objects[&(750 << 8 | 1)], original);
    assert_eq!(state.native_text[&1].font, "Example Font");
}

#[test]
fn snapshot_rejects_disconnected_font_owners_and_invalid_color_words() {
    let mut state = fixture();
    command(&mut state, 256, &[2, 10, 0]).unwrap();
    assert!(validate_cmvs390_vm_state(&state).is_ok());
    let owner = state.slot_objects.remove(&(762 << 8 | 2)).unwrap();
    assert!(validate_cmvs390_vm_state(&state).is_err());
    state.slot_objects.insert(762 << 8 | 2, owner);
    state.native_text.get_mut(&2).unwrap().colors[0] = u32::MAX;
    assert!(validate_cmvs390_vm_state(&state).is_err());
}

#[test]
fn native_surface_translation_and_gradient_mode_are_independent_of_cursor() {
    let mut state = fixture();
    command(&mut state, 256, &[7, 10, 0]).unwrap();
    command(&mut state, 259, &[7, 44, 60, 512, 30]).unwrap();
    command(&mut state, 258, &[7, (-10i32) as u32, 25]).unwrap();
    command(&mut state, 275, &[7, 3]).unwrap();
    assert_eq!(state.native_text[&7].position, [-10, 25]);
    assert_eq!(state.native_text[&7].cursor, [44, 60]);
    assert_eq!(state.native_text[&7].color_mode, 3);
}

#[test]
fn native_bold_changes_only_future_uploaded_glyphs_and_survives_private_restore() {
    let mut state = fixture();
    command(&mut state, 256, &[0, 10, 196]).unwrap();
    command(&mut state, 259, &[0, 0, 0, 1000, 100]).unwrap();
    command(&mut state, 261, &[0, 27, 0, 0]).unwrap();
    layout::append(state.native_text.get_mut(&0).unwrap(), "A", None, None).unwrap();
    state.stamp_native_text_glyphs(0, 0, Ok).unwrap();
    let first = state.native_text[&0].glyphs[0].clone();
    assert!(!first.raster.bold);
    assert_eq!(state.native_text[&0].style, 2);
    command(&mut state, 280, &[0, 1]).unwrap();
    let generation = state.native_font_owners[&762].raster.generation;
    command(&mut state, 280, &[0, 7]).unwrap();
    assert_eq!(state.native_font_owners[&762].raster.generation, generation);
    layout::append(state.native_text.get_mut(&0).unwrap(), "A", None, None).unwrap();
    state.stamp_native_text_glyphs(0, 1, Ok).unwrap();
    assert_eq!(state.native_text[&0].glyphs[0], first);
    assert!(state.native_text[&0].glyphs[1].raster.bold);
    assert!(generation > first.raster.generation);
    assert_eq!(state.native_text[&0].style, 2);
    let mut restored: CmvsPs2aVmState =
        postcard::from_bytes(&postcard::to_allocvec(&state).unwrap()).unwrap();
    assert_eq!(restored.native_font_owners, state.native_font_owners);
    assert_eq!(restored.native_text, state.native_text);
    validate(&restored).unwrap();
    // The host rebinds program pools after restoring private VM state.
    seed_pool_bytes(&mut restored, 0, &[(10, b"Example Font")]);
    let font_owner = restored.slot_objects[&(762 << 8)];
    command(&mut restored, 257, &[0]).unwrap();
    command(&mut restored, 256, &[0, 10, 196]).unwrap();
    assert_eq!(restored.slot_objects[&(762 << 8)], font_owner);
    assert!(restored.native_text[&0].bold);
    assert_eq!(
        restored.native_font_owners[&762].raster.generation,
        generation
    );
}

#[test]
fn native_mode940_retains_cached_character_size_until_real_owner_invalidation() {
    let mut state = fixture();
    command(&mut state, 256, &[0, 10, 196]).unwrap();
    let first = state.native_text_raster(0, 0x41, 27).unwrap();
    assert!(!first.proportional);
    command(&mut state, 940, &[1]).unwrap();
    assert_eq!(state.native_text_raster(0, 0x41, 27).unwrap(), first);
    assert!(state.native_text_raster(0, 0x42, 27).unwrap().proportional);
    assert!(state.native_text_raster(0, 0x41, 28).unwrap().proportional);
    command(&mut state, 761, &[1, 1]).unwrap();
    let alternate = state.native_text_raster(0, 0x41, 27).unwrap();
    assert!(alternate.proportional && alternate.alternate_coverage);
    assert!(alternate.generation > first.generation);
    command(&mut state, 761, &[1, 1]).unwrap();
    assert_eq!(state.native_text_raster(0, 0x41, 27).unwrap(), alternate);
    command(&mut state, 761, &[0, 0]).unwrap();
    assert_eq!(state.read_interpreter_word(81220).unwrap(), 1);
}

#[test]
fn standalone_glyph_font_owner_is_bound_by_real_constructor_and_persists_after_destroy() {
    let mut state = fixture();
    state.native_stage_size = [1280, 720];
    state.native_render_size = [1280, 720];
    command(&mut state, 896, &[7, 10, 27, 0]).unwrap();
    let owner = state.slot_objects[&(783 << 8 | 7)];
    assert_eq!(state.native_glyphs[&7].capacity_hint, 512);
    command(&mut state, 916, &[7, 1]).unwrap();
    assert!(state.native_glyphs[&7].bold);
    assert!(state.native_font_owners[&(783 + 7)].raster.bold);
    assert!(state.native_glyph_raster(7, 0x41, 27).unwrap().bold);
    command(&mut state, 898, &[7]).unwrap();
    assert!(!state.native_glyphs.contains_key(&7));
    command(&mut state, 897, &[7, 10, 40, 196, 1280, 720]).unwrap();
    assert_eq!(state.slot_objects[&(783 << 8 | 7)], owner);
    assert!(state.native_glyphs[&7].bold);
    command(&mut state, 916, &[8, 1]).unwrap();
    assert_ne!(state.interpreter_flag_words[&10532] & 0x1000, 0);
}

#[test]
fn default_family_exchange_preserves_existing_managers_and_uploaded_font_names() {
    let mut state = fixture();
    state.process_string_slots.insert(0, Vec::new());
    command(&mut state, 256, &[0, 0x8000_0000, 196]).unwrap();
    command(&mut state, 259, &[0, 0, 0, 1000, 100]).unwrap();
    command(&mut state, 261, &[0, 27, 0, 0]).unwrap();
    layout::append(state.native_text.get_mut(&0).unwrap(), "A", None, None).unwrap();
    state.stamp_native_text_glyphs(0, 0, Ok).unwrap();
    let first = state.native_text[&0].glyphs[0].clone();
    command(&mut state, 734, &[1, 10]).unwrap();
    assert_eq!(
        state.native_font_owners[&762].family,
        crate::archive::DEFAULT_NATIVE_FONT
    );
    assert_eq!(state.native_text[&0].glyphs[0], first);
    command(&mut state, 734, &[0, 0x8000_0000]).unwrap();
    assert_eq!(state.process_string_slots[&0], b"Example Font");
    state.process_string_slots.insert(0, Vec::new());
    command(&mut state, 262, &[0, 0x8000_0000]).unwrap();
    assert_eq!(state.native_font_owners[&762].family, "Example Font");
    layout::append(state.native_text.get_mut(&0).unwrap(), "A", None, None).unwrap();
    state.stamp_native_text_glyphs(0, 1, Ok).unwrap();
    assert_eq!(state.native_text[&0].glyphs[0].font, first.font);
    assert_eq!(state.native_text[&0].glyphs[1].font, "Example Font");
}
