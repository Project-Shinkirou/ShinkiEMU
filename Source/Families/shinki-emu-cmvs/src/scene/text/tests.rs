use super::*;
use crate::CmvsNativeTextSurface;

pub(in crate::scene) fn fixture() -> (NativeTextRenderer, CmvsPs2aVmState) {
    let mut database = fontdb::Database::new();
    database.load_font_data(
        include_bytes!("../../../../../../Assets/Fonts/NotoSansJP-Variable.ttf").to_vec(),
    );
    let mut renderer = NativeTextRenderer {
        database: Some(database),
        ..Default::default()
    };
    renderer.bind_font("Noto Sans JP").unwrap();
    let mut vm = CmvsPs2aVmState::new(0);
    crate::vm::native_text::font_owner::ensure_owner(&mut vm, 762, 3).unwrap();
    vm.native_font_owners
        .get_mut(&(762 + 3))
        .unwrap()
        .set_family("Noto Sans JP")
        .unwrap();
    vm.native_text.insert(
        3,
        CmvsNativeTextSurface {
            font: "Noto Sans JP".into(),
            bold: false,
            capacity_hint: 0,
            visible: true,
            rect: [44, 60, 512, 30],
            cursor: [44, 60],
            size: 30,
            character_spacing: 2,
            line_spacing: 5,
            colors: [0xffffff, 0],
            kinsoku: true,
            style: 2,
            color_mode: 0,
            position: [0, 0],
            physical_size: [1280, 720],
            virtual_size: [1280, 720],
            draw_order: 1,
            glyph_interval_ms: 30,
            reserve_ruby: false,
            reveal_defaults: [0; 2],
            glyphs: Vec::new(),
        },
    );
    (renderer, vm)
}

fn fixture_at(extent: [u32; 2]) -> (NativeTextRenderer, CmvsPs2aVmState) {
    let (renderer, mut vm) = fixture();
    vm.native_stage_size = extent;
    vm.native_render_size = extent;
    let surface = vm.native_text.get_mut(&3).unwrap();
    surface.physical_size = extent;
    surface.virtual_size = extent;
    (renderer, vm)
}

