//! The UI and native owner share only host-owned WGPU resources. The core
//! sees a C service and an integer target ID, never a Rust or backend handle.
use super::frame::FrameMailboxInner;
use roukaku_platform::SceneFrame;
use roukaku_platform_common::{WgpuSceneTarget, WgpuSharedSceneRenderer};
use shinki_emu_abi as abi;
use shinki_emu_manager_core::model::{FrameAlpha, FrameFormat, FrameInfo, FrameRowOrder};
use std::{ffi::c_void, mem::MaybeUninit, panic::AssertUnwindSafe, ptr, sync::Arc};

// Latest mailbox frame, UI snapshot, displayed texture, old UI texture until
// the replacement takes effect, and one writer. Idle slots are reused.
const SLOT_COUNT: usize = 5;

#[derive(Default)]
pub(super) struct GpuStage {
    device: Option<(wgpu::Device, wgpu::Queue)>,
    next_id: u64,
    // Load opens a replacement scene before it commits and drops the old one.
    targets: [Option<Target>; 2],
}

struct Target {
    id: u64,
    epoch: u64,
    scene: WgpuSharedSceneRenderer,
    scratch: SceneFrame,
    slots: [Option<Slot>; SLOT_COUNT],
    last: Option<usize>,
}

struct Slot {
    target: WgpuSceneTarget,
    lease: Arc<()>,
}

impl GpuStage {
    pub(super) fn setup(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        // Skia's Android path submits to a raw VkQueue outside the WGPU
        // queue lock. Until both producers share synchronization, it stays
        // on the existing CPU lease path. OHOS never sets up this renderer.
        if !cfg!(target_os = "android") && matches!(
            device.adapter_info().device_type,
            wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::DiscreteGpu,
        ) {
            self.device = Some((device.clone(), queue.clone()));
        }
    }

    pub(super) fn teardown(&mut self) {
        self.targets = Default::default();
        self.device = None;
    }

    pub(super) fn available(&self) -> bool {
        self.device.is_some()
    }
}

pub(super) fn service(context: &Arc<FrameMailboxInner>) -> abi::GpuSceneServiceV2 {
    abi::GpuSceneServiceV2 {
        base: abi::GpuSceneServiceV1 {
            version: abi::GPU_SCENE_VERSION,
            size: std::mem::size_of::<abi::GpuSceneServiceV2>() as u32,
            context: Arc::as_ptr(context).cast_mut().cast(),
            create: Some(create),
            submit: Some(submit),
            capture: Some(capture),
            destroy: Some(destroy),
        },
        present_raster: Some(present_raster),
    }
}

struct Failure(abi::Status, &'static str, String);
impl Failure {
    fn status(status: abi::Status) -> Self {
        Self(status, "SHINKI_EMU_GPU_SCENE_ABI", String::new())
    }
    fn render(cause: roukaku_platform::PlatformError) -> Self {
        Self(abi::CORE_ERROR, "SHINKI_EMU_GPU_SCENE_RENDER", format!("{:?} during {}", cause.code, cause.operation))
    }
}

fn boundary(
    error: *mut abi::Error,
    run: impl FnOnce() -> Result<(), Failure>,
) -> abi::Status {
    let result = std::panic::catch_unwind(AssertUnwindSafe(run))
        .unwrap_or_else(|_| Err(Failure::status(abi::PANICKED)));
    let Err(Failure(status, code, message)) = result else { return abi::OK };
    if let Some(error) = unsafe { error.as_mut() } {
        error.set(code, &message);
    }
    status
}

unsafe fn context<'a>(pointer: *mut c_void) -> Result<&'a FrameMailboxInner, Failure> {
    // Manager keeps a mailbox clone through open and through native close.
    unsafe { pointer.cast::<FrameMailboxInner>().as_ref() }
        .ok_or_else(|| Failure::status(abi::INVALID_ARGUMENT))
}

unsafe extern "C" fn create(pointer: *mut c_void, epoch: u64, id: *mut u64, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        let id = unsafe { id.as_mut() }.ok_or_else(|| Failure::status(abi::INVALID_ARGUMENT))?;
        *id = 0;
        let context = unsafe { context(pointer) }?;
        let mut gpu = context.gpu.lock().map_err(|_| Failure::status(abi::PANICKED))?;
        let gpu = &mut *gpu;
        let (device, _) = gpu.device.as_ref().ok_or_else(|| Failure::status(abi::UNSUPPORTED))?;
        context.require_epoch(epoch).map_err(Failure::status)?;
        let slot = gpu.targets.iter_mut().find(|slot| slot.is_none())
            .ok_or_else(|| Failure::status(abi::CAPACITY))?;
        let next = gpu.next_id.checked_add(1).ok_or_else(|| Failure::status(abi::INVALID_STATE))?;
        *slot = Some(Target {
            id: next,
            epoch,
            scene: WgpuSharedSceneRenderer::new(device),
            scratch: SceneFrame {
                sequence: 0, width: 0, height: 0, clear_rgba: [0; 4], commands: Vec::new(), semantics: None,
            },
            slots: Default::default(),
            last: None,
        });
        gpu.next_id = next;
        *id = next;
        tracing::debug!(event = "shinki.emu.host.gpu_scene.created", session_epoch = epoch, target = next);
        Ok(())
    })
}

