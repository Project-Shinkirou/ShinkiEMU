use std::{collections::BTreeMap, path::Path};

use shinki_emu_manager_core::{FILTER_DEFAULT_STRENGTH, FilterSettings, FilterStage};

use super::{
    FilterError,
    anime4k_original::{Anime4kFilter, Anime4kKind},
    classic::{ClassicFilter, ClassicKind},
    format4_gpu::{Format4Filter, PreparedFormat4},
};

pub(super) type Size = (u32, u32);
const RGBA8: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

const RESTORE_SOURCE: &str =
    include_str!("../../../../../Assets/Effects/Anime4K/restore_cnn_s.hlsl");
const UPSCALE_SOURCE: &str =
    include_str!("../../../../../Assets/Effects/Anime4K/upscale_cnn_x2_s.hlsl");
const SCALE_SOURCE: &str = include_str!("../../../../../Assets/Effects/Builtin/scale.hlsl");
const SHARPEN_SOURCE: &str = include_str!("../../../../../Assets/Effects/Builtin/sharpen.hlsl");

#[derive(Clone, Copy)]
pub(super) enum OutputMode {
    SameExtent,
    Resize(Resize),
}

#[derive(Clone, Copy)]
pub(super) enum Resize {
    Factor(f32),
    Exact { x: u32, y: u32 },
}