#[test]
fn presentation_density_preserves_gradient_queue_clip_and_gaiji_at_fractional_scale() {
    use roukaku_media_core::{
        CpuRendererProvider, GlyphBitmapFormat, RenderTargetFormat, Renderer2DProvider,
        RendererCreateRequest,
    };
    use std::num::NonZeroUsize;

    let (mut renderer, mut vm) = fixture_at([64, 48]);
    vm.process_global_words.insert(0x004f_0580, 1);
    let surface = vm.native_text.get_mut(&3).unwrap();
    surface.rect = [0, 0, 128, 48];
    surface.cursor = [0, 0];
    surface.size = 16;
    surface.style = 0;
    surface.color_mode = 2;
    surface.colors = [0xff0000, 0x0000ff];
    vm.native_custom_glyphs
        .load_sheet("fixture.msk".into(), 16, 16, 16, vec![128; 16 * 16])
        .unwrap();
    vm.native_custom_glyphs.register(0, 0x82a0).unwrap();
    assert!(vm.native_custom_glyphs.set_descriptor(0x82a0, 16, 9));
    renderer.append(&mut vm, 3, "WあWW", 64).unwrap();
    let surface = vm.native_text.get_mut(&3).unwrap();
    for (glyph, position) in surface.glyphs.iter_mut().zip([[9, 11], [3, 11], [29, 11], [47, 12]]) {
        glyph.position = position;
    }
    surface.glyphs[1].colors = [0x00ff00, 0];
    surface.glyphs[1].color_mode = 0;
    let native = surface.clone();
    assert_eq!(native.glyphs[1].code, 0x82a0);
    assert_eq!(native.glyphs[1].raster.custom_advance, Some(9));
    let custom_id = native.glyphs[1].raster.custom_bitmap_identity.unwrap();
    let measured = renderer.measure(&mut vm, 3, "WあWW", true, 64).unwrap();
    let mut textures = TextureCache::new(NonZeroUsize::new(4).unwrap(), 1024 * 1024, 256).unwrap();
    let mut frame = |vm: &mut CmvsPs2aVmState, camera| {
        let groups = renderer.prepare_draw_groups(vm, 64, camera, &mut textures).unwrap();
        let mut commands = Vec::new();
        groups.append(&mut commands);
        commands
    };
    let camera = StageCanvas::new(Extent2D::new(64, 48), Extent2D::new(96, 72))
        .unwrap()
        .logical_to_raster_transform();
    let mut cpu = CpuRendererProvider
        .create(RendererCreateRequest {
            width: 96,
            height: 72,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "cmvs-presentation-density".into(),
        })
        .unwrap();
    // Keep the same logical clip and upscale both frames. The control uses
    // the native mask, so this compares true resampling with bitmap scaling.
    let mut draw = |commands: &[SceneCommand]| {
        let mut scene = vec![
            SceneCommand::Rect {
                id: "background".into(),
                x: 0,
                y: 0,
                width: 96,
                height: 72,
                rgba: [0, 0, 0, 255],
            },
            SceneCommand::SetCamera { transform: camera },
            SceneCommand::PushClip { rect: RectI::new(0, 0, 64, 20) },
        ];
        scene.extend_from_slice(commands);
        scene.push(SceneCommand::PopClip);
        cpu.capture_frame(&scene).unwrap().bytes
    };
    let runs = |commands: &[SceneCommand]| {
        commands.iter().filter_map(|cmd| match cmd {
            SceneCommand::LinearGlyphRun { id, glyphs, .. } => {
                Some((id.clone(), glyphs[0].resource_id.clone()))
            }
            _ => None,
        }).collect::<Vec<_>>()
    };
    let base = frame(&mut vm, Transform2D::IDENTITY);
    let base_runs = runs(&base);
    let gaiji = &base_runs[1].1;
    assert!(base.iter().any(|cmd| matches!(
        cmd,
        SceneCommand::UploadGlyph { resource_id, glyph }
            if resource_id == gaiji && glyph.format == GlyphBitmapFormat::Alpha8
                && glyph.pixels.iter().all(|&value| matches!(value, 0 | 128))
    )));
    let upscaled = draw(&base);

    let dense = frame(&mut vm, camera);
    let dense_runs = runs(&dense);
    assert_eq!(
        dense_runs.iter().map(|run| run.0.as_str()).collect::<Vec<_>>(),
        ["cmvs.text.3.0", "cmvs.text.3.1", "cmvs.text.3.2", "cmvs.text.3.3"]
    );
    assert_eq!(&dense_runs[1].1, gaiji);
    assert_eq!(dense_runs[0].1, dense_runs[2].1);
    assert_ne!(dense_runs[0].1, dense_runs[3].1);
    assert_eq!(dense.iter().filter(|cmd| matches!(cmd, SceneCommand::UploadGlyph {
        glyph, ..
    } if glyph.format == GlyphBitmapFormat::Rgba8)).count(), 2);
    let pixels = draw(&dense);
    let pixel = |x: usize, y: usize| (y * 96 + x) * 4;
    let green = pixel(5, 17);
    assert_eq!(&pixels[green..green + 4], &[0, 188, 0, 255]);
    assert!((17..30).any(|y| (14..28).any(|x| {
        let i = pixel(x, y);
        pixels[i] > 0 && pixels[i + 1] == 188 && pixels[i + 2] > 0
    })));
    assert!(pixels[pixel(0, 30)..].chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
    assert!((16..30).any(|y| (44..67).any(|x| {
        let i = pixel(x, y);
        let inked = |p: &[u8]| p[0] != 0 || p[2] != 0;
        inked(&pixels[i..i + 3]) != inked(&upscaled[i..i + 3])
    })));
    assert!(!frame(&mut vm, camera).iter().any(|cmd| matches!(
        cmd, SceneCommand::UploadGlyph { .. } | SceneCommand::ReleaseResource { .. }
    )));
    assert_eq!(vm.native_text[&3], native);
    assert_eq!(renderer.measure(&mut vm, 3, "WあWW", true, 64).unwrap(), measured);
    assert_eq!(vm.native_custom_glyphs.bitmap_by_identity(custom_id).unwrap().descriptor, 9);
}

#[test]
fn native_proportional_switch_uses_real_advance_and_fixed_mode_keeps_half_em() {
    let (renderer, mut vm) = fixture();
    vm.process_global_words.insert(0x004f_0580, 1);
    renderer.append(&mut vm, 3, "WW", 1280).unwrap();
    let proportional = vm.native_text[&3].cursor[0];
    let surface = vm.native_text.get_mut(&3).unwrap();
    surface.glyphs.clear();
    surface.cursor = [44, 60];
    vm.process_global_words.insert(0x004f_0580, 0);
    renderer.append(&mut vm, 3, "WW", 1280).unwrap();
    assert_eq!(vm.native_text[&3].cursor[0], 44 + 2 * 17);
    assert!(proportional > vm.native_text[&3].cursor[0]);
}

#[test]
fn static_controls_and_ruby_preserve_native_append_and_atomic_error() {
    let (renderer, mut vm) = fixture();
    vm.process_global_words.insert(0x004f_0580, 0);
    renderer
        .append(&mut vm, 3, "A\\n{漢x字/かんじ}\\c\\t\\w1\\@B", 1280)
        .unwrap();
    let surface = &vm.native_text[&3];
    assert_eq!(
        surface
            .glyphs
            .iter()
            .map(|glyph| glyph.text.as_str())
            .collect::<String>(),
        "A漢字\\B"
    );
    assert_eq!(surface.glyphs[1].position, [44, 95]);
    let previous = surface.clone();
    assert!(renderer.append(&mut vm, 3, "D\\q", 1280).is_err());
    assert_eq!(vm.native_text[&3], previous);
    assert!(renderer.append(&mut vm, 3, "{漢/ruby", 1280).is_err());
    assert_eq!(vm.native_text[&3], previous);
}

#[test]
fn kinsoku_and_narrow_punctuation_use_native_codes() {
    let (renderer, mut vm) = fixture();
    vm.process_global_words.insert(0x004f_0580, 0);
    let surface = vm.native_text.get_mut(&3).unwrap();
    surface.rect[2] = 62;
    renderer.append(&mut vm, 3, "漢、字", 1280).unwrap();
    let glyphs = &vm.native_text[&3].glyphs;
    assert_eq!(glyphs[1].position, [76, 60]);
    assert_eq!(glyphs[2].position, [44, 95]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_virtual_text_coordinates_scale_origin_local_offsets_and_font_pixels() {
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    let (renderer, mut vm) = fixture_at([128, 128]);
    scene.text = renderer;
    scene.clear(128, 128).unwrap();
    let surface = vm.native_text.get_mut(&3).unwrap();
    surface.virtual_size = [256, 1024];
    surface.position = [48, 128];
    surface.rect = [8, 0, 200, 100];
    surface.cursor = [8, 0];
    surface.style = 0;
    scene.append_text(&mut vm, 3, "W").unwrap();
    let frame = scene
        .render_overlays(&mut vm, 128, 128, None, Vec::new())
        .unwrap();
    let lit: Vec<_> = frame
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, pixel)| pixel[..3].iter().any(|value| *value != 0))
        .map(|(index, _)| ((index % 128) as u32, (index / 128) as u32))
        .collect();
    assert!(!lit.is_empty());
    assert!(
        lit.iter()
            .all(|(x, y)| (28..50).contains(x) && (64..90).contains(y))
    );
    assert!(
        lit.iter().map(|(_, y)| y).max().unwrap() - lit.iter().map(|(_, y)| y).min().unwrap() < 20
    );
}

#[test]
fn missing_and_unbound_default_fonts_do_not_substitute() {
    let database = fontdb::Database::new();
    assert_eq!(
        fonts::load(&database, "", 400).unwrap_err().code(),
        "SHINKI_EMU_CMVS_DEFAULT_FONT_UNBOUND"
    );
    assert_eq!(
        fonts::load(&database, "Absent Font", 400)
            .unwrap_err()
            .code(),
        "SHINKI_EMU_CMVS_FONT_MISSING"
    );
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_dialogue_reveals_fades_expands_and_clears_real_glyph_pixels() {
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    let (renderer, mut vm) = fixture_at([128, 128]);
    scene.text = renderer;
    scene.clear(128, 128).unwrap();
    vm.effect_channels.entry(0).or_default().write_dword(0, 3);
    scene.append_dialogue(&mut vm, 0, 3, "W").unwrap();
    let draw = |scene: &mut CmvsScene, vm: &mut CmvsPs2aVmState| {
        scene
            .render_overlays(vm, 128, 128, None, Vec::new())
            .unwrap()
    };
    let energy = |frame: &TextureFrame| {
        frame
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| u64::from(pixel[0]) + u64::from(pixel[1]) + u64::from(pixel[2]))
            .sum::<u64>()
    };
    assert_eq!(energy(&draw(&mut scene, &mut vm)), 0);
    crate::vm::native_text::dialogue::advance(&mut vm, 175);
    let half = draw(&mut scene, &mut vm);
    crate::vm::native_text::dialogue::advance(&mut vm, 175);
    let full = draw(&mut scene, &mut vm);
    assert!(energy(&half) > 0 && energy(&half) < energy(&full));

    vm.native_text.get_mut(&3).unwrap().glyphs.clear();
    vm.native_text.get_mut(&3).unwrap().cursor = [44, 60];
    vm.write_interpreter_word(1480, 2);
    scene.append_dialogue(&mut vm, 0, 3, "W").unwrap();
    assert_eq!(energy(&draw(&mut scene, &mut vm)), 0);
    crate::vm::native_text::dialogue::advance(&mut vm, 175);
    let narrow = draw(&mut scene, &mut vm);
    crate::vm::native_text::dialogue::advance(&mut vm, 175);
    let wide = draw(&mut scene, &mut vm);
    let span = |frame: &TextureFrame| {
        let xs: Vec<_> = frame
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, pixel)| pixel[..3].iter().any(|value| *value != 0))
            .map(|(index, _)| index as u32 % frame.width)
            .collect();
        xs.iter().max().unwrap() - xs.iter().min().unwrap()
    };
    assert!(span(&narrow) < span(&wide));
    vm.native_text.get_mut(&3).unwrap().visible = false;
    assert_eq!(energy(&draw(&mut scene, &mut vm)), 0);
    vm.native_text.get_mut(&3).unwrap().visible = true;
    assert_eq!(draw(&mut scene, &mut vm).rgba8, wide.rgba8);
    vm.native_text.clear();
    vm.native_text_reveals.clear();
    assert_eq!(energy(&draw(&mut scene, &mut vm)), 0);
}

