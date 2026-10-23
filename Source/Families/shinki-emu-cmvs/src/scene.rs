use crate::{CmvsArchive, CmvsPs2aVmState};
use roukaku_media_core::{BlendMode, Extent2D, RectI, SceneCommand, TextureFrame};
use roukaku_platform::SceneFrame;
use roukaku_platform_common::WgpuOffscreenRenderer;
use shinki_emu_sdk::{CoreError, SceneTextureResidency, StageCanvas, TextureCache};
use std::{collections::BTreeMap, num::NonZeroUsize};
#[cfg(test)]
mod tests;
mod text;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CmvsTextureSlot {
    Parent(u8),
    Child { parent: u8, child: u16 },
}

/// Core-owned GPU composition of the recovered parent/child texture stage.
pub struct CmvsScene {
    renderer: WgpuOffscreenRenderer,
    textures: TextureCache,
    texture_residency: SceneTextureResidency,
    bindings: BTreeMap<CmvsTextureSlot, String>,
    /// Last-known placement per slot. The main tick's collector destroys
    /// slot objects after a scene switch while the native surfaces keep
    /// their transforms; the host reuses the cached placement then.
    slot_geometry: BTreeMap<CmvsTextureSlot, (RectI, u32)>,
    sequence: u64,
    raster: Option<Extent2D>,
    canvas: Option<StageCanvas>,
    text: text::NativeTextRenderer,
    // Keep the loader alive until every session GPU resource has dropped.
    _gpu_instance: wgpu::Instance,
}

impl CmvsScene {
    pub fn new(instance: &wgpu::Instance) -> Result<Self, CoreError> {
        let renderer = pollster::block_on(WgpuOffscreenRenderer::new(instance))
            .map_err(|_| error("SHINKI_EMU_CMVS_GPU_CREATE"))?;
        tracing::info!(event = "cmvs.gpu.created", backend = %renderer.identity().backend,
            device_type = %renderer.identity().device_type, vendor_id = renderer.identity().vendor_id,
            device_id = renderer.identity().device_id);
        Ok(Self {
            renderer,
            textures: TextureCache::new(NonZeroUsize::new(512).unwrap(), 1024 * 1024 * 1024, 16384)
                .map_err(|_| error("SHINKI_EMU_CMVS_TEXTURE_BUDGET"))?,
            texture_residency: SceneTextureResidency::default(),
            bindings: BTreeMap::new(),
            slot_geometry: BTreeMap::new(),
            sequence: 0,
            raster: None,
            canvas: None,
            text: text::NativeTextRenderer::default(),
            _gpu_instance: instance.clone(),
        })
    }

    /// Change only the render target. VM geometry stays in native stage units.
    pub(crate) fn resize_raster(&mut self, width: u32, height: u32) -> Result<(), CoreError> {
        let raster = Extent2D::new(width, height);
        raster
            .validate()
            .map_err(|_| error("SHINKI_EMU_CMVS_CANVAS"))?;
        self.renderer
            .validate_output_extent(width, height)
            .map_err(|_| error("SHINKI_EMU_CMVS_CANVAS"))?;
        self.raster = Some(raster);
        Ok(())
    }

    pub(crate) fn logical_extent(&self) -> Option<Extent2D> {
        self.canvas.map(StageCanvas::logical)
    }

    pub(crate) fn clear(&mut self, width: u32, height: u32) -> Result<TextureFrame, CoreError> {
        self.draw(width, height, Vec::new())
    }

    pub(crate) fn bind_text_font(&mut self, font: &str) -> Result<(), CoreError> {
        self.text.bind_font(font)
    }

    pub(crate) fn set_default_font(&mut self, font: String) {
        let _ = self.text.bind_font(&font);
        self.text.default_font = Some(font);
    }

    pub(crate) fn has_text_renderer(&self) -> bool {
        self.text.is_initialized()
    }

    pub(crate) fn append_text(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        slot: u8,
        source: &str,
    ) -> Result<(), CoreError> {
        self.text.append(vm, slot, source)
    }

