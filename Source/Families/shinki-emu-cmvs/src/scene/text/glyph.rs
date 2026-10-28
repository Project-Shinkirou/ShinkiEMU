use super::super::surface::{NativeDrawGroup, NativeDrawGroups, NativeQueuePosition};
use super::*;
use crate::{
    CmvsNativeGlyphGraph, CmvsNativeGlyphRecord, CmvsNativeGlyphResident, CmvsTextureChildState,
};

pub(super) struct AtlasUpload {
    identity: u64,
    provider_generation: u64,
    frame: TextureFrame,
    resource: Option<String>,
    dirty: bool,
    initialized: bool,
}

impl NativeTextRenderer {
    pub(in crate::scene) fn bind_glyph_owner(
        &mut self,
        vm: &CmvsPs2aVmState,
        slot: u8,
    ) -> Result<(), CoreError> {
        let owner = vm
            .native_font_owners
            .get(&(783 + u16::from(slot)))
            .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_OBJECT"))?;
        self.bind_font(&owner.family)
    }

    pub(in crate::scene) fn append_glyph(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        slot: u8,
        source: &[u8],
    ) -> Result<(), CoreError> {
        let proportional = vm.native_glyph_raster_settings(slot)?.proportional;
        let mut graph = vm
            .native_glyphs
            .remove(&slot)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_OBJECT"))?;
        let result = crate::vm::native_glyph::layout::append(
            &mut graph,
            source,
            proportional,
            |code, size| {
                // 43B360 queries 461FB0 even in fixed-width layout mode.
                let raster = vm.native_glyph_raster(slot, code, size)?;
                let owner = vm
                    .native_font_owners
                    .get(&(783 + u16::from(slot)))
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_OBJECT"))?;
                self.raster.advance(&owner.family, code, size, raster)
            },
        );
        vm.native_glyphs.insert(slot, graph);
        result
    }

    pub(in crate::scene) fn prepare_draw_groups(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        stage_width: u32,
        textures: &mut TextureCache,
    ) -> Result<NativeDrawGroups, CoreError> {
        let mut glyph_groups = Vec::new();
        self.glyph_groups(vm, stage_width, textures, &mut glyph_groups)?;
        let mut owners = Vec::new();
        let text = self.frame_with_owners(vm, stage_width, &mut owners)?;
        let mut output = NativeDrawGroups::default();
        collect_text_groups(vm, stage_width, text, owners, &mut output)?;
        output.resources.append(&mut self.glyph_pending);
        output.groups.append(&mut glyph_groups);
        Ok(output)
    }

