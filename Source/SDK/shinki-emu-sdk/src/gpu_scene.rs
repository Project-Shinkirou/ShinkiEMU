//! Optional same-device scene presentation over the native C ABI.

use crate::runtime::SessionError;
use roukaku_byte_source::OwnedByteBuffer;
use roukaku_media_core::{
    BlendMode, GlyphBitmap, GlyphBitmapFormat, GlyphInstance, MaskRevealMode, OwnedPixelBuffer,
    RectI, SceneCommand, TextureFrame, Transform2D,
};
use roukaku_platform::SceneFrame;
use shinki_emu_abi::{self as abi, *};
use std::{ffi::c_void, num::NonZeroU64, ptr::NonNull, sync::Arc};

mod output;
pub use output::{SceneOutput, SceneRequirements};

type PresentRaster =
    unsafe extern "C" fn(*mut c_void, u64, *const GpuRasterFrameV2, *mut Error) -> Status;

/// Copy of the optional service table and the open request's session epoch.
#[derive(Clone, Copy)]
pub struct GpuSceneHost {
    service: GpuSceneServiceV1,
    present_raster: Option<PresentRaster>,
    session_epoch: u64,
}

// SAFETY: from_request requires a host context and callbacks usable from the
// core's worker. The copied table does not borrow the contents of the request.
unsafe impl Send for GpuSceneHost {}

impl GpuSceneHost {
    /// Returns `None` for a host that did not supply the GPU extension.
    ///
    /// # Safety
    /// `request` refers to the original allocation described by its header.
    /// Its optional GPU table is readable during this call. The context and
    /// callbacks follow the ABI and remain valid until failed-open cleanup or
    /// session close has finished. In particular, callbacks support the core
    /// worker, and a successful capture initializes every destination byte.
    /// Keep all copies and created targets within that lifetime.
    pub unsafe fn from_request(
        request: *const abi::OpenRequest,
    ) -> Result<Option<Self>, abi::Status> {
        let Some(service) = (unsafe { abi::OpenRequest::gpu_scene(request)? }) else {
            return Ok(None);
        };
        let present_raster = if service.size as usize >= std::mem::size_of::<GpuSceneServiceV2>() {
            // The v1 reader validated the original request, service prefix,
            // and its declared size. Both tables have the same alignment.
            let table = unsafe { (*request.cast::<OpenRequestGpuV1>()).gpu_scene };
            unsafe { (*table.cast::<GpuSceneServiceV2>()).present_raster }
        } else {
            None
        };
        Ok(Some(Self {
            service,
            present_raster,
            session_epoch: unsafe { (*request).session_epoch },
        }))
    }

    /// A host returning `UNSUPPORTED` leaves the core on the CPU frame path.
    pub fn create(self) -> Result<Option<GpuSceneClient>, SessionError> {
        let mut target = 0;
        let mut error = Error::default();
        let status = unsafe {
            (self.service.create.unwrap())(
                self.service.context,
                self.session_epoch,
                &mut target,
                &mut error,
            )
        };
        match status {
            UNSUPPORTED => Ok(None),
            OK => {
                let target = NonZeroU64::new(target).ok_or_else(|| {
                    SessionError::new("SHINKI_EMU_GPU_CREATE", "host returned an empty GPU target")
                })?;
                Ok(Some(GpuSceneClient {
                    service: self.service,
                    present_raster: self.present_raster,
                    target,
                    commands: Vec::new(),
                    glyphs: Vec::new(),
                    last_raster: None,
                }))
            }
            _ => Err(host_error(
                &error,
                "SHINKI_EMU_GPU_CREATE",
                "GPU target creation failed",
            )),
        }
    }
}

/// Core-worker owner of one ordered host scene target.
pub struct GpuSceneClient {
    service: GpuSceneServiceV1,
    present_raster: Option<PresentRaster>,
    target: NonZeroU64,
    commands: Vec<GpuSceneCommandV1>,
    glyphs: Vec<GpuGlyphV1>,
    last_raster: Option<Extent>,
}