fn channel_command(
    vm: &mut CmvsPs2aVmState,
    command_id: u16,
    arguments: &[u32],
) -> Option<crate::CmvsPs2aVmAction> {
    let span = crate::CmvsPs2aSourceSpan {
        offset: 0,
        byte_length: 2,
    };
    for value in arguments.iter().rev() {
        crate::vm::execute_cmvs390_frame(
            vm,
            &crate::CmvsPs2aInstructionFrame::Control(
                crate::CmvsPs2aControlInstruction::PushImmediate {
                    span,
                    stack_bytes: 4,
                    value: *value,
                },
            ),
        )
        .unwrap();
    }
    crate::vm::execute_cmvs390_frame(
        vm,
        &crate::CmvsPs2aInstructionFrame::Command { span, command_id },
    )
    .unwrap()
}

fn bind_channel(vm: &mut CmvsPs2aVmState) {
    vm.slot_objects.insert(742 << 8, 1);
    vm.slot_objects.insert(750 << 8 | 3, 2);
    vm.slot_objects.insert(762 << 8 | 3, 3);
    vm.effect_channels.entry(0).or_default().write_dword(0, 3);
    vm.native_text_fonts.insert(3, "Noto Sans JP".into());
    vm.effect_elements.entry((0, u32::MAX)).or_default().visible = 1;
    vm.process_global_words.insert(0x004f_0580, 0);
}