    pub(crate) fn render_text(
        &mut self,
        vm: &CmvsPs2aVmState,
        width: u32,
        height: u32,
    ) -> Result<TextureFrame, CoreError> {
        let commands = self.text.frame(vm)?;
        self.draw(width, height, commands)
    }

    pub fn bind(
        &mut self,
        slot: CmvsTextureSlot,
        uri: &str,
        archive: &CmvsArchive,
    ) -> Result<(), CoreError> {
        if self.bindings.len() >= 512 && !self.bindings.contains_key(&slot) {
            return Err(error("SHINKI_EMU_CMVS_TEXTURE_BINDINGS"));
        }
        // Explicit loads decode again even when the URI has previously been cached.
        let image = archive.decode_pb_image(uri)?;
        let image_type = archive
            .pb_image_type(uri)
            .map(|t| t.to_string())
            .unwrap_or_default();
        if tracing::enabled!(tracing::Level::DEBUG) {
            let bytes = image.as_raw();
            let mean = if bytes.is_empty() {
                0.0
            } else {
                bytes.iter().map(|byte| u32::from(*byte)).sum::<u32>() as f64 / bytes.len() as f64
            };
            tracing::debug!(
                event = "cmvs.session.texture_bound",
                slot = ?slot,
                width = image.width(),
                height = image.height(),
                byte_mean = mean,
                image_type = %image_type
            );
        }
        let frame = TextureFrame {
            width: image.width(),
            height: image.height(),
            rgba8: image.into_raw().into(),
        };
        self.textures
            .insert(uri.to_owned(), frame)
            .map_err(|_| error("SHINKI_EMU_CMVS_TEXTURE_BUDGET"))?;
        self.bindings.insert(slot, uri.to_owned());
        Ok(())
    }

    /// Clones the texture bindings and slot placements (the native
    /// program switch preserves the loaded surface table; the menu's
    /// loads replace slots, so the story snapshot must carry them).
    pub(crate) fn binding_count(&self) -> usize {
        self.bindings.len()
    }

    pub(crate) fn bindings_snapshot(
        &self,
    ) -> (
        BTreeMap<CmvsTextureSlot, String>,
        BTreeMap<CmvsTextureSlot, (RectI, u32)>,
    ) {
        (self.bindings.clone(), self.slot_geometry.clone())
    }

    pub(crate) fn restore_bindings(
        &mut self,
        bindings: BTreeMap<CmvsTextureSlot, String>,
        geometry: BTreeMap<CmvsTextureSlot, (RectI, u32)>,
    ) {
        self.bindings = bindings;
        self.slot_geometry = geometry;
    }

