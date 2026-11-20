use std::{collections::BTreeMap, path::Path};

use wgpu::util::DeviceExt;

use super::{
    FilterError,
    chain::{OutputMode, Resize, Size, extent},
    compiler::shader_module,
    format4::{EffectSource, SamplerFilter, TextureDimension},
    generator::{generate_pass_shader, uniform_size},
    layout::{create_layout, validate_texture_metadata, write_f32, write_u32},
};

const INPUT: usize = 0;
const OUTPUT: usize = 1;
const FIRST_SCRATCH: usize = 2;

pub(super) struct PreparedFormat4 {
    effect: EffectSource,
    strength: f32,
    parameters: Vec<f32>,
    output: OutputMode,
}

impl PreparedFormat4 {
    pub(super) fn new(
        source: &str,
        strength: f32,
        parameters: &BTreeMap<String, f32>,
    ) -> Result<Self, FilterError> {
        let effect = EffectSource::parse(source).map_err(|error| FilterError::Format4(error.to_string()))?;
        let output = effect.textures.iter().find(|texture| texture.name == "OUTPUT").unwrap();
        let factor = |dimension| match dimension {
            Some(TextureDimension::InputTimes(factor)) => factor,
            _ => 1,
        };
        let (x, y) = (factor(output.width), factor(output.height));
        validate_texture_metadata(&effect, x == 2 || y == 2).map_err(FilterError::Format4)?;
        if effect.textures.iter().any(|texture| texture.source.is_some()) {
            return Err(FilterError::Format4("external texture sources are unsupported".into()));
        }
        if effect.passes.last().is_none_or(|pass| pass.output != "OUTPUT") {
            return Err(FilterError::Format4("last pass must write OUTPUT".into()));
        }
        for (name, value) in parameters {
            let Some(parameter) = effect.parameters.iter().find(|parameter| parameter.name == *name) else {
                return Err(FilterError::Format4(format!("SHINKI_EMU_EFFECT_PARAMETER_UNKNOWN_{name}")));
            };
            if !(parameter.min..=parameter.max).contains(value) {
                return Err(FilterError::Format4(format!("SHINKI_EMU_EFFECT_PARAMETER_OUT_OF_RANGE_{name}")));
            }
        }
        let parameters = effect.parameters.iter().map(|parameter| {
            parameters.get(&parameter.name).copied().unwrap_or(parameter.default)
        }).collect();
        let output = if x == 1 && y == 1 {
            OutputMode::SameExtent
        } else {
            OutputMode::Resize(Resize::Exact { x, y })
        };
        Ok(Self { effect, strength, parameters, output })
    }

    pub(super) fn output_mode(&self) -> OutputMode {
        self.output
    }

    pub(super) fn build(
        self,
        device: &wgpu::Device,
        dxc_path: &Path,
        format: wgpu::TextureFormat,
    ) -> Result<Format4Filter, FilterError> {
        let mut indices = BTreeMap::from([("INPUT", INPUT), ("OUTPUT", OUTPUT)]);
        let mut scratch_count = 0;
        let mut passes = Vec::with_capacity(self.effect.passes.len());
        for pass in &self.effect.passes {
            let output = *indices.entry(&pass.output).or_insert_with(|| {
                let index = FIRST_SCRATCH + scratch_count;
                scratch_count += 1;
                index
            });
            let rgba8 = output == OUTPUT && format == wgpu::TextureFormat::Rgba8Unorm;
            let source = generate_pass_shader(&self.effect, pass, rgba8).map_err(FilterError::Compile)?;
            let module = shader_module(device, dxc_path, &format!("shinki_emu_effect_pass_{}", pass.number), &source)
                .map_err(FilterError::Compile)?;
            let layout = create_layout(device, pass, rgba8, &self.effect);
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shinki.emu.effect.pipeline-layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("shinki.emu.effect.pipeline"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
            passes.push(GpuPass {
                pipeline,
                layout,
                inputs: pass.inputs.iter().map(|name| indices[name.as_str()]).collect(),
                output,
                block_size: pass.block_size,
            });
        }
        let samplers = self.effect.samplers.iter().map(|sampler| {
            let filter = match sampler.filter {
                SamplerFilter::Linear => wgpu::FilterMode::Linear,
                SamplerFilter::Point => wgpu::FilterMode::Nearest,
            };
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("shinki.emu.effect.sampler"),
                mag_filter: filter,
                min_filter: filter,
                ..Default::default()
            })
        }).collect();
        Ok(Format4Filter {
            uniform_bytes: uniform_size(&self.effect),
            strength: self.strength,
            parameters: self.parameters,
            passes,
            samplers,
            scratch_count,
            geometry: None,
        })
    }
}

pub(super) struct Format4Filter {
    uniform_bytes: usize,
    strength: f32,
    parameters: Vec<f32>,
    passes: Vec<GpuPass>,
    samplers: Vec<wgpu::Sampler>,
    scratch_count: usize,
    geometry: Option<GpuGeometry>,
}