#[test]
fn channel_setters_reach_native_layout_without_changing_origin_or_rect() {
    let (renderer, mut vm) = fixture();
    bind_channel(&mut vm);
    channel_command(&mut vm, 336, &[0, 0, 80, 40]);
    channel_command(&mut vm, 325, &[0, 4, 6, 100, 1]);
    channel_command(&mut vm, 323, &[0, 20]);
    channel_command(&mut vm, 324, &[0, 3, 5]);
    channel_command(&mut vm, 323, &[0, 0]);
    channel_command(&mut vm, 324, &[0, u32::MAX, u32::MAX]);
    channel_command(&mut vm, 335, &[0, 10, 12]);
    channel_command(&mut vm, 326, &[0, 0x112233, 0x445566]);
    channel_command(&mut vm, 329, &[0, 1]);
    renderer.append(&mut vm, 3, "A\\nB", 1280).unwrap();
    let surface = &vm.native_text[&3];
    assert_eq!(surface.position, [80, 40]);
    assert_eq!(surface.rect, [4, 6, 100, 1]);
    assert_eq!(
        surface
            .glyphs
            .iter()
            .map(|glyph| glyph.position)
            .collect::<Vec<_>>(),
        [[10, 12], [4, 37]]
    );
    assert!(surface.glyphs.iter().all(|glyph| glyph.size == 20
        && glyph.colors == [0x112233, 0x445566]
        && glyph.color_mode == 1));
    assert_eq!(surface.cursor, [17, 37]);
}

