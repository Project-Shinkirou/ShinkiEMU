use super::texture_reload::TextureReload;
use super::*;

/// The CImage moved out of the481 table by ordinary680. Its identity and
/// existing subtree survive the transfer; only the new linked shards are added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeImageShards {
    pub identity: u64,
    pub root: CmvsTextureParentState,
    pub children: BTreeMap<u16, CmvsTextureChildState>,
    pub(crate) reload: Option<TextureReload>,
    pub grid: CmvsNativeImageShardGrid,
}

impl CmvsNativeImageShards {
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        self.grid.validate()?;
        self.root.basic_effects.validate()?;
        for child in self.children.values() {
            child.basic_effects.validate()?;
        }
        if let Some(reload) = &self.reload {
            super::texture_reload::validate_owner(reload)?;
        }
        Ok(())
    }

    pub(crate) fn advance(&mut self, now: u32) -> Result<(), CoreError> {
        //4262A0 calls445660 only for the481/channel tables. After680 removes
        //this CImage from481,449450 invokes only415C60: old basics, indexed
        //children and4448A0's reload clock remain frozen. CMV providers are
        //updated independently by their pinned object identity.
        self.grid.advance(now.wrapping_sub(self.grid.clock_ms))
    }
}

pub(crate) fn transfer(
    vm: &mut CmvsPs2aVmState,
    slot: u32,
    parent: u32,
    parameters: [u32; 2],
) -> Result<bool, CoreError> {
    if slot > 3 {
        return Ok(false);
    }
    if parent > 255 {
        return Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_NATIVE_IMAGE_SHARDS",
            "source image index exceeds255",
        ));
    }
    let key = (1924_u32 << 8) | parent;
    let Some(&identity) = vm.slot_objects.get(&key) else {
        return Ok(false);
    };
    let source = vm.texture_parents.get(&(parent as u8)).ok_or_else(|| {
        CoreError::invalid(
            "SHINKI_EMU_CMVS_NATIVE_IMAGE_SHARDS",
            "live source image has no typed owner",
        )
    })?;
    let extent = [source.image_info[0], source.image_info[1]];
    let mut rng = vm.prng_state;
    let mut grid = CmvsNativeImageShardGrid::new(extent, parameters, &mut rng)?;
    grid.clock_ms = vm.session_clock_millis;
    let mut root = vm.texture_parents.remove(&(parent as u8)).unwrap();
    root.reference_size = Some(extent);
    let image = CmvsNativeImageShards {
        identity,
        root,
        children: vm
            .texture_children
            .remove(&(parent as u8))
            .unwrap_or_default(),
        reload: vm.texture_reloads.remove(&(parent as u8)),
        grid,
    };
    super::native_scene_worker::remove_slot(vm, slot);
    vm.slot_objects.remove(&key);
    vm.slot_object_seed_words.remove(&key);
    vm.write_interpreter_word(1924 + parent * 4, 0);
    vm.native_scene_workers.insert(
        slot as u8,
        CmvsNativeSceneWorker::ImageShards(Box::new(image)),
    );
    vm.write_interpreter_word(2948 + slot * 4, 1);
    vm.prng_state = rng;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::parent_load_tests::command;

    fn occupied() -> CmvsPs2aVmState {
        let mut vm = CmvsPs2aVmState::new(0);
        command(&mut vm, 32, &[0]);
        vm.texture_parents.get_mut(&0).unwrap().image_info = [40, 20, 1];
        vm.session_clock_millis = 1000;
        vm
    }

    #[test]
    fn absent_source_and_ignored_worker_slot_preserve_the_existing_owner() {
        let mut vm = occupied();
        super::super::native_scene_worker::start_scene50(
            &mut vm,
            0,
            [b"a".to_vec(), b"b".to_vec()],
        )
        .unwrap();
        let rng = vm.prng_state;
        assert!(!transfer(&mut vm, 0, 99, [20, 0]).unwrap());
        assert!(matches!(
            vm.native_scene_workers[&0],
            CmvsNativeSceneWorker::Scene50(_)
        ));
        assert!(!transfer(&mut vm, 4, u32::MAX, [0, u32::MAX]).unwrap());
        assert_eq!(vm.prng_state, rng);
        assert!(vm.texture_parents.contains_key(&0));
    }

    #[test]
    fn private_shard_geometry_continues_but_native_marker_retains_only_the_empty_wrapper() {
        let mut vm = occupied();
        transfer(&mut vm, 0, 0, [20, 0xdeadbeef]).unwrap();
        assert!(!vm.texture_parents.contains_key(&0));
        assert!(!vm.slot_objects.contains_key(&(1924 << 8)));
        command(&mut vm, 686, &[0]);
        assert_eq!(vm.interpreter_words[&81220], 0);
        vm.session_clock_millis = 1100;
        super::super::native_scene_worker::tick(&mut vm, 100).unwrap();
        let bytes = postcard::to_allocvec(&vm).unwrap();
        let mut private: CmvsPs2aVmState = postcard::from_bytes(&bytes).unwrap();
        private.session_clock_millis = 1200;
        super::super::native_scene_worker::tick(&mut private, 100).unwrap();
        let image = private.native_scene_workers[&0].image_shards().unwrap();
        assert_eq!(
            image.grid.draw_basic(0).unwrap().auxiliary_pair_words,
            Some([9, 31])
        );
        let records = super::super::native_scene_worker::capture(&vm).unwrap();
        assert_eq!(records[&0].encode().unwrap(), [1, 0, 8, 0]);
        let mut native = CmvsPs2aVmState::new(0);
        super::super::native_scene_worker::restore(&mut native, &records).unwrap();
        command(&mut native, 686, &[0]);
        assert_eq!(native.interpreter_words[&81220], u32::MAX);
        assert_eq!(
            super::super::native_scene_worker::capture(&native).unwrap()[&0]
                .encode()
                .unwrap(),
            [1, 0, 0, 0]
        );
        command(&mut native, 672, &[0]);
        assert!(!native.native_scene_workers.contains_key(&0));
    }

    #[test]
    fn transferred_basics_and_reload_stop_ticking_without_destroying_the_image() {
        let mut vm = occupied();
        vm.texture_parents.get_mut(&0).unwrap().surface_initialized = true;
        super::super::texture_reload::begin(&mut vm, 0, 100).unwrap();
        let reload = vm.texture_reloads.get_mut(&0).unwrap();
        reload.mode = super::super::texture_reload::ReloadMode::Fast;
        reload.elapsed_ms = 50;
        reload.current_uri = Some("new.pb3".to_owned());
        let mut alpha = CmvsAlphaEffect {
            active: 1,
            origin: 64,
            elapsed_ms: 50,
            ..Default::default()
        };
        alpha.segments[0] = CmvsAlphaSegment {
            next: -2,
            target: 255,
            step: 1,
            duration_ms: 100,
        };
        let root = vm.texture_parents.get_mut(&0).unwrap();
        root.surface_initialized = true;
        root.image_info = [40, 20, 1];
        root.alpha = Some(64);
        root.basic_effects.alpha = Some(alpha.clone());
        vm.texture_children.get_mut(&0).unwrap().insert(
            0,
            CmvsTextureChildState {
                surface_initialized: true,
                alpha: Some(32),
                basic_effects: CmvsBasicEffects {
                    alpha: Some(alpha),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        transfer(&mut vm, 0, 0, [20, 0]).unwrap();
        vm.session_clock_millis = 2000;
        super::super::native_scene_worker::tick(&mut vm, 1000).unwrap();
        let image = vm.native_scene_workers[&0].image_shards().unwrap();
        assert_eq!(image.root.alpha, Some(64));
        assert_eq!(
            image.root.basic_effects.alpha.as_ref().unwrap().elapsed_ms,
            50
        );
        assert_eq!(image.children[&0].alpha, Some(32));
        assert_eq!(image.reload.as_ref().unwrap().elapsed_ms, 50);
        assert_eq!(image.grid.status(), -1);
    }
}
