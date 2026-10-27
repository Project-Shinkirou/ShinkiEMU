use super::*;
use std::borrow::Cow;

pub(super) const SHARD_CHILD_BASE: u32 = 1024;
pub(super) fn scene_slots(
    slot: u8,
    image: &crate::CmvsNativeImageShards,
) -> impl Iterator<Item = CmvsTextureSlot> + '_ {
    std::iter::once(CmvsTextureSlot::SceneRoot(slot))
        .chain(
            image
                .children
                .keys()
                .map(move |&child| CmvsTextureSlot::SceneChild {
                    slot,
                    child: u32::from(child),
                }),
        )
        .chain(
            (0..image.grid.particle_capacity().min(8192)).map(move |index| {
                CmvsTextureSlot::SceneChild {
                    slot,
                    child: SHARD_CHILD_BASE + index as u32,
                }
            }),
        )
}

impl CmvsTextureSlot {
    pub(super) fn original_child(self) -> Option<u16> {
        match self {
            Self::Child { child, .. } => Some(child),
            Self::SceneChild { child, .. } if child < SHARD_CHILD_BASE => Some(child as u16),
            _ => None,
        }
    }
    pub(super) fn child_slot(self, child: u16) -> Self {
        match self.root_slot() {
            Self::Parent(parent) => Self::Child { parent, child },
            Self::SceneRoot(slot) => Self::SceneChild {
                slot,
                child: u32::from(child),
            },
            _ => unreachable!(),
        }
    }
    pub(super) fn reload(
        self,
        vm: &CmvsPs2aVmState,
    ) -> Option<&crate::vm::texture_reload::TextureReload> {
        match self.root_slot() {
            Self::Parent(parent) => vm.texture_reloads.get(&parent),
            Self::SceneRoot(slot) => vm
                .native_scene_workers
                .get(&slot)?
                .image_shards()?
                .reload
                .as_ref(),
            _ => unreachable!(),
        }
    }
    pub(super) fn root_slot(self) -> Self {
        match self {
            Self::Parent(_) | Self::SceneRoot(_) => self,
            Self::Child { parent, .. } => Self::Parent(parent),
            Self::SceneChild { slot, .. } => Self::SceneRoot(slot),
        }
    }

    pub(super) fn owner_rank(self) -> u16 {
        match self.root_slot() {
            Self::Parent(parent) => u16::from(parent),
            Self::SceneRoot(slot) => 256 + u16::from(slot),
            _ => unreachable!(),
        }
    }

    pub(crate) fn scene_worker(self) -> Option<u8> {
        match self {
            Self::SceneRoot(slot) | Self::SceneChild { slot, .. } => Some(slot),
            _ => None,
        }
    }

    pub(super) fn root(self, vm: &CmvsPs2aVmState) -> Option<&crate::CmvsTextureParentState> {
        match self.root_slot() {
            Self::Parent(parent) => vm.texture_parents.get(&parent),
            Self::SceneRoot(slot) => vm
                .native_scene_workers
                .get(&slot)?
                .image_shards()
                .map(|owner| &owner.root),
            _ => unreachable!(),
        }
    }

    pub(super) fn child<'a>(
        self,
        vm: &'a CmvsPs2aVmState,
    ) -> Option<Cow<'a, crate::CmvsTextureChildState>> {
        match self {
            Self::Parent(_) | Self::SceneRoot(_) => None,
            Self::Child { parent, child } => vm
                .texture_children
                .get(&parent)?
                .get(&child)
                .map(Cow::Borrowed),
            Self::SceneChild { slot, child } => {
                let owner = vm.native_scene_workers.get(&slot)?.image_shards()?;
                if child < SHARD_CHILD_BASE {
                    owner.children.get(&(child as u16)).map(Cow::Borrowed)
                } else {
                    owner
                        .grid
                        .draw_basic((child - SHARD_CHILD_BASE) as usize)
                        .map(Cow::Owned)
                }
            }
        }
    }

    pub(super) fn is_root(self) -> bool {
        matches!(self, Self::Parent(_) | Self::SceneRoot(_))
    }

    pub(crate) fn relocate(self, source: u8, destination: Self) -> Self {
        match self {
            Self::Parent(parent) if parent == source => destination,
            Self::Child { parent, child } if parent == source => match destination {
                Self::Parent(parent) => Self::Child { parent, child },
                Self::SceneRoot(slot) => Self::SceneChild {
                    slot,
                    child: u32::from(child),
                },
                _ => unreachable!(),
            },
            _ => self,
        }
    }
}

