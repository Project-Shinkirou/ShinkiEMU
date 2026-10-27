use super::*;

pub(super) struct SurfaceImage {
    pub(super) id: String,
    frame: TextureFrame,
}

/// 4473C0 sorts each root/child list by descending float depth, with stable
/// insertion ties. 447350 drains a root's child list before the next root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct NativeQueuePosition {
    pass: u8,
    root_order: u32,
    root_slot: u16,
    root_node: u32,
    nested: u8,
    child_order: u32,
    child_id: u32,
}

/// One native queue record. Empty groups still consume the shared record pool.
pub(super) struct NativeDrawGroup {
    pub(super) position: NativeQueuePosition,
    pub(super) commands: Vec<SceneCommand>,
}

#[derive(Default)]
pub(super) struct NativeDrawGroups {
    pub(super) resources: Vec<SceneCommand>,
    pub(super) groups: Vec<NativeDrawGroup>,
}

impl NativeDrawGroups {
    pub(super) fn admit(&mut self, remaining: &mut usize) {
        self.groups.truncate(*remaining);
        *remaining -= self.groups.len();
    }

    #[cfg(test)]
    pub(super) fn append(mut self, commands: &mut Vec<SceneCommand>) {
        commands.append(&mut self.resources);
        self.groups.sort_by_key(|group| group.position);
        for mut group in self.groups {
            commands.append(&mut group.commands);
        }
    }
}

impl NativeQueuePosition {
    pub(super) fn flat(vm: &CmvsPs2aVmState, owner: u16, node: u32, order: i32) -> Self {
        Self::new(vm, 3, order as f32, owner, node, None)
    }

    pub(super) fn new(
        vm: &CmvsPs2aVmState,
        pass: u8,
        depth: f32,
        owner: u16,
        node: u32,
        indexed: Option<(f32, u32)>,
    ) -> Self {
        Self {
            pass,
            root_order: descending_float_key(depth),
            root_slot: if vm.native_mesh_equal_prepend {
                u16::MAX - owner
            } else {
                owner
            },
            root_node: if vm.native_mesh_equal_prepend {
                u32::MAX - node
            } else {
                node
            },
            nested: u8::from(indexed.is_some()),
            child_order: indexed.map_or(0, |(depth, _)| descending_float_key(depth)),
            child_id: indexed.map_or(0, |(_, child)| {
                if vm.native_mesh_equal_prepend {
                    u32::MAX - child
                } else {
                    child
                }
            }),
        }
    }

    pub(super) fn key(self) -> (u8, u32) {
        (self.pass, self.root_order)
    }
}

pub(super) fn descending_float_key(value: f32) -> u32 {
    let bits = value.to_bits();
    let ascending = if bits & 0x8000_0000 != 0 {
        !bits
    } else {
        bits ^ 0x8000_0000
    };
    !ascending
}

pub(super) fn root_queue_depth(vm: &CmvsPs2aVmState, root: &crate::CmvsTextureParentState) -> f32 {
    if matches!(root.basic_kind, 2 | 3) {
        let words = if root.basic_kind == 3 {
            vm.screen_camera_words.get(&1)
        } else {
            Some(&vm.screen_words)
        };
        let camera_z = words
            .and_then(|words| words.get(&3))
            .copied()
            .map(f32::from_bits)
            .unwrap_or(0.0);
        f32::from_bits(root.float_position_words[2]) - camera_z
    } else {
        root.auxiliary_word.unwrap_or(0) as i32 as f32
    }
}

pub(super) fn queue_key(vm: &CmvsPs2aVmState, slot: CmvsTextureSlot) -> (u8, u32) {
    let root = slot.root(vm).expect("queued image has a live root");
    //447A20 queues linked children independently, unlike the indexed subtree.
    if matches!(slot, CmvsTextureSlot::SceneChild { child, .. } if child >= super::image_owner::SHARD_CHILD_BASE)
    {
        let image = slot.child(vm).expect("linked image child");
        return (
            3,
            descending_float_key(image.auxiliary_word.unwrap_or(0) as i32 as f32),
        );
    }
    let pass = match root.basic_kind {
        1 => 0,
        2 => 1,
        3 => 2,
        _ => 3,
    };
    (pass, descending_float_key(root_queue_depth(vm, root)))
}

