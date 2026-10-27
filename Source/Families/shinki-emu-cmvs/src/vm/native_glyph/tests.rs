use super::*;

fn command(
    state: &mut CmvsPs2aVmState,
    id: u16,
    words: &[u32],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    for word in words.iter().rev() {
        push_word(state, *word)?;
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

fn vm() -> CmvsPs2aVmState {
    let mut state = CmvsPs2aVmState::new(0);
    state.native_stage_size = [1280, 720];
    state.native_render_size = [1920, 1080];
    state
        .process_string_slots
        .insert(0, b"Example Font".to_vec());
    state.process_string_slots.insert(1, b"AB\\nC".to_vec());
    state
}

fn create(state: &mut CmvsPs2aVmState) {
    assert_eq!(
        command(state, 896, &[0, 0x80000000, 20, 64]).unwrap(),
        Some(CmvsPs2aVmAction::NativeText(
            CmvsNativeTextAction::GlyphCreate { slot: 0 }
        ))
    );
}

#[test]
fn constructor_captures_base_mode_dimensions_and_real_kind5_atlas() {
    let mut state = vm();
    create(&mut state);
    let graph = &state.native_glyphs[&0];
    assert_eq!(graph.physical_size, [1920, 1080]);
    assert_eq!(
        graph.viewport.map(f32::from_bits),
        [0.0, 0.0, 1280.0, 720.0]
    );
    assert_eq!(
        (graph.atlas.cell_size, graph.atlas.width, graph.atlas.height),
        (36, 512, 512)
    );
    assert_eq!(graph.atlas.occupancy, vec![0; 1024]);
    assert_eq!(graph.root.basic_kind, 5);
    assert_eq!(graph.root.rect_words, Some([0, 0, 4, 4]));
    assert_eq!(graph.root.reference_size, Some([1920, 1080]));
    assert_eq!(state.interpreter_words[&81220], 1);
}

#[test]
fn position_cursor_layout_and_viewport_remain_independent_native_float_words() {
    let mut state = vm();
    create(&mut state);
    command(&mut state, 899, &[0, (-3i32) as u32, 7]).unwrap();
    assert_eq!(
        state.native_glyphs[&0].position.map(f32::from_bits),
        [-3.0, 7.0]
    );
    assert_eq!(
        state.native_glyphs[&0].root.float_position_words[..2],
        [-4.5, 10.5].map(f32::to_bits)
    );
    command(
        &mut state,
        900,
        &[0, 1.25f32.to_bits(), (-2.5f32).to_bits()],
    )
    .unwrap();
    command(&mut state, 901, &[0, 4, 8, 300, 100]).unwrap();
    command(&mut state, 918, &[0, 11, 22, 33, 44]).unwrap();
    let graph = &state.native_glyphs[&0];
    assert_eq!(graph.position.map(f32::from_bits), [1.25, -2.5]);
    assert_eq!(graph.cursor.map(f32::from_bits), [4.0, 8.0]);
    assert_eq!(graph.rect.map(f32::from_bits), [4.0, 8.0, 300.0, 100.0]);
    assert_eq!(graph.viewport.map(f32::from_bits), [11.0, 22.0, 33.0, 44.0]);
    command(&mut state, 908, &[0, 7, 9]).unwrap();
    command(&mut state, 909, &[0, 0.5f32.to_bits(), 1.5f32.to_bits()]).unwrap();
    assert_eq!(
        state.native_glyphs[&0].cursor.map(f32::from_bits),
        [0.5, 1.5]
    );
}

#[test]
fn descriptor_controls_preserve_raw_style_outline_and_high_byte_color_sentinels() {
    let mut state = vm();
    create(&mut state);
    command(&mut state, 903, &[0, 0, u32::MAX, 3]).unwrap();
    command(&mut state, 905, &[0, 0, 0x800000ff]).unwrap();
    command(&mut state, 906, &[0, 0]).unwrap();
    command(&mut state, 910, &[0, 77]).unwrap();
    command(&mut state, 911, &[0, u32::MAX]).unwrap();
    command(&mut state, 914, &[0, 0xfeedbeef]).unwrap();
    command(&mut state, 915, &[0, 0x83123456]).unwrap();
    let graph = &state.native_glyphs[&0];
    assert_eq!(graph.size, 20);
    assert_eq!(graph.gaps.map(f32::from_bits), [0.0, 3.0]);
    assert_eq!(graph.colors, [0, 0xffffff]);
    assert!(!graph.kinsoku);
    assert_eq!(graph.color_mode, 77);
    assert_eq!(graph.draw_order, -1);
    assert_eq!(graph.root.auxiliary_word, Some(u32::MAX));
    assert_eq!(graph.style, 0xfeedbeef);
    assert_eq!(graph.outline_color, 0x83123456);
}

#[test]
fn append_host_action_keeps_resolved_native_bytes_without_painting_or_relayout() {
    let mut state = vm();
    create(&mut state);
    state.process_string_slots.insert(1, vec![0xa6, 0x82, 0xa0]);
    let action = command(&mut state, 907, &[0, 0x80000001]).unwrap();
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::NativeText(
            CmvsNativeTextAction::GlyphAppend {
                slot: 0,
                source: vec![0xa6, 0x82, 0xa0],
            }
        ))
    );
    assert!(state.native_glyphs[&0].records.is_empty());
}

