use super::*;
use shinki_emu_manager_core::model::{
    FrameAlpha, FrameFormat, FrameInfo, FrameLease, FrameRowOrder,
};

fn info(width: u32, height: u32, stride: u32, alpha: FrameAlpha) -> FrameInfo {
    FrameInfo {
        width,
        height,
        logical_width: width,
        logical_height: height,
        stride,
        format: FrameFormat::Rgba8Srgb { alpha },
        rows: FrameRowOrder::TopToBottom,
    }
}

fn lease(pixels: Vec<u8>, info: FrameInfo, epoch: u64, generation: u64) -> FrameLease {
    FrameLease::from_vec(
        pixels,
        info,
        FrameIdentity {
            session_epoch: epoch,
            output_epoch: 1,
            generation,
        },
    )
    .unwrap()
}

#[test]
fn old_frame_and_shutdown_cannot_replace_or_clear_a_new_session_baseline() {
    let mailbox = FrameMailbox::new();
    mailbox.activate(1).unwrap();
    let old = lease(vec![1, 2, 3, 255], info(1, 1, 4, FrameAlpha::Opaque), 1, 7);
    mailbox.publish(old.clone()).unwrap();
    mailbox.clear(1).unwrap();
    mailbox.activate(2).unwrap();
    mailbox
        .publish(lease(
            vec![5, 6, 7, 255],
            info(1, 1, 4, FrameAlpha::Opaque),
            2,
            1,
        ))
        .unwrap();
    assert!(mailbox.publish(old).is_err());
    mailbox.clear(1).unwrap();
    let current = mailbox.snapshot().unwrap().unwrap();
    assert_eq!(
        current.identity,
        FrameIdentity {
            session_epoch: 2,
            output_epoch: 1,
            generation: 1
        }
    );
    assert_eq!(current.pixels().unwrap(), &[5, 6, 7, 255]);
}

#[test]
fn replacement_and_close_preserve_an_already_acquired_cpu_lease() {
    let mailbox = FrameMailbox::new();
    mailbox.activate(3).unwrap();
    mailbox
        .publish(lease(
            vec![3, 4, 5, 255],
            info(1, 1, 4, FrameAlpha::Opaque),
            3,
            1,
        ))
        .unwrap();
    let held = mailbox.snapshot().unwrap().unwrap();
    mailbox
        .publish(lease(
            vec![7, 8, 9, 255],
            info(1, 1, 4, FrameAlpha::Opaque),
            3,
            2,
        ))
        .unwrap();
    mailbox.clear(3).unwrap();
    assert!(mailbox.snapshot().unwrap().is_none());
    assert_eq!(held.pixels().unwrap(), &[3, 4, 5, 255]);
}

#[test]
fn filtered_output_keeps_the_complete_native_frame_interactive() {
    let content = super::content_viewport_for_output(1280, 720, 721, 1280, 1082, 1920).unwrap();
    assert_eq!(
        content,
        roukaku_media_core::Viewport2D {
            x: 0,
            y: 0,
            width: 1082,
            height: 1920,
        }
    );

    let content = super::content_viewport_for_output(1280, 720, 1920, 1200, 2880, 1800).unwrap();
    assert_eq!(
        content,
        roukaku_media_core::Viewport2D {
            x: 0,
            y: 0,
            width: 2880,
            height: 1800,
        }
    );
}

#[test]
fn saved_filter_is_prepared_without_a_device_or_compiler() {
    let mut renderer = ManagerStageRenderer::with_dxc_path(
        FrameMailbox::new(),
        PathBuf::from("unavailable/dxcompiler.dll"),
    );
    let settings = legacy("anime4k_restore_upscale", 2.0);
    assert_eq!(renderer.configure_filter_inner(&settings).unwrap(), FilterApplyState::Deferred);
    assert!(renderer.take_stage_texture_update().unwrap().is_none());
    let Some(PendingFilter::Prepared(prepared)) = renderer.pending_filter.take() else {
        panic!("legacy settings must be deferred until input dimensions are known");
    };
    assert_eq!(prepared.output_dimensions(721, 1280).unwrap(), (1442, 2560));
}

#[test]
fn invalid_reconfigure_preserves_the_pending_good_request() {
    use shinki_emu_manager_core::FilterStage;

    let mut renderer = ManagerStageRenderer::new(FrameMailbox::new());
    renderer.configure_filter_inner(&legacy("scale", 1.5)).unwrap();
    let invalid = FilterSettings {
        stages: vec![FilterStage::Scale { scale: f32::NAN }],
    };
    assert!(renderer.configure_filter_inner(&invalid).is_err());
    assert!(renderer.configure_filter_inner(&external("invalid format-4 source")).is_err());
    let Some(PendingFilter::Prepared(prepared)) = renderer.pending_filter.take() else {
        panic!("invalid candidates must not displace the pending request");
    };
    assert_eq!(prepared.output_dimensions(721, 1280).unwrap(), (1082, 1920));
}