    pub fn render(
        &mut self,
        vm: &CmvsPs2aVmState,
        requested: CmvsTextureSlot,
        archive: &CmvsArchive,
        stage: (u32, u32),
        movie: Option<&TextureFrame>,
    ) -> Result<Option<TextureFrame>, CoreError> {
        // Native texture slots are destroyed by the main tick's collector
        // right after a scene switch loads its replacements (`menu` 206740
        // tears the channel table down while `intproc` 9404 frees the
        // surfaces); the decoded textures stay cached in the manager and
        // the next present still draws them. Bindings therefore live until
        // a new load replaces the slot, not until the VM slot object dies.
        let requested_present = self.bindings.contains_key(&requested);
        if !requested_present {
            tracing::debug!(
                event = "cmvs.session.render_requested_missing",
                requested = ?requested,
                bindings = self.bindings.len()
            );
            // A missing requested slot — parent or child — composites the
            // remaining layers; the native present of a null surface skips
            // the layer instead of failing the frame.
        }
        let geometry = stage_geometry(
            vm,
            self.bindings.keys().copied(),
            requested,
            requested_present,
            &mut self.slot_geometry,
        )?;
        // The native stage presents one surface: the requested parent and
        // its children. Slots from other families keep their bindings and
        // cached placements but stay out of this frame; drawing them would
        // let a torn-down menu cover the story background.
        let requested_parent = match requested {
            CmvsTextureSlot::Parent(parent) => parent,
            CmvsTextureSlot::Child { parent, .. } => parent,
        };
        let geometry: Vec<_> = geometry
            .into_iter()
            .filter(|(slot, _, _)| match slot {
                CmvsTextureSlot::Parent(parent) => *parent == requested_parent,
                CmvsTextureSlot::Child { parent, .. } => *parent == requested_parent,
            })
            .collect();
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::trace!(
                event = "cmvs.session.render_family",
                requested = ?requested,
                requested_present,
                layers = geometry.len()
            );
        }
        let (stage_width, stage_height) = stage;
        let mut commands = Vec::with_capacity(geometry.len());
        for (slot, source, _) in geometry {
            let uri = &self.bindings[&slot];
            let frame = match self.textures.get(uri) {
                Some(frame) => frame,
                None => {
                    tracing::debug!(event = "cmvs.session.texture_lazy_decode", name = %uri);
                    let image = archive.decode_pb_image(uri)?;
                    let frame = TextureFrame {
                        width: image.width(),
                        height: image.height(),
                        rgba8: image.into_raw().into(),
                    };
                    self.textures
                        .insert(uri.clone(), frame.clone())
                        .map_err(|_| error("SHINKI_EMU_CMVS_TEXTURE_BUDGET"))?;
                    frame
                }
            };
            // The recovered CMVS stage draws every slot as a full-screen
            // quad; the VM rect words select the texture's source crop
            // (a [0, 0, 1, 1] rect on a 1x1 white texture is the full
            // screen fade). Sample the source rect into a stage-sized
            // frame so the quad maps one to one.
            // CMVS parents are background layers: their sampled pixels
            // ignore the source alpha so full-screen fades render as
            // solid color quads.
            let opaque = matches!(slot, CmvsTextureSlot::Parent(_));
            let source_frame = sample_to_stage(&frame, source, stage_width, stage_height, opaque);
            commands.push(SceneCommand::Texture {
                id: slot_id(slot),
                frame: source_frame,
                destination: RectI {
                    x: 0,
                    y: 0,
                    width: stage_width,
                    height: stage_height,
                },
                opacity: 1.0,
                blend: BlendMode::Alpha,
            });
        }
        if let Some(movie) = movie {
            // The native frame composes the movie after the channel layer
            // and before the overlay passes; stage-fit keeps aspect.
            commands.push(SceneCommand::Texture {
                id: "cmvs.movie".into(),
                frame: fit_to_stage(movie, stage_width, stage_height),
                destination: RectI {
                    x: 0,
                    y: 0,
                    width: stage_width,
                    height: stage_height,
                },
                opacity: 1.0,
                blend: BlendMode::Alpha,
            });
        }
        // Channel quads (menu items, window sprites) render as overlay
        // rectangles: the quad's frame geometry gives (x, y, w, h) in the
        // channel's local space plus the channel origin (re-report 5.2).
        // The native surface rasterizes text/sprites into a pixel cache;
        // without that cache the observable subset draws the geometry as a
        // translucent white overlay so menu selection feedback is visible.
        if let CmvsTextureSlot::Parent(parent) = requested {
            if let Some(channel) = vm.effect_channels.get(&parent) {
                let (origin_x, origin_y) = channel.origin();
                for quad in 0..32u32 {
                    if !channel.quad_visible(quad) {
                        continue;
                    }
                    let frame = u32::from(channel.quad_frame(quad));
                    let [x, y, w, h] = channel.quad_frame_geometry(quad, frame as u16);
                    let (x, y, w, h) = (
                        i32::from(x) + origin_x,
                        i32::from(y) + origin_y,
                        i32::from(w),
                        i32::from(h),
                    );
                    if w <= 0 || h <= 0 || x + w <= 0 || y + h <= 0 {
                        continue;
                    }
                    commands.push(SceneCommand::Texture {
                        id: format!("cmvs.quad.{parent}.{quad}"),
                        frame: white_stage_frame(stage_width, stage_height),
                        destination: RectI {
                            x: x.max(0),
                            y: y.max(0),
                            width: w.min(stage_width as i32 - x.max(0)).max(0) as u32,
                            height: h.min(stage_height as i32 - y.max(0)).max(0) as u32,
                        },
                        opacity: 0.35,
                        blend: BlendMode::Alpha,
                    });
                }
            }
        }
        // The headless draw path cannot yet present the GPU text scene
        // (`surface.present_scene` returns InvalidState), so the shaped
        // dialogue stays observed through `cmvs.session.dialogue` until
        // the text surface integration lands.
        if let Ok(text_commands) = self.text.frame(vm) {
            commands.extend(text_commands);
        }
        // Experiment: capture the glyph atlas InvalidState message.
        self.draw(stage_width, stage_height, commands).map(Some)
    }

    fn draw(
        &mut self,
        width: u32,
        height: u32,
        mut commands: Vec<SceneCommand>,
    ) -> Result<TextureFrame, CoreError> {
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_FRAME_SEQUENCE"))?;
        let logical = Extent2D::new(width, height);
        let canvas = StageCanvas::new(logical, self.raster.unwrap_or(logical))?;
        self.texture_residency
            .prepare(&mut commands)
            .map_err(|cause| {
                CoreError::invalid("SHINKI_EMU_CMVS_TEXTURE_RESIDENCY", cause.to_string())
            })?;
        let mut mapped = Vec::with_capacity(commands.len() + 4);
        mapped.push(SceneCommand::PushClip {
            rect: canvas.viewport_rect()?,
        });
        mapped.push(SceneCommand::PushTransform {
            transform: canvas.logical_to_raster_transform(),
        });
        mapped.extend(commands);
        mapped.push(SceneCommand::PopTransform);
        mapped.push(SceneCommand::PopClip);
        let command_count = mapped.len();
        let raster = canvas.raster();
        let output = self
            .renderer
            .render(&SceneFrame {
                sequence,
                width: raster.width,
                height: raster.height,
                clear_rgba: [0, 0, 0, 255],
                commands: mapped,
                semantics: None,
            })
            .map_err(|cause| {
                tracing::error!(event = "cmvs.gpu.failed", operation = %cause.operation, code = ?cause.code, message = %cause);
                error("SHINKI_EMU_CMVS_GPU_RENDER")
            })?;
        self.sequence = sequence;
        if tracing::enabled!(tracing::Level::DEBUG) {
            let mean = if output.rgba8.is_empty() {
                0.0
            } else {
                output
                    .rgba8
                    .iter()
                    .map(|byte| u32::from(*byte))
                    .sum::<u32>() as f64
                    / output.rgba8.len() as f64
            };
            tracing::trace!(
                event = "cmvs.session.draw_output",
                commands = command_count,
                width = raster.width,
                height = raster.height,
                byte_mean = mean
            );
        }
        self.canvas = Some(canvas);
        Ok(TextureFrame {
            width: raster.width,
            height: raster.height,
            rgba8: output.rgba8.into(),
        })
    }
}

