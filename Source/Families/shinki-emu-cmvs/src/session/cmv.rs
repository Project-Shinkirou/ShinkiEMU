use super::*;

pub(super) struct CmvPlayer {
    decoder: crate::cmv::CmvDecoder,
}

fn resolve_cmv_uri(
    archive: &CmvsArchive,
    vm: &CmvsPs2aVmState,
    resource: &str,
    open_mode: u32,
) -> SessionResult<String> {
    match open_mode {
        0 | 1 => archive.resolve_native_file_resource(resource).map_err(core),
        2 | 3 => archive
            .resolve_native_resource(vm, CmvsResourceSearchBank::Native0d18, resource)
            .map_err(core),
        _ => Err(error(
            "SHINKI_EMU_CMVS_CMV_MODE",
            "native auxiliary source mode is unsupported",
        )),
    }
}

/// Reopen against the candidate VM's reconstructed LIVE parent, preserving the
/// image geometry and child tree. The caller commits the candidate atomically.
pub(super) fn restore_native_cmv(
    vm: &mut CmvsPs2aVmState,
    scene: &mut CmvsScene,
    archive: &CmvsArchive,
    players: &mut BTreeMap<u8, CmvPlayer>,
    slot: u16,
    saved: &crate::system_save::slot_cmv::SavedCmv,
) -> SessionResult<()> {
    let slot = u8::try_from(slot)
        .ok()
        .filter(|slot| *slot <= 2)
        .ok_or_else(|| {
            error(
                "SHINKI_EMU_CMVS_CMV_OWNER",
                "native CMV slot exceeds table 3184",
            )
        })?;
    let parent = u8::try_from(saved.parent_slot).map_err(|_| {
        error(
            "SHINKI_EMU_CMVS_CMV_OWNER",
            "native CMV parent slot exceeds table 1924",
        )
    })?;
    let parent_key = (1924u32 << 8) | u32::from(parent);
    let parent_identity = vm
        .slot_objects
        .get(&parent_key)
        .copied()
        .filter(|identity| *identity != 0)
        .ok_or_else(|| {
            error(
                "SHINKI_EMU_CMVS_CMV_OWNER",
                "native CMV requires a reconstructed live parent",
            )
        })?;
    if !vm.texture_parents.contains_key(&parent) {
        return Err(error(
            "SHINKI_EMU_CMVS_CMV_OWNER",
            "native CMV live parent has no image state",
        ));
    }
    let resource = saved.resource().map_err(core)?;
    let uri = resolve_cmv_uri(archive, vm, &resource, saved.reopen_mode())?;
    let mut decoder = archive.load_cmv(&uri).map_err(core)?;
    let metadata = decoder.metadata();
    let controller = saved
        .reconstruct(parent_identity, vm.current_frame, uri.clone(), metadata)
        .map_err(core)?;
    let frame = decoder
        .decode_frame(controller.current_frame)
        .map_err(core)?;
    let identity = vm.next_slot_object_id;
    let next_identity = identity
        .checked_add(1)
        .filter(|_| identity != 0 && !vm.slot_objects.values().any(|old| *old == identity))
        .ok_or_else(|| {
            error(
                "SHINKI_EMU_CMVS_VM_SLOT_OBJECTS",
                "native CMV object identity is invalid",
            )
        })?;
    // All fallible work precedes publication. No retained-root pointer is
    // imported from the old scene, and no command-776 root replacement occurs.
    vm.next_slot_object_id = next_identity;
    vm.slot_objects
        .insert((3184u32 << 8) | u32::from(slot), identity);
    let image = vm.texture_parents.get_mut(&parent).unwrap();
    image.resource = Some(resource);
    image.resource_frame = Some(vm.current_frame);
    image.image_info = [
        metadata.width,
        metadata.height,
        u32::from(metadata.has_alpha),
    ];
    if !image.surface_initialized {
        image.surface_initialized = true;
        if image.rect_words.is_none() {
            image.rect_words = Some([0, 0, metadata.width, metadata.height]);
        }
    }
    vm.cmv_controllers.insert(slot, controller);
    scene.bind_cmv_frame(parent, &uri, frame);
    players.insert(slot, CmvPlayer { decoder });
    Ok(())
}