#[test]
fn teardown_discards_pending_filter_without_activating_it() {
    let mut renderer = ManagerStageRenderer::new(FrameMailbox::new());
    renderer.configure_filter_inner(&legacy("sharpen", 1.0)).unwrap();
    renderer.teardown().unwrap();
    assert!(renderer.pending_filter.is_none());
    assert!(renderer.take_stage_texture_update().unwrap().is_none());
}

#[test]
#[ignore = "requires a native compute adapter and a format-4 DXC installation"]
fn gpu_filter_chain_preserves_pixels_geometry_and_rollback() {
    use shinki_emu_manager::effects::FilterEngine;
    use shinki_emu_manager_core::FilterStage;

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("native GPU adapter");
    eprintln!("Manager filter GPU: {} ({:?})", adapter.get_info().name, adapter.get_info().backend);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: FilterEngine::format4_features(),
        ..Default::default()
    })).expect("native format-4 device");
    let context = || WgpuFrameContext { device: &device, queue: &queue };
    let mailbox = FrameMailbox::new();
    mailbox.activate(7).unwrap();
    let mut renderer = ManagerStageRenderer::new(mailbox.clone());
    renderer.setup(context()).unwrap();

    // Native presentation converts bottom-up BGRA to upright RGBA in the
    // same encoder as the selected filters; logical input remains 640×480.
    let mut frame_info = info(3, 2, 12, FrameAlpha::Ignored);
    frame_info.format = FrameFormat::Bgra8Srgb { alpha: FrameAlpha::Ignored };
    frame_info.rows = FrameRowOrder::BottomToTop;
    frame_info.logical_width = 640;
    frame_info.logical_height = 480;
    let top = [[10, 24, 31, 255], [20, 32, 42, 255], [50, 76, 87, 255]];
    let bottom = [[90, 102, 122, 255], [121, 133, 144, 255], [150, 158, 162, 255]];
    let native = bottom.into_iter().chain(top).flat_map(|[r, g, b, _]| [b, g, r, 17]).collect::<Vec<_>>();
    let frame = lease(native.clone(), frame_info, 7, 1);
    let identity = frame.identity;
    mailbox.publish(frame).unwrap();
    renderer.render(context()).unwrap();
    let bypass = renderer.take_stage_texture_update().unwrap().unwrap();
    assert_eq!(bypass.texture, *renderer.presented_input.as_ref().unwrap());
    assert_eq!(read_pixels(context(), &bypass.texture), [top, bottom].concat());

    let solid = [27, 73, 120, 255];
    let solid_bgra = [solid[2], solid[1], solid[0], 17].repeat(6);
    mailbox.publish(lease(solid_bgra, frame_info, 7, 2)).unwrap();
    let activate = |renderer: &mut ManagerStageRenderer, settings: &FilterSettings| {
        renderer.configure_filter_inner(settings).unwrap();
        renderer.render(context()).unwrap();
        match renderer.take_filter_event() {
            Some(FilterEvent::Activated) => {}
            Some(FilterEvent::Rejected(error) | FilterEvent::RenderFailed(error)) => panic!("{error}"),
            None => panic!("missing activation event"),
        }
        renderer.take_stage_texture_update().unwrap().unwrap()
    };
    let scale = activate(&mut renderer, &legacy("scale", 1.5));
    assert_eq!((scale.raster_width, scale.raster_height), (5, 3));
    assert_eq!(read_pixels(context(), &scale.texture), vec![solid; 15]);
    let anime = activate(&mut renderer, &legacy("anime4k_restore_upscale", 2.0));
    assert_eq!((anime.raster_width, anime.raster_height), (6, 4));
    assert!(read_pixels(context(), &anime.texture).iter().all(|pixel| {
        pixel[3] == 255 && pixel[0] != pixel[2]
    }));

    let paint = external(&effect("float4(1.0, pixel.g, pixel.b, pixel.a)")).stages.remove(0);
    let swap = external(&effect("pixel.grba")).stages.remove(0);
    let ordered = FilterSettings {
        stages: vec![paint.clone(), FilterStage::Scale2x, swap.clone()],
    };
    let reversed = FilterSettings {
        stages: vec![swap, FilterStage::Scale2x, paint],
    };
    mailbox.publish(lease(native.clone(), frame_info, 7, 3)).unwrap();
    let forward = activate(&mut renderer, &ordered);
    let reverse = activate(&mut renderer, &reversed);
    assert_eq!((forward.raster_width, forward.raster_height), (6, 4));
    assert_eq!((forward.logical_width, forward.logical_height), (640, 480));
    assert_eq!(forward.content, Viewport2D { x: 0, y: 0, width: 6, height: 4 });
    assert_eq!(read_pixels(context(), &forward.texture)[0], [24, 255, 31, 255]);
    assert_eq!(read_pixels(context(), &reverse.texture)[0], [255, 10, 31, 255]);
    // No stage target can be written by a texture copy. A later generation
    // writes the same final GPU handle and publishes no replacement image.
    assert!(!reverse.texture.usage().contains(TextureUsages::COPY_DST));
    mailbox.publish(lease(native.clone(), frame_info, 7, 4)).unwrap();
    renderer.render(context()).unwrap();
    assert!(renderer.take_stage_texture_update().unwrap().is_none());
    assert_eq!(renderer.output_texture.as_ref(), Some(&reverse.texture));
    assert_eq!(mailbox.displayed().unwrap().unwrap().1, frame_info);
    assert_eq!(mailbox.displayed().unwrap().unwrap().0.session_epoch, identity.session_epoch);

    // Fail a prepared request on the next rendering notification. Do not
    // overwrite the last working texture or its input frame metadata.
    let previous = renderer.dxc_path.clone();
    renderer.pending_filter = Some(PendingFilter::Prepared(
        PreparedFilter::new(&legacy("anime4k_restore_upscale", 2.0)).unwrap(),
    ));
    renderer.dxc_path = PathBuf::from("unavailable/dxcompiler.dll");
    renderer.render(context()).unwrap();
    assert!(matches!(renderer.take_filter_event(), Some(FilterEvent::Rejected(_))));
    assert_eq!(renderer.output_texture.as_ref(), Some(&reverse.texture));
    renderer.render(context()).unwrap();
    assert!(renderer.take_filter_event().is_none());
    assert_eq!(read_pixels(context(), &reverse.texture)[0], [255, 10, 31, 255]);
    renderer.dxc_path = previous;
    let bypass = activate(&mut renderer, &FilterSettings::default());
    assert_eq!(bypass.texture, *renderer.presented_input.as_ref().unwrap());
    assert_eq!(read_pixels(context(), &bypass.texture), [top, bottom].concat());

    // Built-ins still work without DXC and can follow a classic resize.
    renderer.dxc_path = PathBuf::from("unavailable/dxcompiler.dll");
    let stages = |stages| FilterSettings { stages };
    let classic = activate(&mut renderer, &stages(vec![FilterStage::Scale2x]));
    let restored = activate(&mut renderer, &stages(vec![
        FilterStage::Scale2x, FilterStage::Anime4kOriginalRestore,
    ]));
    assert_eq!(restored.texture.size(), classic.texture.size());
    assert_ne!(read_pixels(context(), &restored.texture), read_pixels(context(), &classic.texture));
    let upscaled = activate(&mut renderer, &stages(vec![
        FilterStage::Scale2x, FilterStage::Anime4kOriginalX2,
    ]));
    assert_eq!((upscaled.raster_width, upscaled.raster_height), (12, 8));
    assert!(read_pixels(context(), &upscaled.texture).iter().all(|pixel| pixel[3] == 255));
    eprintln!("Manager filter GPU: bypass bytes, legacy effects, ordered external/Scale2x, original Anime4K, stable targets, geometry and rollback");
}