#[test]
fn measuring_publishes_native_halfwidth_words_without_81228_or_cursor_changes() {
    let mut state = vm();
    create(&mut state);
    command(&mut state, 901, &[0, 0, 0, 1000, 100]).unwrap();
    state.write_interpreter_word(81228, 0xdeadbeef);
    command(&mut state, 912, &[0, 0x80000001]).unwrap();
    assert_eq!(
        [
            state.interpreter_words[&81220],
            state.interpreter_words[&81224],
            state.interpreter_words[&81236]
        ],
        [2, 2, 1]
    );
    assert_eq!(state.interpreter_words[&81228], 0xdeadbeef);
    assert_eq!(state.native_glyphs[&0].cursor, [0; 2]);
    command(&mut state, 913, &[0, 0x80000001]).unwrap();
    assert_eq!(state.interpreter_words[&81220], 3);
}

#[test]
fn clear_leaves_occupancy_and_manager_while_destroy_recreate_keeps_only_manager() {
    let mut state = vm();
    create(&mut state);
    let identity = state.native_glyphs[&0].identity;
    let owner = state.native_font_owners[&783].raster.owner;
    command(&mut state, 901, &[0, 4, 8, 300, 100]).unwrap();
    state.native_glyphs.get_mut(&0).unwrap().atlas.occupancy[17] = 1;
    command(&mut state, 908, &[0, 30, 40]).unwrap();
    command(&mut state, 902, &[0]).unwrap();
    assert_eq!(state.native_glyphs[&0].atlas.occupancy[17], 1);
    assert_eq!(
        state.native_glyphs[&0].cursor.map(f32::from_bits),
        [4.0, 8.0]
    );
    command(&mut state, 916, &[0, 1]).unwrap();
    command(&mut state, 898, &[0]).unwrap();
    assert!(!state.native_glyphs.contains_key(&0));
    assert_eq!(state.native_font_owners[&783].raster.owner, owner);
    create(&mut state);
    assert_ne!(state.native_glyphs[&0].identity, identity);
    assert!(state.native_glyphs[&0].bold);
    assert_eq!(state.native_glyphs[&0].atlas.occupancy[17], 0);
}

#[test]
fn same_family904_always_invalidates_the_persistent_font_cache() {
    let mut state = vm();
    create(&mut state);
    let before = state.native_glyph_raster(0, 65, 30).unwrap();
    let action = command(&mut state, 904, &[0, 0x80000000]).unwrap();
    assert_eq!(
        action,
        Some(CmvsPs2aVmAction::NativeText(
            CmvsNativeTextAction::GlyphFont { slot: 0 }
        ))
    );
    assert!(state.native_font_owners[&783].cache.is_empty());
    assert!(state.native_glyph_raster(0, 65, 30).unwrap().generation > before.generation);
}