impl CmvsSession {
    fn cmv_uri(&self, slot: u8, resource: &str) -> SessionResult<String> {
        let controller = self.vm.cmv_controllers.get(&slot).ok_or_else(|| {
            error(
                "SHINKI_EMU_CMVS_CMV_OWNER",
                "auxiliary video has no controller",
            )
        })?;
        resolve_cmv_uri(&self.archive, &self.vm, resource, controller.open_mode)
    }

    pub(super) fn open_cmv_resource(
        &mut self,
        slot: u8,
        parent: u8,
        resource: String,
        start: u32,
    ) -> SessionResult<()> {
        self.cmv_players.remove(&slot);
        self.scene.release_parent_surface(parent);
        let uri = self.cmv_uri(slot, &resource)?;
        let mut decoder = self.archive.load_cmv(&uri).map_err(core)?;
        let metadata = decoder.metadata();
        let frame = decoder.decode_frame(start).map_err(core)?;
        crate::vm::cmv::opened(&mut self.vm, slot, metadata).map_err(core)?;
        self.vm.cmv_controllers.get_mut(&slot).unwrap().source_uri = Some(uri.clone());
        let image = self.vm.texture_parents.get_mut(&parent).ok_or_else(|| {
            error(
                "SHINKI_EMU_CMVS_CMV_OWNER",
                "auxiliary video lost its graphics object",
            )
        })?;
        image.resource = Some(resource);
        image.resource_frame = Some(self.vm.current_frame);
        image.image_info = [
            metadata.width,
            metadata.height,
            u32::from(metadata.has_alpha),
        ];
        // 4430E0 creates a CTexture provider and allocates CBasicSurface only
        // when it was absent. A decoder failure never publishes this state.
        if !image.surface_initialized {
            image.surface_initialized = true;
            image.rect_words = Some([0, 0, metadata.width, metadata.height]);
            image.position_words = None;
            image.auxiliary_pair_words = None;
            image.auxiliary_word = None;
            image.alpha = None;
            image.solid_color = None;
            image.scale_words = None;
        }
        self.scene.bind_cmv_frame(parent, &uri, frame);
        self.cmv_players.insert(slot, CmvPlayer { decoder });
        tracing::info!(
            event = "cmvs.session.cmv_opened",
            controller = slot,
            parent,
            width = metadata.width,
            height = metadata.height,
            frames = metadata.frame_count,
            initial_frame = start,
            tick = self.vm.tick
        );
        Ok(())
    }

