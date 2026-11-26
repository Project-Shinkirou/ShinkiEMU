//! Same-device GPU presentation. No backend or Rust object crosses the ABI.
use crate::{Bytes, Error, Extent, Header, OpenRequest, OpenRequestTextV1, Status, INVALID_ARGUMENT};
use std::ffi::c_void;

pub const GPU_SCENE_VERSION: u32 = 1;
pub const GPU_MAX_COMMANDS: usize = 65_536;
pub const GPU_MAX_GLYPHS: usize = 65_536;
pub const GPU_MAX_LABEL_BYTES: usize = 512;
pub const GPU_MAX_IMAGE_BYTES: usize = 128 * 1024 * 1024;

pub const GPU_CLEAR: u32 = 1;
pub const GPU_RECT: u32 = 2;
pub const GPU_UPLOAD_TEXTURE: u32 = 3;
pub const GPU_UPLOAD_GLYPH: u32 = 4;
pub const GPU_RELEASE_RESOURCE: u32 = 5;
pub const GPU_SPRITE: u32 = 6;
pub const GPU_GLYPH_RUN: u32 = 7;
pub const GPU_LINEAR_GLYPH_RUN: u32 = 8;
pub const GPU_PUSH_CLIP: u32 = 9;
pub const GPU_POP_CLIP: u32 = 10;
pub const GPU_PUSH_TRANSFORM: u32 = 11;
pub const GPU_POP_TRANSFORM: u32 = 12;
pub const GPU_MASK_REVEAL: u32 = 13;
pub const GPU_TEXTURE: u32 = 14;
pub const GPU_UPDATE_TEXTURE: u32 = 15;

pub const GPU_BLEND_ALPHA: u32 = 1;
pub const GPU_BLEND_ADD: u32 = 2;
pub const GPU_BLEND_OPAQUE: u32 = 3;
pub const GPU_BLEND_MULTIPLY: u32 = 4;
pub const GPU_BLEND_SCREEN: u32 = 5;
pub const GPU_BLEND_SCREEN_RGB: u32 = 6;
pub const GPU_MASK_GRADIENT: u32 = 1;
pub const GPU_MASK_THRESHOLD: u32 = 2;
pub const GPU_IMAGE_RGBA8: u32 = 1;
pub const GPU_IMAGE_R8: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct GpuRectV1 {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Bytes are immutable through submit. `retain(owner)` optionally extends that
/// lifetime and returns a new owned token for `release`. A nonempty image
/// requires both callbacks. Retain/release may run on any thread; neither
/// callback may unwind or depend on the lifetime of the open session.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GpuImageV1 {
    pub extent: Extent,
    pub format: u32,
    pub pixels: Bytes,
    pub owner: *const c_void,
    pub retain: Option<unsafe extern "C" fn(*const c_void) -> *mut c_void>,
    pub release: Option<unsafe extern "C" fn(*mut c_void)>,
}
impl Default for GpuImageV1 {
    fn default() -> Self {
        Self { extent: Extent::default(), format: 0, pixels: Bytes::EMPTY,
            owner: std::ptr::null(), retain: None, release: None }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GpuGlyphV1 {
    pub resource: Bytes,
    pub x: i32,
    pub y: i32,
    pub rotation_quadrants: u32,
}

/// `op` selects the named fields. Resource mutations use `resource`, draws
/// use `id`, and Sprite also uses `resource`. A zero-size `source` is absent.
/// A transform is [m11, m12, m21, m22, tx, ty]. Unused fields are zero/empty.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GpuSceneCommandV1 {
    pub op: u32,
    pub mode: u32,
    pub id: Bytes,
    pub resource: Bytes,
    pub rect: GpuRectV1,
    pub source: GpuRectV1,
    pub image: GpuImageV1,
    pub mask: GpuImageV1,
    pub glyphs: *const GpuGlyphV1,
    pub glyph_count: usize,
    pub color: [u8; 4],
    pub opacity: f32,
    pub phase: f32,
    pub transform: [f32; 6],
}
impl Default for GpuSceneCommandV1 {
    fn default() -> Self {
        Self { op: 0, mode: 0, id: Bytes::EMPTY, resource: Bytes::EMPTY,
            rect: GpuRectV1::default(), source: GpuRectV1::default(),
            image: GpuImageV1::default(), mask: GpuImageV1::default(),
            glyphs: std::ptr::null(), glyph_count: 0, color: [0; 4], opacity: 0.0,
            phase: 0.0, transform: [0.0; 6] }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GpuSceneFrameV1 {
    pub header: Header,
    pub sequence: u64,
    pub logical: Extent,
    pub raster: Extent,
    pub clear_rgba: [u8; 4],
    pub commands: *const GpuSceneCommandV1,
    pub command_count: usize,
}

/// Optional host table. `create` returns a nonzero opaque target or UNSUPPORTED
/// (choose CPU leases before opening the game). Each target has an ordered
/// resource namespace and strictly increasing nonzero `sequence` values.
/// `submit` queues a complete frame on the host GPU without CPU pixel access.
/// The host publishes it after open commits, with its own generation under
/// output epoch 1. A GPU session's base `frame` reports Pending with generation
/// 0 and no CPU lease; `state` and `open` still report output epoch 1.
/// `capture` synchronously initializes the entire caller-owned destination with
/// packed, top-down RGBA8 (encoded sRGB, opaque alpha) on success. The requested
/// length must equal the last submitted raster's width × height × 4.
/// The host's table is borrowed for `open`; its context must stay live through
/// failed-open cleanup or `close`. Core serializes callbacks per target and
/// destroys every target before returning from open failure or close. Target
/// destruction releases retained images without clearing a published frame.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GpuSceneServiceV1 {
    pub version: u32,
    pub size: u32,
    pub context: *mut c_void,
    pub create: Option<unsafe extern "C" fn(*mut c_void, u64, *mut u64, *mut Error) -> Status>,
    pub submit: Option<unsafe extern "C" fn(*mut c_void, u64, *const GpuSceneFrameV1, *mut Error) -> Status>,
    pub capture: Option<unsafe extern "C" fn(*mut c_void, u64, *mut u8, usize, *mut Error) -> Status>,
    pub destroy: Option<unsafe extern "C" fn(*mut c_void, u64)>,
}

/// A completed CPU-native effect frame. Its ordered `sequence` shares the
/// target namespace with GPU submits; it never changes the base frame API.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GpuRasterFrameV2 {
    pub header: Header,
    pub sequence: u64,
    pub logical: Extent,
    pub image: GpuImageV1,
}

/// Optional tail of the v1 service, advertised by `base.size`. `base.version`
/// remains 1. Old clients copy and use only the v1 prefix.
/// `present_raster` publishes immutable, packed, top-down RGBA8 (encoded sRGB,
/// ignored alpha) through the same ordered output as `submit`. It is for
/// native CPU producers, not for copying back a scene. The receiver may
/// retain the image. It does not change the v1 capture source.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GpuSceneServiceV2 {
    pub base: GpuSceneServiceV1,
    pub present_raster: Option<unsafe extern "C" fn(*mut c_void, u64, *const GpuRasterFrameV2, *mut Error) -> Status>,
}

