use std::{
    env,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[cfg(target_env = "ohos")]
use shinki_emu_manager::StageImageUpdate;
use shinki_emu_manager::{
    FilterApplyState, FilterEvent, ShinkiUnderlayRenderer, StageTextureUpdate, WgpuFrameContext,
    effects::{FilterError, PreparedFilter, gpu_checked},
};
use shinki_emu_manager_core::{
    FilterSettings,
    model::FrameIdentity,
};
use roukaku_media_core::Viewport2D;
use wgpu::{Extent3d, TextureUsages};

const DEFAULT_STAGE_WIDTH: u32 = 800;
const DEFAULT_STAGE_HEIGHT: u32 = 600;

#[path = "stage_filter.rs"]
mod filter;
#[path = "stage_frame.rs"]
mod frame;
#[path = "stage_gpu.rs"]
mod gpu;
#[path = "stage_pixels.rs"]
mod pixels;
#[cfg(target_env = "ohos")]
#[path = "stage_software.rs"]
mod software;
use filter::{ActiveFilter, PendingFilter};
pub(crate) use frame::FrameMailbox;
use frame::{GpuFrame, StageFrame};

pub(crate) struct ManagerStageRenderer {
    mailbox: FrameMailbox,
    device: Option<wgpu::Device>,
    vulkan_stage_sampling: bool,
    input_texture: Option<wgpu::Texture>,
    output_texture: Option<wgpu::Texture>,
    presented_input: Option<wgpu::Texture>,
    presented_gpu: Option<GpuFrame>,
    retired_gpu: Option<GpuFrame>,
    pixel_presentation: pixels::PixelPresentation,
    input_width: u32,
    input_height: u32,
    logical_width: u32,
    logical_height: u32,
    uploaded_identity: Option<FrameIdentity>,
    texture_dirty: bool,
    dxc_path: PathBuf,
    active_filter: Option<ActiveFilter>,
    pending_filter: Option<PendingFilter>,
    failed_filter_size: Option<(u32, u32)>,
    filter_event: Option<FilterEvent>,
    #[cfg(target_env = "ohos")]
    software: software::SoftwarePresentation,
}

enum DrawError {
    Host(String),
    Filter(FilterError),
}

impl ManagerStageRenderer {
    pub(crate) fn new(mailbox: FrameMailbox) -> Self {
        Self::with_dxc_path(mailbox, configured_dxc_path())
    }

    fn with_dxc_path(mailbox: FrameMailbox, dxc_path: PathBuf) -> Self {
        Self {
            mailbox,
            device: None,
            vulkan_stage_sampling: false,
            input_texture: None,
            output_texture: None,
            presented_input: None,
            presented_gpu: None,
            retired_gpu: None,
            pixel_presentation: pixels::PixelPresentation::default(),
            input_width: DEFAULT_STAGE_WIDTH,
            input_height: DEFAULT_STAGE_HEIGHT,
            logical_width: DEFAULT_STAGE_WIDTH,
            logical_height: DEFAULT_STAGE_HEIGHT,
            uploaded_identity: None,
            texture_dirty: false,
            dxc_path,
            active_filter: None,
            pending_filter: None,
            failed_filter_size: None,
            filter_event: None,
            #[cfg(target_env = "ohos")]
            software: software::SoftwarePresentation::default(),
        }
    }

    fn configure_filter_inner(&mut self, settings: &FilterSettings) -> Result<FilterApplyState, String> {
        let prepared = PreparedFilter::new(settings).map_err(|error| error.to_string())?;
        let pending = if let (Some(device), Some(_)) = (&self.device, self.uploaded_identity) {
            PendingFilter::Ready(
                ActiveFilter::build(device, &self.dxc_path, prepared, self.input_width, self.input_height)
                    .map_err(|error| error.to_string())?,
            )
        } else {
            PendingFilter::Prepared(prepared)
        };
        self.pending_filter = Some(pending);
        Ok(FilterApplyState::Deferred)
    }

    fn render_frame(&mut self, context: WgpuFrameContext<'_>, frame: &StageFrame) -> Result<(), String> {
        let info = frame.info();
        let size = (info.width, info.height);
        validate_dimensions(context.device, size.0, size.1)?;
        if let Some(pending) = self.pending_filter.take() {
            let candidate = match pending {
                PendingFilter::Prepared(prepared) => {
                    ActiveFilter::build(context.device, &self.dxc_path, prepared, size.0, size.1)
                }
                PendingFilter::Ready(filter) => Ok(filter),
            };
            match candidate {
                Ok(mut candidate) => match self.draw_frame(&context, frame, Some(&mut candidate)) {
                    Ok(()) => {
                        self.active_filter = (!candidate.targets.is_bypass()).then_some(candidate);
                        self.failed_filter_size = None;
                        self.filter_event = Some(FilterEvent::Activated);
                    }
                    Err(DrawError::Filter(error)) => {
                        self.filter_event = Some(FilterEvent::Rejected(error.to_string()));
                    }
                    Err(DrawError::Host(error)) => return Err(error),
                },
                Err(error) => self.filter_event = Some(FilterEvent::Rejected(error.to_string())),
            }
            return Ok(());
        }
        if Some(frame.identity()) == self.uploaded_identity || self.failed_filter_size == Some(size) {
            return Ok(());
        }
        match self.draw_frame(&context, frame, None) {
            Ok(()) => self.failed_filter_size = None,
            Err(DrawError::Filter(error)) => {
                self.failed_filter_size = Some(size);
                self.filter_event = Some(FilterEvent::RenderFailed(error.to_string()));
            }
            Err(DrawError::Host(error)) => return Err(error),
        }
        Ok(())
    }

    fn draw_frame(
        &mut self,
        context: &WgpuFrameContext<'_>,
        frame: &StageFrame,
        candidate: Option<&mut ActiveFilter>,
    ) -> Result<(), DrawError> {
        let info = frame.info();
        let identity = frame.identity();
        let active = candidate.or(self.active_filter.as_mut());
        let active = if let Some(active) = active {
            active.resize(context.device, info.width, info.height).map_err(DrawError::Filter)?;
            (!active.targets.is_bypass()).then_some(active)
        } else {
            None
        };
        let gpu = match frame {
            StageFrame::Gpu(gpu) => Some(gpu),
            StageFrame::Cpu(_) => None,
        };
        let format = pixels::input_format(info.format);
        let replacement = if gpu.is_none() && self.input_texture.as_ref().is_none_or(|texture| {
            texture.width() != info.width || texture.height() != info.height || texture.format() != format
        }) {
            Some(create_input_texture(context.device, info.width, info.height, format).map_err(DrawError::Host)?)
        } else {
            None
        };
        let input = gpu.map(|gpu| &gpu.texture)
            .or(replacement.as_ref()).or(self.input_texture.as_ref()).unwrap();
        let fresh = replacement.is_some() || Some(identity) != self.uploaded_identity;
        let convert = fresh && pixels::requires_conversion(info);
        let baseline = if fresh {
            input
        } else {
            self.presented_input.as_ref().ok_or_else(|| DrawError::Host("SHINKI_EMU_HOST_FRAME_BASELINE".into()))?
        };
        let (baseline, output, command) = if convert || active.is_some() {
            gpu_checked(context.device, || {
                let mut encoder = context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("shinki.emu.stage.frame"),
                });
                let baseline = if convert {
                    self.pixel_presentation.encode(context.device, context.queue, &mut encoder, input, info)
                } else {
                    baseline.clone()
                };
                let output = if let Some(active) = active {
                    active.engine.encode(context.device, &mut encoder, &baseline, &active.targets)?.clone()
                } else {
                    baseline.clone()
                };
                if self.vulkan_stage_sampling {
                    prepare_stage_sampling(&mut encoder, &output);
                }
                Ok((baseline, output, Some(encoder.finish())))
            }).map_err(DrawError::Filter)?
        } else {
            (baseline.clone(), baseline.clone(), None)
        };
        if let (true, StageFrame::Cpu(cpu)) = (fresh, frame) {
            let data = cpu.pixels().ok_or_else(|| DrawError::Host("SHINKI_EMU_HOST_FRAME_PIXELS".into()))?;
            let data = info.required_bytes().and_then(|length| data.get(..length))
                .ok_or_else(|| DrawError::Host("SHINKI_EMU_HOST_FRAME_SIZE".into()))?;
            context.queue.write_texture(
                input.as_image_copy(),
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(info.stride),
                    rows_per_image: Some(info.height),
                },
                Extent3d {
                    width: info.width,
                    height: info.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        if let Some(command) = command {
            context.queue.submit([command]);
        } else if fresh && self.vulkan_stage_sampling {
            // Submit the deferred write_texture and the sampling barrier
            // together; there was no conversion/filter encoder to reuse.
            let mut encoder =
                context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("shinki.emu.stage.sampling"),
                });
            prepare_stage_sampling(&mut encoder, &output);
            context.queue.submit([encoder.finish()]);
        }
        self.mailbox.presented(identity, info).map_err(DrawError::Host)?;
        if let Some(input) = replacement {
            self.input_texture = Some(input);
        }
        if fresh {
            // Slint installs the returned replacement after render() returns.
            // Keep the previous target leased through that UI render pass.
            self.retired_gpu = self.presented_gpu.take();
            self.presented_gpu = gpu.cloned();
        }
        self.presented_input = Some(baseline);
        self.uploaded_identity = Some(identity);
        if self.output_texture.as_ref() != Some(&output)
            || (self.logical_width, self.logical_height) != (info.logical_width, info.logical_height)
            || (self.input_width, self.input_height) != (info.width, info.height)
        {
            self.output_texture = Some(output);
            self.logical_width = info.logical_width;
            self.logical_height = info.logical_height;
            self.input_width = info.width;
            self.input_height = info.height;
            self.texture_dirty = true;
        }
        Ok(())
    }
}

