use crate::{crx::Crx, text::Text};
use image::{Rgba, RgbaImage};
#[cfg(not(target_env = "ohos"))]
use roukaku_media_core::BlendMode;
use roukaku_media_core::{RectI, SceneCommand};
use shinki_emu_sdk::StageCanvas;

pub(crate) enum Canvas {
    Raster(Raster),
    #[cfg(not(target_env = "ohos"))]
    Scene(Scene),
}

#[cfg(not(target_env = "ohos"))]
pub(crate) struct Scene {
    extent: (u32, u32),
    commands: Vec<SceneCommand>,
    sources: Vec<Option<RectI>>,
    text: Vec<(usize, Text)>,
}

impl Canvas {
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Self::from_image(RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255])), None)
    }

    pub(crate) fn from_image(image: RgbaImage, presentation: Option<StageCanvas>) -> Self {
        let presentation = presentation.filter(|mapping| mapping.raster() != mapping.logical());
        Self::Raster(Raster {
            image,
            presentation,
            physical: false,
        })
    }

    pub(crate) fn for_output(mapping: StageCanvas, _gpu: bool) -> Self {
        let logical = mapping.logical();
        #[cfg(not(target_env = "ohos"))]
        if _gpu {
            return Self::Scene(Scene {
                extent: (logical.width, logical.height),
                commands: Vec::new(),
                sources: Vec::new(),
                text: Vec::new(),
            });
        }
        Self::from_image(
            RgbaImage::from_pixel(logical.width, logical.height, Rgba([0, 0, 0, 255])),
            Some(mapping),
        )
    }

    pub(crate) fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Raster(raster) => match raster.presentation {
                Some(mapping) => (mapping.logical().width, mapping.logical().height),
                None => raster.image.dimensions(),
            },
            #[cfg(not(target_env = "ohos"))]
            Self::Scene(scene) => scene.extent,
        }
    }

    pub(crate) fn width(&self) -> u32 {
        self.dimensions().0
    }

    pub(crate) fn raster_mut(&mut self) -> &mut RgbaImage {
        match self {
            Self::Raster(raster) => {
                assert!(!raster.physical, "native pixel effects precede text");
                &mut raster.image
            }
            #[cfg(not(target_env = "ohos"))]
            Self::Scene(_) => unreachable!("CPU pixel effect must select the raster canvas"),
        }
    }

    pub(crate) fn text_raster(&mut self) -> (&mut RgbaImage, Option<StageCanvas>) {
        match self {
            Self::Raster(raster) => {
                raster.promote();
                (&mut raster.image, raster.presentation)
            }
            #[cfg(not(target_env = "ohos"))]
            Self::Scene(_) => unreachable!("GPU text must be queued"),
        }
    }

    pub(crate) fn physical_raster(&mut self) -> Option<(&mut RgbaImage, StageCanvas)> {
        match self {
            Self::Raster(raster) if raster.physical => {
                Some((&mut raster.image, raster.presentation.unwrap()))
            }
            _ => None,
        }
    }

    pub(crate) fn for_each_raster_pixel(&mut self, mut edit: impl FnMut(u32, u32, &mut Rgba<u8>)) {
        if let Some((image, mapping)) = self.physical_raster() {
            let viewport = mapping.viewport();
            let xs = RasterAxis::x(mapping);
            let ys = RasterAxis::y(mapping);
            let columns: Vec<_> = (viewport.x..viewport.x + viewport.width)
                .map(|x| xs.sample(x))
                .collect();
            for y in viewport.y..viewport.y + viewport.height {
                let logical_y = ys.sample(y);
                for (x, &logical_x) in (viewport.x..).zip(&columns) {
                    edit(logical_x, logical_y, image.get_pixel_mut(x, y));
                }
            }
        } else {
            for (x, y, pixel) in self.raster_mut().enumerate_pixels_mut() {
                edit(x, y, pixel);
            }
        }
    }

    #[allow(unused_variables)]
    pub(crate) fn queue_sprite(
        &mut self,
        asset: &Crx,
        source: RectI,
        destination: RectI,
        opacity: f32,
    ) -> bool {
        #[cfg(not(target_env = "ohos"))]
        if let Self::Scene(scene) = self {
            if destination.width != 0 && destination.height != 0 && opacity > 0.0 {
                scene.commands.push(SceneCommand::Texture {
                    id: format!("sprite:{}", scene.commands.len()),
                    frame: asset.texture(),
                    destination,
                    opacity,
                    blend: BlendMode::Alpha,
                });
                scene.sources.push(Some(source));
            }
            return true;
        }
        false
    }

    #[allow(unused_variables)]
    pub(crate) fn queue(&mut self, command: SceneCommand) -> bool {
        #[cfg(not(target_env = "ohos"))]
        if let Self::Scene(scene) = self {
            scene.commands.push(command);
            scene.sources.push(None);
            return true;
        }
        false
    }

    #[allow(unused_variables)]
    pub(crate) fn queue_text(&mut self, text: &Text) -> bool {
        #[cfg(not(target_env = "ohos"))]
        if let Self::Scene(scene) = self {
            scene.text.push((scene.commands.len(), text.clone()));
            return true;
        }
        false
    }
}