impl GpuSceneClient {
    /// `frame` already contains its raster mapping. `logical` is metadata.
    /// The host must consume the wire spans before returning, and may retain
    /// any pixel data it needs through the image callbacks.
    pub fn submit(&mut self, frame: &SceneFrame, logical: abi::Extent) -> Result<(), SessionError> {
        let raster = Extent {
            width: frame.width,
            height: frame.height,
        };
        validate_extents(logical, raster).map_err(encode_error)?;
        let encoded = encode(&frame.commands, &mut self.commands, &mut self.glyphs);
        let result = encoded.map_err(encode_error).and_then(|()| {
            let wire = GpuSceneFrameV1 {
                header: Header::of::<GpuSceneFrameV1>(),
                sequence: frame.sequence,
                logical,
                raster,
                clear_rgba: frame.clear_rgba,
                commands: self.commands.as_ptr(),
                command_count: self.commands.len(),
            };
            let mut error = Error::default();
            let status = unsafe {
                (self.service.submit.unwrap())(
                    self.service.context,
                    self.target.get(),
                    &wire,
                    &mut error,
                )
            };
            if status == OK {
                self.last_raster = Some(raster);
                Ok(())
            } else {
                Err(host_error(
                    &error,
                    "SHINKI_EMU_GPU_SUBMIT",
                    "GPU scene submission failed",
                ))
            }
        });
        self.commands.clear();
        self.glyphs.clear();
        result
    }

    fn present_raster(
        &self,
        sequence: u64,
        logical: abi::Extent,
        raster: abi::Extent,
        pixels: &OwnedPixelBuffer,
    ) -> Result<(), SessionError> {
        validate_extents(logical, raster).map_err(encode_error)?;
        let present = self
            .present_raster
            .ok_or_else(|| encode_error(UNSUPPORTED))?;
        let frame = GpuRasterFrameV2 {
            header: Header::of::<GpuRasterFrameV2>(),
            sequence,
            logical,
            image: encode_image(raster.width, raster.height, GPU_IMAGE_RGBA8, pixels)
                .map_err(encode_error)?,
        };
        let mut error = Error::default();
        let status =
            unsafe { present(self.service.context, self.target.get(), &frame, &mut error) };
        if status != OK {
            return Err(host_error(
                &error,
                "SHINKI_EMU_RASTER_PRESENT",
                "CPU-native effect presentation failed",
            ));
        }
        Ok(())
    }

    /// Explicitly read the last submitted raster as tightly packed, top-down RGBA8.
    pub fn capture(&self, raster: abi::Extent) -> Result<Arc<[u8]>, SessionError> {
        let len = pixel_len(raster, 4, GPU_MAX_IMAGE_BYTES)
            .filter(|_| self.last_raster == Some(raster))
            .ok_or_else(|| {
                SessionError::new(
                    "SHINKI_EMU_GPU_CAPTURE",
                    "capture extent does not match the submitted raster",
                )
            })?;
        let mut pixels = Arc::<[u8]>::new_uninit_slice(len);
        let output = Arc::get_mut(&mut pixels).unwrap().as_mut_ptr().cast::<u8>();
        let mut error = Error::default();
        let status = unsafe {
            (self.service.capture.unwrap())(
                self.service.context,
                self.target.get(),
                output,
                len,
                &mut error,
            )
        };
        if status == OK {
            // SAFETY: a successful capture must write the entire destination
            // synchronously. The host writes straight into the final owner.
            Ok(unsafe { pixels.assume_init() })
        } else {
            Err(host_error(
                &error,
                "SHINKI_EMU_GPU_CAPTURE",
                "GPU scene capture failed",
            ))
        }
    }
}

impl Drop for GpuSceneClient {
    fn drop(&mut self) {
        unsafe { (self.service.destroy.unwrap())(self.service.context, self.target.get()) };
    }
}

