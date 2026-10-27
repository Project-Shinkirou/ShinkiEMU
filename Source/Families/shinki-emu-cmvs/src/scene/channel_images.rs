use super::*;
mod draw;
use super::surface::{NativeDrawGroup, NativeDrawGroups, NativeQueuePosition};
use draw::{append_channel_layer, append_channel_layer_size};

#[cfg(test)]
mod tests;

impl CmvsScene {
    pub(crate) fn release_channel_element_uploads(&mut self, channel: Option<u8>) {
        self.channel_element_uploads
            .retain(|(owner, _, _), (_, id)| {
                let keep = channel.is_some_and(|channel| channel != *owner);
                if !keep {
                    self.reload_releases.push(id.clone());
                }
                keep
            });
    }

    /// 424160 replaces one existing child of the first channel's container.
    /// A failed load keeps the new empty owner and its detached predecessor.
    pub(crate) fn load_channel_image_reload(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        child: u32,
        resource: String,
        duration_ms: u32,
        archive: &CmvsArchive,
    ) -> Result<bool, CoreError> {
        use crate::vm::texture_reload::{ReloadMode, TextureReload};
        let Some(mut previous) = vm.effect_elements.remove(&(0, child)) else {
            return Ok(false);
        };
        // 444BE0 discards any earlier retained owners before replacement.
        previous.image_reload = None;
        let previous_uri = previous.image_resource.clone().or_else(|| {
            vm.effect_channels
                .get(&0)
                .and_then(|channel| channel.image_resource.clone())
        });
        let mut previous_parent = super::native_save_channels::element_geometry(&previous);
        if let Some(uri) = &previous_uri {
            previous_parent.image_info = archive.native_image_info(uri)?;
        }
        let mut current = crate::vm::CmvsEffectElement {
            visible: 1,
            image_reload: Some(Box::new(TextureReload {
                mode: ReloadMode::Failed,
                duration_ms,
                elapsed_ms: 0,
                previous_identity: 0,
                previous_parent,
                previous_children: BTreeMap::new(),
                previous_uri,
                current_uri: None,
                previous_child_uris: BTreeMap::new(),
            })),
            ..Default::default()
        };
        let uri = match archive.resolve_native_resource(
            vm,
            crate::CmvsResourceSearchBank::Texture,
            &resource,
        ) {
            Ok(uri) => uri,
            Err(error) if error.code() == "SHINKI_EMU_CMVS_RESOURCE_NOT_FOUND" => {
                vm.effect_elements.insert((0, child), current);
                return Ok(false);
            }
            Err(error) => {
                vm.effect_elements.insert((0, child), current);
                return Err(error);
            }
        };
        let info = archive.native_image_info(&uri)?;
        self.prepare_image(&uri, archive)?;
        current.image_resource = Some(uri.clone());
        current.image_info = info;
        if previous.source.is_some() {
            // 444400 copies basic DWORD0..21 and the reference header, then
            // transfers all six worker owners (including DWORD33 / 1941).
            current.source = previous.source;
            current.position = previous.position;
            current.pivot = previous.pivot;
            current.order = previous.order;
            current.alpha = previous.alpha;
            current.blend_mode = previous.blend_mode;
            current.scale_words = previous.scale_words;
            current.basic_kind = previous.basic_kind;
            current.float_position_words = previous.float_position_words;
            current.projection_depth_word = previous.projection_depth_word;
            current.projection_height_word = previous.projection_height_word;
            current.rotation_hundredths = previous.rotation_hundredths;
            current.inheritance_flags = previous.inheritance_flags;
            current.width = previous.width;
            current.height = previous.height;
            current.basic_effects = std::mem::take(&mut previous.basic_effects);
            let reload = current.image_reload.as_mut().unwrap();
            reload.previous_parent.basic_effects = Default::default();
            let previous_info = reload
                .previous_uri
                .as_deref()
                .map(|uri| archive.native_image_info(uri))
                .transpose()?;
            if previous_info.is_some_and(|old| old[..2] != info[..2]) {
                current.source = Some([0, 0, info[0] as i32, info[1] as i32]);
                if current.pivot[0] != 0 {
                    current.pivot[0] = (info[0] >> 1) as i32;
                }
            }
            reload.current_uri = Some(uri);
            if duration_ms == 0 {
                current.image_reload = None;
            } else {
                reload.mode = if previous_info.is_some_and(|old| old[..2] == info[..2]) {
                    ReloadMode::Fast
                } else {
                    ReloadMode::Composite
                };
            }
        }
        vm.effect_elements.insert((0, child), current);
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn channel_image_commands(
        &mut self,
        vm: &CmvsPs2aVmState,
        archive: &CmvsArchive,
    ) -> Result<Vec<SceneCommand>, CoreError> {
        let mut groups = self.prepare_channel_draw_groups(vm, archive)?;
        groups.admit(&mut 8192);
        let mut commands = Vec::new();
        groups.append(&mut commands);
        Ok(commands)
    }

    pub(super) fn prepare_channel_draw_groups(
        &mut self,
        vm: &CmvsPs2aVmState,
        archive: &CmvsArchive,
    ) -> Result<NativeDrawGroups, CoreError> {
        self.channel_uploads.retain(|index, (uri, resource_id)| {
            let live = vm
                .effect_channels
                .get(index)
                .and_then(|channel| channel.image_resource.as_deref())
                == Some(uri.as_str())
                && vm
                    .slot_objects
                    .contains_key(&(742 << 8 | u32::from(*index)));
            if !live {
                self.reload_releases.push(resource_id.clone());
            }
            live
        });
        self.channel_element_uploads
            .retain(|(channel, child, layer), (uri, resource_id)| {
                let owner = vm.effect_elements.get(&(*channel, *child));
                let live = vm
                    .slot_objects
                    .contains_key(&(742 << 8 | u32::from(*channel)))
                    && owner.is_some_and(|owner| match *layer {
                        0 => owner.image_resource.as_ref() == Some(uri),
                        1 | 2 => owner
                            .image_reload
                            .as_ref()
                            .is_some_and(|reload| reload.busy()),
                        _ => false,
                    });
                if !live {
                    self.reload_releases.push(resource_id.clone());
                }
                live
            });
        let mut groups = NativeDrawGroups::default();
        for (index, channel) in &vm.effect_channels {
            if !vm
                .slot_objects
                .contains_key(&(742 << 8 | u32::from(*index)))
            {
                continue;
            }
            let frame = channel
                .image_resource
                .as_deref()
                .map(|uri| self.cached_texture(uri, archive))
                .transpose()?;
            self.append_channel_images_with_atlas(
                vm,
                *index,
                channel.image_resource.as_deref(),
                frame.as_ref(),
                Some(archive),
                &mut groups,
            )?;
        }
        Ok(groups)
    }

    /// A root owns the atlas; children inherit its pixels, not its full image.
    /// Commands 386/387 configure each child's source and local position.
    /// Native 447550 checks 445130(root) before drawing the entire subtree.
    #[cfg(test)]
    pub(super) fn append_channel_images(
        &mut self,
        vm: &CmvsPs2aVmState,
        index: u8,
        uri: &str,
        frame: &TextureFrame,
        commands: &mut Vec<SceneCommand>,
    ) -> Result<(), CoreError> {
        let mut groups = NativeDrawGroups::default();
        self.append_channel_images_with_atlas(
            vm,
            index,
            Some(uri),
            Some(frame),
            None,
            &mut groups,
        )?;
        groups.admit(&mut 8192);
        groups.append(commands);
        Ok(())
    }

    fn append_channel_images_with_atlas(
        &mut self,
        vm: &CmvsPs2aVmState,
        index: u8,
        uri: Option<&str>,
        frame: Option<&TextureFrame>,
        archive: Option<&CmvsArchive>,
        groups: &mut NativeDrawGroups,
    ) -> Result<(), CoreError> {
        let Some(root) = vm.effect_elements.get(&(index, u32::MAX)) else {
            return Ok(());
        };
        let pass = match root.basic_kind {
            1 => 0,
            2 => 1,
            3 => 2,
            _ => 3,
        };
        let camera = if root.basic_kind == 3 {
            vm.screen_camera_words.get(&1)
        } else {
            Some(&vm.screen_words)
        };
        let camera_z = camera
            .and_then(|words| words.get(&3))
            .copied()
            .map(f32::from_bits)
            .unwrap_or(0.0);
        let depth = |element: &crate::vm::CmvsEffectElement| {
            if matches!(element.basic_kind, 2 | 3) {
                f32::from_bits(element.float_position_words[2]) - camera_z
            } else {
                element.order as f32
            }
        };
        if root.visible == 0
            || root.source.is_none()
            || (matches!(root.basic_kind, 2 | 3)
                && depth(root).partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater))
        {
            return Ok(());
        }
        if let (Some(uri), Some(frame)) = (uri, frame)
            && self
                .channel_uploads
                .get(&index)
                .map(|(uri, _)| uri.as_str())
                != Some(uri)
        {
            if let Some((_, resource_id)) = self.channel_uploads.remove(&index) {
                self.reload_releases.push(resource_id);
            }
            self.reload_generation = self
                .reload_generation
                .checked_add(1)
                .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXTURE_GENERATION"))?;
            let texture_id = format!("cmvs.channel.{index}.{}", self.reload_generation);
            groups.resources.push(SceneCommand::UploadTexture {
                resource_id: texture_id.clone(),
                frame: frame.clone(),
            });
            self.channel_uploads
                .insert(index, (uri.to_owned(), texture_id));
        }
        let texture_id = self.channel_uploads.get(&index).map(|(_, id)| id.clone());
        // Owning an atlas is not owning a drawable quad: 443D90 starts with
        // no basic surface. Only configured geometry submits an image.
        let elements = std::iter::once((u32::MAX, root)).chain(
            vm.effect_elements
                .range((index, 0)..(index, u32::MAX))
                .map(|((_, child), element)| (*child, element)),
        );
        for (child, element) in elements {
            if element.source.is_none()
                || element.visible == 0
                || (child != u32::MAX
                    && (element.basic_kind != root.basic_kind
                        || (matches!(element.basic_kind, 0 | 1 | 5) && element.order < 0)))
            {
                continue;
            }
            let mut commands = Vec::new();
            let commands = &mut commands;
            let result: Result<(), CoreError> = (|| {
                let owned = element
                    .image_resource
                    .as_deref()
                    .map(|uri| {
                        let archive =
                            archive.ok_or_else(|| error("SHINKI_EMU_CMVS_CHANNEL_ARCHIVE"))?;
                        self.cached_texture(uri, archive)
                    })
                    .transpose()?;
                let Some(current) = owned.as_ref().or(frame) else {
                    return Ok(());
                };
                let current_id = if let Some(uri) = element.image_resource.as_deref() {
                    self.channel_element_texture((index, child, 0), uri, current, commands)?
                } else {
                    let Some(id) = &texture_id else {
                        return Ok(());
                    };
                    id.clone()
                };
                let mut phase = 256;
                if let Some(reload) = element
                    .image_reload
                    .as_deref()
                    .filter(|reload| reload.busy())
                {
                    let previous_uri = reload
                        .previous_uri
                        .as_deref()
                        .ok_or_else(|| error("SHINKI_EMU_CMVS_RELOAD_SOURCE"))?;
                    let previous = self.cached_texture(
                        previous_uri,
                        archive.ok_or_else(|| error("SHINKI_EMU_CMVS_CHANNEL_ARCHIVE"))?,
                    )?;
                    let previous_id = self.channel_element_texture(
                        (index, child, 1),
                        previous_uri,
                        &previous,
                        commands,
                    )?;
                    phase = reload.alpha_word().min(256);
                    if reload.mode == crate::vm::texture_reload::ReloadMode::Fast {
                        append_channel_layer(
                            self,
                            vm,
                            index,
                            child,
                            root,
                            element,
                            current,
                            current_id,
                            commands,
                            256,
                            Some((
                                previous_id,
                                reload.elapsed_ms as f32 / reload.duration_ms as f32,
                            )),
                        )?;
                        return Ok(());
                    }
                    let mut old = super::native_save_channels::parent_element(
                        &reload.previous_parent,
                        element.inheritance_flags,
                    )?;
                    old.blend_mode = 1;
                    let key = (index, child, 2);
                    let current_uri = element.image_resource.as_deref().unwrap_or("");
                    if self
                        .channel_element_uploads
                        .get(&key)
                        .is_none_or(|(name, _)| {
                            name.split_once('\0') != Some((previous_uri, current_uri))
                        })
                    {
                        let overlap_name = format!("{previous_uri}\0{current_uri}");
                        let overlap = super::reload::opaque_overlap(&previous, current);
                        self.channel_element_texture(key, &overlap_name, &overlap, commands)?;
                    }
                    let overlap_size = [
                        previous.width.min(current.width),
                        previous.height.min(current.height),
                    ];
                    let overlap_basic =
                        super::reload::overlap_basic(&reload.previous_parent, overlap_size);
                    let mut overlap_element = super::native_save_channels::parent_element(
                        &overlap_basic,
                        element.inheritance_flags,
                    )?;
                    overlap_element.blend_mode = 1;
                    let overlap_id = self.channel_element_uploads[&key].1.clone();
                    append_channel_layer_size(
                        self,
                        vm,
                        index,
                        child,
                        root,
                        &overlap_element,
                        overlap_size,
                        overlap_id,
                        commands,
                        256,
                        None,
                        true,
                    )?;
                    append_channel_layer_size(
                        self,
                        vm,
                        index,
                        child,
                        root,
                        &old,
                        [previous.width, previous.height],
                        previous_id,
                        commands,
                        256 - phase,
                        None,
                        true,
                    )?;
                }
                append_channel_layer(
                    self, vm, index, child, root, element, current, current_id, commands, phase,
                    None,
                )?;
                Ok(())
            })();
            result?;
            let nested = child != u32::MAX && element.inheritance_flags & 2 != 0;
            let root_depth = if nested { depth(root) } else { depth(element) };
            let indexed = nested.then(|| {
                let child_depth = if matches!(element.basic_kind, 2 | 3) {
                    depth(root) + f32::from_bits(element.float_position_words[2])
                } else {
                    element.order as f32
                };
                (child_depth, child)
            });
            let mut draw_commands = Vec::new();
            for command in commands.drain(..) {
                match command {
                    SceneCommand::UploadTexture { .. }
                    | SceneCommand::UploadGlyph { .. }
                    | SceneCommand::ReleaseResource { .. } => groups.resources.push(command),
                    _ => draw_commands.push(command),
                }
            }
            if matches!(element.basic_kind, 2 | 3)
                && root_depth.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
            {
                continue;
            }
            groups.groups.push(NativeDrawGroup {
                position: NativeQueuePosition::new(
                    vm,
                    pass,
                    root_depth,
                    280 + u16::from(index),
                    if child == u32::MAX || nested {
                        0
                    } else {
                        child + 1
                    },
                    indexed,
                ),
                commands: draw_commands,
            });
        }
        Ok(())
    }