    pub(super) fn present_cmv_frame(&mut self, slot: u8, index: u32) -> SessionResult<()> {
        let Some(target) = crate::vm::cmv::bound_target(&self.vm, slot) else {
            self.cmv_players.remove(&slot);
            return Ok(());
        };
        let parent = match target {
            crate::vm::cmv::CmvTarget::Live(parent)
            | crate::vm::cmv::CmvTarget::ReloadPrevious(parent) => CmvsTextureSlot::Parent(parent),
            crate::vm::cmv::CmvTarget::SceneImage(slot)
            | crate::vm::cmv::CmvTarget::SceneReloadPrevious(slot) => {
                CmvsTextureSlot::SceneRoot(slot)
            }
        };
        if !self.cmv_players.contains_key(&slot) {
            let controller = self.vm.cmv_controllers.get(&slot).ok_or_else(|| {
                error(
                    "SHINKI_EMU_CMVS_CMV_OWNER",
                    "restored auxiliary video has no controller",
                )
            })?;
            let uri = match &controller.source_uri {
                Some(uri) => uri.clone(),
                None => self.cmv_uri(slot, &controller.resource)?,
            };
            let decoder = self.archive.load_cmv(&uri).map_err(core)?;
            let metadata = decoder.metadata();
            crate::vm::cmv::opened(&mut self.vm, slot, metadata).map_err(core)?;
            self.vm.cmv_controllers.get_mut(&slot).unwrap().source_uri = Some(uri);
            self.cmv_players.insert(slot, CmvPlayer { decoder });
        }
        let frame = self
            .cmv_players
            .get_mut(&slot)
            .unwrap()
            .decoder
            .decode_frame(index)
            .map_err(core)?;
        let uri = self
            .vm
            .cmv_controllers
            .get(&slot)
            .and_then(|controller| controller.source_uri.as_deref())
            .ok_or_else(|| {
                error(
                    "SHINKI_EMU_CMVS_CMV_OWNER",
                    "auxiliary video has no pinned source",
                )
            })?;
        match target {
            crate::vm::cmv::CmvTarget::Live(_) | crate::vm::cmv::CmvTarget::SceneImage(_) => {
                self.scene.bind_memory_image(parent, uri, frame)
            }
            crate::vm::cmv::CmvTarget::ReloadPrevious(_)
            | crate::vm::cmv::CmvTarget::SceneReloadPrevious(_) => {
                self.scene
                    .bind_retained_cmv_frame(&self.vm, parent, frame)
                    .map_err(core)?;
            }
        }
        tracing::debug!(
            event = "cmvs.session.cmv_frame",
            controller = slot,
            parent = ?parent,
            retained = matches!(target, crate::vm::cmv::CmvTarget::ReloadPrevious(_) | crate::vm::cmv::CmvTarget::SceneReloadPrevious(_)),
            frame = index,
            tick = self.vm.tick
        );
        Ok(())
    }
}

#[cfg(test)]
mod native_save_tests {
    use super::*;
    use crate::system_save::slot_cmv::{SavedCmv, SavedCmvRange};

    fn descriptor(slot: u32) -> SavedCmv {
        let mut source = [0; 64];
        source[..11].copy_from_slice(b"fixture.cmv");
        SavedCmv {
            version: 4,
            logical_index: slot,
            parent_slot: slot + 7,
            source,
            active_range: 0,
            pending_range: u32::MAX,
            playing: 1,
            paused: 0,
            ranges: [SavedCmvRange {
                repeat: 0,
                start: 0,
                end: 2,
            }; 16],
            current_frame: 2,
            mode: if slot == 1 { 15 } else { 0 },
            open_mode: 3,
            stop_frame: 0,
        }
    }