pub(crate) fn configured_dxc_path() -> PathBuf {
    env::var_os("SHINKI_EMU_DXC_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(default_dxc_path)
}

fn default_dxc_path() -> PathBuf {
    let executable = shinki_emu_manager::platform::program_file()
        .unwrap_or_else(|_| PathBuf::from("shinki-emu-manager"));
    dxc_path_for_executable(&executable)
}

fn dxc_path_for_executable(executable: &Path) -> PathBuf {
    let filename = format!(
        "{}dxcompiler.{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_EXTENSION
    );
    match executable.parent() {
        Some(directory) => directory.join(filename),
        None => PathBuf::from(filename),
    }
}

impl ShinkiUnderlayRenderer for ManagerStageRenderer {
    fn configure_filter(&mut self, settings: &FilterSettings) -> Result<FilterApplyState, String> {
        #[cfg(target_env = "ohos")]
        {
            if settings.stages.iter().any(|stage| !matches!(
                stage, shinki_emu_manager_core::FilterStage::Format4 { disabled: true, .. }
            )) {
                return Err(
                    "SHINKI_EMU_OHOS_FILTERS_UNAVAILABLE: final-frame filters require a GPU renderer"
                        .into(),
                );
            }
            Ok(FilterApplyState::Applied)
        }
        #[cfg(not(target_env = "ohos"))]
        self.configure_filter_inner(settings)
    }

    fn take_filter_event(&mut self) -> Option<FilterEvent> {
        self.filter_event.take()
    }

    fn setup(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String> {
        if self.device.is_some() {
            return Err("SHINKI_EMU_HOST_RENDERER_DUPLICATE_SETUP".into());
        }
        let adapter = context.device.adapter_info();
        tracing::info!(
            event = "shinki.emu.host.gpu.created",
            backend = ?adapter.backend,
            device_type = ?adapter.device_type,
            device = %adapter.name,
            vendor_id = adapter.vendor,
            device_id = adapter.device,
            driver = %adapter.driver,
        );
        // Android uses Skia's native Vulkan sampler; desktop FemtoVG's
        // texture reads are already tracked by wgpu.
        self.vulkan_stage_sampling =
            cfg!(target_os = "android") && adapter.backend == wgpu::Backend::Vulkan;
        self.mailbox.setup_gpu(&context)?;
        self.device = Some(context.device.clone());
        Ok(())
    }

    fn take_stage_texture_update(&mut self) -> Result<Option<StageTextureUpdate>, String> {
        if !self.texture_dirty {
            return Ok(None);
        }
        let texture = self
            .output_texture
            .clone()
            .ok_or_else(|| "SHINKI_EMU_HOST_RENDERER_OUTPUT_MISSING".to_owned())?;
        let content = content_viewport_for_output(
            self.logical_width,
            self.logical_height,
            self.input_width,
            self.input_height,
            texture.width(),
            texture.height(),
        )?;
        self.texture_dirty = false;
        Ok(Some(StageTextureUpdate {
            raster_width: texture.width(),
            raster_height: texture.height(),
            texture,
            logical_width: self.logical_width,
            logical_height: self.logical_height,
            content,
        }))
    }

    fn render(&mut self, context: WgpuFrameContext<'_>) -> Result<(), String> {
        self.retired_gpu = None;
        let Some(frame) = self.mailbox.stage_snapshot()? else {
            if self.uploaded_identity.is_some() {
                if let (Some(gpu), Some(output)) = (&self.presented_gpu, &mut self.output_texture)
                    && gpu.texture == *output
                {
                    *output = create_input_texture(context.device, output.width(), output.height(), output.format())?;
                    self.texture_dirty = true;
                }
                if let Some(output) = &self.output_texture {
                    let view = output.create_view(&Default::default());
                    let mut encoder = context.device.create_command_encoder(&Default::default());
                    {
                        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            ..Default::default()
                        });
                    }
                    if self.vulkan_stage_sampling {
                        prepare_stage_sampling(&mut encoder, output);
                    }
                    context.queue.submit([encoder.finish()]);
                }
                self.input_texture = None;
                self.presented_input = None;
                self.retired_gpu = self.presented_gpu.take();
                self.pixel_presentation = pixels::PixelPresentation::default();
                self.uploaded_identity = None;
                self.failed_filter_size = None;
            }
            return Ok(());
        };
        self.render_frame(context, &frame)
    }

    #[cfg(target_env = "ohos")]
    fn take_stage_image_update(&mut self) -> Result<Option<StageImageUpdate>, String> {
        self.software.update(&self.mailbox)
    }

    fn teardown(&mut self) -> Result<(), String> {
        self.mailbox.teardown_gpu();
        self.presented_gpu = None;
        self.retired_gpu = None;
        self.input_texture = None;
        self.output_texture = None;
        self.presented_input = None;
        self.pixel_presentation = pixels::PixelPresentation::default();
        self.device = None;
        self.vulkan_stage_sampling = false;
        self.input_width = DEFAULT_STAGE_WIDTH;
        self.input_height = DEFAULT_STAGE_HEIGHT;
        self.logical_width = DEFAULT_STAGE_WIDTH;
        self.logical_height = DEFAULT_STAGE_HEIGHT;
        self.uploaded_identity = None;
        self.texture_dirty = false;
        self.active_filter = None;
        self.pending_filter = None;
        self.failed_filter_size = None;
        self.filter_event = None;
        #[cfg(target_env = "ohos")]
        {
            self.software = software::SoftwarePresentation::default();
        }
        Ok(())
    }
}