fn host_error(error: &Error, code: &str, message: &str) -> SessionError {
    fn field(bytes: &[u8], len: u32) -> Option<&str> {
        let text = std::str::from_utf8(bytes.get(..len as usize)?).ok()?;
        (!text.is_empty() && !text.contains('\0')).then_some(text)
    }
    SessionError::new(
        field(&error.code, error.code_len).unwrap_or(code),
        field(&error.message, error.message_len).unwrap_or(message),
    )
}

fn encode_error(status: Status) -> SessionError {
    SessionError::new(
        "SHINKI_EMU_GPU_ENCODE",
        if status == UNSUPPORTED {
            "scene command is not supported by the GPU wire format"
        } else {
            "scene command or frame is invalid for the GPU wire format"
        },
    )
}

fn pixel_len(extent: Extent, channels: usize, limit: usize) -> Option<usize> {
    (extent.width as usize)
        .checked_mul(channels)?
        .checked_mul(extent.height as usize)
        .filter(|&len| len != 0 && len <= limit)
}

fn validate_extents(logical: Extent, raster: Extent) -> Result<(), Status> {
    if logical.width == 0
        || logical.height == 0
        || pixel_len(raster, 4, GPU_MAX_IMAGE_BYTES).is_none()
    {
        return Err(INVALID_ARGUMENT);
    }
    Ok(())
}

fn encode(
    source: &[SceneCommand],
    commands: &mut Vec<GpuSceneCommandV1>,
    glyphs: &mut Vec<GpuGlyphV1>,
) -> Result<(), Status> {
    if source.len() > GPU_MAX_COMMANDS {
        return Err(INVALID_ARGUMENT);
    }
    let total_glyphs = source.iter().try_fold(0usize, |total, command| {
        let len = match command {
            SceneCommand::GlyphRun { glyphs, .. } | SceneCommand::LinearGlyphRun { glyphs, .. } => {
                glyphs.len()
            }
            _ => 0,
        };
        total
            .checked_add(len)
            .filter(|&n| n <= GPU_MAX_GLYPHS)
            .ok_or(INVALID_ARGUMENT)
    })?;
    commands.reserve_exact(source.len());
    // The destination starts empty. Reserve the complete count so pointers
    // published by early runs survive pushes for every later run.
    glyphs.reserve_exact(total_glyphs);
    for command in source {
        commands.push(encode_command(command, glyphs)?);
    }
    Ok(())
}

