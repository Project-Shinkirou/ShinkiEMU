use super::*;
use std::{ptr, sync::Mutex};

const LOGICAL: Extent = Extent { width: 640, height: 480 };
const RASTER: Extent = Extent { width: 2, height: 1 };
const CHECKPOINT: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

fn wire_frame(commands: &[GpuSceneCommandV1]) -> GpuSceneFrameV1 {
    GpuSceneFrameV1 {
        header: Header::of::<GpuSceneFrameV1>(),
        sequence: 19,
        logical: LOGICAL,
        raster: RASTER,
        clear_rgba: [13, 14, 15, 16],
        commands: commands.as_ptr(),
        command_count: commands.len(),
    }
}

#[test]
fn retained_pixels_survive_the_source_frame_without_copying() {
    let pixels = Arc::new([21, 22, 23, 24]);
    let weak = Arc::downgrade(&pixels);
    let allocation = pixels.as_ptr();
    let source = [SceneCommand::UploadTexture {
        resource_id: "stage".into(),
        frame: TextureFrame {
            width: 1,
            height: 1,
            rgba8: OwnedPixelBuffer::from_owned(OwnedByteBuffer::from_owner(pixels, |bytes| {
                bytes.as_slice()
            })),
        },
    }];
    let mut wire = Vec::new();
    encode(&source, &mut wire, &mut Vec::new()).unwrap();
    let mut decoded = Vec::new();
    unsafe { decode(&wire_frame(&wire), &mut decoded) }.unwrap();
    drop(wire);
    drop(source);

    let SceneCommand::UploadTexture { frame, .. } = decoded.pop().unwrap() else {
        panic!("expected a retained texture");
    };
    assert_eq!(frame.rgba8.allocation_ptr(), allocation);
    assert!(weak.upgrade().is_some());
    std::thread::spawn(move || assert_eq!(frame.rgba8.as_slice(), &[21, 22, 23, 24]))
        .join()
        .unwrap();
    assert!(weak.upgrade().is_none());
}

#[test]
fn malformed_wire_clears_partial_commands_and_keeps_capacity() {
    let texture = TextureFrame::from_vec(1, 1, vec![1, 2, 3, 4]).unwrap();
    let image = encode_texture(&texture).unwrap();
    let valid = GpuSceneCommandV1 {
        op: GPU_UPLOAD_TEXTURE,
        resource: Bytes::from_static(b"stage"),
        image,
        ..Default::default()
    };
    let invalid_glyph = GpuSceneCommandV1 {
        op: GPU_UPLOAD_GLYPH,
        image: GpuImageV1 { format: GPU_IMAGE_R8, ..image },
        ..valid
    };
    let invalid_transform = GpuSceneCommandV1 {
        op: GPU_PUSH_TRANSFORM,
        transform: [1.0, 0.0, 0.0, 1.0, f32::NAN, 0.0],
        ..Default::default()
    };
    let mut decoded = Vec::with_capacity(8);
    let capacity = decoded.capacity();
    for invalid in [invalid_glyph, invalid_transform] {
        assert_eq!(unsafe { decode(&wire_frame(&[valid, invalid]), &mut decoded) }, Err(INVALID_ARGUMENT));
        assert!(decoded.is_empty());
        assert_eq!(decoded.capacity(), capacity);
    }
}

#[derive(Default)]
struct HostProbe {
    epoch: Option<u64>,
    submitted: Option<(u64, Extent, Extent, [u8; 4])>,
    commands: Vec<SceneCommand>,
    captures: usize,
    destroyed: bool,
}

unsafe extern "C" fn create(context: *mut c_void, epoch: u64, out: *mut u64, _: *mut Error) -> Status {
    let probe = unsafe { &*context.cast::<Mutex<HostProbe>>() };
    probe.lock().unwrap().epoch = Some(epoch);
    unsafe { out.write(17) };
    OK
}

unsafe extern "C" fn submit(
    context: *mut c_void,
    target: u64,
    frame: *const GpuSceneFrameV1,
    _: *mut Error,
) -> Status {
    if target != 17 {
        return INVALID_STATE;
    }
    let probe = unsafe { &*context.cast::<Mutex<HostProbe>>() };
    let mut probe = probe.lock().unwrap();
    let frame = unsafe { &*frame };
    if let Err(status) = unsafe { decode(frame, &mut probe.commands) } {
        return status;
    }
    probe.submitted = Some((frame.sequence, frame.logical, frame.raster, frame.clear_rgba));
    OK
}

unsafe extern "C" fn capture(
    context: *mut c_void,
    target: u64,
    pixels: *mut u8,
    len: usize,
    _: *mut Error,
) -> Status {
    let probe = unsafe { &*context.cast::<Mutex<HostProbe>>() };
    let mut probe = probe.lock().unwrap();
    if target != 17 || len != CHECKPOINT.len() || probe.submitted.is_none() {
        return INVALID_STATE;
    }
    unsafe { ptr::copy_nonoverlapping(CHECKPOINT.as_ptr(), pixels, len) };
    probe.captures += 1;
    OK
}

unsafe extern "C" fn destroy(context: *mut c_void, target: u64) {
    let probe = unsafe { &*context.cast::<Mutex<HostProbe>>() };
    let mut probe = probe.lock().unwrap();
    probe.destroyed = target == 17;
    probe.commands.clear();
}