#[test]
fn native_cursor40_does_not_turn_into_an_extra_font_cell_of_spacing() {
    let (renderer, mut vm) = fixture();
    bind_channel(&mut vm);
    channel_command(&mut vm, 325, &[0, 0, 0, 1000, 2400]);
    channel_command(&mut vm, 323, &[0, 40]);
    channel_command(&mut vm, 324, &[0, 0, 0]);
    channel_command(&mut vm, 335, &[0, 40, 40]);
    renderer.append(&mut vm, 3, "漢字\\n漢", 1280).unwrap();
    assert_eq!(
        vm.native_text[&3]
            .glyphs
            .iter()
            .map(|glyph| glyph.position)
            .collect::<Vec<_>>(),
        [[40, 40], [80, 40], [0, 80]]
    );
    channel_command(&mut vm, 324, &[0, 3, 5]);
    channel_command(&mut vm, 330, &[0]);
    channel_command(&mut vm, 335, &[0, 10, 12]);
    renderer.append(&mut vm, 3, "漢字", 1280).unwrap();
    assert_eq!(
        vm.native_text[&3]
            .glyphs
            .iter()
            .map(|glyph| glyph.position)
            .collect::<Vec<_>>(),
        [[10, 12], [53, 12]]
    );
}

#[test]
fn native_extent_queries_use_real_fonts_and_do_not_change_the_layout_cursor() {
    let (renderer, mut vm) = fixture();
    bind_channel(&mut vm);
    channel_command(&mut vm, 325, &[0, 4, 6, 500, 1]);
    channel_command(&mut vm, 323, &[0, 30]);
    channel_command(&mut vm, 324, &[0, 2, 5]);
    let before = vm.native_text[&3].clone();
    vm.process_string_slots.insert(0, b"WW\\nI".to_vec());
    let Some(crate::CmvsPs2aVmAction::NativeText(crate::CmvsNativeTextAction::Measure {
        slot,
        source,
        proportional,
    })) = channel_command(&mut vm, 285, &[3, 0x8000_0000])
    else {
        panic!("native extent query must invoke the bound font renderer");
    };
    let measured = renderer
        .measure(&mut vm, slot, &source, proportional, 1280)
        .unwrap();
    assert_eq!(measured[1], 2);
    assert!(measured[0] > measured[2] && measured[2] > 2);
    assert_eq!(vm.native_text[&3], before);
    vm.process_global_words.insert(0x004f_0580, 1);
    renderer.append(&mut vm, 3, &source, 1280).unwrap();
    let surface = &vm.native_text[&3];
    assert_eq!(surface.cursor[0] - surface.rect[0], measured[2] as i32);
    assert_eq!(surface.glyphs[2].position[1], surface.rect[1] + 35);
    assert_eq!(
        surface.glyphs[1].position[0] - surface.rect[0],
        measured[0] as i32 / 2
    );
    let fixed = renderer
        .measure(&mut vm, 3, "漢A\\nB", false, 1280)
        .unwrap();
    assert_eq!(fixed, [3, 2, 1]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_measured_window_extent_positions_the_real_text_and_publishes_native_results() {
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let mut scene = CmvsScene::new(&instance).unwrap();
    let (renderer, mut vm) = fixture_at([128, 96]);
    scene.text = renderer;
    scene.clear(128, 96).unwrap();
    bind_channel(&mut vm);
    vm.native_text.get_mut(&3).unwrap().virtual_size = [256, 1024];
    channel_command(&mut vm, 325, &[0, 0, 0, 256, 1]);
    channel_command(&mut vm, 323, &[0, 30]);
    channel_command(&mut vm, 324, &[0, 0, 0]);
    vm.native_text.get_mut(&3).unwrap().style = 0;
    vm.process_string_slots.insert(0, b"WW".to_vec());
    let Some(crate::CmvsPs2aVmAction::NativeText(crate::CmvsNativeTextAction::Measure {
        slot,
        source,
        proportional,
    })) = channel_command(&mut vm, 285, &[3, 0x8000_0000])
    else {
        panic!("native extent query must invoke the bound font renderer");
    };
    let before = vm.native_text[&3].clone();
    scene
        .measure_text(&mut vm, slot, &source, proportional)
        .unwrap();
    let width = vm.interpreter_words[&81224];
    assert!(width > 0 && width < 128);
    assert_eq!(vm.interpreter_words[&81220], 1);
    assert_eq!(vm.interpreter_words[&81228], 1);
    assert_eq!(vm.interpreter_words[&81236], width);
    assert_eq!(vm.native_text[&3], before);
    channel_command(&mut vm, 336, &[0, 0, (256 - width) / 2, 64]);
    scene.append_text(&mut vm, 3, &source).unwrap();
    let frame = scene
        .render_overlays(&mut vm, 128, 96, None, Vec::new())
        .unwrap();
    let lit: Vec<_> = frame
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, pixel)| pixel[..3].iter().any(|value| *value != 0))
        .map(|(index, _)| ((index % 128) as u32, (index / 128) as u32))
        .collect();
    assert!(!lit.is_empty());
    let minimum = lit.iter().map(|point| point.0).min().unwrap();
    let maximum = lit.iter().map(|point| point.0).max().unwrap();
    assert!(minimum >= (256 - width) / 4);
    assert!(maximum < (256 - width) / 4 + width / 2 + 2);
    assert!(lit.iter().all(|(_, y)| (32..56).contains(y)));
}

