use super::*;
use crate::vm::texture_reload::{ReloadMode, TextureReload};

pub(super) struct ReloadSurfaces {
    previous: BTreeMap<CmvsTextureSlot, TextureFrame>,
    overlap: Option<TextureFrame>,
    fast: Option<FastUpload>,
    uploads: BTreeMap<ReloadLayer, UploadedImage>,
    geometry: BTreeMap<CmvsTextureSlot, (RectI, u32)>,
}

struct FastUpload {
    previous_id: String,
    current_id: String,
    previous: TextureFrame,
    current: TextureFrame,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ReloadLayer {
    Overlap,
    Previous(CmvsTextureSlot),
    Current,
}

struct UploadedImage {
    id: String,
    frame: TextureFrame,
}

impl ReloadSurfaces {
    fn take_uploads(&mut self, releases: &mut Vec<String>) {
        if let Some(upload) = self.fast.take() {
            releases.push(upload.previous_id);
            releases.push(upload.current_id);
        }
        for (_, upload) in std::mem::take(&mut self.uploads) {
            releases.push(upload.id);
        }
    }
}

impl CmvsScene {
    pub(crate) fn bind_retained_cmv_frame(
        &mut self,
        vm: &CmvsPs2aVmState,
        parent: CmvsTextureSlot,
        mut frame: TextureFrame,
    ) -> Result<(), CoreError> {
        let reload = parent
            .reload(vm)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_RELOAD_OWNER"))?;
        let surfaces = self
            .reload_surfaces
            .get_mut(&parent)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_RELOAD_SOURCE"))?;
        if let Some(parameters) = reload.previous_parent.color_filter {
            image_filter::apply(&mut frame, parameters)?;
        }
        surfaces.previous.insert(parent, frame);
        Ok(())
    }

    pub(crate) fn load_image_reload(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        parent: u8,
        resource: String,
        bank: crate::CmvsResourceSearchBank,
        result_field_offset: u32,
        archive: &CmvsArchive,
    ) -> Result<bool, CoreError> {
        self.begin_image_reload(vm, parent, archive)?;
        let image = vm
            .texture_parents
            .get_mut(&parent)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_RELOAD_OWNER"))?;
        image.resource = Some(resource.clone());
        image.resource_frame = Some(vm.current_frame);
        image.image_info = [0; 3];
        let loaded = archive
            .resolve_native_resource(vm, bank, &resource)
            .and_then(|uri| self.complete_image_reload(vm, parent, resource, &uri, archive));
        let succeeded = match loaded {
            Ok(()) => true,
            Err(cause) => {
                self.fail_image_reload(vm, parent);
                tracing::info!(
                    event = "cmvs.session.image_reload_failed",
                    parent,
                    code = cause.code(),
                    tick = vm.tick
                );
                false
            }
        };
        vm.write_interpreter_word(result_field_offset, u32::from(succeeded));
        Ok(succeeded)
    }

    pub(crate) fn synchronize_image_reloads(&mut self, vm: &CmvsPs2aVmState) {
        let releases = &mut self.reload_releases;
        self.reload_surfaces.retain(|parent, surfaces| {
            if parent.reload(vm).is_some() {
                return true;
            }
            surfaces.take_uploads(releases);
            false
        });
    }