fn open_prefix() -> OpenRequest {
    OpenRequest {
        header: Header::of::<OpenRequest>(),
        game_path: Bytes::EMPTY,
        session_epoch: 23,
        configuration: ptr::null(),
        configuration_count: 0,
        window: WindowState {
            drawable_width: RASTER.width,
            drawable_height: RASTER.height,
            scale_factor: 1.0,
            focused: 1,
            suspended: 0,
        },
        host: HostServices {
            header: Header::of::<HostServices>(),
            text: TextService { context: ptr::null_mut(), request: None, acknowledge: None, invalidate: None },
            window: WindowService { context: ptr::null_mut(), request: None, invalidate: None },
            cancellation: CancellationService { context: ptr::null_mut(), is_cancelled: None },
        },
        module_owner: ptr::null_mut(),
        render_extent: RASTER,
    }
}

unsafe extern "C" fn unsupported(
    _: *mut c_void,
    _: u64,
    _: *mut u64,
    _: *mut Error,
) -> Status {
    UNSUPPORTED
}

fn supplied_host(request: &OpenRequestGpuV1) -> GpuSceneHost {
    let base = ptr::from_ref(request).cast::<OpenRequest>();
    unsafe { GpuSceneHost::from_request(base) }.unwrap().unwrap()
}

#[test]
fn client_preserves_scene_order_and_captures_only_on_request() {
    let mut base = open_prefix();
    assert!(unsafe { GpuSceneHost::from_request(&base) }.unwrap().is_none());
    base.header = Header::of::<OpenRequestTextV1>();
    let mut text = OpenRequestTextV1 { base, text_async: ptr::null() };
    assert!(unsafe { GpuSceneHost::from_request(ptr::from_ref(&text).cast()) }.unwrap().is_none());

    let probe = Mutex::new(HostProbe::default());
    let service = GpuSceneServiceV1 {
        version: GPU_SCENE_VERSION,
        size: std::mem::size_of::<GpuSceneServiceV1>() as u32,
        context: ptr::from_ref(&probe).cast_mut().cast(),
        create: Some(create),
        submit: Some(submit),
        capture: Some(capture),
        destroy: Some(destroy),
    };
    text.base.header = Header::of::<OpenRequestGpuV1>();
    let unsupported_service = GpuSceneServiceV1 { create: Some(unsupported), ..service };
    let request = OpenRequestGpuV1 { text, gpu_scene: &unsupported_service };
    assert!(supplied_host(&request).create().unwrap().is_none());

    let request = OpenRequestGpuV1 { text, gpu_scene: &service };
    let host = supplied_host(&request);
    std::thread::scope(|scope| {
        let probe = &probe;
        scope.spawn(move || {
            let mut client = host.create().unwrap().unwrap();
            let glyphs: Arc<[_]> = [GlyphInstance {
                resource_id: "glyph".into(),
                x: -1,
                y: 5,
                rotation_quadrants: 3,
            }].into();
            let destination = RectI::new(-2, 1, 6, 4);
            let texture = TextureFrame::from_vec(1, 1, vec![6, 7, 8, 9]).unwrap();
            let frame = SceneFrame {
                sequence: 21,
                width: RASTER.width,
                height: RASTER.height,
                clear_rgba: [10, 20, 30, 40],
                semantics: None,
                commands: vec![
                    SceneCommand::Clear { rgba: [0, 0, 0, 255] },
                    SceneCommand::rect("rect", 1, 2, 3, 4, [1, 2, 3, 4]),
                    SceneCommand::UploadTexture { resource_id: "stage".into(), frame: texture.clone() },
                    SceneCommand::UpdateTextureRegion {
                        resource_id: "stage".into(), x: 0, y: 0, width: 1, height: 1,
                        rgba8: vec![33, 44, 55, 66].into(),
                    },
                    SceneCommand::UploadGlyph {
                        resource_id: "glyph".into(),
                        glyph: GlyphBitmap::from_vec(1, 1, GlyphBitmapFormat::Alpha8, vec![80]).unwrap(),
                    },
                    SceneCommand::PushClip { rect: destination },
                    SceneCommand::PushTransform { transform: Transform2D::translation(-2.5, 14.0) },
                    SceneCommand::Sprite {
                        id: "sprite".into(), texture_id: "stage".into(), source: Some(RectI::new(0, 0, 1, 1)),
                        destination, opacity: 0.25, blend: BlendMode::Multiply,
                    },
                    SceneCommand::GlyphRun {
                        id: "first".into(), glyphs: glyphs.clone(), rgba: [0, 1, 2, 3], opacity: 0.75,
                        blend: BlendMode::Add,
                    },
                    SceneCommand::LinearGlyphRun {
                        id: "second".into(), glyphs, rgba: [10, 11, 12, 13], opacity: 0.5,
                    },
                    SceneCommand::Texture {
                        id: "texture".into(), frame: texture.clone(), destination, opacity: 0.75,
                        blend: BlendMode::ScreenRgb,
                    },
                    SceneCommand::MaskReveal {
                        id: "mask".into(), frame: texture.clone(), mask: texture, destination,
                        mode: MaskRevealMode::Gradient, phase: 256.0, opacity: 1.0,
                    },
                    SceneCommand::PopTransform,
                    SceneCommand::PopClip,
                    SceneCommand::ReleaseResource { resource_id: "stage".into() },
                ],
            };
            assert!(client.capture(RASTER).is_err());
            client.submit(&frame, LOGICAL).unwrap();
            {
                let probe = probe.lock().unwrap();
                assert_eq!(probe.epoch, Some(23));
                assert_eq!(probe.submitted, Some((21, LOGICAL, RASTER, frame.clear_rgba)));
                assert_eq!(probe.commands, frame.commands);
                assert_eq!(probe.captures, 0);
            }
            assert_eq!(client.capture(RASTER).unwrap().as_ref(), &CHECKPOINT);
            drop(client);
        }).join().unwrap();
    });
    let probe = probe.into_inner().unwrap();
    assert_eq!(probe.captures, 1);
    assert!(probe.destroyed && probe.commands.is_empty());
}