#[test]
fn private39_preserves_cpu_cells_residents_occupancy_cursor_and_motion() {
    let mut state = vm();
    create(&mut state);
    let graph = state.native_glyphs.get_mut(&0).unwrap();
    graph.cursor = [1.5, -2.5].map(f32::to_bits);
    graph.bounds = [-100, 100, -100, 100];
    motion::start(graph, 1, [20, -20], 81, 1000, 777);
    graph.atlas.occupancy[17] = 1;
    graph.records.push(CmvsNativeGlyphRecord {
        code: 65,
        position: [3, 4],
        size: 20,
        colors: [0xffffff; 2],
        style: 2,
        outline_color: 0x123456,
        color_mode: 1,
        painted: Some(CmvsNativeGlyphPainted {
            bgra: vec![83; 36 * 36 * 4],
            offset: [-1, 4],
            physical_size: 30,
            cell_size: 36,
        }),
        resident: Some(CmvsNativeGlyphResident {
            slot: 17,
            cell_size: 36,
            child: CmvsTextureChildState {
                basic_kind: 0,
                surface_initialized: true,
                auxiliary_word: Some(1),
                ..Default::default()
            },
        }),
    });
    let bytes = postcard::to_allocvec(&state).unwrap();
    assert_eq!(bytes[0], 39);
    let restored: CmvsPs2aVmState = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(restored.native_glyphs, state.native_glyphs);
    validate(&restored).unwrap();
}

#[test]
fn live_query_and_clamp_results_use_native_fields_and_never_wait() {
    let mut state = vm();
    create(&mut state);
    state.write_interpreter_word(81220, 0x12345678);
    state.write_interpreter_word(81236, 0xabcdef01);
    command(
        &mut state,
        900,
        &[0, 12.75f32.to_bits(), (-8.75f32).to_bits()],
    )
    .unwrap();
    assert!(command(&mut state, 920, &[0]).unwrap().is_none());
    assert_eq!(state.interpreter_words[&81224], 12);
    assert_eq!(state.interpreter_words[&81228], (-8i32) as u32);
    assert_eq!(state.interpreter_words[&81220], 0x12345678);
    command(
        &mut state,
        922,
        &[0, (-10i32) as u32, 20, (-30i32) as u32, 40],
    )
    .unwrap();
    command(&mut state, 926, &[0, (-100i32) as u32, 100]).unwrap();
    assert_eq!(state.interpreter_words[&81224], (-10i32) as u32);
    assert_eq!(state.interpreter_words[&81228], 40);
    assert_eq!(
        state.native_glyphs[&0].position.map(f32::from_bits),
        [12.75, -8.75]
    );
    // Lower and upper are separate signed comparisons, even for reversed bounds.
    command(&mut state, 922, &[0, 20, 10, 40, 30]).unwrap();
    command(&mut state, 926, &[0, 0, 100]).unwrap();
    assert_eq!(state.interpreter_words[&81224], 10);
    assert_eq!(state.interpreter_words[&81228], 30);
    assert_eq!(state.interpreter_words[&81220], 0x12345678);
    assert_eq!(state.interpreter_words[&81236], 0xabcdef01);
    assert_eq!(state.stack_cursor_bytes, 0);
    assert!(!state.wait_frame);
    assert!(!state.wait_boundary_hit);
}

#[test]
fn active_query_does_not_advance_and_stop_keeps_current_position() {
    let mut state = vm();
    create(&mut state);
    command(
        &mut state,
        922,
        &[0, (-100i32) as u32, 100, (-100i32) as u32, 100],
    )
    .unwrap();
    command(&mut state, 924, &[0, 80, 40, 1000]).unwrap();
    state.session_clock_millis = 5000;
    command(&mut state, 923, &[0]).unwrap();
    assert_eq!(state.interpreter_words[&81220], 1);
    assert_eq!(state.native_glyphs[&0].position, [0; 2]);
    command(&mut state, 928, &[0]).unwrap();
    command(&mut state, 923, &[0]).unwrap();
    assert_eq!(state.interpreter_words[&81220], 0);
    assert_eq!(state.native_glyphs[&0].position, [0; 2]);
    assert_eq!(state.stack_cursor_bytes, 0);
    assert!(!state.wait_frame);
}

