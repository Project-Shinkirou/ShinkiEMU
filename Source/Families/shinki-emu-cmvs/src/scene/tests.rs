use super::*;
use crate::CmvsTextureParentState;
use std::collections::BTreeMap;

fn state() -> CmvsPs2aVmState {
    let mut vm = CmvsPs2aVmState::new(0);
    // Native 4473C0 draws descending depth: foreground slot 0 must be nearer.
    for (slot, z) in [(0, 1), (1, 2)] {
        vm.texture_parents.insert(
            slot,
            CmvsTextureParentState {
                surface_initialized: true,
                rect_words: Some([0, 0, 2, 2]),
                position_words: Some([0, 0]),
                auxiliary_word: Some(z),
                ..Default::default()
            },
        );
    }
    vm
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_composes_recovered_layer_order_and_replaces_texture_sizes() {
    let mut scene = CmvsScene::new(gpu_instance()).unwrap();
    let vm = state();
    let slots = [CmvsTextureSlot::Parent(0), CmvsTextureSlot::Parent(1)];
    for size in [1, 2, 1] {
        let commands = stage_geometry(&vm, slots.into_iter(), slots[0], true, &mut BTreeMap::new())
            .unwrap()
            .into_iter()
            .map(|(slot, destination, _)| {
                let color = if slot == slots[0] {
                    [255, 0, 0, 255]
                } else {
                    [0, 255, 0, 255]
                };
                SceneCommand::Texture {
                    id: slot_id(slot),
                    frame: TextureFrame {
                        width: size,
                        height: size,
                        rgba8: color.repeat((size * size) as usize).into(),
                    },
                    destination,
                    opacity: 1.0,
                    blend: BlendMode::Alpha,
                }
            })
            .collect();
        let frame = scene.draw(2, 2, commands).unwrap();
        assert!(
            frame
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [255, 0, 0, 255])
        );
    }
    assert_eq!(scene.sequence, 3);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_presents_movie_without_texture_target_and_clears_released_frame() {
    let mut scene = CmvsScene::new(gpu_instance()).unwrap();
    let mut vm = CmvsPs2aVmState::new(0);
    for color in [[255, 0, 0, 255], [0, 255, 0, 255]] {
        let movie = TextureFrame {
            width: 2,
            height: 2,
            rgba8: color.repeat(4).into(),
        };
        let frame = scene
            .render_overlays(&mut vm, 2, 2, Some(&movie), Vec::new())
            .unwrap();
        assert!(
            frame
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == color)
        );
    }
    let native_size_movie =
        TextureFrame::from_vec(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
    let native_size_frame = scene
        .render_overlays(&mut vm, 4, 3, Some(&native_size_movie), Vec::new())
        .unwrap();
    assert_eq!(
        &native_size_frame.rgba8[..8],
        &[255, 0, 0, 255, 0, 0, 255, 255]
    );
    assert!(
        native_size_frame.rgba8[8..]
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [0, 0, 0, 255])
    );
    let frame = scene
        .render_overlays(&mut vm, 2, 2, None, Vec::new())
        .unwrap();
    assert!(
        frame
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [0, 0, 0, 255])
    );
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_raster_resize_keeps_stage_geometry_and_releases_each_renderer() {
    let instance = gpu_instance();
    for _ in 0..32 {
        let mut scene = CmvsScene::new(instance).unwrap();
        let mut failed_candidate = CmvsScene::new(instance).unwrap();
        assert!(failed_candidate.draw(0, 2, Vec::new()).is_err());
        drop(failed_candidate);
        assert!(instance.poll_all(true));
        assert_eq!(
            instance
                .generate_report()
                .unwrap()
                .hub
                .devices
                .num_kept_from_user,
            1
        );
        for (width, height) in [(6, 4), (4, 6), (2, 2)] {
            scene.resize_raster(width, height).unwrap();
            let frame = scene
                .draw(
                    2,
                    2,
                    vec![SceneCommand::Texture {
                        id: "cmvs.test.stage".into(),
                        frame: TextureFrame {
                            width: 1,
                            height: 1,
                            rgba8: vec![255, 0, 0, 255].into(),
                        },
                        destination: RectI {
                            x: 0,
                            y: 0,
                            width: 2,
                            height: 2,
                        },
                        opacity: 1.0,
                        blend: BlendMode::Alpha,
                    }],
                )
                .unwrap();
            assert_eq!((frame.width, frame.height), (width, height));
            assert_eq!(scene.logical_extent(), Some(Extent2D::new(2, 2)));
            let before_canvas = scene.canvas;
            let before_raster = scene.raster;
            let before_sequence = scene.sequence;
            for (bad_width, bad_height) in [(0, height), (width, 0), (u32::MAX, height)] {
                assert!(scene.resize_raster(bad_width, bad_height).is_err());
                assert_eq!(scene.canvas, before_canvas);
                assert_eq!(scene.raster, before_raster);
                assert_eq!(scene.sequence, before_sequence);
            }
            let viewport = scene.canvas.unwrap().viewport();
            for y in 0..height {
                for x in 0..width {
                    let offset = ((y * width + x) * 4) as usize;
                    let inside = x >= viewport.x
                        && x < viewport.x + viewport.width
                        && y >= viewport.y
                        && y < viewport.y + viewport.height;
                    assert_eq!(
                        &frame.rgba8[offset..offset + 4],
                        if inside {
                            &[255, 0, 0, 255]
                        } else {
                            &[0, 0, 0, 255]
                        }
                    );
                }
            }
        }
        drop(scene);
        assert!(instance.poll_all(true));
        let report = instance.generate_report().unwrap();
        for (name, registry) in [
            ("adapters", &report.hub.adapters),
            ("devices", &report.hub.devices),
            ("queues", &report.hub.queues),
            ("buffers", &report.hub.buffers),
            ("textures", &report.hub.textures),
            ("texture_views", &report.hub.texture_views),
            ("bind_group_layouts", &report.hub.bind_group_layouts),
            ("bind_groups", &report.hub.bind_groups),
            ("pipeline_layouts", &report.hub.pipeline_layouts),
            ("shader_modules", &report.hub.shader_modules),
            ("command_encoders", &report.hub.command_encoders),
            ("command_buffers", &report.hub.command_buffers),
            ("render_bundles", &report.hub.render_bundles),
            ("render_pipelines", &report.hub.render_pipelines),
            ("compute_pipelines", &report.hub.compute_pipelines),
            ("pipeline_caches", &report.hub.pipeline_caches),
            ("query_sets", &report.hub.query_sets),
            ("external_textures", &report.hub.external_textures),
            ("samplers", &report.hub.samplers),
            ("surfaces", &report.surfaces),
        ] {
            assert!(registry.is_empty(), "retained {name}: {registry:?}");
        }
    }
}