pub(crate) struct Raster {
    image: RgbaImage,
    presentation: Option<StageCanvas>,
    physical: bool,
}

impl Raster {
    fn promote(&mut self) {
        // Native pixel effects and sprite sampling run unchanged until the
        // first text draw. Subsequent draws use the same logical pixel grid.
        let Some(mapping) = self.presentation.filter(|_| !self.physical) else {
            return;
        };
        let raster = mapping.raster();
        let mut output = RgbaImage::from_pixel(raster.width, raster.height, Rgba([0, 0, 0, 255]));
        let viewport = mapping.viewport();
        let xs = RasterAxis::x(mapping);
        let ys = RasterAxis::y(mapping);
        let columns: Vec<_> = xs
            .span(0, i64::from(mapping.logical().width))
            .map(|x| xs.sample(x) as usize)
            .collect();
        let src_stride = self.image.width() as usize * 4;
        let dst_stride = raster.width as usize * 4;
        let left = viewport.x as usize * 4;
        let span = columns.len() * 4;
        let mut last_source = None;
        for y in ys.span(0, i64::from(mapping.logical().height)) {
            let sy = ys.sample(y) as usize;
            let dst = y as usize * dst_stride + left;
            if last_source == Some(sy) {
                let previous = dst - dst_stride;
                output.as_mut().copy_within(previous..previous + span, dst);
            } else {
                let src = &self.image.as_raw()[sy * src_stride..(sy + 1) * src_stride];
                for (pixel, &x) in output.as_mut()[dst..dst + span]
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(&columns)
                {
                    pixel.copy_from_slice(&src[x * 4..x * 4 + 4]);
                }
                last_source = Some(sy);
            }
        }
        self.image = output;
        self.physical = true;
    }