type SlotState = (bool, Option<[u32; 4]>, Option<[u32; 2]>, Option<u32>);
fn slot_state(vm: &CmvsPs2aVmState, slot: CmvsTextureSlot) -> Option<SlotState> {
    let live = match slot {
        CmvsTextureSlot::Parent(parent) => vm.texture_parents.get(&parent).map(|s| {
            (
                s.surface_initialized,
                s.rect_words,
                s.position_words,
                s.auxiliary_word,
            )
        }),
        CmvsTextureSlot::Child { parent, child } => {
            vm.texture_children.get(&parent)?.get(&child).map(|s| {
                (
                    s.surface_initialized,
                    s.rect_words,
                    s.position_words,
                    s.auxiliary_word,
                )
            })
        }
    };
    live.or_else(|| {
        let key = match slot {
            CmvsTextureSlot::Parent(parent) => crate::cmvs_slot_placement_key_parent(parent),
            CmvsTextureSlot::Child { parent, child } => {
                crate::cmvs_slot_placement_key_child(parent, child)
            }
        };
        vm.retained_slot_placements
            .get(&key)
            .map(|placement| -> SlotState {
                (
                    placement.surface_initialized,
                    placement.rect_words,
                    placement.position_words,
                    placement.auxiliary_word,
                )
            })
    })
}
fn sample_to_stage(
    frame: &TextureFrame,
    source: RectI,
    stage_width: u32,
    stage_height: u32,
    opaque: bool,
) -> TextureFrame {
    let x = source.x.clamp(0, frame.width as i32) as u32;
    let y = source.y.clamp(0, frame.height as i32) as u32;
    let width = source.width.min(frame.width.saturating_sub(x)).max(1);
    let height = source.height.min(frame.height.saturating_sub(y)).max(1);
    let mut rgba8 = vec![0_u8; stage_width as usize * stage_height as usize * 4];
    for row in 0..stage_height as usize {
        let sy = y as usize + row * height as usize / stage_height as usize;
        let dst = row * stage_width as usize * 4;
        let src_row = &frame.rgba8[sy * frame.width as usize * 4..];
        for column in 0..stage_width as usize {
            let sx = x as usize + column * width as usize / stage_width as usize;
            let src = &src_row[sx * 4..sx * 4 + 4];
            let dst_range = dst + column * 4..dst + column * 4 + 4;
            if opaque && src[3] == 0 {
                // Keep the fade color: transparent fade pixels still
                // belong to the overlay quad in the original renderer.
                rgba8[dst_range].copy_from_slice(&[src[0], src[1], src[2], 255]);
            } else {
                rgba8[dst_range].copy_from_slice(src);
            }
        }
    }
    TextureFrame {
        width: stage_width,
        height: stage_height,
        rgba8: rgba8.into(),
    }
}
fn slot_id(slot: CmvsTextureSlot) -> String {
    match slot {
        CmvsTextureSlot::Parent(p) => format!("cmvs.texture.parent.{p}"),
        CmvsTextureSlot::Child { parent, child } => format!("cmvs.texture.child.{parent}.{child}"),
    }
}