// Skia samples imported Vulkan images on the native queue, outside wgpu's
// usage tracker. Slint transitions on import, not when a cached image's
// texture is rewritten. Return the same texture in sampled state after every
// write, ordered with that write in one submission.
fn prepare_stage_sampling(encoder: &mut wgpu::CommandEncoder, texture: &wgpu::Texture) {
    encoder.transition_resources(
        std::iter::empty(),
        std::iter::once(wgpu::TextureTransition {
            texture,
            selector: None,
            state: wgpu::TextureUses::RESOURCE,
        }),
    );
}

fn validate_dimensions(device: &wgpu::Device, width: u32, height: u32) -> Result<(), String> {
    let max = device.limits().max_texture_dimension_2d;
    if width == 0 || height == 0 || width > max || height > max {
        return Err(format!(
            "SHINKI_EMU_HOST_FRAME_DIMENSIONS: requested {width}x{height}, device limit {max}"
        ));
    }
    Ok(())
}

pub(crate) fn content_viewport_for_output(
    logical_width: u32,
    logical_height: u32,
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
) -> Result<Viewport2D, String> {
    if [
        logical_width,
        logical_height,
        input_width,
        input_height,
        output_width,
        output_height,
    ]
    .contains(&0)
    {
        return Err("SHINKI_EMU_HOST_FRAME_DIMENSIONS".into());
    }
    // A native surface already includes the core's own scene/UI/letterboxing.
    // Host V is the complete filtered raster; only the core maps pixels to L.
    Ok(Viewport2D {
        x: 0,
        y: 0,
        width: output_width,
        height: output_height,
    })
}

fn create_input_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> Result<wgpu::Texture, String> {
    validate_dimensions(device, width, height)?;
    let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shinki-emu-manager-stage-input"),
        size: Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_DST
            | TextureUsages::COPY_SRC
            | TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    if let Some(error) = pollster::block_on(error_scope.pop()) {
        return Err(format!("SHINKI_EMU_HOST_RENDERER_TEXTURE: {error}"));
    }
    Ok(texture)
}

#[cfg(test)]
#[path = "stage_renderer_tests.rs"]
mod tests;
