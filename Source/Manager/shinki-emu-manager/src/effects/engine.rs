use std::path::Path;

use shinki_emu_manager_core::FilterSettings;

use super::chain::{ChainPlan, PreparedStage, Stage, TargetSpec, prepared_stages};

#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    #[error("{0}")]
    Settings(String),
    #[error("SHINKI_EMU_FILTER_FORMAT4: {0}")]
    Format4(String),
    #[error("SHINKI_EMU_FILTER_DXC_PATH_MISSING")]
    DxcPathMissing,
    #[error("SHINKI_EMU_FILTER_CAPABILITY_MISSING")]
    CapabilityMissing,
    #[error("SHINKI_EMU_FILTER_TEXTURE_FORMAT")]
    TextureFormat,
    #[error("SHINKI_EMU_FILTER_TEXTURE_USAGE")]
    TextureUsage,
    #[error("SHINKI_EMU_FILTER_TEXTURE_DIMENSIONS")]
    TextureDimensions,
    #[error("SHINKI_EMU_FILTER_WGPU_VALIDATION: {0}")]
    GpuValidation(String),
    #[error("SHINKI_EMU_FILTER_COMPILE: {0}")]
    Compile(String),
}

/// Metadata-only chain. Every output extent is checked before creating targets.
pub struct PreparedFilter(Vec<PreparedStage>);

impl PreparedFilter {
    pub fn new(settings: &FilterSettings) -> Result<Self, FilterError> {
        settings.validate().map_err(FilterError::Settings)?;
        Ok(Self(prepared_stages(settings)?))
    }

    pub fn output_dimensions(&self, width: u32, height: u32) -> Result<(u32, u32), FilterError> {
        let plan = self.plan((width, height), u32::MAX)?;
        Ok(plan.output())
    }

    fn plan(&self, input: (u32, u32), limit: u32) -> Result<ChainPlan, FilterError> {
        ChainPlan::new(input, limit, self.0.iter().map(|stage| stage.layout))
    }
}

/// A compiled chain on the host device. Only the backends own scratch
/// resources; the renderer owns the targets between them and the final image.
#[derive(Default)]
pub struct FilterEngine(Vec<Stage>);

/// Texture slots and their checked stage schedule, owned by the renderer.
pub struct FilterTargets {
    plan: ChainPlan,
    textures: Vec<wgpu::Texture>,
}

impl FilterTargets {
    pub fn matches_input(&self, width: u32, height: u32) -> bool {
        self.plan.input == (width, height)
    }

    pub fn is_bypass(&self) -> bool {
        self.plan.steps.is_empty()
    }
}

impl FilterEngine {
    /// Optional feature used only by the format-4 backend.
    pub fn format4_features() -> wgpu::Features {
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
    }

    pub fn build(
        device: &wgpu::Device,
        dxc_path: &Path,
        prepared: PreparedFilter,
        width: u32,
        height: u32,
    ) -> Result<(Self, FilterTargets), FilterError> {
        let plan = prepared.plan((width, height), device.limits().max_texture_dimension_2d)?;
        if prepared.0.iter().any(PreparedStage::needs_dxc) {
            if !device.features().contains(Self::format4_features()) {
                return Err(FilterError::CapabilityMissing);
            }
            verify_dxc_path(dxc_path)?;
        }
        gpu_checked(device, || {
            let mut engine = Self(
                prepared
                    .0
                    .into_iter()
                    .map(|stage| stage.build(device, dxc_path))
                    .collect::<Result<_, _>>()?,
            );
            let targets = engine.allocate(device, plan)?;
            Ok((engine, targets))
        })
    }

    pub fn resize_targets(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<FilterTargets, FilterError> {
        let plan = ChainPlan::new(
            (width, height),
            device.limits().max_texture_dimension_2d,
            self.0.iter().map(|stage| stage.layout),
        )?;
        gpu_checked(device, || self.allocate(device, plan))
    }

    fn allocate(
        &mut self,
        device: &wgpu::Device,
        plan: ChainPlan,
    ) -> Result<FilterTargets, FilterError> {
        for (stage, step) in self.0.iter_mut().zip(&plan.steps) {
            stage.backend.prepare(device, step.input, step.output)?;
        }
        let textures = plan
            .slots
            .iter()
            .map(|spec| create_target(device, *spec))
            .collect();
        Ok(FilterTargets { plan, textures })
    }

    /// Record the ordered dispatches on the caller's encoder. An empty chain
    /// returns the input directly. No stage boundary requires a texture copy.
    pub fn encode<'a>(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: &'a wgpu::Texture,
        targets: &'a FilterTargets,
    ) -> Result<&'a wgpu::Texture, FilterError> {
        if !targets.matches_input(input.width(), input.height())
            || targets.plan.steps.len() != self.0.len()
        {
            return Err(FilterError::TextureDimensions);
        }
        if targets.is_bypass() {
            return Ok(input);
        }
        if input.format() != wgpu::TextureFormat::Rgba8Unorm {
            return Err(FilterError::TextureFormat);
        }
        if !input.usage().contains(wgpu::TextureUsages::TEXTURE_BINDING) {
            return Err(FilterError::TextureUsage);
        }
        let mut source = input;
        for (stage, step) in self.0.iter_mut().zip(&targets.plan.steps) {
            let target = &targets.textures[step.slot];
            if source == target || target.format() != stage.layout.format {
                return Err(FilterError::TextureFormat);
            }
            stage.backend.encode(device, encoder, source, target)?;
            source = target;
        }
        Ok(source)
    }
}

fn create_target(device: &wgpu::Device, spec: TargetSpec) -> wgpu::Texture {
    use wgpu::TextureUsages as Usage;
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shinki.emu.filter.target"),
        size: super::chain::extent(spec.size),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: spec.format,
        usage: Usage::TEXTURE_BINDING
            | Usage::STORAGE_BINDING
            | if spec.present {
                Usage::RENDER_ATTACHMENT | Usage::COPY_SRC
            } else {
                Usage::empty()
            },
        view_formats: &[],
    })
}

/// Error scopes catch resource or encoding failures before a caller commits.
pub fn gpu_checked<T>(
    device: &wgpu::Device,
    work: impl FnOnce() -> Result<T, FilterError>,
) -> Result<T, FilterError> {
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = work();
    let validation = pollster::block_on(validation.pop());
    let oom = pollster::block_on(oom.pop());
    if let Some(error) = validation.or(oom) {
        return Err(FilterError::GpuValidation(error.to_string()));
    }
    result
}

fn verify_dxc_path(path: &Path) -> Result<(), FilterError> {
    let expected_name = format!(
        "{}dxcompiler.{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_EXTENSION
    );
    if !path.is_file()
        || !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(&expected_name))
    {
        return Err(FilterError::DxcPathMissing);
    }
    Ok(())
}