/// Complete ordering for the default inherited (relative-child) surface graph.
/// Do not append the former global integer order: camera queues use float Z.
pub(super) fn native_queue_position(
    vm: &CmvsPs2aVmState,
    slot: CmvsTextureSlot,
) -> NativeQueuePosition {
    let root = slot.root(vm).expect("queued image has a live root");
    let (pass, root_order) = queue_key(vm, slot);
    let linked_node = match slot {
        CmvsTextureSlot::SceneChild { child, .. }
            if child >= super::image_owner::SHARD_CHILD_BASE =>
        {
            child
        }
        _ => 0,
    };
    let (nested, child_order, child_id) = match slot.child(vm).filter(|_| linked_node == 0) {
        None => (0, 0, 0),
        Some(image) => {
            let depth = if matches!(image.basic_kind, 2 | 3) {
                root_queue_depth(vm, root) + f32::from_bits(image.float_position_words[2])
            } else {
                image.auxiliary_word.unwrap_or(0) as i32 as f32
            };
            let child = match slot {
                CmvsTextureSlot::Child { child, .. } => u32::from(child),
                CmvsTextureSlot::SceneChild { child, .. } => child,
                _ => unreachable!(),
            };
            (1, descending_float_key(depth), child)
        }
    };
    NativeQueuePosition {
        pass,
        root_order,
        root_slot: if vm.native_mesh_equal_prepend {
            u16::MAX - slot.owner_rank()
        } else {
            slot.owner_rank()
        },
        root_node: if vm.native_mesh_equal_prepend {
            u32::MAX - linked_node
        } else {
            linked_node
        },
        nested,
        child_order,
        child_id: if vm.native_mesh_equal_prepend {
            u32::MAX - child_id
        } else {
            child_id
        },
    }
}