    pub(crate) fn into_image(mut self) -> RgbaImage {
        self.promote();
        self.image
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RasterAxis {
    offset: u32,
    length: u32,
    scale: f64,
}

impl RasterAxis {
    pub(crate) fn x(mapping: StageCanvas) -> Self {
        let viewport = mapping.viewport();
        Self {
            offset: viewport.x,
            length: viewport.width,
            scale: f64::from(mapping.scale()),
        }
    }

    pub(crate) fn y(mapping: StageCanvas) -> Self {
        let viewport = mapping.viewport();
        Self {
            offset: viewport.y,
            length: viewport.height,
            scale: f64::from(mapping.scale()),
        }
    }

    pub(crate) fn span(self, start: i64, end: i64) -> std::ops::Range<u32> {
        // A raster sample belongs to the logical pixel containing its center.
        let edge = |logical| {
            self.offset + ((logical as f64 * self.scale - 0.5).ceil() as u32).min(self.length)
        };
        edge(start)..edge(end)
    }

    pub(crate) fn sample(self, raster: u32) -> u32 {
        ((f64::from(raster - self.offset) + 0.5) / self.scale) as u32
    }

    pub(crate) fn center(self, logical: u32) -> u32 {
        self.offset + (((f64::from(logical) + 0.5) * self.scale) as u32).min(self.length - 1)
    }
}

#[cfg(not(target_env = "ohos"))]
pub(crate) use hardware::GpuCompositor;

#[cfg(not(target_env = "ohos"))]
mod hardware {
    use super::*;
    use crate::{Error, Result};
    use encoding_rs::Encoding;
    use roukaku_media_core::{OwnedPixelBuffer, SceneCompositing2D, Transform2D};
    use roukaku_platform::SceneFrame;
    use roukaku_text::PackagedFont;
    use shinki_emu_sdk::{SceneTextureResidency, TextScene};
    use shinki_emu_sdk::{WgpuOffscreenRenderer, native_wgpu_instance};
    use std::{
        sync::{
            Arc,
            mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
        },
        thread::{self, JoinHandle},
    };

    pub(crate) struct GpuCompositor {
        worker: Option<JoinHandle<()>>,
        commands: Option<SyncSender<Scene>>,
        frames: Receiver<Result<OwnedPixelBuffer>>,
        pending: bool,
        publish_pending: bool,
    }

    impl GpuCompositor {
        pub(crate) fn new(
            encoding: &'static Encoding,
            font: Option<Arc<PackagedFont>>,
            mapping: StageCanvas,
        ) -> Result<Self> {
            let (commands, queue) = sync_channel::<Scene>(1);
            let (completed, frames) = sync_channel(1);
            let (initialize, ready) = sync_channel(1);
            let worker = thread::Builder::new()
                .name("adv32-gpu".into())
                .spawn(move || {
                    let mut renderer = match GpuRenderer::new(encoding, font, mapping) {
                        Ok(renderer) => renderer,
                        Err(error) => {
                            let _ = initialize.send(Err(error));
                            return;
                        }
                    };
                    if initialize.send(Ok(())).is_err() {
                        return;
                    }
                    while let Ok(scene) = queue.recv() {
                        let frame = renderer.render(scene);
                        let failed = frame.is_err();
                        if completed.send(frame).is_err() || failed {
                            break;
                        }
                    }
                })
                .map_err(render_error)?;
            if let Err(error) = ready.recv().map_err(render_error).and_then(|ready| ready) {
                let _ = worker.join();
                return Err(error);
            }
            Ok(Self {
                worker: Some(worker),
                commands: Some(commands),
                frames,
                pending: false,
                publish_pending: false,
            })
        }

        pub(crate) fn is_busy(&self) -> bool {
            self.pending
        }

        pub(crate) fn discard_pending(&mut self) {
            self.publish_pending = false;
        }

        pub(crate) fn poll(&mut self) -> Result<Option<OwnedPixelBuffer>> {
            if !self.pending {
                return Ok(None);
            }
            match self.frames.try_recv() {
                Ok(frame) => {
                    self.pending = false;
                    Ok(self.publish_pending.then_some(frame?))
                }
                Err(TryRecvError::Empty) => Ok(None),
                Err(error) => Err(render_error(error)),
            }
        }

        pub(crate) fn submit(&mut self, scene: Scene) -> Result<()> {
            debug_assert!(!self.pending);
            self.commands
                .as_ref()
                .expect("GPU compositor owns its queue")
                .send(scene)
                .map_err(|_| Error::Render("ADV32 GPU compositor stopped".into()))?;
            self.pending = true;
            self.publish_pending = true;
            Ok(())
        }

        pub(crate) fn render_now(&mut self, scene: Scene) -> Result<OwnedPixelBuffer> {
            if self.pending {
                self.frames.recv().map_err(render_error)??;
                self.pending = false;
            }
            self.submit(scene)?;
            let frame = self.frames.recv().map_err(render_error)?;
            self.pending = false;
            frame
        }
    }

    impl Drop for GpuCompositor {
        fn drop(&mut self) {
            self.commands.take();
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    struct GpuRenderer {
        renderer: WgpuOffscreenRenderer,
        residency: SceneTextureResidency,
        text: TextScene,
        positioned: crate::text::NativeTextLayouts,
        sequence: u64,
        encoding: &'static Encoding,
        font: Option<Arc<PackagedFont>>,
        mapping: StageCanvas,
    }

    impl GpuRenderer {
        pub(crate) fn new(
            encoding: &'static Encoding,
            font: Option<Arc<PackagedFont>>,
            mapping: StageCanvas,
        ) -> Result<Self> {
            let instance = native_wgpu_instance().map_err(render_error)?;
            let renderer = pollster::block_on(WgpuOffscreenRenderer::new(&instance))
                .map_err(render_error)?
                .with_default_compositing(SceneCompositing2D::EncodedSrgb);
            let raster = mapping.raster();
            renderer
                .validate_output_extent(raster.width, raster.height)
                .map_err(render_error)?;
            tracing::info!(
                event = "shinki.emu.adv32.gpu.adapter",
                backend = %renderer.identity().backend,
                device_type = %renderer.identity().device_type,
                "Selected hardware compositor adapter"
            );
            let text = crate::text::new_scene(encoding, font.as_deref())?;
            Ok(Self {
                renderer,
                residency: SceneTextureResidency::default(),
                text,
                positioned: crate::text::NativeTextLayouts::new(),
                sequence: 0,
                encoding,
                font,
                mapping,
            })
        }

        fn render(&mut self, mut scene: Scene) -> Result<OwnedPixelBuffer> {
            let (width, height) = scene.extent;
            let encoding = self.encoding;
            let mapping = self.mapping;
            let raster = mapping.raster();
            let scaled = raster != mapping.logical();
            let density = mapping.scale();
            let layouts = scene
                .text
                .iter()
                .enumerate()
                .map(|(index, (_, text))| {
                    text.request(
                        format!("adv32:{index}"),
                        width,
                        encoding,
                        self.font.as_deref(),
                    )
                })
                .collect::<Vec<_>>();
            let text = self
                .text
                .frame_parts_with_layout(&layouts, density, |index, layout| {
                    self.positioned
                        .position(&scene.text[index].1, layout, encoding)
                })
                .map_err(render_error)?;
            let mut commands = text.lifecycle;
            if scaled {
                commands.push(SceneCommand::PushClip {
                    rect: mapping.viewport_rect().map_err(render_error)?,
                });
                commands.push(SceneCommand::PushTransform {
                    transform: mapping.logical_to_raster_transform(),
                });
            }
            let mut sources = vec![None; commands.len()];
            let tail = scene.commands.len();
            let mut textures = scene.commands.drain(..).zip(scene.sources.drain(..));
            let mut position = 0;
            let mut draws = text.layouts.into_iter();
            // Text origins include density but not the integer viewport offset.
            // Cancel only the outer scale, leaving the viewport offset in place.
            let inverse_scale = Transform2D {
                m11: density.recip(),
                m22: density.recip(),
                ..Transform2D::IDENTITY
            };
            for ((at, region), layout) in scene.text.into_iter().zip(layouts) {
                for (command, source) in textures.by_ref().take(at - position) {
                    commands.push(command);
                    sources.push(source);
                }
                position = at;
                if let Some(visible) = region.clip_width {
                    commands.push(SceneCommand::PushClip {
                        rect: RectI::new(region.x, 0, visible, height),
                    });
                    sources.push(None);
                }
                if scaled {
                    commands.push(SceneCommand::PushTransform {
                        transform: inverse_scale,
                    });
                    sources.push(None);
                }
                // The atlas emits outline draws immediately before the foreground.
                for draw in draws.by_ref() {
                    sources.extend(std::iter::repeat_n(None, draw.commands.len()));
                    commands.extend(draw.commands);
                    if draw.layout_id == layout.request.key {
                        break;
                    }
                }
                if scaled {
                    commands.push(SceneCommand::PopTransform);
                    sources.push(None);
                }
                if region.clip_width.is_some() {
                    commands.push(SceneCommand::PopClip);
                    sources.push(None);
                }
            }
            for (command, source) in textures {
                commands.push(command);
                sources.push(source);
            }
            if scaled {
                commands.extend([SceneCommand::PopTransform, SceneCommand::PopClip]);
                sources.extend([None, None]);
            }
            debug_assert!(position <= tail);
            self.residency
                .prepare_with_sources(&mut commands, &sources)
                .map_err(render_error)?;
            self.sequence = self
                .sequence
                .checked_add(1)
                .ok_or(Error::Invalid("ADV32 render sequence"))?;
            let frame = self
                .renderer
                .render(&SceneFrame {
                    sequence: self.sequence,
                    width: raster.width,
                    height: raster.height,
                    clear_rgba: [0, 0, 0, 255],
                    commands,
                    semantics: None,
                })
                .map_err(render_error)?;
            let expected = raster.width as usize * raster.height as usize * 4;
            if frame.rgba8.len() != expected {
                return Err(Error::Invalid("ADV32 GPU readback size"));
            }
            Ok(frame.rgba8.into())
        }
    }

    fn render_error(error: impl std::fmt::Debug) -> Error {
        Error::Render(format!("{error:?}"))
    }
}

#[cfg(all(test, not(target_env = "ohos")))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a hardware GPU"]
    fn hardware_composites_a_clipped_texture_and_sdk_text_in_draw_order() {
        let extent = roukaku_media_core::Extent2D::new(128, 64);
        let mapping = StageCanvas::new(extent, extent).unwrap();
        let mut renderer = GpuCompositor::new(encoding_rs::GBK, None, mapping).unwrap();
        let Canvas::Scene(previous) = Canvas::for_output(mapping, true) else {
            unreachable!()
        };
        renderer.submit(previous).unwrap();
        renderer.discard_pending();
        let sprite = Crx::from_image(
            0,
            0,
            RgbaImage::from_vec(2, 1, vec![190, 0, 0, 255, 0, 120, 0, 255]).unwrap(),
        );
        let mut canvas = Canvas::for_output(mapping, true);
        canvas.queue_sprite(&sprite, RectI::new(1, 0, 1, 1), RectI::new(0, 0, 1, 1), 1.0);
        canvas.queue_text(&Text {
            fixed_pitch: false,
            line_height: None,
            outline: true,
            clip_width: None,
            content: "字".into(),
            x: 30,
            y: 10,
            size: 30,
            color: [255; 3],
            width: None,
        });
        canvas.queue(SceneCommand::rect("cover", 30, 0, 65, 64, [0, 0, 0, 255]));
        canvas.queue_text(&Text {
            fixed_pitch: false,
            line_height: None,
            outline: true,
            clip_width: Some(16),
            content: "字".into(),
            x: 90,
            y: 10,
            size: 30,
            color: [255; 3],
            width: None,
        });
        let Canvas::Scene(scene) = canvas else {
            unreachable!()
        };
        let pixels = renderer.render_now(scene).unwrap();
        assert_eq!(&pixels[..4], &[0, 120, 0, 255]);
        assert!((30..60).all(|x| (0..64).all(|y| pixels[(y * 128 + x) * 4] == 0)));
        assert!((96..106).any(|x| (0..64).any(|y| pixels[(y * 128 + x) * 4] != 0)));
        assert!((106..126).all(|x| (0..64).all(|y| pixels[(y * 128 + x) * 4] == 0)));
    }
}