pub(crate) fn gpu_instance() -> &'static wgpu::Instance {
    static INSTANCE: std::sync::OnceLock<wgpu::Instance> = std::sync::OnceLock::new();
    INSTANCE.get_or_init(|| roukaku_platform_common::native_wgpu_instance().unwrap())
}

#[test]
fn presentation_mask_changes_coverage_without_replacing_color_or_source_alpha() {
    let presentation = crate::vm::texture_presentation::TexturePresentation {
        parent: 28,
        mask: "mask".into(),
        duration_ms: 1000,
        elapsed_ms: 500,
        showing: true,
    };
    let mut frame = TextureFrame {
        width: 1,
        height: 1,
        rgba8: vec![10, 20, 30, 128].into(),
    };
    presentation::apply_mask(&mut frame, &[128], &presentation.alpha_lut());
    assert_eq!(frame.rgba8.as_slice(), &[10, 20, 30, 64]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_masks_composed_parent_and_child_without_drawing_the_mask_as_color() {
    let mut scene = CmvsScene::new(gpu_instance()).unwrap();
    let texture = |id: &str, color: [u8; 4], x, width| SceneCommand::Texture {
        id: id.into(),
        frame: TextureFrame {
            width: 1,
            height: 1,
            rgba8: color.to_vec().into(),
        },
        destination: RectI {
            x,
            y: 0,
            width,
            height: 1,
        },
        opacity: 1.0,
        blend: BlendMode::Alpha,
    };
    let mut family = scene
        .draw(
            2,
            1,
            vec![
                texture("parent", [255, 0, 0, 255], 0, 2),
                texture("child", [0, 255, 0, 255], 1, 1),
            ],
        )
        .unwrap();
    let presentation = crate::vm::texture_presentation::TexturePresentation {
        parent: 28,
        mask: "mask".into(),
        duration_ms: 1000,
        elapsed_ms: 500,
        showing: true,
    };
    presentation::apply_mask(&mut family, &[0, 255], &presentation.alpha_lut());
    let frame = scene
        .draw(
            2,
            1,
            vec![
                texture("background", [0, 0, 255, 255], 0, 2),
                SceneCommand::Texture {
                    id: "masked-family".into(),
                    frame: family,
                    destination: RectI {
                        x: 0,
                        y: 0,
                        width: 2,
                        height: 1,
                    },
                    opacity: 1.0,
                    blend: BlendMode::Alpha,
                },
            ],
        )
        .unwrap();
    assert_eq!(frame.rgba8.as_slice(), &[255, 0, 0, 255, 0, 0, 255, 255]);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_channel_atlas_crops_positions_and_respects_root_subtree_visibility() {
    let mut scene = CmvsScene::new(gpu_instance()).unwrap();
    let mut vm = CmvsPs2aVmState::new(0);
    vm.slot_objects.insert(742 << 8, 1);
    vm.effect_channels.entry(0).or_default().set_origin(1, 0);
    let run = |vm: &mut CmvsPs2aVmState, id, args: &[u32]| {
        let span = crate::CmvsPs2aSourceSpan {
            offset: 0,
            byte_length: 2,
        };
        for value in args.iter().rev() {
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
            &crate::CmvsPs2aInstructionFrame::Command {
                span,
                command_id: id,
            },
        )
        .unwrap();
    };
    let atlas = TextureFrame {
        width: 2,
        height: 1,
        rgba8: vec![255, 0, 0, 255, 0, 255, 0, 255].into(),
    };
    let draw = |scene: &mut CmvsScene, vm: &CmvsPs2aVmState| {
        let mut commands = Vec::new();
        scene
            .append_channel_images(vm, 0, "fixture:atlas", &atlas, &mut commands)
            .unwrap();
        scene.draw(4, 1, commands).unwrap().rgba8.to_vec()
    };
    let expected = |index: usize, color: [u8; 4]| {
        let mut pixels = [0, 0, 0, 255].repeat(4);
        pixels[index * 4..index * 4 + 4].copy_from_slice(&color);
        pixels
    };
    run(&mut vm, 376, &[0, u32::MAX]);
    run(&mut vm, 376, &[0, 16]);
    run(&mut vm, 386, &[0, 16, 1, 0, 1, 1]);
    run(&mut vm, 387, &[0, 16, 1, 0]);
    run(&mut vm, 402, &[0, 0]);
    assert_eq!(draw(&mut scene, &vm), [0, 0, 0, 255].repeat(4));
    let root = vm.effect_elements.entry((0, u32::MAX)).or_default();
    root.visible = 1;
    root.source = Some([0, 0, 0, 0]);
    assert_eq!(draw(&mut scene, &vm), expected(2, [0, 255, 0, 255]));
    vm.effect_elements.get_mut(&(0, u32::MAX)).unwrap().visible = 0;
    assert_eq!(draw(&mut scene, &vm), [0, 0, 0, 255].repeat(4));
    vm.effect_elements.get_mut(&(0, u32::MAX)).unwrap().visible = 1;
    assert_eq!(draw(&mut scene, &vm), expected(2, [0, 255, 0, 255]));
    run(&mut vm, 400, &[0, 0, 1, 0, 0, 1, 1, 0, 0]);
    run(&mut vm, 406, &[0, 0, 1]);
    assert_eq!(draw(&mut scene, &vm), expected(2, [255, 0, 0, 255]));
    run(&mut vm, 403, &[0, 0]);
    run(&mut vm, 406, &[0, 0, 0]);
    assert_eq!(draw(&mut scene, &vm), [0, 0, 0, 255].repeat(4));
    vm.effect_elements.get_mut(&(0, u32::MAX)).unwrap().visible = 0;
    assert_eq!(draw(&mut scene, &vm), [0, 0, 0, 255].repeat(4));
    vm.effect_elements.get_mut(&(0, u32::MAX)).unwrap().visible = 1;
    assert_eq!(draw(&mut scene, &vm), [0, 0, 0, 255].repeat(4));
    run(&mut vm, 386, &[0, 16, u32::MAX, 0, 2, 1]);
    run(&mut vm, 387, &[0, 16, u32::MAX, 0]);
    run(&mut vm, 402, &[0, 0]);
    assert_eq!(draw(&mut scene, &vm), expected(1, [255, 0, 0, 255]));
    run(&mut vm, 376, &[0, 16]);
    run(&mut vm, 402, &[0, 0]);
    assert_eq!(draw(&mut scene, &vm), [0, 0, 0, 255].repeat(4));
    vm.effect_elements.get_mut(&(0, u32::MAX)).unwrap().source = Some([0, 0, 1, 1]);
    assert_eq!(draw(&mut scene, &vm), expected(1, [255, 0, 0, 255]));
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_host_raster_projection_scales_native_surface_translation_after_local_geometry() {
    let (_root, archive, mut vm) = super::parent_load_tests::fixture();
    super::parent_load_tests::command(&mut vm, 32, &[0]);
    super::parent_load_tests::command(&mut vm, 64, &[0]);
    super::parent_load_tests::command(&mut vm, 67, &[0, u32::MAX, 2, 2, 0x00ff_0000]);
    super::parent_load_tests::command(&mut vm, 69, &[0, u32::MAX, 4, 2]);
    let mut scene = CmvsScene::new(gpu_instance()).unwrap();
    scene.resize_raster(8, 4).unwrap();
    let frame = scene
        .render(&mut vm, CmvsTextureSlot::Parent(0), &archive, (16, 8), None)
        .unwrap()
        .unwrap();
    for y in 0..4 {
        for x in 0..8 {
            let expected = if x == 2 && y == 1 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 0, 255]
            };
            let offset = (y * 8 + x) * 4;
            assert_eq!(&frame.rgba8[offset..offset + 4], &expected, "pixel {x},{y}");
        }
    }
}
