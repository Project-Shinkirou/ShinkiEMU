use super::*;
use crate::system_save::slot_image::{SavedBasic, SavedImage, SavedProvider, SavedProviderSource};
use crate::system_save::slot_io::invalid;
use crate::{CmvsResourceSearchBank, CmvsTextureChildState, CmvsTextureParentState};

fn native_name(name: &str) -> Result<Vec<u8>, CoreError> {
    let (bytes, _, malformed) = encoding_rs::SHIFT_JIS.encode(name);
    if malformed || bytes.len() >= 64 || bytes.contains(&0) {
        return Err(invalid("native image resource exceeds its CP932 buffer"));
    }
    Ok(bytes.into_owned())
}
fn resource_name(bytes: &[u8]) -> Result<String, CoreError> {
    encoding_rs::SHIFT_JIS
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|name| name.into_owned())
        .ok_or_else(|| invalid("native image resource is not valid CP932"))
}

fn capture_provider(
    image: &CmvsTextureParentState,
    dynamic: bool,
) -> Result<Option<SavedProvider>, CoreError> {
    if image.resource.is_none() && !dynamic {
        return Ok(None);
    }
    let filter = image
        .color_filter
        .map_or([0; 4], |[mode, color, kernel]| [1, mode, color, kernel]);
    Ok(Some(SavedProvider {
        load_flags: 0,
        frame: 0,
        kind: if dynamic { 3 } else { 0 },
        filter,
        dynamic_parameter: if dynamic { image.image_info[2] } else { 0 },
        source: SavedProviderSource::File {
            // 43F160 writes an empty resource for the dynamic CMV surface.
            resource: if dynamic {
                Vec::new()
            } else {
                native_name(image.resource.as_deref().unwrap())?
            },
            mask: Vec::new(),
            archive: 0,
            width: image.image_info[0],
            height: image.image_info[1],
        },
    }))
}
fn capture_basic(image: &CmvsTextureParentState) -> Result<Option<SavedBasic>, CoreError> {
    if !image.surface_initialized {
        return Ok(None);
    }
    let mut basic = SavedBasic::default();
    let w = &mut basic.words;
    w[0] = u32::from(image.basic_kind);
    w[1] = u32::from(image.blend_mode.unwrap_or(1));
    w[2..6].copy_from_slice(&image.rect_words.unwrap_or([0; 4]));
    w[6..8].copy_from_slice(&image.position_words.unwrap_or([0; 2]));
    w[8..10].copy_from_slice(&image.auxiliary_pair_words.unwrap_or([0; 2]));
    w[10] = image.auxiliary_word.unwrap_or(0);
    w[11..14].copy_from_slice(&image.float_position_words);
    w[14] = image.solid_color.unwrap_or(0);
    w[15] = image.projection_depth_word;
    w[16] = image.projection_height_word;
    w[17] = image.alpha.unwrap_or(255) as u32;
    w[18..20].copy_from_slice(&image.scale_words.unwrap_or([1.0_f32.to_bits(); 2]));
    w[20] = image.rotation_hundredths as u32;
    basic.capture_effects(&image.basic_effects)?;
    Ok(Some(basic))
}
fn parent_from_child(child: &CmvsTextureChildState) -> CmvsTextureParentState {
    CmvsTextureParentState {
        resource: child.resource.clone(),
        resource_frame: child.resource_frame,
        image_info: child.image_info,
        surface_initialized: child.surface_initialized,
        hidden: child.hidden,
        screen_shake_disabled: child.screen_shake_disabled,
        alpha: child.alpha,
        solid_color: child.solid_color,
        scale_words: child.scale_words,
        basic_kind: child.basic_kind,
        blend_mode: child.blend_mode,
        float_position_words: child.float_position_words,
        projection_depth_word: child.projection_depth_word,
        projection_height_word: child.projection_height_word,
        rotation_hundredths: child.rotation_hundredths,
        reference_size: child.reference_size,
        rect_words: child.rect_words,
        position_words: child.position_words,
        auxiliary_pair_words: child.auxiliary_pair_words,
        auxiliary_word: child.auxiliary_word,
        color_filter: None,
        basic_effects: child.basic_effects.clone(),
        image_effect_words: child.image_effect_words,
        collision_geometry: child.collision_geometry.clone(),
    }
}
fn child_from_parent(parent: CmvsTextureParentState, detached: bool) -> CmvsTextureChildState {
    CmvsTextureChildState {
        resource: parent.resource,
        resource_frame: parent.resource_frame,
        image_info: parent.image_info,
        surface_initialized: parent.surface_initialized,
        hidden: parent.hidden,
        parent_context_detached: detached,
        screen_shake_disabled: parent.screen_shake_disabled,
        alpha: parent.alpha,
        solid_color: parent.solid_color,
        scale_words: parent.scale_words,
        basic_kind: parent.basic_kind,
        blend_mode: parent.blend_mode,
        float_position_words: parent.float_position_words,
        projection_depth_word: parent.projection_depth_word,
        projection_height_word: parent.projection_height_word,
        rotation_hundredths: parent.rotation_hundredths,
        reference_size: parent.reference_size,
        rect_words: parent.rect_words,
        position_words: parent.position_words,
        auxiliary_pair_words: parent.auxiliary_pair_words,
        auxiliary_word: parent.auxiliary_word,
        basic_effects: parent.basic_effects,
        image_effect_words: parent.image_effect_words,
        collision_geometry: parent.collision_geometry,
    }
}