impl CmvsScene {
    pub(super) fn prepare_scene_image_bindings(
        &mut self,
        vm: &CmvsPs2aVmState,
        archive: &CmvsArchive,
    ) -> Result<(), CoreError> {
        let mut stale = 0_u8;
        for (key, _) in self.bindings.range(
            CmvsTextureSlot::SceneRoot(0)..=CmvsTextureSlot::SceneChild {
                slot: 3,
                child: u32::MAX,
            },
        ) {
            if let Some(slot) = key.scene_worker()
                && vm
                    .native_scene_workers
                    .get(&slot)
                    .and_then(|worker| worker.image_shards())
                    .is_none()
            {
                stale |= 1 << slot;
            }
        }
        for slot in 0..4 {
            if stale & (1 << slot) != 0 {
                self.release_owner_surface(CmvsTextureSlot::SceneRoot(slot));
            }
        }
        for (&slot, worker) in &vm.native_scene_workers {
            let Some(owner) = worker.image_shards() else {
                continue;
            };
            let root = CmvsTextureSlot::SceneRoot(slot);
            if !self.bindings.contains_key(&root)
                && owner.root.image_info[0] != 0
                && owner.root.image_info[1] != 0
                && let Some(name) = owner.root.resource.as_deref()
            {
                let uri = archive.resolve_native_resource(
                    vm,
                    crate::CmvsResourceSearchBank::Texture,
                    name,
                )?;
                self.bind(root, &uri, archive)?;
            }
            for (&child, image) in &owner.children {
                let key = root.child_slot(child);
                if self.bindings.contains_key(&key)
                    || image.image_info[0] == 0
                    || image.image_info[1] == 0
                {
                    continue;
                }
                if let Some(name) = image.resource.as_deref() {
                    let uri = archive.resolve_native_resource(
                        vm,
                        crate::CmvsResourceSearchBank::Texture,
                        name,
                    )?;
                    self.bind(key, &uri, archive)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::parent_load_tests::{command, fixture, write_image};

    #[test]
    #[ignore = "requires a hardware GPU"]
    fn gpu_moved_indexed_child_outlives_shards_parent_reuse_and_table_clear() {
        let (root, archive, mut vm) = fixture();
        write_image(root.path(), "source.pb3", 16, 16, [255, 0, 0, 255]);
        write_image(root.path(), "child.pb3", 1, 1, [0, 0, 255, 255]);
        vm.native_stage_size = [32, 32];
        vm.native_render_size = [32, 32];
        let mut scene = CmvsScene::new(crate::scene::tests::gpu_instance()).unwrap();
        scene.clear(32, 32).unwrap();
        command(&mut vm, 32, &[0]);
        command(&mut vm, 64, &[0]);
        vm.texture_parents.get_mut(&0).unwrap().resource = Some("source.pb3".to_owned());
        let source = archive
            .resolve_native_resource(&vm, crate::CmvsResourceSearchBank::Texture, "source.pb3")
            .unwrap();
        scene
            .bind_parent_image(&mut vm, 0, &source, &archive)
            .unwrap();
        vm.texture_children.get_mut(&0).unwrap().insert(
            0,
            crate::CmvsTextureChildState {
                resource: Some("child.pb3".to_owned()),
                surface_initialized: true,
                parent_context_detached: true,
                screen_shake_disabled: true,
                basic_kind: 0,
                blend_mode: Some(0),
                reference_size: Some([32, 32]),
                rect_words: Some([0, 0, 1, 1]),
                auxiliary_pair_words: Some([2, 3]),
                ..Default::default()
            },
        );
        let child = archive
            .resolve_native_resource(&vm, crate::CmvsResourceSearchBank::Texture, "child.pb3")
            .unwrap();
        scene
            .bind_child_image(&mut vm, 0, 0, &child, &archive)
            .unwrap();
        command(&mut vm, 680, &[0, 0, 20, 0]);
        scene.move_parent_surface(0, CmvsTextureSlot::SceneRoot(0));
        let first = scene
            .render(
                &mut vm,
                CmvsTextureSlot::SceneRoot(0),
                &archive,
                (32, 32),
                None,
            )
            .unwrap()
            .unwrap();
        let blue = ((3 * 32 + 2) * 4) as usize;
        assert_eq!(&first.rgba8[blue..blue + 3], &[0, 0, 255]);
        assert_eq!(
            &first.rgba8[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 3],
            &[255, 0, 0]
        );
        command(&mut vm, 32, &[0]);
        command(&mut vm, 47, &[]);
        scene.clear_parent_surfaces();
        vm.session_clock_millis = 1000;
        crate::vm::native_scene_worker::tick(&mut vm, 1000).unwrap();
        let last = scene
            .render(
                &mut vm,
                CmvsTextureSlot::SceneRoot(0),
                &archive,
                (32, 32),
                None,
            )
            .unwrap()
            .unwrap();
        for y in 0..32 {
            for x in 0..32 {
                let expected = if (x, y) == (2, 3) {
                    [0, 0, 255]
                } else {
                    [0, 0, 0]
                };
                let offset = (y * 32 + x) * 4;
                assert_eq!(&last.rgba8[offset..offset + 3], &expected, "pixel{x},{y}");
            }
        }
    }

    #[test]
    #[ignore = "requires a hardware GPU"]
    fn gpu_linked_flat_shards_ignore_parent_basic_but_use_its_shake_header() {
        for disabled in [false, true] {
            let (root, archive, mut vm) = fixture();
            write_image(root.path(), "source.pb3", 16, 16, [255, 0, 0, 255]);
            vm.native_stage_size = [32, 32];
            vm.native_render_size = [32, 32];
            vm.native_frame_delta_ms = 10;
            command(&mut vm, 32, &[0]);
            let image = vm.texture_parents.get_mut(&0).unwrap();
            image.resource = Some("source.pb3".to_owned());
            image.surface_initialized = true;
            image.basic_kind = 2;
            image.alpha = Some(0);
            image.rect_words = Some([0, 0, 16, 16]);
            image.float_position_words = [0, 0, 10.0_f32.to_bits()];
            image.projection_depth_word = 10.0_f32.to_bits();
            image.projection_height_word = 10.0_f32.to_bits();
            image.screen_shake_disabled = disabled;
            let mut scene = CmvsScene::new(crate::scene::tests::gpu_instance()).unwrap();
            scene.clear(32, 32).unwrap();
            let uri = archive
                .resolve_native_resource(&vm, crate::CmvsResourceSearchBank::Texture, "source.pb3")
                .unwrap();
            scene.bind_parent_image(&mut vm, 0, &uri, &archive).unwrap();
            command(&mut vm, 680, &[0, 0, 20, 0]);
            scene.move_parent_surface(0, CmvsTextureSlot::SceneRoot(0));
            command(&mut vm, 549, &[0, 1, 10, u32::MAX]);
            crate::vm::screen_shake::advance(&mut vm).unwrap();
            let frame = scene
                .render(
                    &mut vm,
                    CmvsTextureSlot::SceneRoot(0),
                    &archive,
                    (32, 32),
                    None,
                )
                .unwrap()
                .unwrap();
            assert_eq!(
                &frame.rgba8[..3],
                if disabled { &[255, 0, 0] } else { &[0, 0, 0] }
            );
            let center = (16 * 32 + 16) * 4;
            assert_eq!(&frame.rgba8[center..center + 3], &[255, 0, 0]);
        }
    }
}
