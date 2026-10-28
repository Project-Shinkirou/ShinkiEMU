use super::*;
use crate::{CmvsNativeGlyphAtlas, CmvsNativeGlyphPainted, CmvsTextureParentState};

impl NativeTextRenderer {
    fn draw_commands(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        width: u32,
        textures: &mut TextureCache,
    ) -> Result<Vec<SceneCommand>, CoreError> {
        let mut groups = self.prepare_draw_groups(vm, width, textures)?;
        groups.admit(&mut 8192);
        let mut commands = Vec::new();
        groups.append(&mut commands);
        Ok(commands)
    }
}

fn graph() -> CmvsNativeGlyphGraph {
    CmvsNativeGlyphGraph {
        identity: 37,
        provider_generation: 0,
        auxiliary: None,
        secondary_reference: None,
        font: "Noto Sans JP".into(),
        bold: false,
        size: 24,
        capacity_hint: 64,
        virtual_size: [128, 96],
        physical_size: [128, 96],
        position: [0; 2],
        rect: [0, 0, 128.0_f32.to_bits(), 96.0_f32.to_bits()],
        viewport: [0, 0, 128.0_f32.to_bits(), 96.0_f32.to_bits()],
        cursor: [12.0_f32.to_bits(), 12.0_f32.to_bits()],
        gaps: [0; 2],
        colors: [0xff4020; 2],
        kinsoku: true,
        style: 2,
        outline_color: 0x204080,
        color_mode: 0,
        draw_order: 1,
        root: CmvsTextureParentState {
            surface_initialized: true,
            basic_kind: 5,
            alpha: Some(255),
            rect_words: Some([0, 0, 4, 4]),
            reference_size: Some([128, 96]),
            auxiliary_word: Some(1),
            image_info: [512, 512, 1],
            ..Default::default()
        },
        atlas: CmvsNativeGlyphAtlas {
            cell_size: 30,
            width: 512,
            height: 512,
            occupancy: vec![0; 1024],
        },
        records: Vec::new(),
        bounds: [0; 4],
        motion: None,
    }
}

fn record() -> CmvsNativeGlyphRecord {
    CmvsNativeGlyphRecord {
        code: b'W' as u16,
        position: [12, 12],
        size: 24,
        colors: [0xffffff; 2],
        style: 2,
        outline_color: 0,
        color_mode: 0,
        painted: None,
        resident: None,
    }
}

#[test]
fn viewport_uses_position_and_inclusive_two_pixel_margin() {
    let mut owner = graph();
    let mut glyph = record();
    glyph.position = [130, 12];
    assert!(in_viewport(&owner, &glyph));
    glyph.position[0] = 131;
    assert!(!in_viewport(&owner, &glyph));
    owner.position[0] = (-1.0_f32).to_bits();
    assert!(in_viewport(&owner, &glyph));
    glyph.position = [-26, -26];
    owner.position = [0; 2];
    assert!(in_viewport(&owner, &glyph));
    glyph.position[1] = -27;
    assert!(!in_viewport(&owner, &glyph));
}

#[test]
fn atlas_uses_ctor_columns_current_cell_pitch_and_real_allocation() {
    let atlas = graph().atlas;
    assert_eq!(cell_destination(&atlas, 30, 1).unwrap(), [30, 0]);
    assert_eq!(cell_destination(&atlas, 30, 17).unwrap(), [0, 30]);
    assert_eq!(cell_destination(&atlas, 30, 289).unwrap(), [0, 512]);
    assert_eq!(cell_destination(&atlas, 40, 17).unwrap(), [0, 40]);
    let mut frame = TextureFrame::from_vec(512, 512, vec![0; 512 * 512 * 4]).unwrap();
    let painted = CmvsNativeGlyphPainted {
        bgra: [20, 40, 80, 160].repeat(30 * 30),
        offset: [0; 2],
        physical_size: 24,
        cell_size: 30,
    };
    upload_cell(&mut frame, &painted, 30, [30, 0]).unwrap();
    assert_eq!(&frame.rgba8[30 * 4..31 * 4], &[80, 40, 20, 160]);
    assert_eq!(
        &frame.rgba8[(28 * 512 + 58) * 4..(28 * 512 + 59) * 4],
        &[80, 40, 20, 160]
    );
    assert_eq!(&frame.rgba8[59 * 4..60 * 4], &[0; 4]);
    assert!(upload_cell(&mut frame, &painted, 30, [0, 512]).is_err());
    assert!(upload_cell(&mut frame, &painted, 60, [0, 0]).is_err());
}