/// A stage-sized translucent-white source frame for channel-quad overlays.
fn white_stage_frame(width: u32, height: u32) -> TextureFrame {
    let rgba8 = vec![255_u8; (width as usize) * (height as usize) * 4];
    TextureFrame::from_vec(width, height, rgba8).expect("stage-sized white frame is always valid")
}

fn stage_geometry(
    vm: &CmvsPs2aVmState,
    slots: impl Iterator<Item = CmvsTextureSlot>,
    requested: CmvsTextureSlot,
    requested_present: bool,
    geometry_cache: &mut BTreeMap<CmvsTextureSlot, (RectI, u32)>,
) -> Result<Vec<(CmvsTextureSlot, RectI, u32)>, CoreError> {
    let mut geometry = Vec::new();
    let mut requested_degenerate = false;
    for slot in slots {
        let placement = match (slot, slot_state(vm, slot)) {
            (_, Some((true, Some(rect), Some(position), Some(z)))) => {
                let bounds = [
                    rect[0].checked_add(position[0]),
                    rect[1].checked_add(position[1]),
                    rect[2].checked_add(position[0]),
                    rect[3].checked_add(position[1]),
                ];
                let [Some(x0), Some(y0), Some(x1), Some(y1)] = bounds else {
                    return Err(error("SHINKI_EMU_CMVS_TEXTURE_GEOMETRY"));
                };
                if x1 <= x0 || y1 <= y0 || x1 > 16384 || y1 > 16384 || z > i32::MAX as u32 {
                    if rect == [0, 0, 0, 0] && position == [0, 0] {
                        // An all-zero rect is the native unset-rect marker:
                        // the surface keeps its whole-texture default
                        // placement (`sky01` presents slot 29 this way).
                        let placement = (
                            RectI {
                                x: 0,
                                y: 0,
                                width: 16384,
                                height: 16384,
                            },
                            z,
                        );
                        geometry_cache.insert(slot, placement);
                        geometry.push((slot, placement.0, placement.1));
                        continue;
                    }
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            event = "cmvs.session.geometry_degenerate",
                            slot = ?slot,
                            rect = ?rect,
                            position = ?position,
                            z
                        );
                    }
                    // A degenerate quad rasterizes nothing natively; the
                    // layer is skipped rather than failing the frame.
                    if slot == requested {
                        requested_degenerate = true;
                    }
                    continue;
                }
                let placement = (
                    RectI {
                        x: x0 as i32,
                        y: y0 as i32,
                        width: x1 - x0,
                        height: y1 - y0,
                    },
                    z,
                );
                geometry_cache.insert(slot, placement);
                placement
            }
            // A parent surface without an explicit transform draws its
            // whole texture across the stage at z 0: the native default
            // surface state. Children stay hidden until configured.
            (CmvsTextureSlot::Parent(_), Some((true, _, _, _))) => (
                RectI {
                    x: 0,
                    y: 0,
                    width: 16384,
                    height: 16384,
                },
                0,
            ),
            // A destroyed slot object keeps its last placement: the native
            // surface survives the collector with its transform intact.
            _ => match geometry_cache.get(&slot) {
                Some(placement) => *placement,
                None => continue,
            },
        };
        geometry.push((slot, placement.0, placement.1));
    }
    if requested_present
        && !requested_degenerate
        && !geometry.iter().any(|(slot, _, _)| *slot == requested)
    {
        tracing::info!(
            event = "cmvs.session.geometry_missing_detail",
            requested = ?requested,
            parents = vm.texture_parents.len(),
            parent28 = ?vm.texture_parents.get(&28).map(|p| p.surface_initialized),
            binding_count = geometry_cache.len(),
            tick = vm.tick
        );
        return Err(error("SHINKI_EMU_CMVS_TEXTURE_PRESENTATION_STATE"));
    }
    geometry.sort_by_key(|(slot, _, z)| (*z, *slot));
    Ok(geometry)
}
fn error(code: &'static str) -> CoreError {
    CoreError::invalid(code, "CMVS scene operation failed")
}