fn legacy(preset: &str, scale: f32) -> FilterSettings {
    serde_json::from_value(serde_json::json!({
        "configuration": {"preset": preset, "scale": scale, "strength": 0.35, "parameters": {}},
        "source": null,
    })).unwrap()
}

fn external(source: &str) -> FilterSettings {
    FilterSettings {
        stages: vec![shinki_emu_manager_core::FilterStage::Format4 {
            source: source.into(),
            strength: 0.35,
            parameters: Default::default(),
            disabled: false,
        }],
    }
}

fn effect(expression: &str) -> String {
    format!(r#"//!MAGPIE EFFECT
//!VERSION 4
//!TEXTURE
Texture2D INPUT;
//!TEXTURE
Texture2D OUTPUT;
//!SAMPLER
//!FILTER POINT
SamplerState sam;
//!PASS 1
//!DESC Paint test fixture
//!IN INPUT
//!OUT OUTPUT
//!BLOCK_SIZE 8
//!NUM_THREADS 64
void Pass1(uint2 blockStart, uint3 threadId) {{
    uint2 p = blockStart + Rmp8x8(threadId.x);
    if (any(p >= GetOutputSize())) return;
    float4 pixel = INPUT.Load(int3(p, 0));
    OUTPUT[p] = {expression};
}}
"#)
}

fn read_pixels(context: WgpuFrameContext<'_>, texture: &wgpu::Texture) -> Vec<[u8; 4]> {
    let row_bytes = texture.width() * 4;
    let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let stride = row_bytes.div_ceil(alignment) * alignment;
    let readback = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("capture Manager test output only"),
        size: u64::from(stride) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = context.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: None },
        },
        texture.size(),
    );
    let submission = context.queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, move |result| send.send(result).unwrap());
    context.device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: Some(std::time::Duration::from_secs(30)),
    }).unwrap();
    receive.recv().unwrap().unwrap();
    slice.get_mapped_range().unwrap().chunks_exact(stride as usize)
        .flat_map(|row| row[..row_bytes as usize].chunks_exact(4))
        .map(|pixel| pixel.try_into().unwrap())
        .collect()
}