impl CmvsScene {
    pub(super) fn release_surface_image(&mut self, slot: CmvsTextureSlot) {
        if let Some(upload) = self.surface_uploads.remove(&slot) {
            self.reload_releases.push(upload.id);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_surface_image(
        &mut self,
        vm: &CmvsPs2aVmState,
        slot: CmvsTextureSlot,
        frame: TextureFrame,
        source: RectI,
        opacity: f32,
        commands: &mut Vec<SceneCommand>,
        geometry: super::basic_geometry::BasicGeometry,
    ) -> Result<(), CoreError> {
        if opacity == 0.0 {
            return Ok(());
        }
        let Some(source) = super::reload::clipped_source(source, frame.width, frame.height) else {
            return Ok(());
        };
        let provider = if !slot.is_root() && !self.bindings.contains_key(&slot) {
            slot.root_slot()
        } else {
            slot
        };
        let changed = self.surface_uploads.get(&provider).is_none_or(|upload| {
            upload.frame.rgba8.allocation_ptr() != frame.rgba8.allocation_ptr()
                || upload.frame.width != frame.width
                || upload.frame.height != frame.height
        });
        if changed {
            self.release_surface_image(provider);
            self.reload_generation = self
                .reload_generation
                .checked_add(1)
                .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXTURE_GENERATION"))?;
            let id = format!("cmvs.surface.{}", self.reload_generation);
            commands.push(SceneCommand::UploadTexture {
                resource_id: id.clone(),
                frame: frame.clone(),
            });
            self.surface_uploads
                .insert(provider, SurfaceImage { id, frame });
        }
        let blend = blend(vm, slot, opacity);
        let opacity = if blend == BlendMode::ScreenRgb {
            let alpha = slot.child(vm).map_or_else(
                || slot.root(vm).and_then(|root| root.alpha),
                |image| image.alpha,
            );
            alpha.unwrap_or(255).clamp(0, 255) as f32 / 255.0
        } else if blend == BlendMode::Opaque {
            1.0
        } else {
            opacity
        };
        let upload = &self.surface_uploads[&provider];
        geometry.append_sprite(
            commands,
            slot_id(slot),
            upload.id.clone(),
            source,
            [upload.frame.width, upload.frame.height],
            opacity,
            blend,
        );
        Ok(())
    }
}

pub(super) fn opacity(vm: &CmvsPs2aVmState, slot: CmvsTextureSlot) -> f32 {
    let Some(root) = slot.root(vm) else {
        return 0.0;
    };
    let child = slot.child(vm);
    if !slot.is_root() && child.is_none() {
        return 0.0;
    }
    opacity_with_surfaces(vm, root, child.as_deref(), 256)
}

pub(super) fn opacity_with_surfaces(
    vm: &CmvsPs2aVmState,
    root: &crate::CmvsTextureParentState,
    child: Option<&crate::CmvsTextureChildState>,
    root_weight: u32,
) -> f32 {
    let byte = |alpha: Option<i32>| alpha.unwrap_or(255).clamp(0, 255) as u32;
    let detached = child.filter(|child| child.parent_context_detached);
    let alpha = if let Some(child) = detached {
        byte(child.alpha)
    } else {
        let alpha = child.map_or(byte(root.alpha), |child| {
            byte(child.alpha) * byte(root.alpha) / 255
        });
        alpha * root_weight / 256
    };
    let (kind, position, depth, height) = detached.map_or(
        (
            root.basic_kind,
            root.float_position_words,
            root.projection_depth_word,
            root.projection_height_word,
        ),
        |image| {
            (
                image.basic_kind,
                image.float_position_words,
                image.projection_depth_word,
                image.projection_height_word,
            )
        },
    );
    let alpha = if matches!(kind, 2 | 3) {
        let Some(projected) = super::projection::project(
            vm,
            u32::from(kind == 3),
            position.map(f32::from_bits),
            f32::from_bits(depth),
            f32::from_bits(height),
        ) else {
            return 0.0;
        };
        alpha * projected.alpha / 255
    } else {
        alpha
    };
    alpha as f32 / 255.0
}

pub(super) fn blend(vm: &CmvsPs2aVmState, slot: CmvsTextureSlot, opacity: f32) -> BlendMode {
    let root = slot.root(vm).expect("drawable image has a live root");
    let child = slot.child(vm);
    let (mode, has_alpha) = child.as_deref().map_or(
        (root.blend_mode.unwrap_or(1), root.image_info[2] != 0),
        |image| {
            let info = if image.resource.is_some() {
                image.image_info
            } else {
                root.image_info
            };
            (image.blend_mode.unwrap_or(1), info[2] != 0)
        },
    );
    if mode == 0 || (!has_alpha && opacity >= 1.0) {
        BlendMode::Opaque
    } else {
        match mode {
            2 => BlendMode::Add,
            3 => BlendMode::ScreenRgb,
            _ => BlendMode::Alpha,
        }
    }
}

pub(super) fn destination(vm: &CmvsPs2aVmState, slot: CmvsTextureSlot, source: RectI) -> RectI {
    let root = slot
        .root(vm)
        .and_then(|image| image.auxiliary_pair_words)
        .unwrap_or([0; 2]);
    let [x, y] = if let Some(child) = slot.child(vm) {
        let local = child.auxiliary_pair_words.unwrap_or([0; 2]);
        [
            root[0].wrapping_add(local[0]) as i32,
            root[1].wrapping_add(local[1]) as i32,
        ]
    } else {
        root.map(|value| value as i32)
    };
    RectI {
        x,
        y,
        width: source.width,
        height: source.height,
    }
}

pub(super) fn solid_color(vm: &CmvsPs2aVmState, slot: CmvsTextureSlot) -> Option<u32> {
    if slot.is_root() {
        slot.root(vm)?.solid_color
    } else {
        slot.child(vm)?.solid_color
    }
}

pub(super) fn solid_command(
    slot: CmvsTextureSlot,
    destination: RectI,
    color: u32,
    opacity: f32,
    _stage: (u32, u32),
) -> Option<SceneCommand> {
    if destination.width == 0 || destination.height == 0 || opacity == 0.0 {
        return None;
    }
    Some(SceneCommand::Rect {
        id: slot_id(slot),
        x: 0,
        y: 0,
        width: destination.width,
        height: destination.height,
        rgba: [
            (color >> 16) as u8,
            (color >> 8) as u8,
            color as u8,
            ((color >> 24) as f32 * opacity).round() as u8,
        ],
    })
}

/// Native 42C330 keeps crop UVs separate from the basic's signed-short pivot.
pub(super) fn geometry(
    vm: &CmvsPs2aVmState,
    slot: CmvsTextureSlot,
    stage_width: u32,
    textured: bool,
) -> Option<super::basic_geometry::BasicGeometry> {
    let root = slot.root(vm)?;
    let child = slot.child(vm);
    if !slot.is_root() && child.is_none() {
        return None;
    }
    if matches!(slot, CmvsTextureSlot::SceneChild { child, .. } if child >= super::image_owner::SHARD_CHILD_BASE)
        && !(root.basic_kind == 5 && child.as_deref().is_some_and(|image| image.basic_kind == 0))
    {
        let image = child.as_deref().unwrap();
        let independent = crate::CmvsTextureParentState {
            surface_initialized: true,
            basic_kind: image.basic_kind,
            scale_words: image.scale_words,
            float_position_words: image.float_position_words,
            projection_depth_word: image.projection_depth_word,
            projection_height_word: image.projection_height_word,
            rotation_hundredths: image.rotation_hundredths,
            reference_size: image.reference_size,
            position_words: image.position_words,
            auxiliary_pair_words: image.auxiliary_pair_words,
            ..Default::default()
        };
        return geometry_with_surfaces(vm, &independent, None, stage_width, textured);
    }
    geometry_with_surfaces(vm, root, child.as_deref(), stage_width, textured)
}

/// Reloads retain their original basic contexts while the live provider is
/// replaced. Borrow those surfaces rather than cloning or rewriting the VM.
pub(super) fn geometry_with_surfaces(
    vm: &CmvsPs2aVmState,
    root: &crate::CmvsTextureParentState,
    child: Option<&crate::CmvsTextureChildState>,
    stage_width: u32,
    textured: bool,
) -> Option<super::basic_geometry::BasicGeometry> {
    use super::basic_geometry::BasicGeometry;
    if root.basic_kind == 3 && vm.native_view.alternate_active() {
        return None;
    }
    if child.is_some_and(|image| {
        image.basic_kind != root.basic_kind && !(root.basic_kind == 5 && image.basic_kind == 0)
    }) {
        return None;
    }
    if matches!(root.basic_kind, 0 | 1 | 5)
        && child.is_some_and(|image| (image.auxiliary_word.unwrap_or(0) as i32) < 0)
    {
        return None;
    }
    let inherits = child.is_some_and(|image| !image.parent_context_detached);
    let decode_scale =
        |words: Option<[u32; 2]>| words.map(|v| v.map(f32::from_bits)).unwrap_or([1.0; 2]);
    let root_scale = decode_scale(root.scale_words);
    let local_scale = child.map_or(root_scale, |image| decode_scale(image.scale_words));
    let pivot = child
        .map_or(root.position_words, |image| image.position_words)
        .unwrap_or([0; 2])
        .map(|value| value as i16 as f32);
    let integer = |words: Option<[u32; 2]>| words.unwrap_or([0; 2]).map(|v| v as i32 as f32);
    let root_integer = integer(root.auxiliary_pair_words);
    let child_integer = child.map_or([0.0; 2], |image| integer(image.auxiliary_pair_words));
    let rotation =
        child.map_or(root.rotation_hundredths, |image| image.rotation_hundredths) as f32 / 100.0;
    let reference = child
        .filter(|image| image.parent_context_detached)
        .map_or(root.reference_size, |image| image.reference_size);
    let basis =
        super::projection::reference_scale(stage_width, reference.map_or(0, |size| size[0]));
    match root.basic_kind {
        0 | 1 | 5 => {
            let scale = if inherits {
                [
                    local_scale[0] * root_scale[0],
                    local_scale[1] * root_scale[1],
                ]
            } else {
                local_scale
            };
            // 42C330 tests the combined scales, not each basic individually.
            if scale
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.0)
            {
                return None;
            }
            let root_position = if root.basic_kind == 5 && textured {
                [
                    f32::from_bits(root.float_position_words[0]),
                    f32::from_bits(root.float_position_words[1]),
                ]
            } else {
                root_integer
            };
            let local_position = child.map_or([0.0; 2], |image| {
                if image.basic_kind == 5 && textured {
                    [
                        f32::from_bits(image.float_position_words[0]),
                        f32::from_bits(image.float_position_words[1]),
                    ]
                } else {
                    child_integer
                }
            });
            let position = if inherits {
                [
                    root_scale[0] * local_position[0] + root_position[0],
                    root_scale[0] * local_position[1] + root_position[1],
                ]
            } else if child.is_some() {
                local_position
            } else {
                root_position
            };
            Some(BasicGeometry::flat(pivot, scale, rotation, position, basis))
        }
        2 | 3 => {
            let (position, depth, height) =
                child.filter(|image| image.parent_context_detached).map_or(
                    (
                        root.float_position_words,
                        root.projection_depth_word,
                        root.projection_height_word,
                    ),
                    |image| {
                        (
                            image.float_position_words,
                            image.projection_depth_word,
                            image.projection_height_word,
                        )
                    },
                );
            let projected = super::projection::project(
                vm,
                u32::from(root.basic_kind == 3),
                position.map(f32::from_bits),
                f32::from_bits(depth),
                f32::from_bits(height),
            )?;
            let inherited_scale = if inherits { root_scale } else { [1.0; 2] };
            let projected_scale = inherited_scale.map(|scale| scale * projected.scale);
            // 4306A0 accepts signed/zero local scales. Only the projected
            // parent scale is positive-gated before the second geometry pass.
            if projected_scale
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.0)
            {
                return None;
            }
            let parent_pivot = if inherits {
                let parent_pivot = root.position_words.unwrap_or([0; 2]);
                let local_position = child?.auxiliary_pair_words.unwrap_or([0; 2]);
                std::array::from_fn(|axis| {
                    (parent_pivot[axis] as i16).wrapping_sub(local_position[axis] as i16) as f32
                })
            } else {
                [0.0; 2]
            };
            Some(BasicGeometry::camera(
                pivot,
                local_scale,
                rotation,
                parent_pivot,
                projected_scale,
                if inherits {
                    root.rotation_hundredths as f32 / 100.0
                } else {
                    0.0
                },
                projected,
                basis,
            ))
        }
        _ => None,
    }
}

pub(super) fn extend_transformed(
    target: &mut Vec<SceneCommand>,
    transform: roukaku_media_core::Transform2D,
    commands: impl IntoIterator<Item = SceneCommand>,
) {
    let transformed = transform != roukaku_media_core::Transform2D::IDENTITY;
    if transformed {
        target.push(SceneCommand::PushTransform { transform });
    }
    target.extend(commands);
    if transformed {
        target.push(SceneCommand::PopTransform);
    }
}