/// Shared by the top-level texture table and channel-owned CImage roots.
pub(crate) fn capture_image_state(
    parent: &CmvsTextureParentState,
    children: Option<&BTreeMap<u16, CmvsTextureChildState>>,
    dynamic: bool,
) -> Result<SavedImage, CoreError> {
    let mut saved = SavedImage::default();
    saved.header[0] = u32::from(parent.surface_initialized);
    saved.header[1] = u32::from(!parent.hidden);
    if parent.screen_shake_disabled {
        saved.header[2] &= !0x100;
    }
    if let Some(reference) = parent.reference_size {
        saved.header[3..].copy_from_slice(&reference);
    }
    saved.primary = capture_provider(parent, dynamic)?;
    saved.basic = capture_basic(parent)?;
    if parent.image_effect_words[0] != 0 {
        return Err(invalid("native image effect family is not modeled"));
    }
    saved.effect_words = parent.image_effect_words;
    saved.collision_geometry = parent.collision_geometry.clone();
    if let Some(children) = children {
        // Native root helpers mark the object as a child container even when empty.
        saved.header[2] |= 0x8000_0000;
        for (id, child) in children {
            if *id >= 1024 {
                return Err(invalid("native image child id exceeds 1023"));
            }
            let mut image = capture_image_state(&parent_from_child(child), None, false)?;
            if child.parent_context_detached {
                image.header[2] &= !1;
            }
            saved.children.insert(*id, image);
        }
    }
    Ok(saved)
}