#[test]
fn measured_extended_kinsoku_owns_its_lists_and_default_wrap_keeps_paragraph_advance() {
    let (renderer, mut vm) = fixture();
    bind_channel(&mut vm);
    channel_command(&mut vm, 325, &[0, 0, 0, 40, 1]);
    channel_command(&mut vm, 323, &[0, 20]);
    channel_command(&mut vm, 324, &[0, 0, 0]);
    let before = vm.native_text[&3].clone();
    assert_eq!(
        renderer.measure(&mut vm, 3, "漢、字", false, 1280).unwrap(),
        [6, 2, 6]
    );
    vm.process_string_slots
        .insert(0, encoding_rs::SHIFT_JIS.encode("、").0.into_owned());
    vm.process_string_slots.insert(1, Vec::new());
    channel_command(&mut vm, 286, &[3, 0x8000_0000, 0x8000_0001, 1]);
    vm.process_string_slots
        .insert(0, encoding_rs::SHIFT_JIS.encode("字").0.into_owned());
    vm.input_advance_press = true;
    vm.input_advance_release = true;
    assert_eq!(
        renderer.measure(&mut vm, 3, "漢、字", false, 1280).unwrap(),
        [4, 2, 2]
    );
    assert_eq!(vm.native_text[&3], before);
    assert!(vm.input_advance_press && vm.input_advance_release);
    assert_eq!(
        renderer
            .measure(&mut vm, 3, "{漢x字/かんじ}", false, 1280)
            .unwrap(),
        [4, 1, 4]
    );
}