impl OutputMode {
    fn resolve(self, input: Size) -> Result<Size, FilterError> {
        match self {
            Self::SameExtent => Ok(input),
            Self::Resize(Resize::Factor(factor)) => {
                let scaled = |size: u32| {
                    // Retain the legacy f32 rounding, but avoid a saturating cast
                    // at the u32 boundary (u32::MAX is not representable in f32).
                    let size = (size as f32 * factor).round();
                    (1.0..=f64::from(u32::MAX))
                        .contains(&f64::from(size))
                        .then_some(size as u32)
                        .ok_or(FilterError::TextureDimensions)
                };
                Ok((scaled(input.0)?, scaled(input.1)?))
            }
            Self::Resize(Resize::Exact { x, y }) => input
                .0
                .checked_mul(x)
                .zip(input.1.checked_mul(y))
                .ok_or(FilterError::TextureDimensions),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct StageLayout {
    pub(super) output: OutputMode,
    pub(super) format: wgpu::TextureFormat,
}

#[derive(Debug)]
pub(super) struct ChainPlan {
    pub(super) input: Size,
    pub(super) steps: Vec<Step>,
    pub(super) slots: Vec<TargetSpec>,
}

#[derive(Debug)]
pub(super) struct Step {
    pub(super) input: Size,
    pub(super) output: Size,
    pub(super) slot: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TargetSpec {
    pub(super) size: Size,
    pub(super) format: wgpu::TextureFormat,
    pub(super) present: bool,
}

impl ChainPlan {
    pub(super) fn new(
        input: Size,
        limit: u32,
        layouts: impl ExactSizeIterator<Item = StageLayout>,
    ) -> Result<Self, FilterError> {
        let check = |(width, height): Size| {
            ((1..=limit).contains(&width) && (1..=limit).contains(&height))
                .then_some(())
                .ok_or(FilterError::TextureDimensions)
        };
        check(input)?;
        let count = layouts.len();
        let mut steps = Vec::with_capacity(count);
        let mut slots = Vec::<TargetSpec>::new();
        let mut size = input;
        let mut source = None;
        for (index, layout) in layouts.enumerate() {
            let output = layout.output.resolve(size)?;
            check(output)?;
            let present = index + 1 == count;
            let slot = slots
                .iter()
                .enumerate()
                .position(|(i, target)| {
                    Some(i) != source && target.size == output && target.format == layout.format
                })
                .unwrap_or_else(|| {
                    slots.push(TargetSpec { size: output, format: layout.format, present });
                    slots.len() - 1
                });
            if present {
                slots[slot].present = true;
            }
            steps.push(Step { input: size, output, slot });
            size = output;
            source = Some(slot);
        }
        Ok(Self { input, steps, slots })
    }

    pub(super) fn output(&self) -> Size {
        self.steps.last().map_or(self.input, |step| step.output)
    }
}

pub(super) fn extent((width, height): Size) -> wgpu::Extent3d {
    wgpu::Extent3d { width, height, depth_or_array_layers: 1 }
}

pub(super) struct PreparedStage {
    pub(super) layout: StageLayout,
    backend: PreparedBackend,
}

enum PreparedBackend {
    Format4(PreparedFormat4),
    Classic(ClassicKind),
    Anime4k(Anime4kKind),
}

pub(super) struct Stage {
    pub(super) layout: StageLayout,
    pub(super) backend: Backend,
}

pub(super) enum Backend {
    Format4(Format4Filter),
    Classic(ClassicFilter),
    Anime4k(Anime4kFilter),
}

impl PreparedStage {
    pub(super) fn needs_dxc(&self) -> bool {
        matches!(self.backend, PreparedBackend::Format4(_))
    }

    pub(super) fn build(self, device: &wgpu::Device, dxc_path: &Path) -> Result<Stage, FilterError> {
        let backend = match self.backend {
            PreparedBackend::Format4(effect) => {
                Backend::Format4(effect.build(device, dxc_path, self.layout.format)?)
            }
            PreparedBackend::Classic(kind) => Backend::Classic(ClassicFilter::new(device, kind)),
            PreparedBackend::Anime4k(kind) => Backend::Anime4k(Anime4kFilter::new(device, kind)?),
        };
        Ok(Stage { layout: self.layout, backend })
    }
}

impl Backend {
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        input: Size,
        output: Size,
    ) -> Result<(), FilterError> {
        match self {
            Self::Format4(effect) => effect.prepare(device, input, output),
            Self::Classic(_) | Self::Anime4k(_) => Ok(()),
        }
    }

    pub(super) fn encode(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
    ) -> Result<(), FilterError> {
        match self {
            Self::Format4(effect) => effect.encode(device, encoder, input, output),
            Self::Classic(effect) => effect.encode(device, encoder, input, output),
            Self::Anime4k(effect) => effect.encode(device, encoder, input, output),
        }
    }
}

pub(super) fn prepared_stages(settings: &FilterSettings) -> Result<Vec<PreparedStage>, FilterError> {
    let mut stages = Vec::with_capacity(settings.stages.len());
    let empty = BTreeMap::new();
    let format4 = |source: &str, strength, params, mode: Option<OutputMode>, format| {
        let effect = PreparedFormat4::new(source, strength, params)?;
        Ok::<_, FilterError>(PreparedStage {
            layout: StageLayout { output: mode.unwrap_or(effect.output_mode()), format },
            backend: PreparedBackend::Format4(effect),
        })
    };
    let builtin = |source, strength, mode, format| format4(source, strength, &empty, mode, format);
    for stage in &settings.stages {
        match stage {
            FilterStage::Scale { scale } => stages.push(builtin(
                SCALE_SOURCE,
                FILTER_DEFAULT_STRENGTH,
                Some(OutputMode::Resize(Resize::Factor(*scale))),
                RGBA8,
            )?),
            FilterStage::Sharpen { strength } => {
                stages.push(builtin(SHARPEN_SOURCE, *strength, None, RGBA8)?);
            }
            FilterStage::LegacyAnime4k => {
                stages.push(builtin(
                    RESTORE_SOURCE,
                    FILTER_DEFAULT_STRENGTH,
                    None,
                    wgpu::TextureFormat::Rgba16Float,
                )?);
                stages.push(builtin(UPSCALE_SOURCE, FILTER_DEFAULT_STRENGTH, None, RGBA8)?);
            }
            FilterStage::Format4 { source, strength, parameters, disabled: false } => {
                stages.push(format4(source, *strength, parameters, None, RGBA8)?);
            }
            FilterStage::Format4 { disabled: true, .. } => {}
            FilterStage::Lanczos3 { scale } => stages.push(PreparedStage {
                layout: StageLayout {
                    output: OutputMode::Resize(Resize::Factor(*scale)),
                    format: RGBA8,
                },
                backend: PreparedBackend::Classic(ClassicKind::Lanczos3),
            }),
            FilterStage::Scale2x => stages.push(PreparedStage {
                layout: StageLayout {
                    output: OutputMode::Resize(Resize::Exact { x: 2, y: 2 }),
                    format: RGBA8,
                },
                backend: PreparedBackend::Classic(ClassicKind::Scale2x),
            }),
            FilterStage::Anime4kOriginalRestore => stages.push(PreparedStage {
                layout: StageLayout { output: OutputMode::SameExtent, format: RGBA8 },
                backend: PreparedBackend::Anime4k(Anime4kKind::RestoreSmall),
            }),
            FilterStage::Anime4kOriginalX2 => stages.push(PreparedStage {
                layout: StageLayout {
                    output: OutputMode::Resize(Resize::Exact { x: 2, y: 2 }),
                    format: RGBA8,
                },
                backend: PreparedBackend::Anime4k(Anime4kKind::RestoreSmallThenUpscaleSmall2x),
            }),
        }
    }
    Ok(stages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::PreparedFilter;

    #[test]
    fn filter_plan_reuses_nonaliasing_targets_and_checks_intermediate_sizes() {
        let same = StageLayout { output: OutputMode::SameExtent, format: RGBA8 };
        let double = StageLayout {
            output: OutputMode::Resize(Resize::Exact { x: 2, y: 2 }),
            format: RGBA8,
        };
        let plan = ChainPlan::new((721, 1280), 4096, [same, double, same, same].into_iter())
            .unwrap();
        assert_eq!(plan.output(), (1442, 2560));
        assert_eq!(plan.steps[1].input, (721, 1280));
        assert_eq!(plan.steps[2].input, (1442, 2560));
        assert_eq!(plan.steps.iter().map(|s| s.slot).collect::<Vec<_>>(), [0, 1, 2, 1]);
        assert!(plan.slots[1].present);
        assert!(ChainPlan::new((721, 1280), 2048, [same, double].into_iter()).is_err());
        assert!(ChainPlan::new((u32::MAX, 1), u32::MAX, [double].into_iter()).is_err());
    }

    #[test]
    fn format4_filter_dimensions_follow_each_source_including_the_legacy_pair() {
        let external = |source: &str| FilterStage::Format4 {
            source: source.into(),
            strength: FILTER_DEFAULT_STRENGTH,
            parameters: BTreeMap::new(),
            disabled: false,
        };
        for (stages, expected) in [
            (vec![FilterStage::Scale { scale: 1.5 }], (1082, 1920)),
            (vec![external(SCALE_SOURCE)], (721, 1280)),
            (vec![external(UPSCALE_SOURCE)], (1442, 2560)),
            (vec![FilterStage::LegacyAnime4k, FilterStage::Scale { scale: 1.5 }], (2163, 3840)),
            (vec![FilterStage::Scale { scale: 1.5 }, FilterStage::LegacyAnime4k], (2164, 3840)),
        ] {
            let settings = FilterSettings { stages };
            assert_eq!(PreparedFilter::new(&settings).unwrap().output_dimensions(721, 1280).unwrap(), expected);
        }
    }

    #[test]
    fn format4_filter_rejects_invalid_source_metadata_and_parameters() {
        let settings = |source: String, parameters| FilterSettings {
            stages: vec![FilterStage::Format4 {
                source,
                strength: FILTER_DEFAULT_STRENGTH,
                parameters,
                disabled: false,
            }],
        };
        assert!(PreparedFilter::new(&settings("not an effect".into(), BTreeMap::new())).is_err());
        let invalid_format = UPSCALE_SOURCE.replace("R16G16B16A16_FLOAT", "R32_FLOAT");
        assert!(PreparedFilter::new(&settings(invalid_format, BTreeMap::new())).is_err());
        assert!(PreparedFilter::new(&settings(
            SCALE_SOURCE.into(),
            BTreeMap::from([("unknown".into(), 1.0)]),
        )).is_err());
    }
}
