use super::FilterError;

const SHADER: &str = concat!(
    include_str!("../../../../../Assets/Effects/Anime4KOriginal/runtime.wgsl"),
    include_str!("../../../../../Assets/Effects/Anime4KOriginal/cnn.wgsl"),
);
const WORKGROUP: u32 = 8;
const EXTERNAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Anime4kKind {
    RestoreSmall,
    UpscaleSmall2x,
    RestoreSmallThenUpscaleSmall2x,
}

pub(super) struct Anime4kFilter {
    kind: Anime4kKind,
    passes: Vec<Pass>,
    max_workgroups: u32,
    scratch: Option<Scratch>,
    bindings: Option<Bindings>,
}

#[derive(Clone, Copy)]
enum View {
    Input,
    Output,
    A,
    B,
    Restored,
}

#[derive(Clone, Copy)]
struct PassSpec {
    entry: &'static str,
    source: View,
    base: Option<View>,
    target: View,
}

impl PassSpec {
    const fn convolution(entry: &'static str, source: View, target: View) -> Self {
        Self {
            entry,
            source,
            base: None,
            target,
        }
    }

    const fn reconstruct(entry: &'static str, source: View, base: View, target: View) -> Self {
        Self {
            entry,
            source,
            base: Some(base),
            target,
        }
    }
}