#[test]
fn extended_boundaries_match_query_static_and_dispatched_dialogue_line_positions() {
    // 462F80: first-list codes can hang independently; second-list codes
    // must repeat the last code recorded before the edge, within the limit.
    for (first, second, source, positions, measured) in [
        ("※", "", "漢※次", vec![[0, 0], [23, 0], [0, 25]], [4, 2, 2]),
        ("", "※", "漢※", vec![[0, 0], [0, 25]], [2, 2, 2]),
        ("", "※", "※※※", vec![[0, 0], [23, 0], [0, 25]], [4, 2, 2]),
        ("※", "※", "漢※※", vec![[0, 0], [23, 0], [0, 25]], [4, 2, 2]),
    ] {
        let (renderer, mut vm) = fixture();
        bind_channel(&mut vm);
        channel_command(&mut vm, 325, &[0, 0, 0, 46, 200]);
        channel_command(&mut vm, 323, &[0, 20]);
        channel_command(&mut vm, 324, &[0, 3, 5]);
        for (slot, text) in [(0, first), (1, second), (2, source)] {
            vm.process_string_slots
                .insert(slot, encoding_rs::SHIFT_JIS.encode(text).0.into_owned());
        }
        channel_command(&mut vm, 286, &[3, 0x8000_0000, 0x8000_0001, 1]);
        assert_eq!(
            renderer.measure(&mut vm, 3, source, false, 1280).unwrap(),
            measured
        );
        renderer.append(&mut vm, 3, source, 1280).unwrap();
        assert_eq!(
            vm.native_text[&3]
                .glyphs
                .iter()
                .map(|g| g.position)
                .collect::<Vec<_>>(),
            positions
        );
        channel_command(&mut vm, 330, &[0]);
        vm.write_interpreter_word(1464, 1);
        let Some(crate::CmvsPs2aVmAction::NativeText(crate::CmvsNativeTextAction::DrawDialogue {
            channel,
            slot,
            source,
        })) = channel_command(&mut vm, 338, &[0, u32::MAX, 0x8000_0002])
        else {
            panic!("dialogue must reach the bound renderer");
        };
        renderer
            .append_dialogue(&mut vm, channel, slot, &source, 1280)
            .unwrap();
        let surface = &vm.native_text[&3];
        assert_eq!(
            surface
                .glyphs
                .iter()
                .map(|g| g.position)
                .collect::<Vec<_>>(),
            positions
        );
        assert_eq!(surface.cursor, [23, 25]);
        assert_eq!(
            surface.glyphs.last().unwrap().position[1] + surface.size as i32,
            45
        );
    }
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_screen_shake_moves_real_glyphs_in_mode_pixels_and_pause_restores_them() {
    let (renderer, mut vm) = fixture_at([128, 128]);
    vm.native_stage_size = [128, 128];
    vm.native_render_size = [64, 64];
    vm.native_text.get_mut(&3).unwrap().physical_size = [64, 64];
    vm.native_frame_delta_ms = 10;
    let mut scene = CmvsScene::new(super::super::tests::gpu_instance()).unwrap();
    scene.text = renderer;
    scene.clear(128, 128).unwrap();
    scene.append_text(&mut vm, 3, "WW").unwrap();
    let original = scene
        .render_overlays(&mut vm, 128, 128, None, Vec::new())
        .unwrap();
    assert!(
        original
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] != [0, 0, 0])
    );
    super::super::parent_load_tests::command(&mut vm, 549, &[0, 1, 10, u32::MAX]);
    crate::vm::screen_shake::advance(&mut vm).unwrap();
    let shifted = scene
        .render_overlays(&mut vm, 128, 128, None, Vec::new())
        .unwrap();
    for y in 0..128 {
        for x in 0..126 {
            let source = (y * 128 + x) * 4;
            let target = (y * 128 + x + 2) * 4;
            assert_eq!(
                &original.rgba8[source..source + 4],
                &shifted.rgba8[target..target + 4]
            );
        }
    }
    super::super::parent_load_tests::command(&mut vm, 551, &[1]);
    let paused = scene
        .render_overlays(&mut vm, 128, 128, None, Vec::new())
        .unwrap();
    assert_eq!(original.rgba8.as_slice(), paused.rgba8.as_slice());
}

#[test]
#[ignore = "requires a hardware GPU"]
fn native_mode_rasterizes_before_mapping_the_logical_base() {
    let draw = |logical: [u32; 2]| {
        let (renderer, mut vm) = fixture_at(logical);
        vm.native_render_size = [1280, 720];
        let surface = vm.native_text.get_mut(&3).unwrap();
        surface.physical_size = [1280, 720];
        surface.virtual_size = [1920, 1080];
        surface.position = [96, 96];
        surface.rect = [0, 0, 512, 100];
        surface.cursor = [0, 0];
        surface.size = 40;
        surface.character_spacing = 0;
        surface.style = 0;
        let mut scene = CmvsScene::new(super::super::tests::gpu_instance()).unwrap();
        scene.text = renderer;
        scene.resize_raster(1280, 720).unwrap();
        scene.clear(logical[0], logical[1]).unwrap();
        scene.append_text(&mut vm, 3, "そ").unwrap();
        scene
            .render_overlays(&mut vm, logical[0], logical[1], None, Vec::new())
            .unwrap()
    };
    let physical = draw([1280, 720]);
    let split = draw([1920, 1080]);
    assert_eq!(physical.rgba8.as_slice(), split.rgba8.as_slice());
}
