use super::{GpuSceneClient, GpuSceneHost};
use crate::{WgpuOffscreenRenderer, runtime::SessionError};
use roukaku_media_core::{OwnedPixelBuffer, SceneCompositing2D};
use roukaku_platform::{PlatformError, SceneFrame};
use shinki_emu_abi::Extent;
use std::sync::Arc;

/// The complete set of transports a core needs for a hosted session.
#[derive(Clone, Copy)]
pub enum SceneRequirements {
    /// Commands fit the v1 scene wire contract.
    Sprites,
    /// Commands fit v1, with occasional effects already produced by a CPU canvas.
    SpritesWithNativeRaster,
}

/// One scene resource namespace and output transport, selected at open.
pub struct SceneOutput {
    inner: Output,
}

#[allow(clippy::large_enum_variant, reason = "Keep the renderer inline")]
enum Output {
    Offscreen {
        renderer: WgpuOffscreenRenderer,
        // The native backend loader must outlive the renderer and its resources.
        instance: wgpu::Instance,
    },
    Hosted {
        host: GpuSceneHost,
        target: GpuSceneClient,
    },
}

impl SceneOutput {
    /// The offscreen instance factory runs only if the required hosted transport
    /// is absent or the host declines target creation with `UNSUPPORTED`.
    pub fn open(
        host: Option<GpuSceneHost>,
        requirements: SceneRequirements,
        offscreen_instance: impl FnOnce() -> Result<wgpu::Instance, SessionError>,
    ) -> Result<Self, SessionError> {
        match Self::try_hosted(host, requirements)? {
            Some(output) => Ok(output),
            None => Self::offscreen(offscreen_instance()?),
        }
    }

    /// Negotiate first when offscreen creation must happen on a different
    /// thread. `None` always means the complete session must use compatibility.
    pub fn try_hosted(
        host: Option<GpuSceneHost>,
        requirements: SceneRequirements,
    ) -> Result<Option<Self>, SessionError> {
        let Some(host) = host.filter(|host| {
            matches!(requirements, SceneRequirements::Sprites) || host.present_raster.is_some()
        }) else {
            return Ok(None);
        };
        Ok(host.create()?.map(|target| Self {
            inner: Output::Hosted { host, target },
        }))
    }

    /// Start with the offscreen renderer. Use this for command graphs that the
    /// hosted renderer cannot execute in full, and for independent pixel tests.
    pub fn offscreen(instance: wgpu::Instance) -> Result<Self, SessionError> {
        let renderer = pollster::block_on(WgpuOffscreenRenderer::new(&instance))
            .map_err(|cause| render_error("SHINKI_EMU_GPU_CREATE", cause))?
            .with_default_compositing(SceneCompositing2D::EncodedSrgb);
        tracing::info!(event = "shinki.emu.sdk.gpu.offscreen", backend = %renderer.identity().backend,
            device_type = %renderer.identity().device_type, vendor_id = renderer.identity().vendor_id,
            device_id = renderer.identity().device_id);
        Ok(Self {
            inner: Output::Offscreen { renderer, instance },
        })
    }

    /// Start a fresh resource namespace on the selected transport. Failure does
    /// not switch transport or discard the existing target.
    pub fn rebuild(&self) -> Result<Self, SessionError> {
        match &self.inner {
            Output::Offscreen { instance, .. } => Self::offscreen(instance.clone()),
            Output::Hosted { host, .. } => {
                let target = host.create()?.ok_or_else(|| {
                    SessionError::new(
                        "SHINKI_EMU_GPU_REBUILD",
                        "host declined a replacement scene target",
                    )
                })?;
                Ok(Self {
                    inner: Output::Hosted {
                        host: *host,
                        target,
                    },
                })
            }
        }
    }

    pub fn is_hosted(&self) -> bool {
        matches!(self.inner, Output::Hosted { .. })
    }

    pub fn validate_raster(&self, raster: Extent) -> Result<(), SessionError> {
        match &self.inner {
            Output::Hosted { .. } => {
                super::validate_extents(raster, raster).map_err(super::encode_error)
            }
            Output::Offscreen { renderer, .. } => renderer
                .validate_output_extent(raster.width, raster.height)
                .map_err(|cause| render_error("SHINKI_EMU_GPU_EXTENT", cause)),
        }
    }

    /// Returns pixels only for the offscreen compatibility path. A hosted
    /// render publishes directly; the caller should leave the base CPU lease
    /// pending, without synthesizing a generation or pixel data.
    pub fn submit(
        &mut self,
        frame: &SceneFrame,
        logical: Extent,
    ) -> Result<Option<Arc<[u8]>>, SessionError> {
        match &mut self.inner {
            Output::Hosted { target, .. } => {
                target.submit(frame, logical)?;
                Ok(None)
            }
            Output::Offscreen { renderer, .. } => renderer
                .render(frame)
                .map(|rendered| Some(rendered.rgba8))
                .map_err(|cause| render_error("SHINKI_EMU_GPU_RENDER", cause)),
        }
    }

    /// Publish pixels originally produced by the native CPU canvas in the
    /// hosted stream's order. This never reads a GPU target or opens a CPU lease
    /// through the base frame API. The host may retain the immutable image.
    pub fn present_native_raster(
        &self,
        sequence: u64,
        logical: Extent,
        raster: Extent,
        pixels: &OwnedPixelBuffer,
    ) -> Result<(), SessionError> {
        let Output::Hosted { target, .. } = &self.inner else {
            return Err(SessionError::new(
                "SHINKI_EMU_RASTER_PRESENT",
                "native raster presentation requires a hosted target",
            ));
        };
        target.present_raster(sequence, logical, raster, pixels)
    }

    /// Read the most recent hosted scene only when an event needs its pixels.
    /// Offscreen callers already have the pixels returned by `submit`.
    pub fn capture(&self) -> Result<Arc<[u8]>, SessionError> {
        if let Output::Hosted { target, .. } = &self.inner
            && let Some(raster) = target.last_raster
        {
            return target.capture(raster);
        }
        Err(SessionError::new(
            "SHINKI_EMU_GPU_CAPTURE",
            "no submitted hosted scene to capture",
        ))
    }

    /// Diagnostics for the compatibility path.
    pub fn offscreen_renderer(&self) -> Option<&WgpuOffscreenRenderer> {
        match &self.inner {
            Output::Offscreen { renderer, .. } => Some(renderer),
            Output::Hosted { .. } => None,
        }
    }
}

fn render_error(code: &str, cause: PlatformError) -> SessionError {
    SessionError::new(
        code,
        format!("{:?} during {}: {}", cause.code, cause.operation, cause),
    )
}