    fn channel_element_texture(
        &mut self,
        key: (u8, u32, u8),
        uri: &str,
        frame: &TextureFrame,
        commands: &mut Vec<SceneCommand>,
    ) -> Result<String, CoreError> {
        if self
            .channel_element_uploads
            .get(&key)
            .is_none_or(|(old, _)| old != uri)
        {
            if let Some((_, id)) = self.channel_element_uploads.remove(&key) {
                self.reload_releases.push(id);
            }
            self.reload_generation = self
                .reload_generation
                .checked_add(1)
                .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXTURE_GENERATION"))?;
            let id = format!("cmvs.channel.element.{}", self.reload_generation);
            commands.push(SceneCommand::UploadTexture {
                resource_id: id.clone(),
                frame: frame.clone(),
            });
            self.channel_element_uploads
                .insert(key, (uri.to_owned(), id));
        }
        Ok(self.channel_element_uploads[&key].1.clone())
    }
}

fn clipped_region(
    [x, y, width, height]: [i32; 4],
    position: [i32; 2],
    origin: [i32; 2],
    size: [u32; 2],
) -> Result<Option<(RectI, RectI)>, CoreError> {
    if width <= 0 || height <= 0 {
        return Ok(None);
    }
    let left = i64::from(x).max(0);
    let top = i64::from(y).max(0);
    let right = (i64::from(x) + i64::from(width)).min(i64::from(size[0]));
    let bottom = (i64::from(y) + i64::from(height)).min(i64::from(size[1]));
    if right <= left || bottom <= top {
        return Ok(None);
    }
    let destination_x = i64::from(origin[0]) + i64::from(position[0]) + left - i64::from(x);
    let destination_y = i64::from(origin[1]) + i64::from(position[1]) + top - i64::from(y);
    let source = RectI {
        x: left as i32,
        y: top as i32,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    };
    let destination = RectI {
        x: i32::try_from(destination_x).map_err(|_| error("SHINKI_EMU_CMVS_TEXTURE_GEOMETRY"))?,
        y: i32::try_from(destination_y).map_err(|_| error("SHINKI_EMU_CMVS_TEXTURE_GEOMETRY"))?,
        width: source.width,
        height: source.height,
    };
    Ok(Some((source, destination)))
}