#[test]
fn secondary_reference_is_non_owning_and_updates_integer_and_float_basic_coordinates() {
    let mut state = vm();
    create(&mut state);
    command(&mut state, 32, &[7]).unwrap();
    command(&mut state, 64, &[7]).unwrap();
    let identity = state.slot_objects[&(1924 << 8 | 7)];
    command(&mut state, 925, &[0, 7]).unwrap();
    assert_eq!(state.native_glyphs[&0].secondary_reference, Some(identity));
    command(&mut state, 899, &[0, (-4i32) as u32, 6]).unwrap();
    assert_eq!(
        state.texture_parents[&7].auxiliary_pair_words,
        Some([(-4i32) as u32, 6])
    );
    assert_eq!(
        state.native_glyphs[&0].root.float_position_words[..2],
        [-6.0, 9.0].map(f32::to_bits)
    );
    command(
        &mut state,
        900,
        &[0, 1.25f32.to_bits(), (-2.5f32).to_bits()],
    )
    .unwrap();
    assert_eq!(
        state.texture_parents[&7].float_position_words,
        [1.25f32.to_bits(), (-2.5f32).to_bits(), 0]
    );
    let saved = postcard::to_allocvec(&state).unwrap();
    let restored: CmvsPs2aVmState = postcard::from_bytes(&saved).unwrap();
    assert_eq!(
        restored.native_glyphs[&0].secondary_reference,
        Some(identity)
    );
    // Invalid positive binding leaves the previous pointer untouched.
    command(&mut state, 925, &[0, 8]).unwrap();
    assert_eq!(state.native_glyphs[&0].secondary_reference, Some(identity));
    assert_ne!(state.interpreter_flag_words[&10532] & 0x1000, 0);
    command(&mut state, 925, &[0, u32::MAX]).unwrap();
    assert_eq!(state.native_glyphs[&0].secondary_reference, None);
    command(&mut state, 899, &[0, 8, 9]).unwrap();
    assert_eq!(
        state.texture_parents[&7].auxiliary_pair_words,
        Some([(-4i32) as u32, 6])
    );
    command(&mut state, 925, &[0, 7]).unwrap();
    command(&mut state, 33, &[7]).unwrap();
    command(&mut state, 32, &[7]).unwrap();
    command(&mut state, 64, &[7]).unwrap();
    // Native has a dangling pointer; stop/query remain valid until dereference.
    command(&mut state, 928, &[0]).unwrap();
    command(&mut state, 920, &[0]).unwrap();
    assert!(command(&mut state, 899, &[0, 1, 2]).is_err());
}

#[test]
fn remaining_handlers_guard_real775_without_resetting_results_and_pop_exact_operands() {
    for (id, operands) in [
        (919, vec![8, 0x80000001]),
        (920, vec![8]),
        (923, vec![8]),
        (925, vec![8, u32::MAX]),
        (926, vec![8, 1, 2]),
        (928, vec![8]),
    ] {
        let mut state = vm();
        for (field, value) in [(81220, 11), (81224, 22), (81228, 33), (81236, 44)] {
            state.write_interpreter_word(field, value);
        }
        assert!(command(&mut state, id, &operands).unwrap().is_none());
        assert_ne!(state.interpreter_flag_words[&10532] & 0x1000, 0);
        assert_eq!(state.interpreter_words[&81220], 11);
        assert_eq!(state.interpreter_words[&81224], 22);
        assert_eq!(state.interpreter_words[&81228], 33);
        assert_eq!(state.interpreter_words[&81236], 44);
        assert_eq!(state.stack_cursor_bytes, 0);
        assert!(!state.wait_frame);
        assert!(!state.wait_boundary_hit);
    }
    let mut state = vm();
    assert!(command(&mut state, 919, &[8, 0x80000080]).is_err());
}