struct Pass {
    spec: PassSpec,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

struct Scratch {
    size: wgpu::Extent3d,
    features: [wgpu::TextureView; 2],
    restored: Option<wgpu::TextureView>,
}

struct Bindings {
    input: wgpu::Texture,
    output: wgpu::Texture,
    groups: Vec<wgpu::BindGroup>,
}

impl Anime4kFilter {
    pub(super) fn new(device: &wgpu::Device, kind: Anime4kKind) -> Result<Self, FilterError> {
        use View::{A, B, Input, Output, Restored};

        let restore = kind != Anime4kKind::UpscaleSmall2x;
        let upscale = kind != Anime4kKind::RestoreSmall;
        let combined = restore && upscale;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shinki.emu.anime4k-original"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let mut passes = Vec::with_capacity(usize::from(restore) * 4 + usize::from(upscale) * 5);
        let mut add_passes = |specs: &[PassSpec]| {
            passes.extend(specs.iter().map(|&spec| {
                let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(spec.entry),
                    layout: None,
                    module: &shader,
                    entry_point: Some(spec.entry),
                    compilation_options: Default::default(),
                    cache: None,
                });
                let layout = pipeline.get_bind_group_layout(0);
                Pass {
                    spec,
                    pipeline,
                    layout,
                }
            }));
        };
        if restore {
            let (entry, target) = if combined {
                ("restore_to_float", Restored)
            } else {
                ("restore_to_unorm", Output)
            };
            add_passes(&[
                PassSpec::convolution("restore_0_features", Input, A),
                PassSpec::convolution("restore_1_features", A, B),
                PassSpec::convolution("restore_2_features", B, A),
                PassSpec::reconstruct(entry, A, Input, target),
            ]);
        }
        if upscale {
            let base = if combined { Restored } else { Input };
            add_passes(&[
                PassSpec::convolution("upscale_0_features", base, A),
                PassSpec::convolution("upscale_1_features", A, B),
                PassSpec::convolution("upscale_2_features", B, A),
                PassSpec::convolution("upscale_3_features", A, B),
                PassSpec::reconstruct("depth_to_space", B, base, Output),
            ]);
        }
        if let Some(error) = pollster::block_on(scope.pop()) {
            return Err(FilterError::GpuValidation(error.to_string()));
        }
        Ok(Self {
            kind,
            passes,
            max_workgroups: device.limits().max_compute_workgroups_per_dimension,
            scratch: None,
            bindings: None,
        })
    }

    pub(super) fn encode(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
    ) -> Result<(), FilterError> {
        let size = input.size();
        let scale = if self.kind == Anime4kKind::RestoreSmall {
            1
        } else {
            2
        };
        if input.format() != EXTERNAL_FORMAT || output.format() != EXTERNAL_FORMAT {
            return Err(FilterError::TextureFormat);
        }
        if input == output
            || !input.usage().contains(wgpu::TextureUsages::TEXTURE_BINDING)
            || !output
                .usage()
                .contains(wgpu::TextureUsages::STORAGE_BINDING)
        {
            return Err(FilterError::TextureUsage);
        }
        if [input, output].into_iter().any(|texture| {
            texture.dimension() != wgpu::TextureDimension::D2
                || texture.depth_or_array_layers() != 1
                || texture.sample_count() != 1
                || texture.mip_level_count() != 1
        }) || size.width.checked_mul(scale) != Some(output.width())
            || size.height.checked_mul(scale) != Some(output.height())
        {
            return Err(FilterError::TextureDimensions);
        }
        let x = size.width.div_ceil(WORKGROUP);
        let y = size.height.div_ceil(WORKGROUP);
        if x.max(y) > self.max_workgroups {
            return Err(FilterError::TextureDimensions);
        }

        if !self
            .bindings
            .as_ref()
            .is_some_and(|bindings| &bindings.input == input && &bindings.output == output)
        {
            self.bind(device, input, output);
        }
        let bindings = self.bindings.as_ref().expect("anime4k frame bindings");
        for (pass, group) in self.passes.iter().zip(&bindings.groups) {
            // A pass ends before a feature texture is rebound for the opposite usage.
            let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(pass.spec.entry),
                timestamp_writes: None,
            });
            compute.set_pipeline(&pass.pipeline);
            compute.set_bind_group(0, group, &[]);
            compute.dispatch_workgroups(x, y, 1);
        }
        Ok(())
    }

    fn bind(&mut self, device: &wgpu::Device, input: &wgpu::Texture, output: &wgpu::Texture) {
        let size = input.size();
        if !self
            .scratch
            .as_ref()
            .is_some_and(|scratch| scratch.size == size)
        {
            self.scratch = Some(Scratch::new(
                device,
                size,
                self.kind == Anime4kKind::RestoreSmallThenUpscaleSmall2x,
            ));
        }
        let scratch = self.scratch.as_ref().expect("anime4k scratch textures");
        let input_view = input.create_view(&wgpu::TextureViewDescriptor::default());
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let view = |target| match target {
            View::Input => &input_view,
            View::Output => &output_view,
            View::A => &scratch.features[0],
            View::B => &scratch.features[1],
            View::Restored => scratch.restored.as_ref().expect("anime4k restored frame"),
        };
        let groups = self
            .passes
            .iter()
            .map(|pass| {
                let source = view(pass.spec.source);
                let mut entries = [
                    texture_entry(0, source),
                    texture_entry(2, view(pass.spec.target)),
                    texture_entry(1, source),
                ];
                let count = if let Some(base) = pass.spec.base {
                    entries[2] = texture_entry(1, view(base));
                    3
                } else {
                    2
                };
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(pass.spec.entry),
                    layout: &pass.layout,
                    entries: &entries[..count],
                })
            })
            .collect();
        self.bindings = Some(Bindings {
            input: input.clone(),
            output: output.clone(),
            groups,
        });
    }
}

impl Scratch {
    fn new(device: &wgpu::Device, size: wgpu::Extent3d, combined: bool) -> Self {
        let make = |label| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba16Float,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::STORAGE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        Self {
            size,
            features: [
                make("shinki.emu.anime4k.features-a"),
                make("shinki.emu.anime4k.features-b"),
            ],
            restored: combined.then(|| make("shinki.emu.anime4k.restored")),
        }
    }
}