/// Reject semantic state for which the runtime has no corresponding consumer.
/// The wire codec retains these fields so importing then exporting cannot erase them.
fn restore_single(
    saved: &SavedImage,
    child: bool,
    retain_flags: bool,
) -> Result<CmvsTextureParentState, CoreError> {
    if saved.unsupported_object_1861 {
        return Err(invalid("native image object 1861 is not modeled"));
    }
    if saved.secondary.is_some() {
        return Err(invalid("native secondary image provider is not modeled"));
    }
    // 445EB0 clears DWORD1040 and only reads the remaining parameters when
    // the serialized enable word is nonzero. Disabled tail words are stale.
    if saved.effect_words[0] != 0 {
        return Err(invalid("native image effect family is not modeled"));
    }
    if saved.header[0] > 1
        || (!retain_flags && saved.header[1] > 1)
        || (saved.header[0] != 0) != saved.basic.is_some()
    {
        return Err(invalid(
            "native image allocation or visibility mode is not modeled",
        ));
    }
    let supported = 0x8000_0100 | if child { 1 } else { 0 };
    if !retain_flags && (saved.header[2] ^ 0x7fff_ffff) & !supported != 0 {
        return Err(invalid("native image inheritance flags are not modeled"));
    }
    let mut image = CmvsTextureParentState {
        surface_initialized: saved.basic.is_some(),
        hidden: saved.header[1] == 0,
        screen_shake_disabled: saved.header[2] & 0x100 == 0,
        reference_size: (saved.header[3..] != [0, 0]).then_some([saved.header[3], saved.header[4]]),
        image_effect_words: saved.effect_words,
        collision_geometry: saved.collision_geometry.clone(),
        ..Default::default()
    };
    if let Some(provider) = &saved.primary {
        let SavedProviderSource::File {
            resource,
            mask,
            width,
            height,
            ..
        } = &provider.source
        else {
            return Err(invalid("native composed texture provider is not modeled"));
        };
        if !mask.is_empty() {
            return Err(invalid(
                "native external image mask provider is not modeled",
            ));
        }
        if provider.frame != 0 || provider.load_flags > 1 || provider.filter[0] > 1 {
            return Err(invalid(
                "native image provider frame or flags are not modeled",
            ));
        }
        match provider.kind {
            0 => {
                if provider.dynamic_parameter != 0
                    || resource.is_empty()
                    || provider.load_flags != 0
                {
                    return Err(invalid(
                        "native blank, resampled or parameterized file provider is not modeled",
                    ));
                }
                image.resource = (!resource.is_empty())
                    .then(|| resource_name(resource))
                    .transpose()?;
                image.image_info = [*width, *height, 1];
            }
            3 => {
                if !resource.is_empty() || provider.dynamic_parameter > 1 || provider.filter[0] != 0
                {
                    return Err(invalid(
                        "native dynamic image provider fields are not modeled",
                    ));
                }
                image.image_info = [*width, *height, provider.dynamic_parameter];
            }
            _ => return Err(invalid("native image provider kind is not modeled")),
        }
        if provider.filter[0] != 0 {
            image.color_filter = Some([provider.filter[1], provider.filter[2], provider.filter[3]]);
        } else if provider.filter[1..].iter().any(|word| *word != 0) {
            return Err(invalid(
                "native disabled image filter parameters are not modeled",
            ));
        }
    }
    if let Some(basic) = &saved.basic {
        let w = &basic.words;
        if w[22..].iter().any(|word| *word != 0) || w[0] > 5 || w[0] == 4 || w[1] > 3 {
            return Err(invalid(
                "native basic effect, kind or extended fields are not modeled",
            ));
        }
        image.basic_kind = w[0] as u8;
        image.blend_mode = Some(w[1] as u8);
        image.rect_words = Some(w[2..6].try_into().unwrap());
        image.position_words = Some(w[6..8].try_into().unwrap());
        image.auxiliary_pair_words = Some(w[8..10].try_into().unwrap());
        image.auxiliary_word = Some(w[10]);
        image.float_position_words.copy_from_slice(&w[11..14]);
        image.solid_color = (w[14] != 0).then_some(w[14]);
        image.projection_depth_word = w[15];
        image.projection_height_word = w[16];
        image.alpha = Some(w[17] as i32);
        image.scale_words = Some(w[18..20].try_into().unwrap());
        image.rotation_hundredths = w[20] as i32;
        // 42E260 deliberately resets transient reload-opacity DWORD21 to 256.
        image.basic_effects = basic.restore_effects()?;
    }
    Ok(image)
}

pub(crate) fn restore_image_state(
    saved: &SavedImage,
) -> Result<(CmvsTextureParentState, BTreeMap<u16, CmvsTextureChildState>), CoreError> {
    restore_image_tree(saved, false)
}

/// Channel consumers retain the complete inheritance DWORD in their element model.
pub(crate) fn restore_channel_image_state(
    saved: &SavedImage,
) -> Result<(CmvsTextureParentState, BTreeMap<u16, CmvsTextureChildState>), CoreError> {
    restore_image_tree(saved, true)
}

fn restore_image_tree(
    saved: &SavedImage,
    retain_flags: bool,
) -> Result<(CmvsTextureParentState, BTreeMap<u16, CmvsTextureChildState>), CoreError> {
    let parent = restore_single(saved, false, retain_flags)?;
    if !saved.children.is_empty() && saved.header[2] as i32 >= 0 {
        return Err(invalid(
            "native image children require the native container flag",
        ));
    }
    let mut children = BTreeMap::new();
    for (id, image) in &saved.children {
        if *id >= 1024 || !image.children.is_empty() {
            return Err(invalid(
                "runtime native texture tree supports one child level with ids below 1024",
            ));
        }
        let restored = restore_single(image, true, retain_flags)?;
        if restored.color_filter.is_some() {
            return Err(invalid("native child image filter is not modeled"));
        }
        children.insert(*id, child_from_parent(restored, image.header[2] & 1 == 0));
    }
    Ok((parent, children))
}

