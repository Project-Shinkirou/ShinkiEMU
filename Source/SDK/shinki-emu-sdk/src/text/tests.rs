use super::*;
use roukaku_media_core::{
    CpuRendererProvider, RenderTargetFormat, Renderer2DProvider, RendererCreateRequest,
};
use roukaku_text::*;

fn fixture() -> (TextScene, TextLayoutRequest) {
    let bytes = include_bytes!("../../../../../Assets/Fonts/NotoSansJP-Variable.ttf");
    let font = PackagedFont {
        asset_id: "asset:/test/noto".into(),
        family: "Noto Sans JP".into(),
        face_index: 0,
        hash: Hash256::from_sha256(bytes),
        license_id: "OFL-1.1".into(),
        subset: None,
        coverage: font_unicode_coverage(bytes, 0).unwrap(),
        targets: vec!["test".into()],
        profiles: vec!["test".into()],
        bytes: bytes.to_vec(),
    };
    let provider = CosmicTextLayoutProvider::new(
        FontBindingContext {
            target: "test".into(),
            profile: "test".into(),
            default_locale: "ja-JP".into(),
        },
        vec![font],
        TextLayoutConfig::production_defaults(),
    )
    .unwrap();
    let request = TextLayoutRequest {
        key: "native-character".into(),
        runs: vec![TextRun {
            text: "W".into(),
            language: "ja-JP".into(),
            script: Some("Jpan".into()),
            direction: TextDirection::LeftToRight,
            ruby: Vec::new(),
            voice: None,
        }],
        constraint: LayoutConstraint {
            max_width: 128.0,
            max_height: None,
            max_lines: None,
            font_size: 30.0,
            line_height: 30.0,
            wrap: WrapPolicy::None,
            overflow: OverflowPolicy::Visible,
        },
        font_families: vec!["Noto Sans JP".into()],
        features: Vec::new(),
    };
    (TextScene::new(provider), request)
}

#[test]
fn measuring_shares_layout_without_consuming_glyph_lifecycle() {
    let (mut scene, request) = fixture();
    let first = scene.layout_shared(&request).unwrap();
    let second = scene.layout_shared(&request).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert!(first.shaped_runs[0].glyphs[0].advance > 15.0);
    assert!(scene.clear().is_empty());
    let region = TextSceneLayout {
        request,
        translation: (0, 0),
        rgba: [255; 4],
        outline: None,
    };
    assert!(
        scene
            .frame(std::slice::from_ref(&region))
            .unwrap()
            .iter()
            .any(|command| matches!(command, SceneCommand::UploadGlyph { .. }))
    );
    assert!(
        !scene
            .frame(&[region])
            .unwrap()
            .iter()
            .any(|command| matches!(command, SceneCommand::UploadGlyph { .. }))
    );
    assert!(!scene.clear().is_empty());
}

#[test]
fn outline_matches_a_disk_max_minus_fill_using_color_glyph_alpha() {
    let coverage = [0, 90, 255, 128, 20, 0];
    let rgba = coverage
        .iter()
        .flat_map(|&alpha| [255, 40, 75, alpha])
        .collect();
    let source = GlyphBitmap::from_vec(3, 2, GlyphBitmapFormat::Rgba8, rgba).unwrap();
    for radius in [1, 2, 5, MAX_RASTER_OUTLINE_RADIUS] {
        let (width, height) = outline_dimensions(&source, radius).unwrap();
        let result = dilate_outline(&source, radius, width, height).unwrap();
        let r = isize::from(radius);
        for (i, &actual) in result.pixels.iter().enumerate() {
            let x = (i % width) as isize - r;
            let y = (i / width) as isize - r;
            let mut max = 0;
            let mut fill = 0;
            for (j, &alpha) in coverage.iter().enumerate() {
                let dx = x - (j % 3) as isize;
                let dy = y - (j / 3) as isize;
                if dx * dx + dy * dy <= r * r {
                    max = max.max(alpha);
                }
                if dx == 0 && dy == 0 {
                    fill = alpha;
                }
            }
            assert_eq!(actual, max - fill, "radius {radius} at ({x}, {y})");
        }
    }
}

#[test]
fn translucent_outline_uses_one_coverage_mask_before_the_clipped_fill() {
    let (mut scene, request) = fixture();
    let mut region = TextSceneLayout {
        request,
        translation: (1, 1),
        rgba: [0, 255, 0, 255],
        outline: Some(TextOutline {
            radius: 2,
            rgba: [255, 0, 0, 128],
        }),
    };
    // Replace only the raster inputs so the expected pixel coverage is exact.
    let mut positioned = scene.layout_shared(&region.request).unwrap();
    let layout = Arc::make_mut(&mut positioned);
    let glyph = &mut layout.shaped_runs[0].glyphs[0];
    glyph.render_x = Some(3);
    glyph.render_y = Some(3);
    layout.glyph_resources[0].bitmap =
        GlyphBitmap::from_vec(2, 1, GlyphBitmapFormat::Alpha8, vec![128, 255]).unwrap();
    layout.clip = Some(LayoutClip {
        x: 2,
        y: 0,
        width: 6,
        height: 8,
    });

    let mut renderer = CpuRendererProvider
        .create(RendererCreateRequest {
            width: 10,
            height: 9,
            format: RenderTargetFormat::Rgba8Srgb,
            profile: "text-outline".into(),
        })
        .unwrap();
    let mut draw = |scene: &mut TextScene, region: &TextSceneLayout| {
        let frame = scene
            .frame_parts_with_layout(std::slice::from_ref(region), 1.0, |_, _| {
                Arc::clone(&positioned)
            })
            .unwrap();
        let mut commands = frame.lifecycle;
        let lifecycle = commands.len();
        commands.push(SceneCommand::clear([0; 4]));
        for layer in frame.layouts {
            commands.extend(layer.commands);
        }
        (lifecycle, renderer.capture_frame(&commands).unwrap().bytes)
    };
    let (lifecycle, pixels) = draw(&mut scene, &region);
    let pixel = |x: usize, y: usize| &pixels[(y * 10 + x) * 4..(y * 10 + x) * 4 + 4];
    assert_eq!(lifecycle, 2);
    assert_eq!(pixel(4, 3), &[255, 0, 0, 128]); // Two source samples overlap in the disk.
    assert_eq!(pixel(5, 4), &[0, 255, 0, 255]); // Opaque fill has no underlying stroke.
    assert_eq!(pixel(4, 4)[3], 159);
    assert!(pixel(4, 4)[1] > pixel(4, 4)[0]); // Partial fill sits above the stroke.
    assert_eq!(pixel(2, 4), &[0; 4]); // Same region clip for outline and fill.
    assert_eq!(pixel(7, 2), &[0; 4]); // The disk does not include its square corner.

    region.translation.0 += 1;
    let (lifecycle, moved) = draw(&mut scene, &region);
    assert_eq!(lifecycle, 0);
    assert_eq!(&moved[(3 * 10 + 5) * 4..][..4], pixel(4, 3));
    region.outline = None;
    let (lifecycle, without_stroke) = draw(&mut scene, &region);
    assert_eq!(lifecycle, 1);
    assert_eq!(&without_stroke[(3 * 10 + 5) * 4..][..4], &[0; 4]);
}
