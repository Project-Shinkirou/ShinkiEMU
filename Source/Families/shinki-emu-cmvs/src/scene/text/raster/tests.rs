use super::*;

fn settings() -> NativeRasterSettings {
    NativeRasterSettings {
        proportional: true,
        alternate_coverage: false,
    }
}
fn raster() -> CmvsNativeFontRasterState {
    CmvsNativeFontRasterState {
        proportional: true,
        ..Default::default()
    }
}
fn style(color: u32) -> CmvsNativeTextGlyph {
    CmvsNativeTextGlyph {
        text: "g".into(),
        position: [0, 0],
        font: "fixture".into(),
        size: 4,
        colors: [color; 2],
        style: 1,
        color_mode: 0,
        code: b'g' as u16,
        raster: raster(),
    }
}
fn renderer() -> NativeRasterRenderer {
    let mut renderer = NativeRasterRenderer::default();
    let state = renderer.state.get_mut().unwrap();
    state.bindings.insert("fixture".into(), [0, 0]);
    let key = state.key("fixture", b'g' as u16, 4, raster()).unwrap();
    let gm = cell::GlyphMetrics {
        black_box: [1, 3],
        origin: [-1, 1],
        cell_inc_x: 2,
    };
    let tm = cell::FaceMetrics {
        ascent: 4,
        ..Default::default()
    };
    // A deterministic pre-existing cell enters the same composition,
    // resource ownership, placement and retirement path as a real raster.
    let value = cell::compose(
        4,
        b'g' as u16,
        &gm,
        &tm,
        &[16, 0, 0, 0, 8, 0, 0, 0, 1, 0, 0, 0],
        settings(),
    )
    .unwrap();
    state.cells.insert(key, CachedCell { value, live: true });
    renderer
}
fn run<'a>(id: &str, style: &'a CmvsNativeTextGlyph) -> NativeRasterRun<'a> {
    NativeRasterRun {
        id: id.into(),
        font_name: "fixture",
        style,
        origin: [10, 20],
        visual: [0.5, 0.75],
        pixel_size: 4,
        logical_scale: 1.0,
        custom: None,
    }
}

#[test]
fn cache_hits_do_not_upload_and_only_live_paint_variants_survive() {
    let mut renderer = renderer();
    let red = style(0xff0000);
    let blue = style(0x0000ff);
    let first = renderer.frame(&[run("same", &red)]).unwrap();
    let SceneCommand::UploadGlyph {
        resource_id: old, ..
    } = &first[0]
    else {
        panic!("missing upload")
    };
    let cached = renderer.frame(&[run("same", &red)]).unwrap();
    assert_eq!(cached.len(), 3);
    assert!(
        !cached
            .iter()
            .any(|cmd| matches!(cmd, SceneCommand::UploadGlyph { .. }))
    );
    let changed = renderer.frame(&[run("same", &blue)]).unwrap();
    assert!(
        matches!(&changed[0], SceneCommand::ReleaseResource { resource_id } if resource_id == old)
    );
    let SceneCommand::UploadGlyph {
        resource_id: new, ..
    } = &changed[1]
    else {
        panic!("missing recolor upload")
    };
    assert_ne!(new, old);
    assert_eq!(renderer.variants.len(), 1);
    let released = renderer.frame(&[]).unwrap();
    assert!(
        matches!(released.as_slice(), [SceneCommand::ReleaseResource { resource_id }] if resource_id == new)
    );
    assert!(renderer.variants.is_empty());
    assert!(renderer.state.get_mut().unwrap().cells.is_empty());
    assert!(renderer.frame(&[]).unwrap().is_empty());
}

#[test]
fn failed_frame_does_not_publish_or_reuse_an_unuploaded_variant() {
    let mut renderer = renderer();
    let valid = style(0xff0000);
    let invalid = style(0x0000ff);
    let mut invalid_run = run("invalid", &invalid);
    invalid_run.pixel_size = 0;
    assert!(
        renderer
            .frame(&[run("valid", &valid), invalid_run])
            .is_err()
    );
    assert!(renderer.variants.is_empty());
    let commands = renderer.frame(&[run("valid", &valid)]).unwrap();
    assert!(matches!(commands[0], SceneCommand::UploadGlyph { .. }));
}

#[test]
fn font_rebinding_retires_uploaded_resources_once() {
    let mut renderer = renderer();
    let style = style(0xffffff);
    renderer.frame(&[run("cell", &style)]).unwrap();
    renderer
        .bind_fonts(&BTreeMap::new(), &BTreeMap::new())
        .unwrap();
    renderer
        .bind_fonts(&BTreeMap::new(), &BTreeMap::new())
        .unwrap();
    let commands = renderer.frame(&[]).unwrap();
    assert!(matches!(
        commands.as_slice(),
        [SceneCommand::ReleaseResource { .. }]
    ));
    assert!(renderer.frame(&[]).unwrap().is_empty());
}

#[test]
fn custom_descriptor_changes_update_advance_without_reuploading_pixels() {
    let mut renderer = renderer();
    let mut style = style(0xffffff);
    style.style = 0;
    let mut bitmap = CmvsNativeCustomGlyphBitmap {
        identity: 17,
        size: 4,
        descriptor: 7,
        coverage: vec![17, 128, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    };
    style.raster.custom_bitmap_identity = Some(bitmap.identity);
    style.raster.custom_advance = Some(bitmap.descriptor);
    let mut request = run("custom", &style);
    request.custom = Some(&bitmap);
    let commands = renderer.frame(&[request]).unwrap();
    let SceneCommand::UploadGlyph { resource_id, .. } = &commands[0] else {
        panic!("missing custom upload")
    };
    let resource_id = resource_id.clone();
    assert_eq!(
        renderer
            .advance("fixture", style.code, 4, style.raster)
            .unwrap(),
        7
    );
    bitmap.descriptor = 11;
    // Existing uploaded glyph keeps its measured advance.
    assert_eq!(
        renderer
            .advance("fixture", style.code, 4, style.raster)
            .unwrap(),
        7
    );
    style.raster.custom_advance = Some(bitmap.descriptor);
    let mut request = run("custom", &style);
    request.custom = Some(&bitmap);
    let commands = renderer.frame(&[request]).unwrap();
    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, SceneCommand::UploadGlyph { .. }))
    );
    let SceneCommand::GlyphRun { glyphs, .. } = &commands[1] else {
        panic!("missing retained custom draw")
    };
    assert_eq!(glyphs[0].resource_id, resource_id);
    assert_eq!(
        renderer
            .advance("fixture", style.code, 4, style.raster)
            .unwrap(),
        11
    );
    // Custom bitmaps never rasterize the bound face.
    assert!(renderer.state.get_mut().unwrap().fonts.is_empty());
}