/// Letterbox-fits one decoded movie frame into the stage, preserving the
/// aspect ratio with opaque black bars.
fn fit_to_stage(frame: &TextureFrame, stage_width: u32, stage_height: u32) -> TextureFrame {
    let mut rgba8 = vec![0_u8; stage_width as usize * stage_height as usize * 4];
    let scale_x = f64::from(stage_width) / f64::from(frame.width.max(1));
    let scale_y = f64::from(stage_height) / f64::from(frame.height.max(1));
    let scale = scale_x.min(scale_y);
    let out_w = ((f64::from(frame.width) * scale).round() as u32).clamp(1, stage_width);
    let out_h = ((f64::from(frame.height) * scale).round() as u32).clamp(1, stage_height);
    let off_x = (stage_width - out_w) / 2;
    let off_y = (stage_height - out_h) / 2;
    for row in 0..out_h {
        let sy = (u64::from(row) * u64::from(frame.height) / u64::from(out_h)) as usize;
        for column in 0..out_w {
            let sx = (u64::from(column) * u64::from(frame.width) / u64::from(out_w)) as usize;
            let src = (sy * frame.width as usize + sx) * 4;
            let dst =
                ((off_y + row) as usize * stage_width as usize + (off_x + column) as usize) * 4;
            rgba8[dst..dst + 4].copy_from_slice(&frame.rgba8[src..src + 4]);
        }
    }
    TextureFrame {
        width: stage_width,
        height: stage_height,
        rgba8: rgba8.into(),
    }
}