unsafe extern "C" fn submit(pointer: *mut c_void, id: u64, frame: *const abi::GpuSceneFrameV1, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        let context = unsafe { context(pointer) }?;
        let header = unsafe { frame.cast::<abi::Header>().as_ref() }
            .ok_or_else(|| Failure::status(abi::INVALID_ARGUMENT))?;
        if !header.accepts::<abi::GpuSceneFrameV1>() {
            return Err(Failure::status(abi::INVALID_ARGUMENT));
        }
        let frame = unsafe { &*frame };
        let info = FrameInfo {
            width: frame.raster.width,
            height: frame.raster.height,
            logical_width: frame.logical.width,
            logical_height: frame.logical.height,
            stride: frame.raster.width.checked_mul(4).ok_or_else(|| Failure::status(abi::INVALID_ARGUMENT))?,
            format: FrameFormat::Rgba8Srgb { alpha: FrameAlpha::Opaque },
            rows: FrameRowOrder::TopToBottom,
        };
        info.validate().map_err(|_| Failure::status(abi::INVALID_ARGUMENT))?;
        if info.required_bytes().is_none_or(|size| size > abi::GPU_MAX_IMAGE_BYTES) {
            return Err(Failure::status(abi::INVALID_ARGUMENT));
        }
        let mut gpu = context.gpu.lock().map_err(|_| Failure::status(abi::PANICKED))?;
        let gpu = &mut *gpu;
        let (device, queue) = gpu.device.as_ref().ok_or_else(|| Failure::status(abi::INVALID_STATE))?;
        let target = gpu.targets.iter_mut().flatten().find(|target| target.id == id)
            .ok_or_else(|| Failure::status(abi::STALE_CONTEXT))?;
        context.require_epoch(target.epoch).map_err(Failure::status)?;
        target.submit(device, queue, frame, info, context)
    })
}

unsafe extern "C" fn present_raster(
    pointer: *mut c_void,
    id: u64,
    frame: *const abi::GpuRasterFrameV2,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        if !frame.is_aligned() {
            return Err(Failure::status(abi::INVALID_ARGUMENT));
        }
        let header = unsafe { frame.cast::<abi::Header>().as_ref() }
            .ok_or_else(|| Failure::status(abi::INVALID_ARGUMENT))?;
        if !header.accepts::<abi::GpuRasterFrameV2>() {
            return Err(Failure::status(abi::INVALID_ARGUMENT));
        }
        let frame = unsafe { &*frame };
        let context = unsafe { context(pointer) }?;
        let mut gpu = context.gpu.lock().map_err(|_| Failure::status(abi::PANICKED))?;
        let target = gpu.targets.iter_mut().flatten().find(|target| target.id == id)
            .ok_or_else(|| Failure::status(abi::STALE_CONTEXT))?;
        context.require_epoch(target.epoch).map_err(Failure::status)?;
        if frame.sequence <= target.scratch.sequence {
            return Err(Failure::status(abi::STALE_CONTEXT));
        }
        let raster = frame.image.extent;
        let info = FrameInfo {
            width: raster.width,
            height: raster.height,
            logical_width: frame.logical.width,
            logical_height: frame.logical.height,
            stride: raster.width.checked_mul(4).ok_or_else(|| Failure::status(abi::INVALID_ARGUMENT))?,
            format: FrameFormat::Rgba8Srgb { alpha: FrameAlpha::Ignored },
            rows: FrameRowOrder::TopToBottom,
        };
        let pixels = unsafe { shinki_emu_sdk::gpu_scene::decode_raster(frame) }.map_err(Failure::status)?;
        context.publish_native_raster(target.epoch, info, pixels).map_err(Failure::status)?;
        target.scratch.sequence = frame.sequence;
        Ok(())
    })
}