#[test]
fn mixed_zero_five_geometry_keeps_integer_child_and_float_parent_context() {
    let vm = CmvsPs2aVmState::new(0);
    let mut owner = graph();
    owner.root.float_position_words = [10.5_f32.to_bits(), 20.25_f32.to_bits(), 0];
    owner.root.scale_words = Some([2.0_f32.to_bits(), 3.0_f32.to_bits()]);
    owner.root.alpha = Some(128);
    let mut child = CmvsTextureChildState {
        surface_initialized: true,
        basic_kind: 0,
        auxiliary_pair_words: Some([4, 5]),
        alpha: Some(128),
        auxiliary_word: Some(1),
        ..Default::default()
    };
    let geometry = super::super::super::surface::geometry_with_surfaces(
        &vm,
        &owner.root,
        Some(&child),
        128,
        true,
    )
    .unwrap();
    assert_eq!(
        geometry.corners([8, 8]),
        [[18.5, 30.25], [34.5, 30.25], [18.5, 54.25], [34.5, 54.25]]
    );
    assert_eq!(
        super::super::super::surface::opacity_with_surfaces(&vm, &owner.root, Some(&child), 256),
        64.0 / 255.0
    );
    child.parent_context_detached = true;
    child.reference_size = Some([128, 96]);
    let geometry = super::super::super::surface::geometry_with_surfaces(
        &vm,
        &owner.root,
        Some(&child),
        128,
        true,
    )
    .unwrap();
    assert_eq!(geometry.corners([8, 8])[0], [4.0, 5.0]);
    child.basic_kind = 1;
    assert!(
        super::super::super::surface::geometry_with_surfaces(
            &vm,
            &owner.root,
            Some(&child),
            128,
            true
        )
        .is_none()
    );
}

#[test]
fn raw_style_and_outline_color_use_shared_native_composition() {
    use roukaku_media_core::{GlyphBitmap, GlyphBitmapFormat};
    let source = GlyphBitmap::from_vec(1, 1, GlyphBitmapFormat::Alpha8, vec![128]).unwrap();
    let style = paint::PaintStyle {
        style: 2,
        colors: [0xff0000; 2],
        color_mode: 0,
        outline_color: 0x0000ff,
    };
    let painted = paint::paint(&source, style, [0, 0], 1, 2).unwrap();
    assert_eq!(
        &painted.pixels.as_slice()[(2 * 5 + 2) * 4..(2 * 5 + 3) * 4],
        &[127, 0, 127, 255]
    );
    assert_eq!(&painted.pixels.as_slice()[2 * 4..3 * 4], &[0, 0, 255, 128]);
    let raw = paint::paint(
        &source,
        paint::PaintStyle {
            style: 0xffff,
            ..style
        },
        [0, 0],
        1,
        2,
    )
    .unwrap();
    assert_eq!(
        &raw.pixels.as_slice()[(2 * 5 + 2) * 4..(2 * 5 + 3) * 4],
        &[127, 0, 127, 255]
    );
    assert_eq!(&raw.pixels.as_slice()[2 * 4..3 * 4], &[0; 4]);
}

fn fixture() -> (NativeTextRenderer, CmvsPs2aVmState, TextureCache) {
    let (renderer, mut vm) = super::super::tests::fixture();
    crate::vm::native_text::font_owner::ensure_owner(&mut vm, 783, 0).unwrap();
    vm.native_font_owners
        .get_mut(&783)
        .unwrap()
        .set_family("Noto Sans JP")
        .unwrap();
    vm.native_glyphs.insert(0, graph());
    vm.slot_objects.insert(775 << 8, 37);
    let cache =
        TextureCache::new(NonZeroUsize::new(512).unwrap(), 1024 * 1024 * 1024, 16384).unwrap();
    (renderer, vm, cache)
}