impl GpuSceneServiceV1 {
    pub fn validate(&self) -> Result<(), Status> {
        if self.version != GPU_SCENE_VERSION || (self.size as usize) < std::mem::size_of::<Self>()
            || self.context.is_null() || self.create.is_none() || self.submit.is_none()
            || self.capture.is_none() || self.destroy.is_none()
        {
            return Err(INVALID_ARGUMENT);
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct OpenRequestGpuV1 {
    pub text: OpenRequestTextV1,
    pub gpu_scene: *const GpuSceneServiceV1,
}

impl OpenRequest {
    /// # Safety
    /// `request` is the original ABI pointer to the allocation described by
    /// its header. A nonnull GPU table is readable through its declared size.
    pub unsafe fn gpu_scene(request: *const Self) -> Result<Option<GpuSceneServiceV1>, Status> {
        if request.is_null() || !request.is_aligned() {
            return Err(INVALID_ARGUMENT);
        }
        let header = unsafe { (*request).header };
        if !header.accepts::<OpenRequest>() {
            return Err(INVALID_ARGUMENT);
        }
        if !header.accepts::<OpenRequestGpuV1>() {
            return Ok(None);
        }
        let service = unsafe { (*request.cast::<OpenRequestGpuV1>()).gpu_scene };
        if service.is_null() {
            return Ok(None);
        }
        if !service.is_aligned() {
            return Err(INVALID_ARGUMENT);
        }
        let header = unsafe { &*service.cast::<[u32; 2]>() };
        if header[0] != GPU_SCENE_VERSION || (header[1] as usize) < std::mem::size_of::<GpuSceneServiceV1>() {
            return Err(INVALID_ARGUMENT);
        }
        let service = unsafe { *service };
        service.validate()?;
        Ok(Some(service))
    }
}