impl Target {
    fn submit(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, raw: &abi::GpuSceneFrameV1, info: FrameInfo, context: &FrameMailboxInner) -> Result<(), Failure> {
        if raw.sequence <= self.scratch.sequence {
            return Err(Failure::status(abi::STALE_CONTEXT));
        }
        let index = self.slots.iter().position(|slot| {
            slot.as_ref().is_some_and(|slot| Arc::strong_count(&slot.lease) == 1
                && slot.target.texture().width() == info.width
                && slot.target.texture().height() == info.height)
        }).or_else(|| self.slots.iter().position(|slot| {
            slot.as_ref().is_none_or(|slot| Arc::strong_count(&slot.lease) == 1)
        })).ok_or_else(|| Failure::status(abi::CAPACITY))?;
        if self.slots[index].as_ref().is_none_or(|slot| {
            slot.target.texture().width() != info.width || slot.target.texture().height() != info.height
        }) {
            self.slots[index] = Some(Slot {
                target: WgpuSceneTarget::new(device, info.width, info.height).map_err(Failure::render)?,
                lease: Arc::new(()),
            });
        }
        let slot = self.slots[index].as_mut().unwrap();
        unsafe { shinki_emu_sdk::gpu_scene::decode(raw, &mut self.scratch.commands) }
            .map_err(Failure::status)?;
        // Decode holds no data across submit except via explicit image leases.
        let previous_sequence = self.scratch.sequence;
        self.scratch.sequence = raw.sequence;
        self.scratch.width = info.width;
        self.scratch.height = info.height;
        self.scratch.clear_rgba = raw.clear_rgba;
        let rendered = self.scene.render(device, queue, &self.scratch, &mut slot.target);
        self.scratch.commands.clear();
        if let Err(cause) = rendered {
            self.scratch.sequence = previous_sequence;
            return Err(Failure::render(cause));
        }
        self.last = Some(index);
        context.publish_gpu(self.epoch, info, slot.target.texture().clone(), Arc::clone(&slot.lease))
            .map_err(Failure::status)
    }
}

unsafe extern "C" fn capture(pointer: *mut c_void, id: u64, bytes: *mut u8, len: usize, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        if bytes.is_null() || len == 0 || len > abi::GPU_MAX_IMAGE_BYTES {
            return Err(Failure::status(abi::INVALID_ARGUMENT));
        }
        let context = unsafe { context(pointer) }?;
        let gpu = context.gpu.lock().map_err(|_| Failure::status(abi::PANICKED))?;
        let (device, queue) = gpu.device.as_ref().ok_or_else(|| Failure::status(abi::INVALID_STATE))?;
        let target = gpu.targets.iter().flatten().find(|target| target.id == id)
            .ok_or_else(|| Failure::status(abi::STALE_CONTEXT))?;
        context.require_epoch(target.epoch).map_err(Failure::status)?;
        let slot = target.last.and_then(|index| target.slots[index].as_ref())
            .ok_or_else(|| Failure::status(abi::INVALID_STATE))?;
        let dst = unsafe { std::slice::from_raw_parts_mut(bytes.cast::<MaybeUninit<u8>>(), len) };
        slot.target.capture_into(device, queue, dst).map_err(Failure::render)
    })
}

unsafe extern "C" fn destroy(pointer: *mut c_void, id: u64) {
    let _ = boundary(ptr::null_mut(), || {
        let context = unsafe { context(pointer) }?;
        // A render panic still must release the core's retained asset owners.
        let mut gpu = context.gpu.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(slot) = gpu.targets.iter_mut().find(|slot| slot.as_ref().is_some_and(|t| t.id == id)) {
            *slot = None;
        }
        Ok(())
    });
}