#[test]
fn append_queries_fixed_advance_but_paints_only_at_frame_and_retains_pixels_on_eviction() {
    let (mut renderer, mut vm, mut cache) = fixture();
    renderer.append_glyph(&mut vm, 0, b"Wg").unwrap();
    assert_eq!(vm.native_glyphs[&0].records.len(), 2);
    assert!(
        vm.native_glyphs[&0]
            .records
            .iter()
            .all(|record| record.painted.is_none() && record.resident.is_none())
    );
    assert!(
        vm.native_font_owners[&783]
            .cache
            .contains_key(&(b'W' as u16, 24))
    );
    let commands = renderer.draw_commands(&mut vm, 128, &mut cache).unwrap();
    assert!(commands.iter().any(|command| matches!(command, SceneCommand::UploadTexture { frame, .. } if frame.width == 512 && frame.height == 512)));
    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, SceneCommand::UploadGlyph { .. }))
    );
    let painted = vm.native_glyphs[&0].records[0].painted.clone().unwrap();
    assert_eq!(painted.bgra.len(), 30 * 30 * 4);
    assert!(
        painted
            .bgra
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] != 0)
    );
    assert_eq!(
        vm.native_glyphs[&0].records[0]
            .resident
            .as_ref()
            .unwrap()
            .slot,
        1
    );
    vm.native_glyphs.get_mut(&0).unwrap().viewport[0] = 1000.0_f32.to_bits();
    renderer.draw_commands(&mut vm, 128, &mut cache).unwrap();
    assert!(
        vm.native_glyphs[&0]
            .records
            .iter()
            .all(|record| record.resident.is_none())
    );
    assert!(
        vm.native_glyphs[&0]
            .atlas
            .occupancy
            .iter()
            .all(|&value| value == 0)
    );
    assert_eq!(
        vm.native_glyphs[&0].records[0].painted.as_ref().unwrap(),
        &painted
    );
    vm.native_font_owners
        .get_mut(&783)
        .unwrap()
        .set_bold(true)
        .unwrap();
    vm.native_glyphs.get_mut(&0).unwrap().viewport[0] = 0;
    renderer.draw_commands(&mut vm, 128, &mut cache).unwrap();
    assert_eq!(
        vm.native_glyphs[&0].records[0].painted.as_ref().unwrap(),
        &painted
    );
    let bytes = serde_json::to_vec(&vm.native_glyphs[&0]).unwrap();
    let restored: CmvsNativeGlyphGraph = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(restored, vm.native_glyphs[&0]);
    let old_frame = renderer.glyph_atlases[&0].frame.clone();
    let mut cold = NativeTextRenderer::default();
    cold.draw_commands(&mut vm, 128, &mut cache).unwrap();
    assert_eq!(cold.glyph_atlases[&0].frame, old_frame);
}

#[test]
fn pending_records_use_later_family_and_bold_while_painted_records_do_not() {
    let (mut renderer, mut vm, mut cache) = fixture();
    // Arial is a native DEFAULT_CHARSET face, not a SHIFTJIS_CHARSET face.
    vm.process_global_words
        .insert(crate::vm::NATIVE_FONT_SHIFT_JIS, 0);
    renderer.append_glyph(&mut vm, 0, b"W").unwrap();
    renderer.draw_commands(&mut vm, 128, &mut cache).unwrap();
    let original = vm.native_glyphs[&0].records[0].painted.clone().unwrap();
    renderer.append_glyph(&mut vm, 0, b"W").unwrap();
    renderer.database.as_mut().unwrap().load_system_fonts();
    let owner = vm.native_font_owners.get_mut(&783).unwrap();
    owner.set_family("Arial").unwrap();
    owner.set_bold(true).unwrap();
    renderer.bind_glyph_owner(&vm, 0).unwrap();
    let raster = vm.native_glyph_raster(0, b'W' as u16, 24).unwrap();
    let expected = renderer
        .raster
        .paint_glyph(
            "Arial",
            &vm.native_glyphs[&0].records[1],
            24,
            30,
            raster,
            None,
        )
        .unwrap();
    renderer.draw_commands(&mut vm, 128, &mut cache).unwrap();
    assert_eq!(
        vm.native_glyphs[&0].records[0].painted.as_ref().unwrap(),
        &original
    );
    assert_eq!(
        vm.native_glyphs[&0].records[1].painted.as_ref().unwrap(),
        &expected
    );
    assert_ne!(&original.bgra, &expected.bgra);
}