impl CmvsScene {
    /// Native 4262A0/445660: update allocated basics even while hidden, then
    /// delete images whose effect terminated with -2 before composition.
    pub(crate) fn advance_native_image_effects(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        delta_ms: u32,
    ) -> Result<(), CoreError> {
        use crate::vm::image_effects::advance_texture_fields;
        for parent in 0..=u8::MAX {
            let Some(image) = vm.texture_parents.get_mut(&parent) else {
                continue;
            };
            let destroy = advance_texture_fields(
                image.surface_initialized,
                &mut image.basic_effects,
                &mut image.alpha,
                &mut image.rect_words,
                &mut image.position_words,
                delta_ms,
                &mut image.float_position_words,
            )?;
            if let Some(children) = vm.texture_children.get_mut(&parent) {
                let mut failure = None;
                children.retain(|child, image| {
                    match advance_texture_fields(
                        image.surface_initialized,
                        &mut image.basic_effects,
                        &mut image.alpha,
                        &mut image.rect_words,
                        &mut image.position_words,
                        delta_ms,
                        &mut image.float_position_words,
                    ) {
                        Ok(false) => true,
                        Ok(true) => {
                            self.dynamic_images.remove(&CmvsTextureSlot::Child {
                                parent,
                                child: *child,
                            });
                            self.release_child_surface(parent, *child);
                            false
                        }
                        Err(error) => {
                            failure = Some(error);
                            true
                        }
                    }
                });
                if let Some(error) = failure {
                    return Err(error);
                }
            }
            if destroy {
                vm.texture_parents.remove(&parent);
                vm.texture_children.remove(&parent);
                vm.texture_reloads.remove(&parent);
                let key = (1924_u32 << 8) | u32::from(parent);
                vm.slot_objects.remove(&key);
                vm.slot_object_seed_words.remove(&key);
                vm.channel_child_presenters
                    .retain(|(owner, _), _| *owner != u32::from(parent));
                self.release_parent_surface(parent);
            }
        }
        // Channels have their own image roots, but use the same CBasic owners
        // and atlas source/pivot fields consumed by append_channel_images.
        let mut live_channels = [false; 8];
        for (index, live) in live_channels.iter_mut().enumerate() {
            *live = vm.effect_channels.contains_key(&(index as u8))
                && vm.effect_elements.contains_key(&(index as u8, u32::MAX));
        }
        let mut failure = None;
        vm.effect_elements.retain(|(channel, child), image| {
            if !live_channels
                .get(*channel as usize)
                .copied()
                .unwrap_or(false)
                || !image.basic_effects.active()
            {
                return true;
            }
            let Some(source) = image.source.as_mut() else {
                return true;
            };
            let mut source_words = source.map(|word| word as u32);
            let mut pivot_words = image.pivot.map(|word| word as u32);
            match image
                .basic_effects
                .advance(
                    delta_ms,
                    &mut image.alpha,
                    &mut source_words,
                    &mut pivot_words,
                )
                .and_then(|destroy| {
                    image
                        .basic_effects
                        .advance_motion(delta_ms, &mut image.float_position_words)
                        .map(|motion_destroy| destroy | motion_destroy)
                }) {
                Ok(destroy) => {
                    *source = source_words.map(|word| word as i32);
                    image.pivot = pivot_words.map(|word| word as i32);
                    // 4262A0 does not delete the channel's root on DWORD3099.
                    !destroy || *child == u32::MAX
                }
                Err(error) => {
                    failure = Some(error);
                    true
                }
            }
        });
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn capture_native_image(
        &self,
        vm: &CmvsPs2aVmState,
        parent: u8,
        archive: &CmvsArchive,
    ) -> Result<SavedImage, CoreError> {
        let image = vm
            .texture_parents
            .get(&parent)
            .ok_or_else(|| invalid("native image capture has no live parent"))?;
        let slot = CmvsTextureSlot::Parent(parent);
        let mut saved = capture_image_state(
            image,
            vm.texture_children.get(&parent),
            self.dynamic_images.contains_key(&slot),
        )?;
        if let Some(provider) = &mut saved.primary {
            self.capture_provider_reference(
                vm,
                archive,
                slot,
                image.resource.as_deref(),
                provider,
            )?;
        }
        if let Some(children) = vm.texture_children.get(&parent) {
            for (id, child) in children {
                let slot = CmvsTextureSlot::Child { parent, child: *id };
                if self.dynamic_images.contains_key(&slot) {
                    return Err(invalid("native dynamic child provider is not modeled"));
                }
                if let Some(provider) = &mut saved.children.get_mut(id).unwrap().primary {
                    self.capture_provider_reference(
                        vm,
                        archive,
                        slot,
                        child.resource.as_deref(),
                        provider,
                    )?;
                }
            }
        }
        Ok(saved)
    }

    fn capture_provider_reference(
        &self,
        vm: &CmvsPs2aVmState,
        archive: &CmvsArchive,
        slot: CmvsTextureSlot,
        name: Option<&str>,
        provider: &mut SavedProvider,
    ) -> Result<(), CoreError> {
        if provider.kind == 3 {
            return Ok(());
        }
        let (Some(name), Some(uri)) = (name, self.bindings.get(&slot)) else {
            return Err(invalid(
                "native image provider has no corresponding live binding",
            ));
        };
        let marker = [
            (0, CmvsResourceSearchBank::Texture),
            (1, CmvsResourceSearchBank::Native0d00),
        ]
        .into_iter()
        .find_map(|(marker, bank)| {
            archive
                .resolve_native_resource(vm, bank, name)
                .ok()
                .filter(|found| found == uri)
                .map(|_| marker)
        })
        .ok_or_else(|| {
            invalid("native image binding cannot be represented by its saved resource reference")
        })?;
        if let SavedProviderSource::File {
            archive: source,
            width,
            height,
            ..
        } = &mut provider.source
        {
            *source = marker;
            let info = archive.native_image_info(uri)?;
            *width = info[0];
            *height = info[1];
        }
        Ok(())
    }

    pub(crate) fn restore_native_image(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        parent: u8,
        saved: &SavedImage,
        archive: &CmvsArchive,
    ) -> Result<(), CoreError> {
        let (mut root, mut children) = restore_image_state(saved)?;
        root.resource_frame = root.resource.as_ref().map(|_| vm.current_frame);
        for child in children.values_mut() {
            child.resource_frame = child.resource.as_ref().map(|_| vm.current_frame);
        }
        let identity = vm.next_slot_object_id;
        vm.next_slot_object_id = identity
            .checked_add(1)
            .ok_or_else(|| invalid("native image identity space overflowed"))?;
        vm.slot_objects
            .insert((1924_u32 << 8) | u32::from(parent), identity);
        vm.texture_reloads.remove(&parent);
        self.release_image_reload(parent);
        let stale = self
            .bindings
            .keys()
            .filter_map(|slot| match slot {
                CmvsTextureSlot::Child {
                    parent: owner,
                    child,
                } if *owner == parent => Some(*child),
                _ => None,
            })
            .collect::<Vec<_>>();
        for child in stale {
            self.dynamic_images
                .remove(&CmvsTextureSlot::Child { parent, child });
            self.release_child_surface(parent, child);
        }
        self.clear_parent_image(parent);
        self.slot_geometry.remove(&CmvsTextureSlot::Parent(parent));
        vm.texture_parents.insert(parent, root);
        vm.texture_children.insert(parent, children);
        self.restore_provider_binding(
            vm,
            CmvsTextureSlot::Parent(parent),
            saved.primary.as_ref(),
            archive,
        )?;
        for (id, child) in &saved.children {
            self.restore_provider_binding(
                vm,
                CmvsTextureSlot::Child { parent, child: *id },
                child.primary.as_ref(),
                archive,
            )?;
        }
        Ok(())
    }

    fn restore_provider_binding(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        slot: CmvsTextureSlot,
        provider: Option<&SavedProvider>,
        archive: &CmvsArchive,
    ) -> Result<(), CoreError> {
        let Some(provider) = provider else {
            return Ok(());
        };
        let SavedProviderSource::File {
            resource,
            archive: marker,
            width,
            height,
            ..
        } = &provider.source
        else {
            return Err(invalid("native composed provider is not modeled"));
        };
        if provider.kind == 0 && !resource.is_empty() {
            let bank = if *marker != 0 {
                CmvsResourceSearchBank::Native0d00
            } else {
                CmvsResourceSearchBank::Texture
            };
            let uri = archive.resolve_native_resource(vm, bank, &resource_name(resource)?)?;
            match slot {
                CmvsTextureSlot::Parent(parent) => {
                    self.bind_parent_image(vm, parent, &uri, archive)?
                }
                CmvsTextureSlot::Child { parent, child } => {
                    self.bind_child_image(vm, parent, child, &uri, archive)?
                }
                CmvsTextureSlot::SceneRoot(_) | CmvsTextureSlot::SceneChild { .. } => {
                    return Err(invalid(
                        "native image records require a481-table image owner",
                    ));
                }
            }
        } else {
            // Native kind 3 is an allocated dynamic texture, not a PB/CMV file.
            // Its CMV controller (if saved) supplies decoded pixels after root construction.
            let size = (*width as usize)
                .checked_mul(*height as usize)
                .and_then(|size| size.checked_mul(4))
                .filter(|size| {
                    *size > 0 && *size <= 1024 * 1024 * 1024 && *width <= 16384 && *height <= 16384
                })
                .ok_or_else(|| {
                    invalid("native dynamic provider extent exceeds the texture budget")
                })?;
            let mut pixels = vec![0; size];
            if provider.kind == 0 || provider.dynamic_parameter == 0 {
                for pixel in pixels.as_chunks_mut::<4>().0 {
                    pixel[3] = 255;
                }
            }
            self.bindings
                .insert(slot, format!("cmvs.native.dynamic.{slot:?}"));
            self.dynamic_images.insert(
                slot,
                TextureFrame {
                    width: *width,
                    height: *height,
                    rgba8: pixels.into(),
                },
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::parent_load_tests::{fixture, write_image};
    use super::*;
    #[test]
    fn native_basic_mapping_preserves_raw_geometry_and_filter() {
        let root = CmvsTextureParentState {
            surface_initialized: true,
            alpha: Some(-7),
            solid_color: Some(0xff12_3456),
            rect_words: Some([3, 4, 91, 53]),
            position_words: Some([7, 11]),
            auxiliary_pair_words: Some([101, 103]),
            auxiliary_word: Some(19),
            scale_words: Some([0x3f800001, 0x80000000]),
            rotation_hundredths: 3700,
            reference_size: Some([800, 600]),
            hidden: true,
            screen_shake_disabled: true,
            ..Default::default()
        };
        let child = CmvsTextureChildState {
            surface_initialized: true,
            parent_context_detached: true,
            position_words: Some([13, 17]),
            alpha: Some(91),
            ..Default::default()
        };
        let saved =
            capture_image_state(&root, Some(&BTreeMap::from([(27, child)])), false).unwrap();
        let w = &saved.basic.as_ref().unwrap().words;
        assert_eq!(w[14], 0xff12_3456);
        assert_eq!(w[20], 3700);
        assert_eq!(w[17] as i32, -7);
        assert_eq!(&w[6..10], &[7, 11, 101, 103]);
        let (restored, children) =
            restore_image_state(&SavedImage::decode(&saved.encode().unwrap()).unwrap()).unwrap();
        assert_eq!(restored.position_words, root.position_words);
        assert_eq!(restored.scale_words, root.scale_words);
        assert_eq!(restored.alpha, root.alpha);
        assert_eq!(restored.rotation_hundredths, root.rotation_hundredths);
        let fractional = CmvsTextureParentState {
            surface_initialized: true,
            rotation_hundredths: -537,
            ..Default::default()
        };
        let saved = capture_image_state(&fractional, None, false).unwrap();
        assert_eq!(saved.basic.as_ref().unwrap().words[20], (-537_i32) as u32);
        let (restored, _) =
            restore_image_state(&SavedImage::decode(&saved.encode().unwrap()).unwrap()).unwrap();
        assert_eq!(restored.rotation_hundredths, -537);
        assert!(children[&27].parent_context_detached);
        assert_eq!(children[&27].position_words, Some([13, 17]));
    }
    #[test]
    fn unsupported_semantics_are_errors_and_dynamic_is_not_a_file() {
        let root = CmvsTextureParentState {
            resource: Some("movie.cmv".into()),
            image_info: [2, 3, 1],
            ..Default::default()
        };
        let mut saved = capture_image_state(&root, None, true).unwrap();
        assert_eq!(saved.primary.as_ref().unwrap().kind, 3);
        assert_eq!(restore_image_state(&saved).unwrap().0.resource, None);
        saved.effect_words[0] = 1;
        assert!(restore_image_state(&saved).is_err());
        saved.effect_words[0] = 0;
        saved.primary.as_mut().unwrap().kind = 5;
        assert!(restore_image_state(&saved).is_err());
    }
    #[test]
    fn hardware_native_restore_rebinds_real_providers_and_preserves_draw_geometry() {
        let (root, archive, mut vm) = fixture();
        let mut scene = CmvsScene::new(super::super::tests::gpu_instance()).unwrap();
        write_image(root.path(), "plane.pb3", 8, 6, [240, 80, 24, 255]);
        vm.texture_parents.insert(
            3,
            CmvsTextureParentState {
                resource: Some("plane.pb3".into()),
                image_info: [8, 6, 0],
                surface_initialized: true,
                rect_words: Some([1, 1, 5, 4]),
                position_words: Some([2, 1]),
                auxiliary_pair_words: Some([7, 6]),
                alpha: Some(113),
                color_filter: Some([2, 0x00808080, 0]),
                reference_size: Some([32, 24]),
                ..Default::default()
            },
        );
        vm.texture_children.insert(
            3,
            BTreeMap::from([(
                9,
                CmvsTextureChildState {
                    surface_initialized: true,
                    rect_words: Some([2, 1, 3, 2]),
                    auxiliary_pair_words: Some([5, 3]),
                    alpha: Some(171),
                    ..Default::default()
                },
            )]),
        );
        let uri = archive
            .resolve_native_resource(&vm, CmvsResourceSearchBank::Texture, "plane.pb3")
            .unwrap();
        scene.bind_parent_image(&mut vm, 3, &uri, &archive).unwrap();
        let before = scene
            .render(
                &mut vm,
                CmvsTextureSlot::Parent(3),
                &archive,
                (32, 24),
                None,
            )
            .unwrap()
            .unwrap();
        vm.resource_search_paths.insert(
            CmvsResourceSearchBank::Texture,
            BTreeMap::from([(0, "absent/".into())]),
        );
        let saved = scene.capture_native_image(&vm, 3, &archive).unwrap();
        let saved = SavedImage::decode(&saved.encode().unwrap()).unwrap();
        assert!(matches!(
            saved.primary.as_ref().unwrap().source,
            SavedProviderSource::File { archive: 1, .. }
        ));
        let mut restored = CmvsPs2aVmState::new(0);
        restored.resource_search_paths = vm.resource_search_paths.clone();
        scene
            .restore_native_image(&mut restored, 3, &saved, &archive)
            .unwrap();
        assert_eq!(scene.bindings[&CmvsTextureSlot::Parent(3)], uri);
        assert_eq!(
            restored.texture_parents[&3].color_filter,
            Some([2, 0x00808080, 0])
        );
        assert_eq!(
            restored.texture_children[&3][&9].rect_words,
            Some([2, 1, 3, 2])
        );
        assert_eq!(
            restored.texture_parents[&3].image_info,
            vm.texture_parents[&3].image_info
        );
        assert_ne!(restored.slot_objects[&((1924 << 8) | 3)], 0);
        let after = scene
            .render(
                &mut restored,
                CmvsTextureSlot::Parent(3),
                &archive,
                (32, 24),
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(before.rgba8.as_slice(), after.rgba8.as_slice());
        let dynamic = capture_image_state(
            &CmvsTextureParentState {
                image_info: [2, 3, 1],
                surface_initialized: true,
                rect_words: Some([0, 0, 2, 3]),
                ..Default::default()
            },
            None,
            true,
        )
        .unwrap();
        scene
            .restore_native_image(&mut restored, 4, &dynamic, &archive)
            .unwrap();
        assert_eq!(
            scene.dynamic_images[&CmvsTextureSlot::Parent(4)]
                .rgba8
                .as_slice(),
            &[0; 24]
        );
        assert_eq!(restored.texture_parents[&4].resource, None);
        assert_eq!(
            scene
                .capture_native_image(&restored, 4, &archive)
                .unwrap()
                .primary
                .unwrap()
                .kind,
            3
        );
    }

    #[test]
    fn native_disabled_image_effect_tail_survives_runtime_and_capture() {
        let mut saved = SavedImage {
            effect_words: [0, 0, 0, u32::MAX, 0xdeadbeef, 6, 7, 8, 9, 10, 0],
            ..Default::default()
        };
        let (root, _) = restore_image_state(&saved).unwrap();
        assert_eq!(
            capture_image_state(&root, None, false)
                .unwrap()
                .effect_words,
            saved.effect_words
        );
        saved.effect_words[0] = 1;
        assert!(restore_image_state(&saved).is_err());
    }

    #[test]
    #[ignore = "requires a hardware GPU"]
    fn hardware_native_alpha_effect_restores_clock_and_changes_real_pixels() {
        use crate::vm::{CmvsAlphaEffect, CmvsAlphaSegment, CmvsBasicEffects};
        let (root, archive, mut vm) = fixture();
        let mut scene = CmvsScene::new(super::super::tests::gpu_instance()).unwrap();
        write_image(root.path(), "alpha.pb3", 2, 1, [255, 0, 0, 255]);
        let parent = CmvsTextureParentState {
            // An atlas owner can have a degenerate basic while its child draws.
            surface_initialized: true,
            rect_words: Some([0; 4]),
            resource: Some("alpha.pb3".into()),
            image_info: [2, 1, 1],
            ..Default::default()
        };
        let mut alpha = CmvsAlphaEffect {
            active: 1,
            elapsed_ms: 625,
            ..Default::default()
        };
        alpha.segments[0] = CmvsAlphaSegment {
            next: 1,
            target: 255,
            duration_ms: 750,
            ..Default::default()
        };
        alpha.segments[1] = CmvsAlphaSegment {
            next: 2,
            target: 255,
            duration_ms: 1000,
            ..Default::default()
        };
        alpha.segments[2] = CmvsAlphaSegment {
            next: -2,
            target: 0,
            duration_ms: 750,
            ..Default::default()
        };
        let child = CmvsTextureChildState {
            surface_initialized: true,
            rect_words: Some([0, 0, 1, 1]),
            alpha: Some(212),
            basic_effects: CmvsBasicEffects {
                alpha: Some(alpha),
                ..Default::default()
            },
            ..Default::default()
        };
        let saved =
            capture_image_state(&parent, Some(&BTreeMap::from([(1, child)])), false).unwrap();
        scene
            .restore_native_image(
                &mut vm,
                69,
                &SavedImage::decode(&saved.encode().unwrap()).unwrap(),
                &archive,
            )
            .unwrap();
        let before = scene
            .render(&mut vm, CmvsTextureSlot::Parent(69), &archive, (2, 1), None)
            .unwrap()
            .unwrap();
        assert!(
            (before.rgba8[0] as i32 - 212).abs() <= 1,
            "actual={:?}",
            &before.rgba8[..4]
        );
        scene.advance_native_image_effects(&mut vm, 25).unwrap();
        let after = scene
            .render(&mut vm, CmvsTextureSlot::Parent(69), &archive, (2, 1), None)
            .unwrap()
            .unwrap();
        assert!(
            (after.rgba8[0] as i32 - 221).abs() <= 1,
            "actual={:?}",
            &after.rgba8[..4]
        );
        let captured = scene.capture_native_image(&vm, 69, &archive).unwrap();
        let effects = captured.children[&1]
            .basic
            .as_ref()
            .unwrap()
            .restore_effects()
            .unwrap();
        assert_eq!(effects.alpha.unwrap().elapsed_ms, 650);
        for _ in 0..9 {
            scene.advance_native_image_effects(&mut vm, 200).unwrap();
        }
        scene.advance_native_image_effects(&mut vm, 50).unwrap();
        assert!(!vm.texture_children[&69].contains_key(&1));
    }
}