    fn glyph_groups(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        stage_width: u32,
        textures: &mut TextureCache,
        groups: &mut Vec<NativeDrawGroup>,
    ) -> Result<(), CoreError> {
        let now = crate::vm::native_glyph::clock(vm);
        for slot in 0..8 {
            let Some(mut graph) = vm.native_glyphs.remove(&slot) else {
                continue;
            };
            let result = (|| {
                let was_moving = graph.motion.is_some();
                crate::vm::native_glyph::motion::advance(&mut graph, now)?;
                if was_moving {
                    crate::vm::native_glyph::update_secondary(
                        vm,
                        graph.secondary_reference,
                        graph.position,
                        true,
                    )?;
                }
                let recreate = self.glyph_atlases.get(&slot).is_none_or(|atlas| {
                    atlas.identity != graph.identity
                        || atlas.provider_generation != graph.provider_generation
                });
                if recreate {
                    let frame = super::glyph_aux::provider_frame(&graph)?;
                    if let Some(old) = self.glyph_atlases.remove(&slot)
                        && let Some(resource_id) = old.resource
                    {
                        self.glyph_pending
                            .push(SceneCommand::ReleaseResource { resource_id });
                    }
                    if let Some(frame) = frame {
                        textures
                            .insert(format!("cmvs.glyph.atlas.{slot}"), frame.clone())
                            .map_err(|_| error("SHINKI_EMU_CMVS_TEXTURE_BUDGET"))?;
                        self.glyph_atlases.insert(
                            slot,
                            AtlasUpload {
                                identity: graph.identity,
                                provider_generation: graph.provider_generation,
                                frame,
                                resource: None,
                                dirty: true,
                                initialized: graph.auxiliary.is_some(),
                            },
                        );
                    }
                }
                let mut atlas = self.glyph_atlases.get_mut(&slot);
                let rebuild = atlas.as_ref().is_some_and(|atlas| !atlas.initialized);
                let cell_size = graph.scale(graph.size)?.wrapping_add(6);
                let auxiliary_info = graph.auxiliary.as_ref().map(|provider| provider.image_info);
                let mut provider_changed = false;
                for index in 0..graph.records.len() {
                    let inside = in_viewport(&graph, &graph.records[index]);
                    if !inside {
                        if let Some(resident) = graph.records[index].resident.take() {
                            let occupied = graph
                                .atlas
                                .occupancy
                                .get_mut(usize::from(resident.slot))
                                .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_OCCUPANCY"))?;
                            *occupied = 0;
                        }
                        continue;
                    }
                    let needs_child = graph.records[index].resident.is_none();
                    if needs_child {
                        let Some(free) =
                            (1..1024).find(|&cell| graph.atlas.occupancy.get(cell) == Some(&0))
                        else {
                            continue;
                        };
                        let descriptor = &graph.records[index];
                        let pixel_size = graph.scale(descriptor.size)?;
                        let position = [
                            graph.scale(descriptor.position[0] as u32)? as i32,
                            graph.scale(descriptor.position[1] as u32)? as i32,
                        ];
                        let record = &mut graph.records[index];
                        if record.painted.is_none() {
                            let raster = vm.native_glyph_raster(slot, record.code, pixel_size)?;
                            let owner = vm
                                .native_font_owners
                                .get(&(783 + u16::from(slot)))
                                .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_OBJECT"))?;
                            let custom = raster.custom_bitmap_identity.and_then(|identity| {
                                vm.native_custom_glyphs.bitmap_by_identity(identity)
                            });
                            record.painted = Some(self.raster.paint_glyph(
                                &owner.family,
                                record,
                                pixel_size,
                                cell_size,
                                raster,
                                custom,
                            )?);
                        }
                        let painted = record.painted.as_ref().expect("painted glyph cell");
                        let destination = cell_destination(&graph.atlas, cell_size, free as u16)?;
                        provider_changed |= upload_provider_cell(
                            atlas.as_deref_mut(),
                            auxiliary_info,
                            painted,
                            cell_size,
                            destination,
                        )?;
                        let draw_size = pixel_size.wrapping_add(4);
                        record.resident = Some(CmvsNativeGlyphResident {
                            slot: free as u16,
                            cell_size,
                            child: CmvsTextureChildState {
                                surface_initialized: true,
                                basic_kind: 0,
                                alpha: Some(255),
                                reference_size: Some(graph.physical_size),
                                rect_words: Some([
                                    destination[0],
                                    destination[1],
                                    draw_size,
                                    draw_size,
                                ]),
                                auxiliary_pair_words: Some([
                                    painted.offset[0].wrapping_sub(2).wrapping_add(position[0])
                                        as u32,
                                    painted.offset[1].wrapping_sub(2).wrapping_add(position[1])
                                        as u32,
                                ]),
                                auxiliary_word: Some(graph.draw_order as u32),
                                ..Default::default()
                            },
                        });
                        graph.atlas.occupancy[free] = 1;
                    } else if rebuild {
                        let record = &graph.records[index];
                        let painted = record
                            .painted
                            .as_ref()
                            .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_PAINTED_STATE"))?;
                        let resident = record.resident.as_ref().expect("resident glyph");
                        let rect = resident
                            .child
                            .rect_words
                            .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_CHILD"))?;
                        provider_changed |= upload_provider_cell(
                            atlas.as_deref_mut(),
                            auxiliary_info,
                            painted,
                            resident.cell_size,
                            [rect[0], rect[1]],
                        )?;
                    }
                }
                if let Some(atlas) = atlas {
                    atlas.initialized = true;
                    if provider_changed && let Some(auxiliary) = &mut graph.auxiliary {
                        auxiliary.rgba = atlas.frame.rgba8.as_slice().to_vec();
                    }
                }
                Ok(())
            })();
            vm.native_glyphs.insert(slot, graph);
            result?;
        }
        // Commit uploads only after all font/materialization boundaries succeed.
        for slot in 0..8 {
            let Some(graph) = vm.native_glyphs.get(&slot) else {
                if let Some(old) = self.glyph_atlases.remove(&slot)
                    && let Some(resource_id) = old.resource
                {
                    self.glyph_pending
                        .push(SceneCommand::ReleaseResource { resource_id });
                }
                continue;
            };
            if let Some(atlas) = self.glyph_atlases.get_mut(&slot)
                && atlas.dirty
            {
                textures
                    .insert(format!("cmvs.glyph.atlas.{slot}"), atlas.frame.clone())
                    .map_err(|_| error("SHINKI_EMU_CMVS_TEXTURE_BUDGET"))?;
                self.glyph_sequence = self
                    .glyph_sequence
                    .checked_add(1)
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_RESOURCE"))?;
                let resource_id = format!("cmvs.glyph.atlas.{slot}.{}", self.glyph_sequence);
                self.glyph_pending.push(SceneCommand::UploadTexture {
                    resource_id: resource_id.clone(),
                    frame: atlas.frame.clone(),
                });
                if let Some(resource_id) = atlas.resource.replace(resource_id) {
                    self.glyph_pending
                        .push(SceneCommand::ReleaseResource { resource_id });
                }
                atlas.dirty = false;
            }
            if graph.root.hidden {
                continue;
            }
            let atlas = self.glyph_atlases.get(&slot).and_then(|atlas| {
                atlas
                    .resource
                    .as_deref()
                    .map(|resource| (resource, &atlas.frame))
            });
            append_draw(vm, slot, 0, graph, None, stage_width, atlas, groups)?;
            for (index, record) in graph.records.iter().enumerate() {
                if let Some(resident) = &record.resident {
                    let order = resident.child.auxiliary_word.unwrap_or(0) as i32;
                    if order >= 0 && !resident.child.hidden {
                        append_draw(
                            vm,
                            slot,
                            index as u32 + 1,
                            graph,
                            Some(&resident.child),
                            stage_width,
                            atlas,
                            groups,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn in_viewport(graph: &CmvsNativeGlyphGraph, record: &CmvsNativeGlyphRecord) -> bool {
    let [x, y] = graph.position.map(f32::from_bits);
    let [left, top, width, height] = graph.viewport.map(f32::from_bits);
    let gx = record.position[0] as f32 + x;
    let gy = record.position[1] as f32 + y;
    let size = record.size as f32;
    !(gx - 2.0 > left + width
        || left > gx + size + 2.0
        || gy - 2.0 > top + height
        || top > gy + size + 2.0)
}

fn cell_destination(
    atlas: &crate::CmvsNativeGlyphAtlas,
    cell: u32,
    slot: u16,
) -> Result<[u32; 2], CoreError> {
    let columns = atlas
        .width
        .checked_div(atlas.cell_size)
        .filter(|&value| value != 0)
        .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_ATLAS_CELL"))?;
    let page = columns.wrapping_mul(columns);
    let slot = u32::from(slot);
    Ok([
        cell.wrapping_mul(slot % columns),
        (slot / page)
            .wrapping_mul(atlas.width)
            .wrapping_add(cell.wrapping_mul((slot % page) / columns)),
    ])
}

fn upload_provider_cell(
    atlas: Option<&mut AtlasUpload>,
    auxiliary_info: Option<[u32; 3]>,
    painted: &crate::CmvsNativeGlyphPainted,
    stride: u32,
    destination: [u32; 2],
) -> Result<bool, CoreError> {
    if let Some(atlas) = atlas {
        let changed = upload_cell_clipped(
            &mut atlas.frame,
            painted,
            stride,
            destination,
            auxiliary_info.is_some(),
        )?;
        atlas.dirty |= changed;
        Ok(changed)
    } else if auxiliary_info.is_some_and(|info| info[0] != 0 && info[1] != 0) {
        Err(error("SHINKI_EMU_CMVS_GLYPH_PROVIDER_SURFACE"))
    } else {
        // A lookup-failed provider has zero extent;43D6A0 copies no pixels.
        Ok(false)
    }
}

#[cfg(test)]
fn upload_cell(
    atlas: &mut TextureFrame,
    painted: &crate::CmvsNativeGlyphPainted,
    stride: u32,
    destination: [u32; 2],
) -> Result<(), CoreError> {
    upload_cell_clipped(atlas, painted, stride, destination, false).map(|_| ())
}

fn upload_cell_clipped(
    atlas: &mut TextureFrame,
    painted: &crate::CmvsNativeGlyphPainted,
    stride: u32,
    destination: [u32; 2],
    clip: bool,
) -> Result<bool, CoreError> {
    let side = painted.physical_size.wrapping_add(5).min(stride);
    let width = side.min(atlas.width.saturating_sub(destination[0]));
    let height = side.min(atlas.height.saturating_sub(destination[1]));
    if !clip && (width != side || height != side) {
        return Err(error("SHINKI_EMU_CMVS_GLYPH_ATLAS_ALLOCATION"));
    }
    if width == 0 || height == 0 {
        return Ok(false);
    }
    let required = ((height as usize - 1) * stride as usize + width as usize) * 4;
    if painted.bgra.len() < required {
        return Err(error("SHINKI_EMU_CMVS_GLYPH_PAINTED_ALLOCATION"));
    }
    let pixels = atlas.rgba8.make_mut_for_update();
    for y in 0..height {
        let src = (y * stride * 4) as usize;
        let dst = (((destination[1] + y) * atlas.width + destination[0]) * 4) as usize;
        for (source, target) in painted.bgra[src..src + width as usize * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .zip(
                pixels[dst..dst + width as usize * 4]
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut(),
            )
        {
            target.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn append_draw(
    vm: &CmvsPs2aVmState,
    slot: u8,
    node: u32,
    graph: &CmvsNativeGlyphGraph,
    child: Option<&CmvsTextureChildState>,
    stage_width: u32,
    atlas: Option<(&str, &TextureFrame)>,
    groups: &mut Vec<NativeDrawGroup>,
) -> Result<(), CoreError> {
    let root = &graph.root;
    let order = child
        .map_or(root.auxiliary_word, |child| child.auxiliary_word)
        .unwrap_or(0) as i32;
    let mut group = NativeDrawGroup {
        position: NativeQueuePosition::flat(vm, 272 + u16::from(slot), node, order),
        commands: Vec::new(),
    };
    if let Some((resource, atlas)) = atlas {
        let geometry =
            super::super::surface::geometry_with_surfaces(vm, root, child, stage_width, true);
        let rect = child
            .map_or(root.rect_words, |child| child.rect_words)
            .unwrap_or([0; 4]);
        let source = RectI::new(rect[0] as i32, rect[1] as i32, rect[2], rect[3]);
        if let (Some(geometry), Some(source)) = (
            geometry,
            super::super::reload::clipped_source(source, atlas.width, atlas.height),
        ) {
            let shake = crate::vm::screen_shake::logical_offset(vm, stage_width, 0)?;
            let opacity = super::super::surface::opacity_with_surfaces(vm, root, child, 256);
            let blend = match child
                .map_or(root.blend_mode, |child| child.blend_mode)
                .unwrap_or(1)
            {
                0 => BlendMode::Opaque,
                2 => BlendMode::Add,
                3 => BlendMode::ScreenRgb,
                _ => BlendMode::Alpha,
            };
            geometry.with_offset(shake).append_sprite(
                &mut group.commands,
                format!("cmvs.glyph.{}.{}", graph.identity, node),
                resource.to_owned(),
                source,
                [atlas.width, atlas.height],
                opacity,
                blend,
            );
        }
    }
    groups.push(group);
    Ok(())
}

fn collect_text_groups(
    vm: &CmvsPs2aVmState,
    stage_width: u32,
    commands: Vec<SceneCommand>,
    owners: Vec<(u8, usize)>,
    output: &mut NativeDrawGroups,
) -> Result<(), CoreError> {
    let mut transforms = Vec::new();
    let mut owners = owners.into_iter();
    let mut draws = BTreeMap::<(u8, usize), Vec<SceneCommand>>::new();
    let shake = crate::vm::screen_shake::logical_offset(vm, stage_width, 0)?;
    for command in commands {
        match command {
            SceneCommand::UploadGlyph { .. }
            | SceneCommand::UploadTexture { .. }
            | SceneCommand::ReleaseResource { .. } => output.resources.push(command),
            SceneCommand::PushTransform { transform } => transforms.push(transform),
            SceneCommand::PopTransform => {
                transforms
                    .pop()
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_COMMAND"))?;
            }
            SceneCommand::GlyphRun { .. } => {
                let owner = owners
                    .next()
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_COMMAND"))?;
                let target = draws.entry(owner).or_default();
                target.extend(
                    transforms
                        .iter()
                        .map(|transform| SceneCommand::PushTransform {
                            transform: *transform,
                        }),
                );
                if shake != [0.0; 2] {
                    target.push(SceneCommand::PushTransform {
                        transform: roukaku_media_core::Transform2D {
                            tx: shake[0],
                            ty: shake[1],
                            ..roukaku_media_core::Transform2D::IDENTITY
                        },
                    });
                }
                target.push(command);
                for _ in 0..transforms.len() + usize::from(shake != [0.0; 2]) {
                    target.push(SceneCommand::PopTransform);
                }
            }
            _ => return Err(error("SHINKI_EMU_CMVS_TEXT_COMMAND")),
        }
    }
    if !transforms.is_empty() || owners.next().is_some() {
        return Err(error("SHINKI_EMU_CMVS_TEXT_COMMAND"));
    }
    for (&slot, surface) in &vm.native_text {
        if !surface.visible {
            continue;
        }
        let owner = 260 + u16::from(slot);
        // 4647D0 creates a transparent4x4 root before any linked glyphs.
        output.groups.push(NativeDrawGroup {
            position: NativeQueuePosition::flat(vm, owner, 0, surface.draw_order),
            commands: Vec::new(),
        });
        if surface.draw_order < 0 {
            continue;
        }
        for index in 0..surface.glyphs.len() {
            let hidden = vm
                .native_text_reveals
                .get(&slot)
                .and_then(|nodes| {
                    nodes
                        .binary_search_by_key(&index, |node| node.glyph_index)
                        .ok()
                        .map(|index| &nodes[index])
                })
                .is_some_and(|node| !node.started);
            //4646D0 hides delayed nodes. A started alpha/width-zero node
            // remains visible to447550 even though it produces no pixels.
            if hidden {
                continue;
            }
            output.groups.push(NativeDrawGroup {
                position: NativeQueuePosition::flat(
                    vm,
                    owner,
                    index as u32 + 1,
                    surface.draw_order,
                ),
                commands: draws.remove(&(slot, index)).unwrap_or_default(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