#[test]
fn clear_keeps_occupancy_and_capacity_does_not_limit_append() {
    let (mut renderer, mut vm, mut cache) = fixture();
    vm.native_glyphs.get_mut(&0).unwrap().capacity_hint = 1;
    renderer.append_glyph(&mut vm, 0, b"WW").unwrap();
    renderer.draw_commands(&mut vm, 128, &mut cache).unwrap();
    let occupied = vm.native_glyphs[&0].atlas.occupancy.clone();
    super::super::super::parent_load_tests::command(&mut vm, 902, &[0]);
    assert!(vm.native_glyphs[&0].records.is_empty());
    assert_eq!(vm.native_glyphs[&0].atlas.occupancy, occupied);
    renderer.append_glyph(&mut vm, 0, b"W").unwrap();
    renderer.draw_commands(&mut vm, 128, &mut cache).unwrap();
    assert_eq!(
        vm.native_glyphs[&0].records[0]
            .resident
            .as_ref()
            .unwrap()
            .slot,
        3
    );
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_atlas_draws_actual_glyph_children_and_evicts_without_losing_cpu_pixels() {
    let (renderer, mut vm, _) = fixture();
    let mut scene = CmvsScene::new(super::super::super::tests::gpu_instance()).unwrap();
    scene.text = renderer;
    scene.append_glyph(&mut vm, 0, b"W").unwrap();
    let first = scene
        .render_overlays(&mut vm, 128, 96, None, Vec::new())
        .unwrap();
    assert!(
        first
            .rgba8
            .as_slice()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0)
    );
    vm.native_glyphs.get_mut(&0).unwrap().viewport[0] = 1000.0_f32.to_bits();
    let outside = scene
        .render_overlays(&mut vm, 128, 96, None, Vec::new())
        .unwrap();
    assert!(
        outside
            .rgba8
            .as_slice()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[..3] == [0; 3])
    );
    vm.native_glyphs.get_mut(&0).unwrap().viewport[0] = 0;
    let again = scene
        .render_overlays(&mut vm, 128, 96, None, Vec::new())
        .unwrap();
    assert_eq!(again, first);
}

#[test]
fn linked_children_sort_independently_and_negative_order_skips_only_that_child() {
    let (mut renderer, mut vm, mut cache) = fixture();
    renderer.append(&mut vm, 3, "W", 128).unwrap();
    vm.native_text.get_mut(&3).unwrap().draw_order = 3;
    renderer.append_glyph(&mut vm, 0, b"WW").unwrap();
    renderer.draw_commands(&mut vm, 128, &mut cache).unwrap();
    let graph = vm.native_glyphs.get_mut(&0).unwrap();
    graph.root.auxiliary_word = Some(0);
    graph.records[0]
        .resident
        .as_mut()
        .unwrap()
        .child
        .auxiliary_word = Some(10);
    graph.records[1]
        .resident
        .as_mut()
        .unwrap()
        .child
        .auxiliary_word = Some(u32::MAX);
    let commands = renderer.draw_commands(&mut vm, 128, &mut cache).unwrap();
    let ids: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            SceneCommand::Sprite { id, .. }
            | SceneCommand::Mesh2D { id, .. }
            | SceneCommand::GlyphRun { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 3);
    assert_eq!(ids[0], "cmvs.glyph.37.1");
    assert!(ids[1].starts_with("cmvs.text.3."));
    assert_eq!(ids[2], "cmvs.glyph.37.0");
}

