use crate::{crx::Crx, text::Text};
use image::{Rgba, RgbaImage};
use roukaku_media_core::{BlendMode, RectI, SceneCommand};

pub(crate) enum Canvas {
    Raster(RgbaImage),
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
    pub(crate) fn new(width: u32, height: u32, _gpu: bool) -> Self {
        #[cfg(not(target_env = "ohos"))]
        if _gpu {
            return Self::Scene(Scene {
                extent: (width, height),
                commands: Vec::new(),
                sources: Vec::new(),
                text: Vec::new(),
            });
        }
        Self::Raster(RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255])))
    }

    pub(crate) fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Raster(image) => image.dimensions(),
            #[cfg(not(target_env = "ohos"))]
            Self::Scene(scene) => scene.extent,
        }
    }

    pub(crate) fn width(&self) -> u32 {
        self.dimensions().0
    }

    pub(crate) fn raster_mut(&mut self) -> &mut RgbaImage {
        match self {
            Self::Raster(image) => image,
            #[cfg(not(target_env = "ohos"))]
            Self::Scene(_) => unreachable!("CPU pixel effect must select the raster canvas"),
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

#[cfg(not(target_env = "ohos"))]
pub(crate) use hardware::GpuCompositor;

#[cfg(not(target_env = "ohos"))]
mod hardware {
    use super::*;
    use crate::{Error, Result};
    use roukaku_core::Hash256;
    use roukaku_media_core::{OwnedPixelBuffer, SceneCompositing2D};
    use roukaku_platform::SceneFrame;
    use roukaku_platform_common::{WgpuOffscreenRenderer, native_wgpu_instance};
    use roukaku_text::{
        CosmicTextLayoutProvider, FontBindingContext, LayoutConstraint, OverflowPolicy,
        PackagedFont, TextDirection, TextLayoutConfig, TextLayoutRequest, TextRun, WrapPolicy,
        font_unicode_coverage,
    };
    use shinki_emu_sdk::{SceneTextureResidency, TextScene, TextSceneLayout};
    use std::{
        sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
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
        pub(crate) fn new(chinese: bool) -> Result<Self> {
            let (commands, queue) = sync_channel::<Scene>(1);
            let (completed, frames) = sync_channel(1);
            let (initialize, ready) = sync_channel(1);
            let worker = thread::Builder::new()
                .name("adv32-gpu".into())
                .spawn(move || {
                    let mut renderer = match GpuRenderer::new(chinese) {
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
        sequence: u64,
        chinese: bool,
    }

    impl GpuRenderer {
        pub(crate) fn new(chinese: bool) -> Result<Self> {
            let instance = native_wgpu_instance().map_err(render_error)?;
            let renderer = pollster::block_on(WgpuOffscreenRenderer::new(&instance))
                .map_err(render_error)?
                .with_default_compositing(SceneCompositing2D::EncodedSrgb);
            tracing::info!(
                event = "shinki.emu.adv32.gpu.adapter",
                backend = %renderer.identity().backend,
                device_type = %renderer.identity().device_type,
                "Selected hardware compositor adapter"
            );
            let fonts = [
                (
                    "Noto Sans JP",
                    "asset:/font/adv32/noto-jp",
                    crate::text::FONT,
                ),
                (
                    "Noto Sans SC",
                    "asset:/font/adv32/noto-sc",
                    crate::text::FONT_SC,
                ),
            ]
            .into_iter()
            .map(|(family, id, bytes)| {
                Ok(PackagedFont {
                    asset_id: id.into(),
                    family: family.into(),
                    face_index: 0,
                    hash: Hash256::from_sha256(bytes),
                    license_id: "OFL-1.1".into(),
                    subset: None,
                    coverage: font_unicode_coverage(bytes, 0).map_err(render_error)?,
                    targets: vec!["adv32".into()],
                    profiles: vec!["adv32".into()],
                    bytes: bytes.to_vec(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
            let provider = CosmicTextLayoutProvider::new(
                FontBindingContext {
                    target: "adv32".into(),
                    profile: "adv32".into(),
                    default_locale: if chinese { "zh-Hans" } else { "ja-JP" }.into(),
                },
                fonts,
                TextLayoutConfig::production_defaults(),
            )
            .map_err(render_error)?;
            Ok(Self {
                renderer,
                residency: SceneTextureResidency::default(),
                text: TextScene::new(provider),
                sequence: 0,
                chinese,
            })
        }

        fn render(&mut self, mut scene: Scene) -> Result<OwnedPixelBuffer> {
            let (width, height) = scene.extent;
            let (language, families) = if self.chinese {
                ("zh-Hans", ["Noto Sans SC", "Noto Sans JP"])
            } else {
                ("ja-JP", ["Noto Sans JP", "Noto Sans SC"])
            };
            let layouts = scene
                .text
                .iter()
                .enumerate()
                .map(|(index, (_, text))| {
                    let font_size = f32::from(text.size.min(256));
                    let available =
                        (i64::from(width) - i64::from(text.x)).clamp(1, i64::from(width));
                    let max_width = text
                        .width
                        .map_or(available, |width| available.min(i64::from(width)));
                    let [r, g, b] = text.color;
                    TextSceneLayout {
                        request: TextLayoutRequest {
                            key: format!("adv32:{index}"),
                            runs: vec![TextRun {
                                text: text.content.clone(),
                                language: language.into(),
                                script: None,
                                direction: TextDirection::LeftToRight,
                                ruby: Vec::new(),
                                voice: None,
                            }],
                            constraint: LayoutConstraint {
                                max_width: max_width.max(1) as f32,
                                max_height: None,
                                max_lines: None,
                                font_size,
                                line_height: font_size * 1.25,
                                wrap: WrapPolicy::WordOrGlyph,
                                overflow: OverflowPolicy::Visible,
                            },
                            font_families: families
                                .iter()
                                .map(|family| (*family).to_owned())
                                .collect(),
                            features: Vec::new(),
                        },
                        translation: (text.x, text.y),
                        rgba: [r, g, b, 255],
                        outline: None,
                    }
                })
                .collect::<Vec<_>>();
            let text = self.text.frame_parts(&layouts, 1.0).map_err(render_error)?;
            let mut commands = text.lifecycle;
            let mut sources = vec![None; commands.len()];
            let tail = scene.commands.len();
            let mut textures = scene.commands.drain(..).zip(scene.sources.drain(..));
            let mut position = 0;
            for ((at, _), layout) in scene.text.into_iter().zip(text.layouts) {
                for (command, source) in textures.by_ref().take(at - position) {
                    commands.push(command);
                    sources.push(source);
                }
                position = at;
                sources.extend(std::iter::repeat_n(None, layout.commands.len()));
                commands.extend(layout.commands);
            }
            for (command, source) in textures {
                commands.push(command);
                sources.push(source);
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
                    width,
                    height,
                    clear_rgba: [0, 0, 0, 255],
                    commands,
                    semantics: None,
                })
                .map_err(render_error)?;
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
        let mut renderer = GpuCompositor::new(true).unwrap();
        let Canvas::Scene(previous) = Canvas::new(128, 64, true) else {
            unreachable!()
        };
        renderer.submit(previous).unwrap();
        renderer.discard_pending();
        let sprite = Crx::from_image(
            0,
            0,
            RgbaImage::from_vec(2, 1, vec![190, 0, 0, 255, 0, 120, 0, 255]).unwrap(),
        );
        let mut canvas = Canvas::new(128, 64, true);
        canvas.queue_sprite(&sprite, RectI::new(1, 0, 1, 1), RectI::new(0, 0, 1, 1), 1.0);
        canvas.queue_text(&Text {
            content: "字".into(),
            x: 30,
            y: 10,
            size: 30,
            color: [255; 3],
            width: None,
        });
        canvas.queue(SceneCommand::rect("cover", 30, 0, 65, 64, [0, 0, 0, 255]));
        canvas.queue_text(&Text {
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
        assert!((96..126).any(|x| (0..64).any(|y| pixels[(y * 128 + x) * 4] != 0)));
    }
}