    pub(crate) fn restore_image_reload_bindings(&mut self, vm: &CmvsPs2aVmState) {
        let owners = vm
            .texture_reloads
            .iter()
            .map(|(&parent, reload)| (CmvsTextureSlot::Parent(parent), reload))
            .chain(
                vm.native_scene_workers
                    .iter()
                    .filter_map(|(&slot, worker)| {
                        worker
                            .image_shards()?
                            .reload
                            .as_ref()
                            .map(|reload| (CmvsTextureSlot::SceneRoot(slot), reload))
                    }),
            );
        for (slot, reload) in owners {
            if let Some(uri) = reload.current_uri.as_ref().filter(|_| reload.busy())
                && let std::collections::btree_map::Entry::Vacant(e) = self.bindings.entry(slot)
            {
                e.insert(uri.clone());
                self.filtered_images.remove(&slot);
            }
        }
    }
    /// Detach the old family before looking up the new resource. A failed
    /// load owns the old objects but does not restore their table identity.
    pub(crate) fn begin_image_reload(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        parent: u8,
        archive: &CmvsArchive,
    ) -> Result<(), CoreError> {
        let reload = vm
            .texture_reloads
            .get_mut(&parent)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_RELOAD_OWNER"))?;
        let mut previous = BTreeMap::new();
        let bindings: Vec<_> = self
            .bindings
            .iter()
            .filter_map(|(slot, uri)| {
                let retained = match *slot {
                    CmvsTextureSlot::Parent(value) => value == parent,
                    CmvsTextureSlot::Child {
                        parent: value,
                        child,
                    } => value == parent && reload.previous_children.contains_key(&child),
                    CmvsTextureSlot::SceneRoot(_) | CmvsTextureSlot::SceneChild { .. } => false,
                };
                retained.then(|| (*slot, uri.clone()))
            })
            .collect();
        for (slot, uri) in bindings {
            let mut frame = match self.dynamic_images.get(&slot) {
                Some(frame) => frame.clone(),
                None => self.cached_texture(&uri, archive)?,
            };
            if matches!(slot, CmvsTextureSlot::Parent(_))
                && let Some(parameters) = reload.previous_parent.color_filter
            {
                image_filter::apply(&mut frame, parameters)?;
            }
            match slot {
                CmvsTextureSlot::Parent(_) => reload.previous_uri = Some(uri),
                CmvsTextureSlot::Child { child, .. } => {
                    reload.previous_child_uris.insert(child, uri);
                }
                CmvsTextureSlot::SceneRoot(_) | CmvsTextureSlot::SceneChild { .. } => {
                    unreachable!()
                }
            }
            previous.insert(slot, frame);
        }
        self.release_parent_surface(parent);
        self.reload_surfaces.insert(
            CmvsTextureSlot::Parent(parent),
            ReloadSurfaces {
                previous,
                overlap: None,
                fast: None,
                uploads: BTreeMap::new(),
                geometry: BTreeMap::new(),
            },
        );
        Ok(())
    }

    pub(crate) fn fail_image_reload(&mut self, vm: &mut CmvsPs2aVmState, parent: u8) {
        if let Some(reload) = vm.texture_reloads.get_mut(&parent) {
            reload.mode = ReloadMode::Failed;
        }
    }

    pub(crate) fn release_image_reload(&mut self, parent: u8) {
        self.release_image_reload_owner(CmvsTextureSlot::Parent(parent));
    }

    pub(crate) fn release_image_reload_owner(&mut self, owner: CmvsTextureSlot) {
        if let Some(mut surfaces) = self.reload_surfaces.remove(&owner) {
            surfaces.take_uploads(&mut self.reload_releases);
        }
    }

    pub(super) fn clear_parent_image_reloads(&mut self) {
        self.reload_surfaces.retain(|owner, surfaces| {
            if owner.scene_worker().is_some() {
                return true;
            }
            surfaces.take_uploads(&mut self.reload_releases);
            false
        });
    }