#[test]
fn actual_allocation_failure_retains_owner_and_never_marks_a_failed_slot_resident() {
    let (mut renderer, mut vm, mut cache) = fixture();
    renderer.append_glyph(&mut vm, 0, b"W").unwrap();
    vm.native_glyphs.get_mut(&0).unwrap().atlas.occupancy[1..289].fill(1);
    assert!(renderer.draw_commands(&mut vm, 128, &mut cache).is_err());
    let owner = &vm.native_glyphs[&0];
    assert!(owner.records[0].painted.is_some());
    assert!(owner.records[0].resident.is_none());
    assert_eq!(owner.atlas.occupancy[289], 0);
    assert!(renderer.glyph_atlases[&0].resource.is_none());
}

#[test]
fn motion_advances_before_viewport_materialization() {
    let (mut renderer, mut vm, mut cache) = fixture();
    renderer.append_glyph(&mut vm, 0, b"W").unwrap();
    let owner = vm.native_glyphs.get_mut(&0).unwrap();
    owner.bounds = [0, 2000, 0, 0];
    owner.motion = Some(crate::CmvsNativeGlyphMotion {
        start: [0, 0],
        end: [1000, 0],
        curve: 0,
        extra: 0,
        duration: 10,
        timestamp: 0,
    });
    vm.session_clock_millis = 10;
    renderer.draw_commands(&mut vm, 128, &mut cache).unwrap();
    let owner = &vm.native_glyphs[&0];
    assert_eq!(f32::from_bits(owner.position[0]), 1000.0);
    assert_eq!(f32::from_bits(owner.root.float_position_words[0]), 1000.0);
    assert!(owner.motion.is_none());
    assert!(owner.records[0].painted.is_none());
    assert!(owner.records[0].resident.is_none());
}

fn mixed_fixture(table: u16) -> (tempfile::TempDir, CmvsArchive, CmvsScene, CmvsPs2aVmState) {
    let (renderer, mut vm, _) = fixture();
    let (root, archive, archive_vm) = super::super::super::parent_load_tests::fixture();
    vm.resource_search_paths = archive_vm.resource_search_paths;
    vm.native_stage_size = [128, 96];
    vm.native_render_size = [128, 96];
    let mut scene = CmvsScene::new(super::super::super::tests::gpu_instance()).unwrap();
    scene.text = renderer;
    if table == 750 {
        vm.native_glyphs.clear();
        let text = vm.native_text.get_mut(&3).unwrap();
        text.physical_size = [128, 96];
        text.virtual_size = [128, 96];
        text.position = [0; 2];
        text.rect = [0, 0, 128, 96];
        text.cursor = [12, 12];
        text.size = 24;
        scene.text.append(&mut vm, 3, "W", 128).unwrap();
    } else {
        vm.native_text.clear();
        scene.append_glyph(&mut vm, 0, b"W").unwrap();
    }
    super::super::super::parent_load_tests::write_image(
        root.path(),
        "opaque.pb3",
        128,
        96,
        [0, 0, 255, 255],
    );
    vm.texture_parents.insert(
        0,
        CmvsTextureParentState {
            surface_initialized: true,
            rect_words: Some([0, 0, 128, 96]),
            reference_size: Some([128, 96]),
            auxiliary_word: Some(1),
            blend_mode: Some(0),
            ..Default::default()
        },
    );
    let uri = archive
        .resolve_native_resource(&vm, crate::CmvsResourceSearchBank::Texture, "opaque.pb3")
        .unwrap();
    scene.bind_parent_image(&mut vm, 0, &uri, &archive).unwrap();
    (root, archive, scene, vm)
}

fn set_mixed_order(vm: &mut CmvsPs2aVmState, table: u16, order: i32) {
    if table == 750 {
        vm.native_text.get_mut(&3).unwrap().draw_order = order;
    } else {
        let graph = vm.native_glyphs.get_mut(&0).unwrap();
        graph.draw_order = order;
        graph.root.auxiliary_word = Some(order as u32);
        for record in &mut graph.records {
            if let Some(resident) = &mut record.resident {
                resident.child.auxiliary_word = Some(order as u32);
            }
        }
    }
}