fn texture_entry(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = include_str!("../../../../../Assets/Effects/Anime4KOriginal/parity.txt");

    fn surface(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        usage: wgpu::TextureUsages,
    ) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: EXTERNAL_FORMAT,
            usage,
            view_formats: &[],
        })
    }

    fn readback(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mut encoder: wgpu::CommandEncoder,
        output: &wgpu::Texture,
    ) -> Vec<u8> {
        let row_bytes = output.width() * 4;
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let aligned = row_bytes.div_ceil(alignment) * alignment;
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(aligned) * u64::from(output.height()),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(aligned),
                    rows_per_image: Some(output.height()),
                },
            },
            output.size(),
        );
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        read.slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        read.slice(..)
            .get_mapped_range()
            .unwrap()
            .chunks_exact(aligned as usize)
            .flat_map(|row| row[..row_bytes as usize].iter().copied())
            .collect()
    }

    #[test]
    #[ignore = "requires a native wgpu compute adapter"]
    fn anime4k_original_matches_upstream_glsl() {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .expect("headless wgpu adapter");
        eprintln!("Anime4K parity adapter: {:?}", adapter.get_info());
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        for (index, kind) in [
            Anime4kKind::RestoreSmall,
            Anime4kKind::UpscaleSmall2x,
            Anime4kKind::RestoreSmallThenUpscaleSmall2x,
        ]
        .into_iter()
        .enumerate()
        {
            let mut filter = Anime4kFilter::new(&device, kind).unwrap();
            let scale = if index == 0 { 1 } else { 2 };
            let mut surfaces: Option<(wgpu::Texture, wgpu::Texture)> = None;
            for case in FIXTURES.split("\n\n") {
                let mut fields = case.split_whitespace();
                let label = fields.next().unwrap();
                let width: u32 = fields.next().unwrap().parse().unwrap();
                let height: u32 = fields.next().unwrap().parse().unwrap();
                let input_bytes = hex::decode(fields.next().unwrap()).unwrap();
                let oracle = hex::decode(fields.nth(index).unwrap()).unwrap();
                if !surfaces
                    .as_ref()
                    .is_some_and(|(input, _)| (input.width(), input.height()) == (width, height))
                {
                    let input = surface(
                        &device,
                        width,
                        height,
                        wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                    );
                    let output = surface(
                        &device,
                        width * scale,
                        height * scale,
                        wgpu::TextureUsages::COPY_SRC
                            | wgpu::TextureUsages::STORAGE_BINDING
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                    );
                    surfaces = Some((input, output));
                }
                let (input, output) = surfaces.as_ref().unwrap();
                queue.write_texture(
                    input.as_image_copy(),
                    &input_bytes,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width * 4),
                        rows_per_image: Some(height),
                    },
                    input.size(),
                );
                let mut encoder = device.create_command_encoder(&Default::default());
                if label == "odd-first" {
                    let wrong_size =
                        surface(&device, width * scale + 1, height * scale, output.usage());
                    assert!(matches!(
                        filter.encode(&device, &mut encoder, input, &wrong_size),
                        Err(FilterError::TextureDimensions)
                    ));
                }
                filter.encode(&device, &mut encoder, input, output).unwrap();
                let actual = readback(&device, &queue, encoder, output);
                assert_eq!(actual.len(), oracle.len());
                let (byte, (&a, &b)) = actual
                    .iter()
                    .zip(&oracle)
                    .enumerate()
                    .max_by_key(|(_, (a, b))| a.abs_diff(**b))
                    .unwrap();
                let delta = a.abs_diff(b);
                eprintln!("{kind:?} {label}: {width}x{height} x{scale}, max byte delta {delta}");
                // Covers FP16 rounding, shader contraction and RGBA8 conversion.
                assert!(
                    delta <= 1,
                    "{kind:?} {label} byte {byte}: WGSL {a}, GLSL {b}"
                );
                if scale == 1 {
                    assert!(
                        actual
                            .chunks_exact(4)
                            .zip(input_bytes.chunks_exact(4))
                            .all(|(a, b)| a[3] == b[3])
                    );
                }
            }
        }
    }
}