struct GpuPass {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    inputs: Vec<usize>,
    output: usize,
    block_size: u32,
}

struct GpuGeometry {
    input: Size,
    output: Size,
    scratch: Vec<wgpu::TextureView>,
    uniforms: Vec<(wgpu::Buffer, Size)>,
    bindings: Option<Bindings>,
}

struct Bindings {
    input: wgpu::Texture,
    output: wgpu::Texture,
    groups: Vec<wgpu::BindGroup>,
}

impl Format4Filter {
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        input: Size,
        output: Size,
    ) -> Result<(), FilterError> {
        if self.geometry.as_ref().is_some_and(|geometry| geometry.input == input && geometry.output == output) {
            return Ok(());
        }
        let limits = device.limits();
        let pass_sizes = self.passes.iter().map(|pass| {
            let size = if pass.output == OUTPUT { output } else { input };
            let dispatch = (size.0.div_ceil(pass.block_size), size.1.div_ceil(pass.block_size));
            if dispatch.0 > limits.max_compute_workgroups_per_dimension
                || dispatch.1 > limits.max_compute_workgroups_per_dimension
            {
                return Err(FilterError::TextureDimensions);
            }
            Ok((size, dispatch))
        }).collect::<Result<Vec<_>, _>>()?;
        let scratch = (0..self.scratch_count).map(|_| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("shinki.emu.effect.intermediate"),
                size: extent(input),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
                view_formats: &[],
            }).create_view(&Default::default())
        }).collect();
        let mut uniform = vec![0_u8; self.uniform_bytes];
        write_u32(&mut uniform, 0, input.0);
        write_u32(&mut uniform, 4, input.1);
        write_f32(&mut uniform, 16, 1.0 / input.0 as f32);
        write_f32(&mut uniform, 20, 1.0 / input.1 as f32);
        write_f32(&mut uniform, 40, self.strength);
        for (index, &value) in self.parameters.iter().enumerate() {
            write_f32(&mut uniform, 48 + index * 4, value);
        }
        let uniforms = pass_sizes.into_iter().map(|((width, height), dispatch)| {
            write_u32(&mut uniform, 8, width);
            write_u32(&mut uniform, 12, height);
            write_f32(&mut uniform, 24, 1.0 / width as f32);
            write_f32(&mut uniform, 28, 1.0 / height as f32);
            write_f32(&mut uniform, 32, width as f32 / input.0 as f32);
            write_f32(&mut uniform, 36, height as f32 / input.1 as f32);
            (device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("shinki.emu.effect.params"),
                contents: &uniform,
                usage: wgpu::BufferUsages::UNIFORM,
            }), dispatch)
        }).collect();
        self.geometry = Some(GpuGeometry { input, output, scratch, uniforms, bindings: None });
        Ok(())
    }

    pub(super) fn encode(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
    ) -> Result<(), FilterError> {
        let geometry = self.geometry.as_mut().ok_or(FilterError::TextureDimensions)?;
        if geometry.input != (input.width(), input.height()) || geometry.output != (output.width(), output.height()) {
            return Err(FilterError::TextureDimensions);
        }
        if geometry.bindings.as_ref().is_none_or(|bindings| &bindings.input != input || &bindings.output != output) {
            let input_view = input.create_view(&Default::default());
            let output_view = output.create_view(&Default::default());
            let view = |index| match index {
                0 => &input_view,
                1 => &output_view,
                index => &geometry.scratch[index - 2],
            };
            let groups = self.passes.iter().zip(&geometry.uniforms).map(|(pass, (uniform, _))| {
                let mut entries = Vec::with_capacity(2 + pass.inputs.len() + self.samplers.len());
                entries.push(wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() });
                for (index, &input) in pass.inputs.iter().enumerate() {
                    entries.push(wgpu::BindGroupEntry {
                        binding: 1 + index as u32,
                        resource: wgpu::BindingResource::TextureView(view(input)),
                    });
                }
                let output_binding = 1 + pass.inputs.len() as u32;
                entries.push(wgpu::BindGroupEntry {
                    binding: output_binding,
                    resource: wgpu::BindingResource::TextureView(view(pass.output)),
                });
                for (index, sampler) in self.samplers.iter().enumerate() {
                    entries.push(wgpu::BindGroupEntry {
                        binding: output_binding + 1 + index as u32,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    });
                }
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("shinki.emu.effect.bind-group"),
                    layout: &pass.layout,
                    entries: &entries,
                })
            }).collect();
            geometry.bindings = Some(Bindings { input: input.clone(), output: output.clone(), groups });
        }
        let groups = &geometry.bindings.as_ref().unwrap().groups;
        for ((pass, bind), (_, (x, y))) in self.passes.iter().zip(groups).zip(&geometry.uniforms) {
            let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("shinki.emu.effect.pass"),
                timestamp_writes: None,
            });
            compute.set_pipeline(&pass.pipeline);
            compute.set_bind_group(0, bind, &[]);
            compute.dispatch_workgroups(*x, *y, 1);
        }
        Ok(())
    }
}