fn mixed_frame(
    scene: &mut CmvsScene,
    vm: &mut CmvsPs2aVmState,
    archive: &CmvsArchive,
) -> TextureFrame {
    scene
        .render(vm, CmvsTextureSlot::Parent(0), archive, (128, 96), None)
        .unwrap()
        .unwrap()
}

fn is_blue(frame: &TextureFrame) -> bool {
    frame
        .rgba8
        .as_slice()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| *pixel == [0, 0, 255, 255])
}

fn fill_ordinary_records(vm: &mut CmvsPs2aVmState, count: usize) {
    // Configured zero-area/alpha-zero basics still enter4473C0. Eight real
    // indexed containers cover the boundary without exceeding1024 children.
    for index in 0..count {
        let parent = 1 + (index / 1025) as u8;
        let offset = index % 1025;
        if offset == 0 {
            vm.texture_parents.insert(
                parent,
                CmvsTextureParentState {
                    surface_initialized: true,
                    rect_words: Some([0; 4]),
                    alpha: Some(0),
                    ..Default::default()
                },
            );
        } else {
            vm.texture_children.entry(parent).or_default().insert(
                (offset - 1) as u16,
                CmvsTextureChildState {
                    surface_initialized: true,
                    rect_words: Some([0; 4]),
                    alpha: Some(0),
                    ..Default::default()
                },
            );
        }
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_global_queue_mixes_750_and_775_with_opaque_images_and_both_41_tie_policies() {
    for table in [750, 775] {
        let (_root, archive, mut scene, mut vm) = mixed_fixture(table);
        for prepend in [false, true] {
            vm.native_mesh_equal_prepend = prepend;
            set_mixed_order(&mut vm, table, 64);
            assert!(
                is_blue(&mixed_frame(&mut scene, &mut vm, &archive)),
                "table{table} behind"
            );
            set_mixed_order(&mut vm, table, 0);
            assert!(
                !is_blue(&mixed_frame(&mut scene, &mut vm, &archive)),
                "table{table} front"
            );
            set_mixed_order(&mut vm, table, 1);
            assert_eq!(
                is_blue(&mixed_frame(&mut scene, &mut vm, &archive)),
                prepend,
                "table{table} equal"
            );
        }
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_global_queue_counts_transparent_750_and_775_roots_at_8192_boundary() {
    for table in [750, 775] {
        let (_root, archive, mut scene, mut vm) = mixed_fixture(table);
        set_mixed_order(&mut vm, table, 0);
        fill_ordinary_records(&mut vm, 8190);
        // 8191 ordinary records + the transparent native root: no glyph fits.
        assert!(
            is_blue(&mixed_frame(&mut scene, &mut vm, &archive)),
            "table{table} root must count"
        );
        let parent = 1 + (8189 / 1025) as u8;
        let child = (8189 % 1025 - 1) as u16;
        vm.texture_children
            .get_mut(&parent)
            .unwrap()
            .get_mut(&child)
            .unwrap()
            .hidden = true;
        assert!(
            !is_blue(&mixed_frame(&mut scene, &mut vm, &archive)),
            "hidden basic frees one record"
        );
        vm.texture_children
            .get_mut(&parent)
            .unwrap()
            .get_mut(&child)
            .unwrap()
            .hidden = false;
        assert!(
            is_blue(&mixed_frame(&mut scene, &mut vm, &archive)),
            "alpha-zero basic consumes one record"
        );
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_full_global_queue_still_updates_paints_uploads_evicts_and_releases_775() {
    let (_root, archive, mut scene, mut vm) = mixed_fixture(775);
    fill_ordinary_records(&mut vm, 8191);
    assert!(is_blue(&mixed_frame(&mut scene, &mut vm, &archive)));
    let graph = &vm.native_glyphs[&0];
    assert!(graph.records[0].painted.is_some());
    assert!(graph.records[0].resident.is_some());
    assert!(scene.text.glyph_atlases[&0].resource.is_some());
    let pixels = graph.records[0].painted.clone();
    vm.native_glyphs.get_mut(&0).unwrap().viewport[0] = 1000.0_f32.to_bits();
    assert!(is_blue(&mixed_frame(&mut scene, &mut vm, &archive)));
    assert!(vm.native_glyphs[&0].records[0].resident.is_none());
    assert_eq!(vm.native_glyphs[&0].records[0].painted, pixels);
    vm.native_glyphs.get_mut(&0).unwrap().viewport[0] = 0;
    assert!(is_blue(&mixed_frame(&mut scene, &mut vm, &archive)));
    assert!(vm.native_glyphs[&0].records[0].resident.is_some());
    vm.native_glyphs.clear();
    assert!(is_blue(&mixed_frame(&mut scene, &mut vm, &archive)));
    assert!(scene.text.glyph_atlases.is_empty());
    assert!(scene.text.glyph_pending.is_empty());
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_742_admission_follows_glyphs_and_flat_ties_without_an_overlay_escape() {
    for table in [750, 775] {
        let (root, archive, mut scene, mut vm) = mixed_fixture(table);
        set_mixed_order(&mut vm, table, 0);
        super::super::super::parent_load_tests::write_image(
            root.path(),
            "channel.pb3",
            128,
            96,
            [0, 255, 0, 255],
        );
        let uri = archive
            .resolve_native_resource(&vm, crate::CmvsResourceSearchBank::Texture, "channel.pb3")
            .unwrap();
        vm.slot_objects.insert(742 << 8, 99);
        let channel = vm.effect_channels.entry(0).or_default();
        channel.image_resource = Some(uri);
        channel.image_has_alpha = true;
        let element = vm.effect_elements.entry((0, u32::MAX)).or_default();
        element.visible = 1;
        element.source = Some([0, 0, 128, 96]);
        element.position = Some([0; 2]);
        element.width = 128;
        element.height = 96;
        element.order = 0;
        let green = |frame: TextureFrame| {
            frame
                .rgba8
                .as_slice()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [0, 255, 0, 255])
        };
        fill_ordinary_records(&mut vm, 8188);
        assert!(
            green(mixed_frame(&mut scene, &mut vm, &archive)),
            "742 tie follows table{table}"
        );
        fill_ordinary_records(&mut vm, 8189);
        assert!(
            !green(mixed_frame(&mut scene, &mut vm, &archive)),
            "742 must not bypass full pool"
        );
        if table == 750 {
            vm.native_text.get_mut(&3).unwrap().visible = false;
        } else {
            vm.native_glyphs.get_mut(&0).unwrap().root.hidden = true;
        }
        assert!(
            green(mixed_frame(&mut scene, &mut vm, &archive)),
            "hidden owner admits742"
        );
        if table == 775 {
            let graph = vm.native_glyphs.get_mut(&0).unwrap();
            graph.root.hidden = false;
            graph.root.alpha = Some(0);
            assert!(
                is_blue(&mixed_frame(&mut scene, &mut vm, &archive)),
                "alpha-zero775 still consumes two records"
            );
        }
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_full_global_queue_does_not_hide_native_font_materialization_errors() {
    let (_root, archive, mut scene, mut vm) = mixed_fixture(775);
    fill_ordinary_records(&mut vm, 8191);
    assert!(is_blue(&mixed_frame(&mut scene, &mut vm, &archive)));
    scene.append_glyph(&mut vm, 0, b"g").unwrap();
    vm.native_font_owners
        .get_mut(&783)
        .unwrap()
        .set_family("Arial")
        .unwrap();
    assert!(
        scene
            .render(
                &mut vm,
                CmvsTextureSlot::Parent(0),
                &archive,
                (128, 96),
                None,
            )
            .is_err()
    );
    assert!(vm.native_glyphs[&0].records[1].painted.is_none());
    assert!(vm.native_glyphs[&0].records[1].resident.is_none());
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_global_queue_drains_indexed_children_but_flat_sorts_detached_channel_records() {
    let (root, archive, mut scene, mut vm) = mixed_fixture(775);
    set_mixed_order(&mut vm, 775, 32);
    vm.texture_parents.get_mut(&0).unwrap().auxiliary_word = Some(128);
    super::super::super::parent_load_tests::write_image(
        root.path(),
        "indexed.pb3",
        128,
        96,
        [0, 255, 0, 255],
    );
    let uri = archive
        .resolve_native_resource(&vm, crate::CmvsResourceSearchBank::Texture, "indexed.pb3")
        .unwrap();
    vm.texture_parents.insert(
        1,
        CmvsTextureParentState {
            surface_initialized: true,
            rect_words: Some([0; 4]),
            reference_size: Some([128, 96]),
            auxiliary_word: Some(64),
            ..Default::default()
        },
    );
    vm.texture_children.entry(1).or_default().insert(
        0,
        CmvsTextureChildState {
            surface_initialized: true,
            rect_words: Some([0, 0, 128, 96]),
            reference_size: Some([128, 96]),
            auxiliary_word: Some(0),
            ..Default::default()
        },
    );
    scene.bind_parent_image(&mut vm, 1, &uri, &archive).unwrap();
    let green = |frame: TextureFrame| {
        frame
            .rgba8
            .as_slice()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [0, 255, 0, 255])
    };
    assert!(
        !green(mixed_frame(&mut scene, &mut vm, &archive)),
        "indexed image stays in parent64 bucket"
    );
    vm.texture_parents.remove(&1);
    vm.texture_children.remove(&1);
    scene.clear_parent_image(1);
    vm.slot_objects.insert(742 << 8, 99);
    let channel = vm.effect_channels.entry(0).or_default();
    channel.image_resource = Some(uri);
    channel.image_has_alpha = true;
    let root = vm.effect_elements.entry((0, u32::MAX)).or_default();
    root.visible = 1;
    root.source = Some([0; 4]);
    root.position = Some([0; 2]);
    root.width = 128;
    root.height = 96;
    root.order = 64;
    let child = vm.effect_elements.entry((0, 0)).or_default();
    child.visible = 1;
    child.source = Some([0, 0, 128, 96]);
    child.position = Some([0; 2]);
    child.width = 128;
    child.height = 96;
    child.order = 0;
    assert!(
        !green(mixed_frame(&mut scene, &mut vm, &archive)),
        "indexed742 stays in root64 bucket"
    );
    vm.effect_elements
        .get_mut(&(0, 0))
        .unwrap()
        .inheritance_flags &= !2;
    assert!(
        green(mixed_frame(&mut scene, &mut vm, &archive)),
        "bit1-clear742 enters global flat0 bucket"
    );
}

#[test]
fn text_queue_counts_started_alpha_and_width_zero_nodes_but_not_delayed_hidden_nodes() {
    let (mut renderer, mut vm, mut cache) = fixture();
    vm.native_glyphs.clear();
    renderer.append(&mut vm, 3, "W", 128).unwrap();
    vm.native_text_reveals.insert(
        3,
        vec![crate::vm::native_text::dialogue::CmvsNativeTextReveal {
            glyph_index: 0,
            delay_ms: 0,
            elapsed_ms: 0,
            effect: 1,
            duration_ms: 100,
            started: true,
        }],
    );
    for effect in [1, 2] {
        vm.native_text_reveals.get_mut(&3).unwrap()[0].effect = effect;
        let groups = renderer
            .prepare_draw_groups(&mut vm, 128, &mut cache)
            .unwrap();
        assert_eq!(
            groups.groups.len(),
            2,
            "started effect{effect} keeps root plus child"
        );
    }
    vm.native_text_reveals.get_mut(&3).unwrap()[0].started = false;
    assert_eq!(
        renderer
            .prepare_draw_groups(&mut vm, 128, &mut cache)
            .unwrap()
            .groups
            .len(),
        1
    );
    vm.native_text.get_mut(&3).unwrap().visible = false;
    assert!(
        renderer
            .prepare_draw_groups(&mut vm, 128, &mut cache)
            .unwrap()
            .groups
            .is_empty()
    );
}
