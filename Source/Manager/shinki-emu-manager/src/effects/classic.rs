use super::FilterError;

const SCALE2X: &str = include_str!("../../../../../Assets/Effects/Builtin/scale2x.wgsl");
const LANCZOS3: &str = include_str!("../../../../../Assets/Effects/Builtin/lanczos3.wgsl");
const BLOCK: u32 = 8;
const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SCRATCH: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ClassicKind {
    Scale2x,
    Lanczos3,
}

pub(super) struct ClassicFilter {
    direct: Pass,
    lanczos: Option<SeparablePasses>,
    bound: Option<BoundTextures>,
}

struct SeparablePasses {
    first: Pass,
    last: Pass,
    scratch: Option<wgpu::Texture>,
}

struct BoundTextures {
    input: wgpu::Texture,
    output: wgpu::Texture,
    first: Option<BoundPass>,
    last: BoundPass,
}

impl ClassicFilter {
    pub(super) fn new(device: &wgpu::Device, kind: ClassicKind) -> Self {
        let (label, source, entry) = match kind {
            ClassicKind::Scale2x => ("shinki.emu.effect.scale2x", SCALE2X, "main"),
            ClassicKind::Lanczos3 => ("shinki.emu.effect.lanczos3", LANCZOS3, "direct"),
        };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        Self {
            direct: Pass::new(device, &module, label, entry, COLOR),
            lanczos: (kind == ClassicKind::Lanczos3).then(|| SeparablePasses {
                first: Pass::new(device, &module, label, "first", SCRATCH),
                last: Pass::new(device, &module, label, "finish", COLOR),
                scratch: None,
            }),
            bound: None,
        }
    }

    pub(super) fn encode(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
    ) -> Result<(), FilterError> {
        if !self
            .bound
            .as_ref()
            .is_some_and(|bound| &bound.input == input && &bound.output == output)
        {
            let (first, last) = self.bind(device, input, output)?;
            self.bound = Some(BoundTextures {
                input: input.clone(),
                output: output.clone(),
                first,
                last,
            });
        }
        if let Some(bound) = &self.bound {
            if let Some(first) = &bound.first {
                first.encode(encoder);
            }
            bound.last.encode(encoder);
        }
        Ok(())
    }

    fn bind(
        &mut self,
        device: &wgpu::Device,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
    ) -> Result<(Option<BoundPass>, BoundPass), FilterError> {
        if input.format() != COLOR || output.format() != COLOR {
            return Err(FilterError::TextureFormat);
        }
        let sampled = wgpu::TextureUsages::TEXTURE_BINDING;
        let storage = wgpu::TextureUsages::STORAGE_BINDING;
        if input == output
            || !input.usage().contains(sampled)
            || !output.usage().contains(sampled | storage)
        {
            return Err(FilterError::TextureUsage);
        }
        if [input, output].into_iter().any(|texture| {
            texture.dimension() != wgpu::TextureDimension::D2
                || texture.depth_or_array_layers() != 1
                || texture.sample_count() != 1
        }) {
            return Err(FilterError::TextureDimensions);
        }
        let (iw, ih) = (input.width(), input.height());
        let (ow, oh) = (output.width(), output.height());
        let Some(lanczos) = &mut self.lanczos else {
            if iw.checked_mul(2) != Some(ow) || ih.checked_mul(2) != Some(oh) {
                return Err(FilterError::TextureDimensions);
            }
            return Ok((None, self.direct.bind(device, input, output, input.size())));
        };
        if iw == ow || ih == oh {
            lanczos.scratch = None;
            return Ok((None, self.direct.bind(device, input, output, output.size())));
        }

        // Change the more minifying (or less magnifying) axis first. Both
        // orders implement the same separable kernel; this picks less scratch.
        let (width, height) = if u64::from(ow) * u64::from(ih) <= u64::from(iw) * u64::from(oh) {
            (ow, ih)
        } else {
            (iw, oh)
        };
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        if lanczos.scratch.as_ref().is_some_and(|t| t.size() != size) {
            lanczos.scratch = None;
        }
        let scratch = lanczos.scratch.get_or_insert_with(|| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("shinki.emu.effect.lanczos3.scratch"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SCRATCH,
                usage: sampled | storage,
                view_formats: &[],
            })
        });
        Ok((
            Some(lanczos.first.bind(device, input, scratch, size)),
            lanczos.last.bind(device, scratch, output, output.size()),
        ))
    }
}

struct Pass {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

impl Pass {
    fn new(
        device: &wgpu::Device,
        module: &wgpu::ShaderModule,
        label: &'static str,
        entry: &'static str,
        format: wgpu::TextureFormat,
    ) -> Self {
        let entries = [
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            },
        ];
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { layout, pipeline }
    }