fn encode_rect(rect: RectI) -> GpuRectV1 {
    GpuRectV1 {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}

fn unsigned_rect(x: u32, y: u32, width: u32, height: u32) -> Result<GpuRectV1, Status> {
    Ok(GpuRectV1 {
        x: i32::try_from(x).map_err(|_| INVALID_ARGUMENT)?,
        y: i32::try_from(y).map_err(|_| INVALID_ARGUMENT)?,
        width,
        height,
    })
}

fn label_bytes(value: &str) -> Result<Bytes, Status> {
    if value.len() > GPU_MAX_LABEL_BYTES || value.contains('\0') {
        return Err(INVALID_ARGUMENT);
    }
    Ok(Bytes::new(value.as_bytes()))
}

fn encode_image(
    width: u32,
    height: u32,
    format: u32,
    pixels: &OwnedPixelBuffer,
) -> Result<GpuImageV1, Status> {
    let extent = Extent { width, height };
    let bytes = Bytes::new(pixels.as_slice());
    if pixel_len(extent, image_channels(format)?, GPU_MAX_IMAGE_BYTES) != Some(bytes.len) {
        return Err(INVALID_ARGUMENT);
    }
    Ok(GpuImageV1 {
        extent,
        format,
        pixels: bytes,
        owner: std::ptr::from_ref(pixels).cast(),
        retain: Some(retain_pixels),
        release: Some(release_pixels),
    })
}

unsafe extern "C" fn retain_pixels(owner: *const c_void) -> *mut c_void {
    // The borrowed token points to the scene's owner only during submit. A
    // retained token has its own owner reference, independent of the scene.
    let pixels = unsafe { &*owner.cast::<OwnedPixelBuffer>() };
    Box::into_raw(Box::new(pixels.clone())).cast()
}

unsafe extern "C" fn release_pixels(owner: *mut c_void) {
    drop(unsafe { Box::from_raw(owner.cast::<OwnedPixelBuffer>()) });
}

fn encode_texture(frame: &TextureFrame) -> Result<GpuImageV1, Status> {
    encode_image(frame.width, frame.height, GPU_IMAGE_RGBA8, &frame.rgba8)
}

fn encode_blend(blend: BlendMode) -> u32 {
    match blend {
        BlendMode::Alpha => GPU_BLEND_ALPHA,
        BlendMode::Add => GPU_BLEND_ADD,
        BlendMode::Opaque => GPU_BLEND_OPAQUE,
        BlendMode::Multiply => GPU_BLEND_MULTIPLY,
        BlendMode::Screen => GPU_BLEND_SCREEN,
        BlendMode::ScreenRgb => GPU_BLEND_SCREEN_RGB,
    }
}

fn encode_glyphs(
    run: &[GlyphInstance],
    glyphs: &mut Vec<GpuGlyphV1>,
) -> Result<*const GpuGlyphV1, Status> {
    if run.is_empty() {
        return Ok(std::ptr::null());
    }
    let start = glyphs.len();
    for glyph in run {
        glyphs.push(GpuGlyphV1 {
            resource: label_bytes(&glyph.resource_id)?,
            x: glyph.x,
            y: glyph.y,
            rotation_quadrants: u32::from(glyph.rotation_quadrants),
        });
    }
    Ok(glyphs.as_ptr().wrapping_add(start))
}

fn encode_command(
    command: &SceneCommand,
    glyphs: &mut Vec<GpuGlyphV1>,
) -> Result<GpuSceneCommandV1, Status> {
    use SceneCommand::*;
    let mut wire = GpuSceneCommandV1::default();
    wire.op = match command {
        Clear { rgba } => {
            wire.color = *rgba;
            GPU_CLEAR
        }
        Rect {
            id,
            x,
            y,
            width,
            height,
            rgba,
        } => {
            wire.id = label_bytes(id)?;
            wire.rect = unsigned_rect(*x, *y, *width, *height)?;
            wire.color = *rgba;
            GPU_RECT
        }
        UploadTexture { resource_id, frame } => {
            wire.resource = label_bytes(resource_id)?;
            wire.image = encode_texture(frame)?;
            GPU_UPLOAD_TEXTURE
        }
        UpdateTextureRegion {
            resource_id,
            x,
            y,
            width,
            height,
            rgba8,
        } => {
            wire.resource = label_bytes(resource_id)?;
            wire.rect = unsigned_rect(*x, *y, *width, *height)?;
            wire.image = encode_image(*width, *height, GPU_IMAGE_RGBA8, rgba8)?;
            GPU_UPDATE_TEXTURE
        }
        UploadGlyph { resource_id, glyph } => {
            wire.resource = label_bytes(resource_id)?;
            let format = match glyph.format {
                GlyphBitmapFormat::Alpha8 => GPU_IMAGE_R8,
                GlyphBitmapFormat::Rgba8 => GPU_IMAGE_RGBA8,
            };
            wire.image = encode_image(glyph.width, glyph.height, format, &glyph.pixels)?;
            GPU_UPLOAD_GLYPH
        }
        ReleaseResource { resource_id } => {
            wire.resource = label_bytes(resource_id)?;
            GPU_RELEASE_RESOURCE
        }
        Sprite {
            id,
            texture_id,
            source,
            destination,
            opacity,
            blend,
        } => {
            wire.id = label_bytes(id)?;
            wire.resource = label_bytes(texture_id)?;
            if let Some(source) = source {
                wire.source = encode_rect(*source);
                source_rect(wire.source)?.ok_or(INVALID_ARGUMENT)?;
            }
            wire.rect = encode_rect(*destination);
            wire.opacity = unit_opacity(*opacity)?;
            wire.mode = encode_blend(*blend);
            GPU_SPRITE
        }
        GlyphRun {
            id,
            glyphs: run,
            rgba,
            opacity,
            ..
        }
        | LinearGlyphRun {
            id,
            glyphs: run,
            rgba,
            opacity,
        } => {
            wire.id = label_bytes(id)?;
            wire.color = *rgba;
            wire.opacity = unit_opacity(*opacity)?;
            wire.glyph_count = run.len();
            wire.glyphs = encode_glyphs(run, glyphs)?;
            if let GlyphRun { blend, .. } = command {
                wire.mode = encode_blend(*blend);
                GPU_GLYPH_RUN
            } else {
                GPU_LINEAR_GLYPH_RUN
            }
        }
        PushClip { rect } => {
            wire.rect = encode_rect(*rect);
            GPU_PUSH_CLIP
        }
        PopClip => GPU_POP_CLIP,
        PushTransform { transform } => {
            let Transform2D {
                m11,
                m12,
                m21,
                m22,
                tx,
                ty,
            } = *transform;
            wire.transform = [m11, m12, m21, m22, tx, ty];
            finite_transform(wire.transform)?;
            GPU_PUSH_TRANSFORM
        }
        PopTransform => GPU_POP_TRANSFORM,
        MaskReveal {
            id,
            frame,
            mask,
            destination,
            mode,
            phase,
            opacity,
        } => {
            wire.id = label_bytes(id)?;
            wire.rect = encode_rect(*destination);
            wire.opacity = unit_opacity(*opacity)?;
            wire.phase = mask_phase(*mode, *phase)?;
            wire.mode = match mode {
                MaskRevealMode::Gradient => GPU_MASK_GRADIENT,
                MaskRevealMode::Threshold => GPU_MASK_THRESHOLD,
            };
            wire.image = encode_texture(frame)?;
            wire.mask = encode_texture(mask)?;
            GPU_MASK_REVEAL
        }
        Texture {
            id,
            frame,
            destination,
            opacity,
            blend,
        } => {
            wire.id = label_bytes(id)?;
            wire.rect = encode_rect(*destination);
            wire.opacity = unit_opacity(*opacity)?;
            wire.mode = encode_blend(*blend);
            wire.image = encode_texture(frame)?;
            GPU_TEXTURE
        }
        _ => return Err(UNSUPPORTED),
    };
    Ok(wire)
}

/// Decode borrowed wire commands into owned scene commands, reusing the
/// caller's command-vector capacity. The vector is empty on failure.
/// Resource references, transform composition, stack order, and device bounds
/// are checked by the scene renderer when it walks the commands in order.
///
/// # Safety
/// The producer's command, glyph and byte spans must be readable and immutable
/// for this call. A successful image retain must keep the same immutable span
/// readable until release. Both callbacks are thread-safe. The module that
/// owns the pixels and callbacks must stay loaded until all decoded commands
/// and renderer-held leases have been dropped.
pub unsafe fn decode(
    frame: &abi::GpuSceneFrameV1,
    commands: &mut Vec<SceneCommand>,
) -> Result<(), abi::Status> {
    commands.clear();
    if !frame.header.accepts::<GpuSceneFrameV1>() {
        return Err(INVALID_ARGUMENT);
    }
    validate_extents(frame.logical, frame.raster)?;
    let source =
        unsafe { abi::borrowed_array(frame.commands, frame.command_count, GPU_MAX_COMMANDS)? };
    commands.reserve_exact(source.len());
    let mut glyph_budget = GPU_MAX_GLYPHS;
    for command in source {
        match unsafe { decode_command(command, &mut glyph_budget) } {
            Ok(command) => commands.push(command),
            Err(status) => {
                commands.clear();
                return Err(status);
            }
        }
    }
    Ok(())
}

fn decode_rect(rect: GpuRectV1) -> RectI {
    RectI::new(rect.x, rect.y, rect.width, rect.height)
}

fn source_rect(rect: GpuRectV1) -> Result<Option<RectI>, Status> {
    match (rect.width, rect.height) {
        (0, 0) => Ok(None),
        (0, _) | (_, 0) => Err(INVALID_ARGUMENT),
        _ if rect.x < 0 || rect.y < 0 => Err(INVALID_ARGUMENT),
        _ => Ok(Some(decode_rect(rect))),
    }
}

fn coordinate(value: i32) -> Result<u32, Status> {
    u32::try_from(value).map_err(|_| INVALID_ARGUMENT)
}

fn unit_opacity(value: f32) -> Result<f32, Status> {
    (0.0..=1.0)
        .contains(&value)
        .then_some(value)
        .ok_or(INVALID_ARGUMENT)
}

fn mask_phase(mode: MaskRevealMode, value: f32) -> Result<f32, Status> {
    let end = match mode {
        MaskRevealMode::Gradient => 512.0,
        MaskRevealMode::Threshold => 256.0,
    };
    (0.0..=end)
        .contains(&value)
        .then_some(value)
        .ok_or(INVALID_ARGUMENT)
}

fn finite_transform(values: [f32; 6]) -> Result<(), Status> {
    if values.into_iter().all(f32::is_finite) {
        Ok(())
    } else {
        Err(INVALID_ARGUMENT)
    }
}

fn image_channels(format: u32) -> Result<usize, Status> {
    match format {
        GPU_IMAGE_RGBA8 => Ok(4),
        GPU_IMAGE_R8 => Ok(1),
        _ => Err(INVALID_ARGUMENT),
    }
}

struct ImageLease {
    pixels: NonNull<u8>,
    len: usize,
    token: NonNull<c_void>,
    release: unsafe extern "C" fn(*mut c_void),
}

// SAFETY: the wire contract makes the retained bytes immutable and the
// release callback valid on any thread. Only this lease owns the token.
unsafe impl Send for ImageLease {}
unsafe impl Sync for ImageLease {}

impl ImageLease {
    fn bytes(&self) -> &[u8] {
        // SAFETY: the live token owns an immutable span of this exact length.
        unsafe { std::slice::from_raw_parts(self.pixels.as_ptr(), self.len) }
    }
}

impl Drop for ImageLease {
    fn drop(&mut self) {
        unsafe { (self.release)(self.token.as_ptr()) };
    }
}

unsafe fn decode_image(
    image: &GpuImageV1,
    expected_format: u32,
) -> Result<OwnedPixelBuffer, Status> {
    if image.format != expected_format
        || image.owner.is_null()
        || pixel_len(
            image.extent,
            image_channels(expected_format)?,
            GPU_MAX_IMAGE_BYTES,
        ) != Some(image.pixels.len)
    {
        return Err(INVALID_ARGUMENT);
    }
    let pixels = NonNull::new(image.pixels.data.cast_mut()).ok_or(INVALID_ARGUMENT)?;
    let release = image.release.ok_or(INVALID_ARGUMENT)?;
    let retain = image.retain.ok_or(INVALID_ARGUMENT)?;
    let token = NonNull::new(unsafe { retain(image.owner) }).ok_or(INVALID_ARGUMENT)?;
    let lease = ImageLease {
        pixels,
        len: image.pixels.len,
        token,
        release,
    };
    Ok(OwnedPixelBuffer::from_owned(OwnedByteBuffer::from_owner(
        lease,
        ImageLease::bytes,
    )))
}

/// Validate and retain an ordered CPU-native raster without copying pixels.
///
/// # Safety
/// The image and its callbacks must satisfy `GpuImageV1`'s lifetime contract.
pub unsafe fn decode_raster(frame: &GpuRasterFrameV2) -> Result<OwnedPixelBuffer, Status> {
    if !frame.header.accepts::<GpuRasterFrameV2>() || frame.sequence == 0 {
        return Err(INVALID_ARGUMENT);
    }
    validate_extents(frame.logical, frame.image.extent)?;
    unsafe { decode_image(&frame.image, GPU_IMAGE_RGBA8) }
}

unsafe fn decode_texture(image: &GpuImageV1) -> Result<TextureFrame, Status> {
    let rgba8 = unsafe { decode_image(image, GPU_IMAGE_RGBA8)? };
    Ok(TextureFrame {
        width: image.extent.width,
        height: image.extent.height,
        rgba8,
    })
}

fn decode_blend(mode: u32) -> Result<BlendMode, Status> {
    match mode {
        GPU_BLEND_ALPHA => Ok(BlendMode::Alpha),
        GPU_BLEND_ADD => Ok(BlendMode::Add),
        GPU_BLEND_OPAQUE => Ok(BlendMode::Opaque),
        GPU_BLEND_MULTIPLY => Ok(BlendMode::Multiply),
        GPU_BLEND_SCREEN => Ok(BlendMode::Screen),
        GPU_BLEND_SCREEN_RGB => Ok(BlendMode::ScreenRgb),
        _ => Err(INVALID_ARGUMENT),
    }
}

unsafe fn decode_label(bytes: Bytes) -> Result<String, Status> {
    Ok(unsafe { bytes.text(GPU_MAX_LABEL_BYTES)? }.to_owned())
}

unsafe fn decode_glyph(glyph: &GpuGlyphV1) -> Result<GlyphInstance, Status> {
    let rotation_quadrants =
        u8::try_from(glyph.rotation_quadrants).map_err(|_| INVALID_ARGUMENT)?;
    Ok(GlyphInstance {
        resource_id: unsafe { decode_label(glyph.resource)? },
        x: glyph.x,
        y: glyph.y,
        rotation_quadrants,
    })
}

unsafe fn decode_glyphs(
    command: &GpuSceneCommandV1,
    glyph_budget: &mut usize,
) -> Result<Arc<[GlyphInstance]>, Status> {
    let source =
        unsafe { abi::borrowed_array(command.glyphs, command.glyph_count, *glyph_budget)? };
    *glyph_budget -= source.len();
    let mut glyphs = Arc::<[GlyphInstance]>::new_uninit_slice(source.len());
    let destination = Arc::get_mut(&mut glyphs).unwrap();
    for (i, glyph) in source.iter().enumerate() {
        match unsafe { decode_glyph(glyph) } {
            Ok(glyph) => {
                destination[i].write(glyph);
            }
            Err(status) => {
                for glyph in &mut destination[..i] {
                    // SAFETY: these entries were initialized by earlier iterations.
                    unsafe { glyph.assume_init_drop() };
                }
                return Err(status);
            }
        }
    }
    // SAFETY: every entry was written, including the empty-run case.
    Ok(unsafe { glyphs.assume_init() })
}

unsafe fn decode_command(
    wire: &GpuSceneCommandV1,
    glyph_budget: &mut usize,
) -> Result<SceneCommand, Status> {
    use SceneCommand::*;
    Ok(match wire.op {
        GPU_CLEAR => Clear { rgba: wire.color },
        GPU_RECT => Rect {
            id: unsafe { decode_label(wire.id)? },
            x: coordinate(wire.rect.x)?,
            y: coordinate(wire.rect.y)?,
            width: wire.rect.width,
            height: wire.rect.height,
            rgba: wire.color,
        },
        GPU_UPLOAD_TEXTURE => UploadTexture {
            resource_id: unsafe { decode_label(wire.resource)? },
            frame: unsafe { decode_texture(&wire.image)? },
        },
        GPU_UPDATE_TEXTURE => {
            if wire.rect.width != wire.image.extent.width
                || wire.rect.height != wire.image.extent.height
            {
                return Err(INVALID_ARGUMENT);
            }
            UpdateTextureRegion {
                resource_id: unsafe { decode_label(wire.resource)? },
                x: coordinate(wire.rect.x)?,
                y: coordinate(wire.rect.y)?,
                width: wire.rect.width,
                height: wire.rect.height,
                rgba8: unsafe { decode_image(&wire.image, GPU_IMAGE_RGBA8)? },
            }
        }
        GPU_UPLOAD_GLYPH => {
            let format = match wire.image.format {
                GPU_IMAGE_R8 => GlyphBitmapFormat::Alpha8,
                GPU_IMAGE_RGBA8 => GlyphBitmapFormat::Rgba8,
                _ => return Err(INVALID_ARGUMENT),
            };
            let resource_id = unsafe { decode_label(wire.resource)? };
            let pixels = unsafe { decode_image(&wire.image, wire.image.format)? };
            let glyph = GlyphBitmap::from_pixels(
                wire.image.extent.width,
                wire.image.extent.height,
                format,
                pixels,
            )
            .map_err(|_| INVALID_ARGUMENT)?;
            UploadGlyph { resource_id, glyph }
        }
        GPU_RELEASE_RESOURCE => ReleaseResource {
            resource_id: unsafe { decode_label(wire.resource)? },
        },
        GPU_SPRITE => Sprite {
            id: unsafe { decode_label(wire.id)? },
            texture_id: unsafe { decode_label(wire.resource)? },
            source: source_rect(wire.source)?,
            destination: decode_rect(wire.rect),
            opacity: unit_opacity(wire.opacity)?,
            blend: decode_blend(wire.mode)?,
        },
        GPU_GLYPH_RUN => GlyphRun {
            id: unsafe { decode_label(wire.id)? },
            rgba: wire.color,
            opacity: unit_opacity(wire.opacity)?,
            blend: decode_blend(wire.mode)?,
            glyphs: unsafe { decode_glyphs(wire, glyph_budget)? },
        },
        GPU_LINEAR_GLYPH_RUN => LinearGlyphRun {
            id: unsafe { decode_label(wire.id)? },
            rgba: wire.color,
            opacity: unit_opacity(wire.opacity)?,
            glyphs: unsafe { decode_glyphs(wire, glyph_budget)? },
        },
        GPU_PUSH_CLIP => PushClip {
            rect: decode_rect(wire.rect),
        },
        GPU_POP_CLIP => PopClip,
        GPU_PUSH_TRANSFORM => {
            finite_transform(wire.transform)?;
            let [m11, m12, m21, m22, tx, ty] = wire.transform;
            PushTransform {
                transform: Transform2D {
                    m11,
                    m12,
                    m21,
                    m22,
                    tx,
                    ty,
                },
            }
        }
        GPU_POP_TRANSFORM => PopTransform,
        GPU_MASK_REVEAL => {
            let mode = match wire.mode {
                GPU_MASK_GRADIENT => MaskRevealMode::Gradient,
                GPU_MASK_THRESHOLD => MaskRevealMode::Threshold,
                _ => return Err(INVALID_ARGUMENT),
            };
            MaskReveal {
                id: unsafe { decode_label(wire.id)? },
                destination: decode_rect(wire.rect),
                mode,
                phase: mask_phase(mode, wire.phase)?,
                opacity: unit_opacity(wire.opacity)?,
                frame: unsafe { decode_texture(&wire.image)? },
                mask: unsafe { decode_texture(&wire.mask)? },
            }
        }
        GPU_TEXTURE => Texture {
            id: unsafe { decode_label(wire.id)? },
            destination: decode_rect(wire.rect),
            opacity: unit_opacity(wire.opacity)?,
            blend: decode_blend(wire.mode)?,
            frame: unsafe { decode_texture(&wire.image)? },
        },
        _ => return Err(INVALID_ARGUMENT),
    })
}

#[cfg(test)]
mod tests;
