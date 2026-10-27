use crate::{CmvsArchive, CmvsPs2aVmState};
use roukaku_media_core::{BlendMode, Extent2D, RectI, SceneCommand, TextureFrame};
use roukaku_platform::SceneFrame;
use roukaku_platform_common::WgpuOffscreenRenderer;
use shinki_emu_sdk::{CoreError, SceneTextureResidency, StageCanvas, TextureCache};
use std::{collections::BTreeMap, num::NonZeroUsize};
mod basic_geometry;
#[cfg(test)]
mod basic_geometry_tests;
#[cfg(test)]
mod camera_transition_tests;
mod channel_images;
mod cmv;
mod image_filter;
mod image_owner;
#[cfg(test)]
mod independent_view_tests;
mod native_mesh;
pub(crate) mod native_save_channels;
pub(crate) mod native_save_images;
#[cfg(test)]
mod native_view_tests;
mod native_viewport;
#[cfg(test)]
pub(crate) mod parent_load_tests;
mod plane_particles;
mod presentation;
mod projection;
#[cfg(test)]
mod projection_tests;
mod reload;
#[cfg(test)]
mod reload_tests;
#[cfg(test)]
mod screen_shake_tests;
mod surface;
#[cfg(test)]
pub(crate) mod tests;
mod text;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CmvsTextureSlot {
    Parent(u8),
    Child { parent: u8, child: u16 },
    SceneRoot(u8),
    SceneChild { slot: u8, child: u32 },
}

enum NativeQueuedDraw {
    Image(CmvsTextureSlot, RectI),
    Group(surface::NativeDrawGroup),
}

/// Core-owned GPU composition of the recovered parent/child texture stage.
pub struct CmvsScene {
    renderer: WgpuOffscreenRenderer,
    textures: TextureCache,
    texture_residency: SceneTextureResidency,
    bindings: BTreeMap<CmvsTextureSlot, String>,
    dynamic_images: BTreeMap<CmvsTextureSlot, TextureFrame>,
    /// Placement cache for live surfaces, moved and cleared with their owners.
    slot_geometry: BTreeMap<CmvsTextureSlot, (RectI, u32)>,
    sequence: u64,
    logical: Option<Extent2D>,
    raster: Option<Extent2D>,
    canvas: Option<StageCanvas>,
    text: text::NativeTextRenderer,
    presentation_mask: Option<presentation::PresentationMask>,
    filtered_images: BTreeMap<CmvsTextureSlot, image_filter::FilteredImage>,
    channel_uploads: BTreeMap<u8, (String, String)>,
    pub(super) channel_element_uploads: BTreeMap<(u8, u32, u8), (String, String)>,
    mesh_uploads: BTreeMap<u8, native_mesh::NativeMeshUpload>,
    mesh_order: Vec<native_mesh::MeshOrder>,
    plane_uploads: BTreeMap<(u8, usize), plane_particles::PlaneUpload>,
    plane_order: Vec<((u8, u32), u8, usize, usize)>,
    surface_uploads: BTreeMap<CmvsTextureSlot, surface::SurfaceImage>,
    reload_surfaces: BTreeMap<CmvsTextureSlot, reload::ReloadSurfaces>,
    reload_releases: Vec<String>,
    reload_generation: u64,
    // Keep the loader alive until every session GPU resource has dropped.
    _gpu_instance: wgpu::Instance,
}