    fn bind(
        &self,
        device: &wgpu::Device,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
        extent: wgpu::Extent3d,
    ) -> BoundPass {
        let input = input.create_view(&wgpu::TextureViewDescriptor {
            usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
            mip_level_count: Some(1),
            ..Default::default()
        });
        let output = output.create_view(&wgpu::TextureViewDescriptor {
            usage: Some(wgpu::TextureUsages::STORAGE_BINDING),
            mip_level_count: Some(1),
            ..Default::default()
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&input),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&output),
                },
            ],
        });
        BoundPass {
            pipeline: self.pipeline.clone(),
            group,
            groups: (extent.width.div_ceil(BLOCK), extent.height.div_ceil(BLOCK)),
        }
    }
}

struct BoundPass {
    pipeline: wgpu::ComputePipeline,
    group: wgpu::BindGroup,
    groups: (u32, u32),
}

impl BoundPass {
    fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.dispatch_workgroups(self.groups.0, self.groups.1, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Pixel = [u8; 4];
    type Size = (u32, u32);

    #[test]
    #[ignore = "requires a native wgpu compute adapter"]
    fn classic_gpu_matches_references() {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("native compute adapter");
        let info = adapter.get_info();
        eprintln!("Classic filter proof: {} ({:?})", info.name, info.backend);
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("wgpu device without optional features");
        let mut scale = ClassicFilter::new(&device, ClassicKind::Scale2x);
        let mut lanczos = ClassicFilter::new(&device, ClassicKind::Lanczos3);

        let r = [201, 28, 13, 255];
        let g = [37, 191, 63, 219];
        let a = [201, 28, 13, 0]; // Same RGB as r, different alpha.
        let c = [120, 61, 199, 139];
        let art = [r, r, a, r, c, g, a, g, g];
        let scaled = scale2x_reference(&art, (3, 3));
        let actual = dispatch(&device, &queue, &art, (3, 3), &mut [(&mut scale, (6, 6))]);
        assert_eq!(actual, scaled, "Scale2x must match every RGBA8 byte");
        assert_eq!(
            [actual[14], actual[15], actual[20], actual[21]],
            [r, c, c, g]
        );
        eprintln!("Scale2x 3x3 -> 6x6: exact, including border and edge pixels");

        // Two stages sampling directly from one another in the same encoder.
        let chained = dispatch(
            &device,
            &queue,
            &art,
            (3, 3),
            &mut [(&mut scale, (6, 6)), (&mut lanczos, (9, 4))],
        );
        assert_close(
            "Scale2x -> Lanczos3",
            &chained,
            &lanczos_reference(&scaled, (6, 6), (9, 4)),
        );

        let image: Vec<Pixel> = (0..35u8)
            .map(|i| {
                [
                    i.wrapping_mul(73),
                    i.wrapping_mul(29),
                    i.wrapping_mul(131),
                    i % 6 * 51,
                ]
            })
            .collect();
        for size in [
            (11, 8),
            (11, 8),
            (4, 3),
            (3, 9),
            (9, 2),
            (7, 9),
            (13, 5),
            (7, 5),
            (1, 1),
        ] {
            let actual = dispatch(&device, &queue, &image, (7, 5), &mut [(&mut lanczos, size)]);
            assert_close(
                &format!("Lanczos3 7x5 -> {}x{}", size.0, size.1),
                &actual,
                &lanczos_reference(&image, (7, 5), size),
            );
        }
        let solid = [[37, 193, 71, 123]];
        let actual = dispatch(
            &device,
            &queue,
            &solid,
            (1, 1),
            &mut [(&mut lanczos, (4, 3))],
        );
        assert_eq!(
            actual,
            solid.repeat(12),
            "normalized clamped edges preserve constants"
        );
    }

    fn dispatch(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pixels: &[Pixel],
        size: Size,
        stages: &mut [(&mut ClassicFilter, Size)],
    ) -> Vec<Pixel> {
        let extent = |(width, height)| wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = |size, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: extent(size),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: COLOR,
                usage: usage | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let mut current = texture(size, wgpu::TextureUsages::COPY_DST);
        queue.write_texture(
            current.as_image_copy(),
            pixels.as_flattened(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.0 * 4),
                rows_per_image: None,
            },
            extent(size),
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let last_stage = stages.len() - 1;
        for (stage, (filter, size)) in stages.iter_mut().enumerate() {
            let mut usage = wgpu::TextureUsages::STORAGE_BINDING;
            if stage == last_stage {
                usage |= wgpu::TextureUsages::COPY_SRC;
            }
            let output = texture(*size, usage);
            filter
                .encode(device, &mut encoder, &current, &output)
                .unwrap();
            current = output;
        }

        let row_bytes = current.width() * 4;
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let stride = row_bytes.div_ceil(alignment) * alignment;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(stride) * u64::from(current.height()),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            current.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: None,
                },
            },
            current.size(),
        );
        let submission = queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap()
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .unwrap();
        receive.recv().unwrap().unwrap();
        slice
            .get_mapped_range()
            .unwrap()
            .chunks_exact(stride as usize)
            .flat_map(|row| row[..row_bytes as usize].chunks_exact(4))
            .map(|pixel| pixel.try_into().unwrap())
            .collect()
    }

    fn scale2x_reference(input: &[Pixel], (width, height): Size) -> Vec<Pixel> {
        let pixel = |x: i32, y: i32| {
            input[(y.clamp(0, height as i32 - 1) * width as i32 + x.clamp(0, width as i32 - 1))
                as usize]
        };
        let mut output = Vec::new();
        for y in 0..2 * height as i32 {
            for x in 0..2 * width as i32 {
                let (i, j) = (x / 2, y / 2);
                let (dx, dy) = (2 * (x % 2) - 1, 2 * (y % 2) - 1);
                let x_neighbor = pixel(i + dx, j);
                let y_neighbor = pixel(i, j + dy);
                output.push(
                    if x_neighbor == y_neighbor
                        && x_neighbor != pixel(i - dx, j)
                        && y_neighbor != pixel(i, j - dy)
                    {
                        x_neighbor
                    } else {
                        pixel(i, j)
                    },
                );
            }
        }
        output
    }

    fn lanczos_reference(input: &[Pixel], src: Size, dst: Size) -> Vec<Pixel> {
        if src == dst {
            return input.to_vec();
        }
        let weights = |i, src: u32, dst: u32| {
            if src == dst {
                return vec![(i, 1.0)];
            }
            let stride = f64::from(src) / f64::from(dst);
            let center = (f64::from(i) + 0.5) * stride - 0.5;
            let normalize = stride.max(1.0);
            let radius = (3.0 * normalize).ceil() as i32 + 1;
            let center_base = center.floor() as i32;
            let sinc = |x: f64| {
                let angle = std::f64::consts::PI * x;
                if angle == 0.0 {
                    1.0
                } else {
                    angle.sin() / angle
                }
            };
            let mut taps: Vec<_> = (center_base - radius..=center_base + radius)
                .filter_map(|p| {
                    let x = (f64::from(p) - center) / normalize;
                    (x.abs() < 3.0)
                        .then(|| (p.clamp(0, src as i32 - 1) as u32, sinc(x) * sinc(x / 3.0)))
                })
                .collect();
            let sum: f64 = taps.iter().map(|tap| tap.1).sum();
            for (_, weight) in &mut taps {
                *weight /= sum;
            }
            taps
        };
        let mut output = Vec::new();
        let unorm = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        // Evaluate the 2D product in f64 without intermediate GPU passes.
        for y in 0..dst.1 {
            let vertical = weights(y, src.1, dst.1);
            for x in 0..dst.0 {
                let horizontal = weights(x, src.0, dst.0);
                let mut premul = [0.0; 4];
                for &(j, wy) in &vertical {
                    for &(i, wx) in &horizontal {
                        let rgba = input[(j * src.0 + i) as usize].map(|v| f64::from(v) / 255.0);
                        let alpha = wx * wy * rgba[3];
                        premul[3] += alpha;
                        for ch in 0..3 {
                            premul[ch] += alpha * rgba[ch];
                        }
                    }
                }
                let a = unorm(premul[3]);
                output.push(if a == 0 {
                    [0; 4]
                } else {
                    [
                        unorm(premul[0] / premul[3]),
                        unorm(premul[1] / premul[3]),
                        unorm(premul[2] / premul[3]),
                        a,
                    ]
                });
            }
        }
        output
    }

    fn assert_close(label: &str, actual: &[Pixel], expected: &[Pixel]) {
        assert_eq!(actual.len(), expected.len());
        let mut max_error = 0;
        for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
            for ch in 0..4 {
                let error = actual[ch].abs_diff(expected[ch]);
                assert!(
                    error <= 1,
                    "{label}: pixel {index}, got {actual:?}, want {expected:?}"
                );
                max_error = max_error.max(error);
            }
        }
        eprintln!("{label}: max error {max_error} UNORM8 levels");
    }
}