    #[test]
    #[ignore = "requires a hardware GPU"]
    fn gpu_native_restore_decodes_three_cmv_without_replacing_parent_trees_or_movies() {
        let (root, mut archive) = crate::archive::cache_tests::fixture();
        archive
            .bind_game_root(root.path().canonicalize().unwrap())
            .unwrap();
        let bytes = crate::cmv::tests::container(
            false,
            &[
                crate::cmv::tests::frame(None, -1024, false),
                crate::cmv::tests::frame(None, 0, true),
                crate::cmv::tests::frame(None, 0, false),
            ],
        );
        std::fs::write(root.path().join("fixture.cmv"), bytes).unwrap();
        let mut vm = CmvsPs2aVmState::new(0);
        vm.resource_search_paths.insert(
            CmvsResourceSearchBank::Native0d18,
            BTreeMap::from([(0, String::new())]),
        );
        for movie in 0..8u32 {
            vm.slot_objects
                .insert((3100 << 8) | movie, u64::from(movie) + 1);
        }
        for parent in 7..=9u8 {
            vm.slot_objects
                .insert((1924 << 8) | u32::from(parent), u64::from(parent) + 80);
            vm.texture_parents.insert(
                parent,
                CmvsTextureParentState {
                    surface_initialized: true,
                    rect_words: Some([2, 3, 4, 5]),
                    position_words: Some([6, 7]),
                    auxiliary_pair_words: Some([8, 9]),
                    alpha: Some(191),
                    ..Default::default()
                },
            );
            vm.texture_children.insert(
                parent,
                BTreeMap::from([(
                    12,
                    CmvsTextureChildState {
                        alpha: Some(127),
                        ..Default::default()
                    },
                )]),
            );
        }
        let parents = vm.texture_parents.clone();
        let children = vm.texture_children.clone();
        let objects = vm.slot_objects.clone();
        vm.next_slot_object_id = 100;
        let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
        let mut scene = CmvsScene::new(&instance).unwrap();
        let mut players = BTreeMap::new();
        for slot in 0..3u16 {
            restore_native_cmv(
                &mut vm,
                &mut scene,
                &archive,
                &mut players,
                slot,
                &descriptor(u32::from(slot)),
            )
            .unwrap();
        }
        assert_eq!(players.len(), 3);
        assert_eq!(vm.next_slot_object_id, 103);
        assert_eq!(vm.texture_children, children);
        for (parent, original) in parents {
            let actual = &vm.texture_parents[&parent];
            assert_eq!(actual.rect_words, original.rect_words);
            assert_eq!(actual.position_words, original.position_words);
            assert_eq!(actual.auxiliary_pair_words, original.auxiliary_pair_words);
            assert_eq!(actual.alpha, original.alpha);
            let identity = objects[&((1924 << 8) | u32::from(parent))];
            assert_eq!(
                vm.slot_objects[&((1924 << 8) | u32::from(parent))],
                identity
            );
            assert_eq!(vm.cmv_controllers[&(parent - 7)].parent_identity, identity);
        }
        for movie in 0..8u32 {
            assert_eq!(
                vm.slot_objects[&((3100 << 8) | movie)],
                objects[&((3100 << 8) | movie)]
            );
        }
        let mut visible = vm.clone();
        visible.texture_parents.get_mut(&8).unwrap().hidden = true;
        visible.texture_parents.get_mut(&9).unwrap().hidden = true;
        let gray = scene
            .render(
                &mut visible,
                CmvsTextureSlot::Parent(7),
                &archive,
                (16, 16),
                None,
            )
            .unwrap()
            .unwrap();
        let at = (3 * 16 + 3) * 4;
        for &component in &gray.rgba8[at..at + 3] {
            assert!(component.abs_diff(96) <= 1);
        }
        assert_eq!(&gray.rgba8[..4], &[0, 0, 0, 255]);
        visible.texture_parents.get_mut(&7).unwrap().hidden = true;
        visible.texture_parents.get_mut(&8).unwrap().hidden = false;
        let black = scene
            .render(
                &mut visible,
                CmvsTextureSlot::Parent(8),
                &archive,
                (16, 16),
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(&black.rgba8[at..at + 4], &[0, 0, 0, 255]);
        let full = scene
            .render(
                &mut vm,
                CmvsTextureSlot::Parent(7),
                &archive,
                (16, 16),
                None,
            )
            .unwrap()
            .unwrap();
        for controller in vm.cmv_controllers.values() {
            assert_eq!(controller.open_mode, 2);
            assert_eq!(
                (
                    controller.clock_remainder,
                    controller.elapsed_ms,
                    controller.audio_clock_ms
                ),
                (0, 0, None)
            );
        }
        assert!(SavedCmv::capture(&vm, 3).is_err());
        assert_eq!(SavedCmv::capture(&vm, 0).unwrap().current_frame, 2);

        // Invalid reopened metadata cannot publish even one partial controller.
        let mut invalid = descriptor(0);
        invalid.ranges[4].end = 3;
        let before = vm.clone();
        assert!(
            restore_native_cmv(&mut vm, &mut scene, &archive, &mut players, 0, &invalid).is_err()
        );
        assert_eq!(vm, before);
        assert_eq!(
            scene
                .render(
                    &mut vm,
                    CmvsTextureSlot::Parent(7),
                    &archive,
                    (16, 16),
                    None
                )
                .unwrap()
                .unwrap(),
            full
        );
    }
}