impl CmvsScene {
    pub fn new(instance: &wgpu::Instance) -> Result<Self, CoreError> {
        let renderer = pollster::block_on(WgpuOffscreenRenderer::new(instance))
            .map_err(|_| error("SHINKI_EMU_CMVS_GPU_CREATE"))?
            .with_default_compositing(roukaku_media_core::SceneCompositing2D::EncodedSrgb);
        tracing::info!(event = "cmvs.gpu.created", backend = %renderer.identity().backend,
            device_type = %renderer.identity().device_type, vendor_id = renderer.identity().vendor_id,
            device_id = renderer.identity().device_id);
        Ok(Self {
            renderer,
            textures: TextureCache::new(NonZeroUsize::new(512).unwrap(), 1024 * 1024 * 1024, 16384)
                .map_err(|_| error("SHINKI_EMU_CMVS_TEXTURE_BUDGET"))?,
            texture_residency: SceneTextureResidency::default(),
            bindings: BTreeMap::new(),
            dynamic_images: BTreeMap::new(),
            slot_geometry: BTreeMap::new(),
            sequence: 0,
            raster: None,
            canvas: None,
            text: text::NativeTextRenderer::default(),
            logical: None,
            presentation_mask: None,
            filtered_images: BTreeMap::new(),
            channel_uploads: BTreeMap::new(),
            channel_element_uploads: BTreeMap::new(),
            mesh_uploads: BTreeMap::new(),
            mesh_order: Vec::new(),
            plane_uploads: BTreeMap::new(),
            plane_order: Vec::new(),
            surface_uploads: BTreeMap::new(),
            reload_surfaces: BTreeMap::new(),
            reload_releases: Vec::new(),
            reload_generation: 0,
            _gpu_instance: instance.clone(),
        })
    }

    pub(crate) fn native_restore_candidate(&self) -> Result<Self, CoreError> {
        let mut candidate = Self::new(&self._gpu_instance)?;
        candidate.text = self.text.native_restore_context()?;
        // Native load retains the live stage. Font cells are rebuilt before
        // the candidate's first draw and need its logical width immediately.
        candidate.logical = self.logical;
        if let Some(raster) = self.raster {
            candidate.resize_raster(raster.width, raster.height)?;
        }
        Ok(candidate)
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
        self.logical
    }

    pub(crate) fn clear(&mut self, width: u32, height: u32) -> Result<TextureFrame, CoreError> {
        self.draw(width, height, Vec::new())
    }

    pub(crate) fn bind_text_font(&mut self, font: &str) -> Result<(), CoreError> {
        self.text.bind_font(font)
    }
    pub(crate) fn bind_text_owner(
        &mut self,
        vm: &CmvsPs2aVmState,
        slot: u8,
    ) -> Result<(), CoreError> {
        let owner = vm
            .native_font_owners
            .get(&(762 + u16::from(slot)))
            .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_OBJECT"))?;
        self.text.bind_font(&owner.family)
    }

    pub(crate) fn bind_glyph_owner(
        &mut self,
        vm: &CmvsPs2aVmState,
        slot: u8,
    ) -> Result<(), CoreError> {
        self.text.bind_glyph_owner(vm, slot)
    }

    pub(crate) fn append_glyph(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        slot: u8,
        source: &[u8],
    ) -> Result<(), CoreError> {
        self.text.append_glyph(vm, slot, source)
    }

    pub(crate) fn set_default_font(&mut self, font: String) {
        self.text.default_font = Some(font);
    }

    pub(crate) fn append_text(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        slot: u8,
        source: &str,
    ) -> Result<(), CoreError> {
        let width = self.logical_extent().map_or(0, |extent| extent.width);
        self.text.append(vm, slot, source, width)
    }

    pub(crate) fn append_dialogue(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        channel: u8,
        slot: u8,
        source: &str,
    ) -> Result<(), CoreError> {
        let width = self.logical_extent().map_or(0, |extent| extent.width);
        self.text.append_dialogue(vm, channel, slot, source, width)
    }

    #[cfg(test)]
    pub(crate) fn render_overlays(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        width: u32,
        height: u32,
        movie: Option<&TextureFrame>,
        mut commands: Vec<SceneCommand>,
    ) -> Result<TextureFrame, CoreError> {
        let mut groups = self
            .text
            .prepare_draw_groups(vm, width, &mut self.textures)?;
        groups.admit(&mut 8192);
        groups.append(&mut commands);
        if let Some(movie) = movie {
            let overlays = commands.len();
            append_movie(&mut commands, vm, movie, width)?;
            let added = commands.len() - overlays;
            commands.rotate_right(added);
        }
        self.draw_final(vm, width, height, commands)
    }

    pub(crate) fn clear_parent_image(&mut self, parent: u8) {
        self.bindings.remove(&CmvsTextureSlot::Parent(parent));
        self.dynamic_images.remove(&CmvsTextureSlot::Parent(parent));
        self.filtered_images
            .remove(&CmvsTextureSlot::Parent(parent));
        self.release_surface_image(CmvsTextureSlot::Parent(parent));
    }