    pub(crate) fn move_parent_surface(&mut self, source: u8, destination: CmvsTextureSlot) {
        let source_owner = CmvsTextureSlot::Parent(source);
        if source_owner == destination {
            return;
        }
        let remap = |slot: CmvsTextureSlot| slot.relocate(source, destination);
        let moved = self.reload_surfaces.remove(&source_owner);
        self.release_owner_surface(destination);
        let slots: Vec<_> = self
            .bindings
            .keys()
            .copied()
            .filter(|slot| slot.root_slot() == source_owner)
            .collect();
        for slot in slots {
            if let Some(uri) = self.bindings.remove(&slot) {
                self.bindings.insert(remap(slot), uri);
            }
            if let Some(frame) = self.dynamic_images.remove(&slot) {
                self.dynamic_images.insert(remap(slot), frame);
            }
            if let Some(geometry) = self.slot_geometry.remove(&slot) {
                self.slot_geometry.insert(remap(slot), geometry);
            }
        }
        let uploaded_slots: Vec<_> = self
            .surface_uploads
            .keys()
            .copied()
            .filter(|slot| slot.root_slot() == source_owner)
            .collect();
        for slot in uploaded_slots {
            if let Some(upload) = self.surface_uploads.remove(&slot) {
                self.surface_uploads.insert(remap(slot), upload);
            }
        }
        self.filtered_images.remove(&source_owner);
        if let Some(mut surfaces) = moved {
            surfaces.previous = surfaces
                .previous
                .into_iter()
                .map(|(slot, frame)| (remap(slot), frame))
                .collect();
            surfaces.geometry = surfaces
                .geometry
                .into_iter()
                .map(|(slot, geometry)| (remap(slot), geometry))
                .collect();
            surfaces.uploads = surfaces
                .uploads
                .into_iter()
                .map(|(key, upload)| {
                    let key = match key {
                        ReloadLayer::Previous(slot) => ReloadLayer::Previous(remap(slot)),
                        key => key,
                    };
                    (key, upload)
                })
                .collect();
            self.reload_surfaces.insert(destination, surfaces);
        }
    }

    pub(crate) fn complete_image_reload(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        parent: u8,
        resource: String,
        uri: &str,
        archive: &CmvsArchive,
    ) -> Result<(), CoreError> {
        // bind performs a real decode even if the URI already resides in cache.
        self.bind(CmvsTextureSlot::Parent(parent), uri, archive)?;
        let frame = self.cached_texture(uri, archive)?;
        let reload = vm
            .texture_reloads
            .get_mut(&parent)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_RELOAD_OWNER"))?;
        reload.current_uri = Some(uri.to_owned());
        let previous = self
            .reload_surfaces
            .get(&CmvsTextureSlot::Parent(parent))
            .and_then(|surfaces| surfaces.previous.get(&CmvsTextureSlot::Parent(parent)));
        let mut image = if reload.previous_parent.surface_initialized {
            reload.previous_parent.clone()
        } else {
            crate::CmvsTextureParentState::default()
        };
        image.resource = Some(resource);
        image.resource_frame = Some(vm.current_frame);
        image.image_info = archive.native_image_info(uri)?;
        image.hidden = false;
        image.color_filter = reload.previous_parent.color_filter;
        if let Some(previous) = previous
            && (previous.width != frame.width || previous.height != frame.height)
        {
            image.rect_words = Some([0, 0, frame.width, frame.height]);
        }
        vm.texture_parents.insert(parent, image);
        if !reload.previous_parent.surface_initialized {
            // 444400 returns success without arming either transition flag.
            reload.mode = ReloadMode::Failed;
        } else if reload.duration_ms == 0 {
            vm.texture_reloads.remove(&parent);
            self.release_image_reload(parent);
        } else {
            let same_size = previous.is_some_and(|previous| {
                previous.width == frame.width && previous.height == frame.height
            });
            reload.mode = if same_size {
                ReloadMode::Fast
            } else {
                ReloadMode::Composite
            };
            if reload.mode == ReloadMode::Fast {
                // 444400 detaches only the old image provider, then frees
                // its old root and every child before arming flag 3101.
                reload.previous_children.clear();
                reload.previous_child_uris.clear();
                self.reload_surfaces
                    .get_mut(&CmvsTextureSlot::Parent(parent))
                    .unwrap()
                    .previous
                    .retain(|slot, _| matches!(slot, CmvsTextureSlot::Parent(_)));
            }
        }
        Ok(())
    }

    fn restore_reload_surfaces(
        &mut self,
        parent: CmvsTextureSlot,
        reload: &TextureReload,
        archive: &CmvsArchive,
    ) -> Result<(), CoreError> {
        if self.reload_surfaces.contains_key(&parent) {
            return Ok(());
        }
        let mut previous = BTreeMap::new();
        if let Some(uri) = &reload.previous_uri {
            let mut frame = self.cached_texture(uri, archive)?;
            if let Some(parameters) = reload.previous_parent.color_filter {
                image_filter::apply(&mut frame, parameters)?;
            }
            previous.insert(parent, frame);
        }
        for (child, uri) in &reload.previous_child_uris {
            previous.insert(
                parent.child_slot(*child),
                self.cached_texture(uri, archive)?,
            );
        }
        self.reload_surfaces.insert(
            parent,
            ReloadSurfaces {
                previous,
                overlap: None,
                fast: None,
                uploads: BTreeMap::new(),
                geometry: BTreeMap::new(),
            },
        );
        Ok(())
    }