    pub(crate) fn bind_parent_image(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        parent: u8,
        uri: &str,
        archive: &CmvsArchive,
    ) -> Result<(), CoreError> {
        let info = archive.native_image_info(uri)?;
        self.bind(CmvsTextureSlot::Parent(parent), uri, archive)?;
        vm.texture_parents
            .get_mut(&parent)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_VM_TEXTURE"))?
            .image_info = info;
        Ok(())
    }

    pub(crate) fn bind_child_image(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        parent: u8,
        child: u16,
        uri: &str,
        archive: &CmvsArchive,
    ) -> Result<(), CoreError> {
        let info = archive.native_image_info(uri)?;
        self.bind(CmvsTextureSlot::Child { parent, child }, uri, archive)?;
        vm.texture_children
            .get_mut(&parent)
            .and_then(|children| children.get_mut(&child))
            .ok_or_else(|| error("SHINKI_EMU_CMVS_VM_TEXTURE_CHILD"))?
            .image_info = info;
        Ok(())
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
        let frame = TextureFrame {
            width: image.width(),
            height: image.height(),
            rgba8: image.into_raw().into(),
        };
        self.textures
            .insert(uri.to_owned(), frame)
            .map_err(|_| error("SHINKI_EMU_CMVS_TEXTURE_BUDGET"))?;
        self.bindings.insert(slot, uri.to_owned());
        self.dynamic_images.remove(&slot);
        if slot.is_root() {
            self.filtered_images.remove(&slot);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn bound_p(&self, slot: CmvsTextureSlot) -> bool {
        self.bindings.contains_key(&slot)
    }

    /// With no explicit target the standing scene composes every bound
    /// family sorted by z, based on the first bound parent.
    pub(crate) fn standing_scene_slot(&self) -> Option<CmvsTextureSlot> {
        self.bindings.keys().find(|slot| slot.is_root()).copied()
    }

    pub(crate) fn release_child_surface(&mut self, parent: u8, child: u16) {
        let slot = CmvsTextureSlot::Child { parent, child };
        self.bindings.remove(&slot);
        self.slot_geometry.remove(&slot);
        self.release_surface_image(slot);
    }

    pub(crate) fn clear_parent_surfaces(&mut self) {
        let keep = |slot: &CmvsTextureSlot| slot.scene_worker().is_some();
        self.bindings.retain(|slot, _| keep(slot));
        self.dynamic_images.retain(|slot, _| keep(slot));
        self.slot_geometry.retain(|slot, _| keep(slot));
        self.surface_uploads.retain(|slot, upload| {
            if keep(slot) {
                return true;
            }
            self.reload_releases.push(upload.id.clone());
            false
        });
        self.filtered_images.retain(|slot, _| keep(slot));
        self.clear_parent_image_reloads();
    }

    pub fn render(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        requested: CmvsTextureSlot,
        archive: &CmvsArchive,
        stage: (u32, u32),
        movie: Option<&TextureFrame>,
    ) -> Result<Option<TextureFrame>, CoreError> {
        self.restore_image_reload_bindings(vm);
        self.prepare_scene_image_bindings(vm, archive)?;
        // A native root owns its bindings; destruction/rebuild actions remove
        // the complete family before later script loads or presents.
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
        let mut geometry = stage_geometry_with(
            vm,
            vm.texture_parents.keys().copied().flat_map(|parent| {
                std::iter::once(CmvsTextureSlot::Parent(parent)).chain(
                    vm.texture_children
                        .get(&parent)
                        .into_iter()
                        .flat_map(move |children| {
                            children.keys().map(move |child| CmvsTextureSlot::Child {
                                parent,
                                child: *child,
                            })
                        }),
                )
            }),
            requested,
            requested_present && requested.scene_worker().is_none(),
            &mut self.slot_geometry,
            slot_state,
        )?;
        tracing::info!(
            event = "cmvs.session.render_family",
            requested = ?requested,
            requested_present,
            layers = geometry.len(),
        );
        let (stage_width, stage_height) = stage;
        self.synchronize_image_reloads(vm);
        let mut commands = Vec::with_capacity(geometry.len());
        let mut transition_commands = Vec::new();
        let mut transition_insert_at = None;
        let plane_counts = self.prepare_plane_particles(vm, archive, &mut commands)?;
        let mut remaining_records = 8192_usize.saturating_sub(geometry.len());
        let mut mesh_budgets = [0; 4];
        let mut plane_budgets = [0; 4];
        for slot in 0..4 {
            let Some(worker) = vm.native_scene_workers.get(&(slot as u8)) else {
                continue;
            };
            if let Some(image) = worker.image_shards() {
                let start = geometry.len();
                append_stage_geometry_with(
                    vm,
                    image_owner::scene_slots(slot as u8, image),
                    (requested, false),
                    &mut self.slot_geometry,
                    slot_state,
                    start + remaining_records,
                    &mut geometry,
                )?;
                remaining_records -= geometry.len() - start;
            } else {
                mesh_budgets[slot] = worker.mesh_queue_count().min(remaining_records);
                remaining_records -= mesh_budgets[slot];
                plane_budgets[slot] = plane_counts[slot].min(remaining_records);
                remaining_records -= plane_budgets[slot];
            }
        }
        // Owner updates and GPU resource lifetimes run even after the shared
        // 4473C0 record pool fills. Only draw admission consumes this counter.
        let mut draw_groups = self
            .text
            .prepare_draw_groups(vm, stage_width, &mut self.textures)?;
        draw_groups.admit(&mut remaining_records);
        let mut channels = self.prepare_channel_draw_groups(vm, archive)?;
        channels.admit(&mut remaining_records);
        draw_groups.resources.append(&mut channels.resources);
        draw_groups.groups.append(&mut channels.groups);
        commands.append(&mut draw_groups.resources);
        draw_groups.groups.sort_by_key(|group| group.position);
        let presentation = vm.texture_presentation.as_ref();
        geometry.sort_by_key(|(slot, _, _)| surface::native_queue_position(vm, *slot));
        self.limit_plane_particles(vm, plane_budgets);
        let mut plane_cursor = 0;
        let mut snow_commands = self.native_mesh_commands(vm, archive, stage, mesh_budgets)?;
        let mut images = geometry
            .into_iter()
            .map(|(slot, source, _)| {
                (
                    surface::native_queue_position(vm, slot),
                    NativeQueuedDraw::Image(slot, source),
                )
            })
            .peekable();
        let mut groups = draw_groups
            .groups
            .into_iter()
            .map(|group| (group.position, NativeQueuedDraw::Group(group)))
            .peekable();
        let queue = std::iter::from_fn(|| {
            if images
                .peek()
                .is_some_and(|image| groups.peek().is_none_or(|group| image.0 <= group.0))
            {
                images.next()
            } else {
                groups.next()
            }
        });
        for (position, item) in queue {
            let queue_key = position.key();
            if queue_key.0 >= 3 {
                self.append_plane_particles_before(
                    vm,
                    &mut plane_cursor,
                    Some((2, u32::MAX)),
                    stage,
                    &mut commands,
                )?;
                commands.append(&mut snow_commands);
            }
            self.append_plane_particles_before(
                vm,
                &mut plane_cursor,
                Some(queue_key),
                stage,
                &mut commands,
            )?;
            let (slot, source) = match item {
                NativeQueuedDraw::Image(slot, source) => (slot, source),
                NativeQueuedDraw::Group(mut group) => {
                    commands.append(&mut group.commands);
                    continue;
                }
            };
            let in_presentation =
                presentation.is_some_and(|p| slot.root_slot() == CmvsTextureSlot::Parent(p.parent));
            if slot.is_root()
                && let Some(reload) = slot.reload(vm).filter(|reload| reload.busy())
            {
                let reloaded = self.reload_commands(vm, slot, reload, source, archive, stage)?;
                if in_presentation {
                    transition_insert_at.get_or_insert(commands.len());
                    transition_commands.extend(reloaded);
                } else {
                    commands.extend(reloaded);
                }
                continue;
            }
            let opacity = surface::opacity(vm, slot);
            let destination = surface::destination(vm, slot, source);
            let uri = self.bindings.get(&slot).or_else(|| {
                if slot.is_root() {
                    None
                } else {
                    self.bindings.get(&slot.root_slot())
                }
            });
            let Some(uri) = uri else {
                if let Some(command) = surface::solid_color(vm, slot).and_then(|color| {
                    let child = slot.child(vm);
                    let root = slot.root(vm).expect("live drawable root");
                    let opaque = child.as_deref().map_or(
                        root.basic_kind == 0 && root.blend_mode == Some(0),
                        |image| image.basic_kind == 0 && image.blend_mode == Some(0),
                    );
                    let solid_opacity = if opaque { 1.0 } else { opacity };
                    surface::solid_command(slot, destination, color, solid_opacity, stage)
                }) {
                    let Some(geometry) = surface_geometry(vm, slot, stage.0, false)? else {
                        continue;
                    };
                    if in_presentation {
                        transition_insert_at.get_or_insert(commands.len());
                        geometry.append(&mut transition_commands, command, None)?;
                    } else {
                        geometry.append(&mut commands, command, None)?;
                    }
                }
                continue;
            };
            let Some(geometry) = surface_geometry(vm, slot, stage.0, true)? else {
                continue;
            };
            let dynamic = self.dynamic_images.get(&slot).or_else(|| {
                if !slot.is_root() && !self.bindings.contains_key(&slot) {
                    self.dynamic_images.get(&slot.root_slot())
                } else {
                    None
                }
            });
            let frame = match dynamic.cloned().or_else(|| self.textures.get(uri)) {
                Some(frame) => frame,
                None => {
                    tracing::debug!(event = "cmvs.session.texture_lazy_decode", slot = ?slot);
                    let frame = if uri == crate::vm::texture_presentation::CLEAR_SURFACE_URI {
                        TextureFrame {
                            width: stage_width,
                            height: stage_height,
                            rgba8: [0, 0, 0, 255]
                                .repeat(stage_width as usize * stage_height as usize)
                                .into(),
                        }
                    } else {
                        let image = archive.decode_pb_image(uri)?;
                        TextureFrame {
                            width: image.width(),
                            height: image.height(),
                            rgba8: image.into_raw().into(),
                        }
                    };
                    self.textures
                        .insert(uri.clone(), frame.clone())
                        .map_err(|_| error("SHINKI_EMU_CMVS_TEXTURE_BUDGET"))?;
                    frame
                }
            };
            let provider = if self.bindings.contains_key(&slot) {
                slot
            } else {
                slot.root_slot()
            };
            let frame = if let Some(parameters) = provider
                .root(vm)
                .and_then(|image| provider.is_root().then_some(image.color_filter).flatten())
            {
                image_filter::frame(
                    &mut self.textures,
                    &mut self.filtered_images,
                    provider,
                    uri,
                    parameters,
                    frame,
                )?
            } else {
                frame
            };
            let target = if in_presentation {
                transition_insert_at.get_or_insert(commands.len());
                &mut transition_commands
            } else {
                &mut commands
            };
            self.append_surface_image(vm, slot, frame, source, opacity, target, geometry)?;
        }
        self.append_plane_particles_before(
            vm,
            &mut plane_cursor,
            Some((2, u32::MAX)),
            stage,
            &mut commands,
        )?;
        commands.append(&mut snow_commands);
        self.append_plane_particles_before(vm, &mut plane_cursor, None, stage, &mut commands)?;
        if let (Some(presentation), Some(index)) = (presentation, transition_insert_at) {
            // Mask the composed parent and its children once, rather than
            // independently fading overlapping child layers.
            let mut family = self.draw(stage_width, stage_height, transition_commands)?;
            self.mask_presentation_frame(&mut family, presentation, archive)?;
            commands.insert(
                index,
                SceneCommand::Texture {
                    id: format!("cmvs.presentation.{}", presentation.parent),
                    frame: family,
                    destination: RectI {
                        x: 0,
                        y: 0,
                        width: stage_width,
                        height: stage_height,
                    },
                    opacity: 1.0,
                    blend: BlendMode::Alpha,
                },
            );
        }
        if let Some(movie) = movie {
            // The native frame composes the movie after the channel layer
            // Native 412800 creates a decoded-size quad at the origin.
            append_movie(&mut commands, vm, movie, stage_width)?;
        }
        self.draw_final(vm, stage_width, stage_height, commands)
            .map(Some)
    }

    pub(crate) fn native_pointer(
        &self,
        vm: &CmvsPs2aVmState,
        point: [f32; 2],
    ) -> Result<[i32; 2], CoreError> {
        let Some(viewport) = native_viewport::NativeViewport::from_vm(vm)? else {
            return Ok(point.map(|value| value as i32));
        };
        let base = self.logical.unwrap_or(Extent2D::new(
            vm.native_stage_size[0],
            vm.native_stage_size[1],
        ));
        let raster = self.raster.unwrap_or(viewport.extent());
        let host = StageCanvas::new(base, raster)?;
        let native = StageCanvas::new(viewport.extent(), raster)?;
        let apply = |transform: roukaku_media_core::Transform2D, [x, y]: [f32; 2]| {
            [
                transform.m11 * x + transform.m21 * y + transform.tx,
                transform.m12 * x + transform.m22 * y + transform.ty,
            ]
        };
        let raster_point = apply(host.logical_to_raster_transform(), point);
        Ok(
            viewport
                .inverse_view_pointer(apply(native.raster_to_logical_transform(), raster_point)),
        )
    }

    fn draw_final(
        &mut self,
        vm: &CmvsPs2aVmState,
        width: u32,
        height: u32,
        commands: Vec<SceneCommand>,
    ) -> Result<TextureFrame, CoreError> {
        let viewport = native_viewport::NativeViewport::from_vm(vm)?;
        self.draw_mapped(width, height, commands, viewport)
    }

    fn draw(
        &mut self,
        width: u32,
        height: u32,
        commands: Vec<SceneCommand>,
    ) -> Result<TextureFrame, CoreError> {
        self.draw_mapped(width, height, commands, None)
    }

    fn draw_mapped(
        &mut self,
        width: u32,
        height: u32,
        mut commands: Vec<SceneCommand>,
        viewport: Option<native_viewport::NativeViewport>,
    ) -> Result<TextureFrame, CoreError> {
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_FRAME_SEQUENCE"))?;
        let logical = Extent2D::new(width, height);
        let output_extent = viewport.map_or(logical, |viewport| viewport.extent());
        let canvas = StageCanvas::new(output_extent, self.raster.unwrap_or(output_extent))?;
        self.texture_residency
            .prepare(&mut commands)
            .map_err(|cause| {
                CoreError::invalid("SHINKI_EMU_CMVS_TEXTURE_RESIDENCY", cause.to_string())
            })?;
        let mut mapped = Vec::with_capacity(commands.len() + self.reload_releases.len() + 8);
        mapped.extend(
            self.reload_releases
                .drain(..)
                .map(|resource_id| SceneCommand::ReleaseResource { resource_id }),
        );
        mapped.push(SceneCommand::PushClip {
            rect: canvas.viewport_rect()?,
        });
        let canvas_transform = canvas.logical_to_raster_transform();
        let mut flat_camera = canvas_transform;
        // Camera is applied AFTER the command-local transform stack.
        mapped.push(SceneCommand::SetCamera {
            transform: canvas_transform,
        });
        if let Some(viewport) = viewport {
            mapped.push(SceneCommand::PushClip {
                rect: viewport.clip_rect(),
            });
            let native = viewport.stage_to_view_transform();
            let canvas = canvas_transform;
            flat_camera = roukaku_media_core::Transform2D {
                m11: canvas.m11 * native.m11 + canvas.m21 * native.m12,
                m12: canvas.m12 * native.m11 + canvas.m22 * native.m12,
                m21: canvas.m11 * native.m21 + canvas.m21 * native.m22,
                m22: canvas.m12 * native.m21 + canvas.m22 * native.m22,
                tx: canvas.m11 * native.tx + canvas.m21 * native.ty + canvas.tx,
                ty: canvas.m12 * native.tx + canvas.m22 * native.ty + canvas.ty,
            };
            mapped.push(SceneCommand::SetCamera {
                transform: flat_camera,
            });
        }
        // Mesh projection already includes the D3D viewport origin and size;
        // only flat 42C330 vertices receive the extra native origin/scale.
        let mut projective_camera = false;
        for command in commands {
            let projective = matches!(command, SceneCommand::ProjectiveMesh2D { .. });
            if projective != projective_camera {
                mapped.push(SceneCommand::SetCamera {
                    transform: if projective {
                        canvas_transform
                    } else {
                        flat_camera
                    },
                });
                projective_camera = projective;
            }
            mapped.push(command);
        }
        if viewport.is_some() {
            mapped.push(SceneCommand::PopClip);
        }
        mapped.push(SceneCommand::SetCamera {
            transform: roukaku_media_core::Transform2D::IDENTITY,
        });
        mapped.push(SceneCommand::PopClip);
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
        self.logical = Some(logical);
        self.canvas = Some(canvas);
        Ok(TextureFrame {
            width: raster.width,
            height: raster.height,
            rgba8: output.rgba8.into(),
        })
    }
}

fn surface_geometry(
    vm: &CmvsPs2aVmState,
    slot: CmvsTextureSlot,
    stage_width: u32,
    textured: bool,
) -> Result<Option<basic_geometry::BasicGeometry>, CoreError> {
    let Some(geometry) = surface::geometry(vm, slot, stage_width, textured) else {
        return Ok(None);
    };
    let root = slot.root(vm).expect("drawable image has a live root");
    //4454D0 reads the parent CImage header for every child, even detached ones.
    let offset = if root.screen_shake_disabled {
        [0.0; 2]
    } else {
        crate::vm::screen_shake::logical_offset(vm, stage_width, 0)?
    };
    Ok(Some(geometry.with_offset(offset)))
}

type SlotState = (bool, Option<[u32; 4]>, Option<[u32; 2]>, Option<u32>);
fn slot_state(vm: &CmvsPs2aVmState, slot: CmvsTextureSlot) -> Option<SlotState> {
    if slot.is_root() {
        slot.root(vm).map(|s| {
            (
                s.surface_initialized
                    && !s.hidden
                    && (!matches!(s.basic_kind, 2 | 3) || surface::root_queue_depth(vm, s) > 0.0),
                s.rect_words,
                s.position_words,
                s.auxiliary_word,
            )
        })
    } else {
        let root = slot.root(vm)?;
        slot.child(vm).map(|s| {
            let matching_indexed_kind =
                slot.original_child().is_none() || s.basic_kind == root.basic_kind;
            let nonnegative_order =
                !matches!(s.basic_kind, 0 | 1 | 5) || s.auxiliary_word.unwrap_or(0) as i32 >= 0;
            (
                s.surface_initialized
                    && !s.hidden
                    && !root.hidden
                    && matching_indexed_kind
                    && nonnegative_order,
                s.rect_words,
                s.position_words,
                s.auxiliary_word,
            )
        })
    }
}
fn slot_id(slot: CmvsTextureSlot) -> String {
    match slot {
        CmvsTextureSlot::Parent(p) => format!("cmvs.texture.parent.{p}"),
        CmvsTextureSlot::Child { parent, child } => format!("cmvs.texture.child.{parent}.{child}"),
        CmvsTextureSlot::SceneRoot(slot) => format!("cmvs.texture.scene.{slot}"),
        CmvsTextureSlot::SceneChild { slot, child } => format!("cmvs.texture.scene.{slot}.{child}"),
    }
}

#[cfg(test)]
fn stage_geometry(
    vm: &CmvsPs2aVmState,
    slots: impl Iterator<Item = CmvsTextureSlot>,
    requested: CmvsTextureSlot,
    requested_present: bool,
    geometry_cache: &mut BTreeMap<CmvsTextureSlot, (RectI, u32)>,
) -> Result<Vec<(CmvsTextureSlot, RectI, u32)>, CoreError> {
    let mut geometry = stage_geometry_with(
        vm,
        slots,
        requested,
        requested_present,
        geometry_cache,
        slot_state,
    )?;
    geometry.sort_by_key(|(slot, _, _)| surface::native_queue_position(vm, *slot));
    Ok(geometry)
}

fn stage_geometry_with(
    vm: &CmvsPs2aVmState,
    slots: impl Iterator<Item = CmvsTextureSlot>,
    requested: CmvsTextureSlot,
    requested_present: bool,
    geometry_cache: &mut BTreeMap<CmvsTextureSlot, (RectI, u32)>,
    state: impl Fn(&CmvsPs2aVmState, CmvsTextureSlot) -> Option<SlotState>,
) -> Result<Vec<(CmvsTextureSlot, RectI, u32)>, CoreError> {
    let mut geometry = Vec::new();
    append_stage_geometry_with(
        vm,
        slots,
        (requested, requested_present),
        geometry_cache,
        state,
        8192,
        &mut geometry,
    )?;
    Ok(geometry)
}

fn append_stage_geometry_with(
    vm: &CmvsPs2aVmState,
    slots: impl Iterator<Item = CmvsTextureSlot>,
    request: (CmvsTextureSlot, bool),
    geometry_cache: &mut BTreeMap<CmvsTextureSlot, (RectI, u32)>,
    state: impl Fn(&CmvsPs2aVmState, CmvsTextureSlot) -> Option<SlotState>,
    limit: usize,
    geometry: &mut Vec<(CmvsTextureSlot, RectI, u32)>,
) -> Result<(), CoreError> {
    let (requested, requested_present) = request;
    let mut queue_full = false;
    let mut requested_degenerate = false;
    for slot in slots {
        if geometry.len() >= limit {
            queue_full = true;
            break;
        }
        if slot.original_child().is_some()
            && matches!(state(vm, slot.root_slot()), Some((false, _, _, _)))
        {
            if slot == requested {
                requested_degenerate = true;
            }
            continue;
        }
        let placement = match (slot, state(vm, slot)) {
            (_, Some((false, _, _, _))) => {
                if slot == requested {
                    requested_degenerate = true;
                }
                continue;
            }
            (_, Some((true, rect, _pivot, z))) => {
                let rect = rect.unwrap_or([0, 0, 0, 0]);
                let x = rect[0] as i16 as i32;
                let y = rect[1] as i16 as i32;
                let width = rect[2] as i16;
                let height = rect[3] as i16;
                // Queue insertion depends on the live basic, not on its
                // painted pixels or a nondegenerate source rectangle.
                let [width, height] = [width.max(0) as u32, height.max(0) as u32];
                let placement = (
                    RectI {
                        x,
                        y,
                        width,
                        height,
                    },
                    match slot {
                        CmvsTextureSlot::Child { parent, .. } => {
                            state(vm, CmvsTextureSlot::Parent(parent))
                                .and_then(|(_, _, _, order)| order)
                                .unwrap_or(0)
                                .wrapping_add(z.unwrap_or(0))
                        }
                        _ => z.unwrap_or(0),
                    },
                );
                geometry_cache.insert(slot, placement);
                placement
            }
            _ => continue,
        };
        geometry.push((slot, placement.0, placement.1));
    }
    if requested_present
        && !queue_full
        && !requested_degenerate
        && !geometry.iter().any(|(slot, _, _)| *slot == requested)
    {
        return Err(error("SHINKI_EMU_CMVS_TEXTURE_PRESENTATION_STATE"));
    }
    Ok(())
}
fn error(code: &'static str) -> CoreError {
    CoreError::invalid(code, "CMVS scene operation failed")
}

fn append_movie(
    commands: &mut Vec<SceneCommand>,
    vm: &CmvsPs2aVmState,
    frame: &TextureFrame,
    stage_width: u32,
) -> Result<(), CoreError> {
    let [x, y] = crate::vm::screen_shake::logical_offset(vm, stage_width, 0)?;
    let shifted = x != 0.0 || y != 0.0;
    if shifted {
        commands.push(SceneCommand::PushTransform {
            transform: roukaku_media_core::Transform2D {
                tx: x,
                ty: y,
                ..roukaku_media_core::Transform2D::IDENTITY
            },
        });
    }
    commands.push(SceneCommand::Texture {
        id: "cmvs.movie".into(),
        frame: frame.clone(),
        destination: RectI::new(0, 0, frame.width, frame.height),
        opacity: 1.0,
        blend: BlendMode::Alpha,
    });
    if shifted {
        commands.push(SceneCommand::PopTransform);
    }
    Ok(())
}