    pub(super) fn reload_commands(
        &mut self,
        vm: &CmvsPs2aVmState,
        parent: CmvsTextureSlot,
        reload: &TextureReload,
        source: RectI,
        archive: &CmvsArchive,
        stage: (u32, u32),
    ) -> Result<Vec<SceneCommand>, CoreError> {
        self.restore_reload_surfaces(parent, reload, archive)?;
        let uri = self.bindings[&parent].clone();
        let mut current = self.cached_texture(&uri, archive)?;
        if let Some(parameters) = parent.root(vm).and_then(|image| image.color_filter) {
            current = image_filter::frame(
                &mut self.textures,
                &mut self.filtered_images,
                parent,
                &uri,
                parameters,
                current,
            )?;
        }
        let surfaces = self.reload_surfaces.get_mut(&parent).unwrap();
        if reload.mode == ReloadMode::Fast {
            let previous = surfaces
                .previous
                .get(&parent)
                .ok_or_else(|| error("SHINKI_EMU_CMVS_RELOAD_SOURCE"))?;
            let changed = surfaces.fast.as_ref().is_none_or(|upload| {
                upload.previous.rgba8.allocation_ptr() != previous.rgba8.allocation_ptr()
                    || upload.current.rgba8.allocation_ptr() != current.rgba8.allocation_ptr()
            });
            let mut commands = Vec::with_capacity(if changed { 3 } else { 1 });
            if changed {
                if let Some(upload) = surfaces.fast.take() {
                    self.reload_releases.push(upload.previous_id);
                    self.reload_releases.push(upload.current_id);
                }
                self.reload_generation = self
                    .reload_generation
                    .checked_add(1)
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_RELOAD_GENERATION"))?;
                let previous_id = format!("cmvs.reload.fast.{}.previous", self.reload_generation);
                let current_id = format!("cmvs.reload.fast.{}.current", self.reload_generation);
                commands.push(SceneCommand::UploadTexture {
                    resource_id: previous_id.clone(),
                    frame: previous.clone(),
                });
                commands.push(SceneCommand::UploadTexture {
                    resource_id: current_id.clone(),
                    frame: current.clone(),
                });
                surfaces.fast = Some(FastUpload {
                    previous_id,
                    current_id,
                    previous: previous.clone(),
                    current,
                });
            }
            let upload = surfaces.fast.as_ref().unwrap();
            let source = clipped_source(source, upload.current.width, upload.current.height);
            if let Some(source) = source {
                let command = SceneCommand::CrossfadeSprite {
                    id: format!("cmvs.reload.fast.{parent:?}"),
                    previous_texture_id: upload.previous_id.clone(),
                    current_texture_id: upload.current_id.clone(),
                    source: Some(source),
                    destination: surface::destination(vm, parent, source),
                    progress: reload.elapsed_ms as f32 / reload.duration_ms as f32,
                    opacity: surface::opacity(vm, parent),
                };
                if let Some(geometry) = surface_geometry(vm, parent, stage.0, true)? {
                    geometry.append(&mut commands, command, None)?;
                }
            }
            return Ok(commands);
        }
        let phase = reload.alpha_word().min(256);
        let mut geometry = stage_geometry_with(
            vm,
            surfaces.previous.keys().copied(),
            parent,
            surfaces.previous.contains_key(&parent),
            &mut surfaces.geometry,
            |_, slot| {
                if slot.is_root() {
                    let image = &reload.previous_parent;
                    Some((
                        image.surface_initialized && !image.hidden,
                        image.rect_words,
                        parent.root(vm).and_then(|image| image.position_words),
                        image.auxiliary_word,
                    ))
                } else {
                    reload
                        .previous_children
                        .get(&slot.original_child()?)
                        .map(|image| {
                            (
                                image.surface_initialized && !image.hidden,
                                image.rect_words,
                                image.position_words,
                                image.auxiliary_word,
                            )
                        })
                }
            },
        )?;
        // The old subtree is detached from the live VM during replacement.
        // Traverse its root first, then its own descending child list.
        geometry.sort_by(|(left, _, _), (right, _, _)| {
            let key = |slot: &CmvsTextureSlot| {
                if let Some(child) = slot.original_child() {
                    let image = &reload.previous_children[&child];
                    let depth = if matches!(image.basic_kind, 2 | 3) {
                        f32::from_bits(image.float_position_words[2])
                    } else {
                        image.auxiliary_word.unwrap_or(0) as i32 as f32
                    };
                    (1, depth)
                } else {
                    (0, 0.0)
                }
            };
            let left = key(left);
            let right = key(right);
            left.0
                .cmp(&right.0)
                .then_with(|| right.1.total_cmp(&left.1))
        });
        let mut commands = Vec::with_capacity((geometry.len() + 2) * 2);
        if surfaces.overlap.is_none()
            && let Some(previous) = surfaces.previous.get(&parent)
        {
            surfaces.overlap = Some(opaque_overlap(previous, &current));
        }
        let overlap_context = surfaces
            .overlap
            .as_ref()
            .map(|frame| overlap_basic(&reload.previous_parent, [frame.width, frame.height]));
        let uploads = &mut surfaces.uploads;
        let releases = &mut self.reload_releases;
        let generation = &mut self.reload_generation;
        let mut append = |key: ReloadLayer,
                          frame: &TextureFrame,
                          source: RectI,
                          opacity: f32|
         -> Result<(), CoreError> {
            let Some(source) = clipped_source(source, frame.width, frame.height) else {
                return Ok(());
            };
            let (root, child) = match key {
                ReloadLayer::Previous(slot) => (
                    &reload.previous_parent,
                    slot.original_child()
                        .and_then(|child| reload.previous_children.get(&child)),
                ),
                ReloadLayer::Overlap => (overlap_context.as_ref().unwrap(), None),
                ReloadLayer::Current => (parent.root(vm).expect("live reload root"), None),
            };
            let Some(geometry) = surface::geometry_with_surfaces(vm, root, child, stage.0, true)
            else {
                return Ok(());
            };
            let shake = if root.screen_shake_disabled {
                [0.0; 2]
            } else {
                crate::vm::screen_shake::logical_offset(vm, stage.0, 0)?
            };
            let geometry = geometry.with_offset(shake);
            let changed = uploads.get(&key).is_none_or(|upload| {
                upload.frame.rgba8.allocation_ptr() != frame.rgba8.allocation_ptr()
                    || upload.frame.width != frame.width
                    || upload.frame.height != frame.height
            });
            if changed {
                if let Some(upload) = uploads.remove(&key) {
                    releases.push(upload.id);
                }
                *generation = generation
                    .checked_add(1)
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_RELOAD_GENERATION"))?;
                let id = format!("cmvs.reload.layer.{generation}");
                commands.push(SceneCommand::UploadTexture {
                    resource_id: id.clone(),
                    frame: frame.clone(),
                });
                uploads.insert(
                    key,
                    UploadedImage {
                        id,
                        frame: frame.clone(),
                    },
                );
            }
            let id = uploads[&key].id.clone();
            geometry.append_sprite(
                &mut commands,
                format!("{id}.draw"),
                id,
                source,
                [frame.width, frame.height],
                opacity,
                BlendMode::Alpha,
            );
            Ok(())
        };
        if let (Some(overlap), Some(context)) = (&surfaces.overlap, &overlap_context) {
            let [x, y, width, height] = context.rect_words.unwrap_or([0; 4]);
            if width as i16 > 0 && height as i16 > 0 {
                append(
                    ReloadLayer::Overlap,
                    overlap,
                    RectI::new(
                        x as i16 as i32,
                        y as i16 as i32,
                        width as i16 as u32,
                        height as i16 as u32,
                    ),
                    surface::opacity_with_surfaces(vm, context, None, 256),
                )?;
            }
        }
        for (slot, previous_source, _) in geometry {
            let child = slot
                .original_child()
                .and_then(|child| reload.previous_children.get(&child));
            append(
                ReloadLayer::Previous(slot),
                &surfaces.previous[&slot],
                previous_source,
                surface::opacity_with_surfaces(vm, &reload.previous_parent, child, 256 - phase),
            )?;
        }
        append(
            ReloadLayer::Current,
            &current,
            source,
            surface::opacity_with_surfaces(
                vm,
                parent.root(vm).expect("live reload root"),
                None,
                phase,
            ),
        )?;
        Ok(commands)
    }
}

/// 444400 copies the retained basic block, not its image provider/filter.
pub(super) fn overlap_basic(
    previous: &crate::CmvsTextureParentState,
    size: [u32; 2],
) -> crate::CmvsTextureParentState {
    let mut basic = crate::CmvsTextureParentState {
        image_info: [size[0], size[1], 1],
        surface_initialized: true,
        alpha: previous.alpha,
        scale_words: previous.scale_words,
        basic_kind: previous.basic_kind,
        blend_mode: previous.blend_mode,
        float_position_words: previous.float_position_words,
        projection_depth_word: previous.projection_depth_word,
        projection_height_word: previous.projection_height_word,
        rotation_hundredths: previous.rotation_hundredths,
        reference_size: previous.reference_size,
        rect_words: previous.rect_words,
        position_words: previous.position_words,
        auxiliary_pair_words: previous.auxiliary_pair_words,
        auxiliary_word: previous.auxiliary_word,
        ..Default::default()
    };
    if size != [previous.image_info[0], previous.image_info[1]] {
        basic.rect_words = Some([0, 0, size[0], size[1]]);
        let mut pivot = previous.position_words.unwrap_or([0; 2]);
        if pivot[0] != 0 {
            pivot[0] = size[0] / 2;
        }
        basic.position_words = Some(pivot);
    }
    basic
}

/// 43BCF0 intersects the dimensions, horizontally centers both inputs and
/// retains pixels only where both original alpha bytes are exactly 255.
pub(super) fn opaque_overlap(previous: &TextureFrame, current: &TextureFrame) -> TextureFrame {
    let width = previous.width.min(current.width);
    let height = previous.height.min(current.height);
    let previous_x = (previous.width - width) / 2;
    let current_x = (current.width - width) / 2;
    let mut bytes = vec![0; width as usize * height as usize * 4];
    for y in 0..height {
        for x in 0..width {
            let old = ((y * previous.width + previous_x + x) * 4) as usize;
            let new = ((y * current.width + current_x + x) * 4) as usize;
            if previous.rgba8[old + 3] == 255 && current.rgba8[new + 3] == 255 {
                let target = ((y * width + x) * 4) as usize;
                bytes[target..target + 4].copy_from_slice(&current.rgba8[new..new + 4]);
            }
        }
    }
    TextureFrame {
        width,
        height,
        rgba8: bytes.into(),
    }
}

pub(super) fn clipped_source(source: RectI, width: u32, height: u32) -> Option<RectI> {
    let x = source.x.max(0) as u32;
    let y = source.y.max(0) as u32;
    if x >= width || y >= height {
        return None;
    }
    Some(RectI {
        x: x as i32,
        y: y as i32,
        width: source.width.min(width - x),
        height: source.height.min(height - y),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_centers_horizontally_copies_new_color_and_requires_both_opaque() {
        let previous = TextureFrame {
            width: 4,
            height: 1,
            rgba8: vec![1, 1, 1, 255, 2, 2, 2, 255, 3, 3, 3, 254, 4, 4, 4, 255].into(),
        };
        let current = TextureFrame {
            width: 2,
            height: 1,
            rgba8: vec![20, 30, 40, 255, 50, 60, 70, 255].into(),
        };
        let overlap = opaque_overlap(&previous, &current);
        assert_eq!((overlap.width, overlap.height), (2, 1));
        assert_eq!(overlap.rgba8.as_slice(), &[20, 30, 40, 255, 0, 0, 0, 0]);
    }
}
