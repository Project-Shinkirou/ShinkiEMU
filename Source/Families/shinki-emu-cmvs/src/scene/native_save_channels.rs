use super::native_save_images::{capture_image_state, restore_channel_image_state};
use super::*;
use crate::system_save::slot_channel::{SavedChannel, SavedText};
use crate::system_save::slot_image::SavedProviderSource;
use crate::system_save::slot_io::invalid;
use crate::vm::{
    CmvsEffectChannel, CmvsEffectElement, CmvsEffectPlaybackRecord, CmvsEffectTextSurface,
    CmvsNativeTextGlyph, CmvsNativeTextSurface,
};

impl CmvsScene {
    pub(crate) fn capture_native_channel(
        &self,
        vm: &CmvsPs2aVmState,
        slot: u8,
        archive: &CmvsArchive,
    ) -> Result<SavedChannel, CoreError> {
        let channel = vm
            .effect_channels
            .get(&slot)
            .ok_or_else(|| invalid("native channel has no VM object"))?;
        if slot >= 8 || !vm.slot_objects.contains_key(&(742 << 8 | u32::from(slot))) {
            return Err(invalid("native channel owner is not allocated"));
        }
        if channel
            .words
            .keys()
            .any(|word| *word >= 1248 && !matches!(*word, 1264 | 1265))
        {
            return Err(invalid("native channel has unsupported unsaved fields"));
        }
        let mut words = [0; 1248];
        for (&index, &value) in &channel.words {
            if index < 1248 {
                words[index as usize] = value;
            }
        }
        let root = vm
            .effect_elements
            .get(&(slot, u32::MAX))
            .ok_or_else(|| invalid("native channel has no image container"))?;
        let mut parent = element_parent(root);
        if let Some(uri) = &channel.image_resource {
            parent.image_info = archive.native_image_info(uri)?;
            parent.resource = Some(channel_resource_name(vm, archive, uri)?);
        }
        let mut children = BTreeMap::new();
        for ((_, index), element) in vm.effect_elements.range((slot, 0)..(slot, u32::MAX)) {
            if *index >= 1024 {
                return Err(invalid("native channel child index exceeds its container"));
            }
            let mut child = element_child(element);
            if let Some(uri) = &element.image_resource {
                child.resource = Some(channel_resource_name(vm, archive, uri)?);
                child.image_info = archive.native_image_info(uri)?;
            }
            children.insert(*index as u16, child);
        }
        let mut image = capture_image_state(&parent, Some(&children), false)?;
        image.header[1] = root.visible;
        image.header[2] = root.inheritance_flags | 0x8000_0000;
        for (index, child) in &mut image.children {
            child.header[1] = vm.effect_elements[&(slot, u32::from(*index))].visible;
            child.header[2] = vm.effect_elements[&(slot, u32::from(*index))].inheritance_flags;
        }
        if channel.read_dword(0) >= 12 {
            return Err(invalid("native channel text owner exceeds its table"));
        }
        let owner = vm
            .native_text
            .get(&(channel.read_dword(0) as u8))
            .ok_or_else(|| invalid("native channel text owner is absent"))?;
        Ok(SavedChannel {
            id: words[0],
            playback: owner.virtual_size,
            words,
            image,
        })
    }

    pub(crate) fn restore_native_channel(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        slot: u8,
        saved: &SavedChannel,
        archive: &CmvsArchive,
    ) -> Result<(), CoreError> {
        if slot >= 8 || saved.words[0] != saved.id || saved.playback.contains(&0) {
            return Err(invalid(
                "native channel index, owner, or virtual extent is invalid",
            ));
        }
        let owner = u32::from(saved.words[0]) | (u32::from(saved.words[1]) << 16);
        if owner >= 12
            || vm
                .native_text
                .get(&(owner as u8))
                .map(|text| text.virtual_size)
                .unwrap_or(vm.native_stage_size)
                != saved.playback
            || !vm.native_text.contains_key(&(owner as u8))
        {
            return Err(invalid(
                "native channel text owner or virtual extent disagrees",
            ));
        }
        if saved
            .image
            .primary
            .as_ref()
            .is_some_and(|provider| provider.kind != 0)
        {
            return Err(invalid(
                "native channel dynamic atlas provider is not modeled",
            ));
        }
        let (parent, children) = restore_channel_image_state(&saved.image)?;
        if parent.resource_frame.is_some()
            || children
                .values()
                .any(|child| child.resource_frame.is_some())
        {
            return Err(invalid("native channel animated provider is not modeled"));
        }
        let mut channel = CmvsEffectChannel::default();
        for (index, &word) in saved.words.iter().enumerate() {
            if word != 0 {
                channel.words.insert(index as u32, word);
            }
        }
        channel.write_dword(1264, u32::MAX);
        if let Some(provider) = &saved.image.primary {
            let SavedProviderSource::File {
                archive: marker, ..
            } = &provider.source
            else {
                return Err(invalid(
                    "native channel composed atlas provider is not modeled",
                ));
            };
            let bank = match *marker {
                0 => crate::CmvsResourceSearchBank::Texture,
                1 => crate::CmvsResourceSearchBank::Native0d00,
                _ => return Err(invalid("native channel archive selector is not modeled")),
            };
            let name = parent
                .resource
                .as_deref()
                .ok_or_else(|| invalid("native channel atlas has no source name"))?;
            channel.image_resource = Some(archive.resolve_native_resource(vm, bank, name)?);
        }
        if let Some(uri) = &channel.image_resource {
            let info = archive.native_image_info(uri)?;
            self.prepare_image(uri, archive)?;
            channel.image_has_alpha = info[2] != 0;
        }
        let mut elements = BTreeMap::new();
        let mut root = parent_element(&parent, saved.image.header[2] & 0x7fff_ffff)?;
        root.visible = saved.image.header[1];
        // The root atlas binding lives on the channel, not on its basic.
        root.image_resource = None;
        elements.insert((slot, u32::MAX), root);
        for (index, child) in children {
            let mut element = child_element(&child, saved.image.children[&index].header[2])?;
            element.visible = saved.image.children[&index].header[1];
            if let Some(name) = child.resource.as_deref() {
                let provider = saved.image.children[&index]
                    .primary
                    .as_ref()
                    .ok_or_else(|| invalid("native child source has no provider"))?;
                let bank = match &provider.source {
                    SavedProviderSource::File { archive: 0, .. } => {
                        crate::CmvsResourceSearchBank::Texture
                    }
                    SavedProviderSource::File { archive: 1, .. } => {
                        crate::CmvsResourceSearchBank::Native0d00
                    }
                    _ => return Err(invalid("native child provider source is not modeled")),
                };
                let uri = archive.resolve_native_resource(vm, bank, name)?;
                element.image_info = archive.native_image_info(&uri)?;
                self.prepare_image(&uri, archive)?;
                element.image_resource = Some(uri);
            }
            elements.insert((slot, u32::from(index)), element);
        }
        self.release_channel_element_uploads(Some(slot));
        allocate_owner(vm, 742, slot)?;
        if let Some((_, resource)) = self.channel_uploads.remove(&slot) {
            self.reload_releases.push(resource);
        }
        vm.effect_elements.retain(|(owner, _), _| *owner != slot);
        vm.effect_elements.extend(elements);
        vm.effect_channels.insert(slot, channel);
        Ok(())
    }

    pub(crate) fn capture_native_text(
        &self,
        vm: &CmvsPs2aVmState,
        slot: u8,
    ) -> Result<SavedText, CoreError> {
        let surface = vm
            .native_text
            .get(&slot)
            .ok_or_else(|| invalid("native text surface is absent"))?;
        if slot >= 12
            || !vm.slot_objects.contains_key(&(750 << 8 | u32::from(slot)))
            || !vm.slot_objects.contains_key(&(762 << 8 | u32::from(slot)))
            || vm.native_text_fonts.get(&slot) != Some(&surface.font)
        {
            return Err(invalid("native text/font owners are inconsistent"));
        }
        capture_text(surface, vm.effect_playback.get(&slot))
    }

    pub(crate) fn restore_native_text(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        slot: u8,
        saved: &SavedText,
    ) -> Result<(), CoreError> {
        if slot >= 12 {
            return Err(invalid("native text slot exceeds its table"));
        }
        let (surface, playback) = restore_text(saved, vm.native_render_size)?;
        let family = if surface.font.is_empty() {
            vm.native_default_font.clone()
        } else {
            surface.font.clone()
        };
        self.bind_text_font(&family)?;
        crate::vm::native_text::font_owner::ensure_owner(vm, 762, slot)?;
        vm.native_font_owners
            .get_mut(&(762 + u16::from(slot)))
            .unwrap()
            .set_family(&family)?;
        allocate_owner(vm, 750, slot)?;
        vm.native_text_fonts.insert(slot, surface.font.clone());
        vm.native_text.insert(slot, surface);
        vm.effect_playback.insert(slot, playback);
        // `4662D0` resets node DWORD12..16; native slots do not resume reveal clocks.
        vm.native_text_reveals.remove(&slot);
        let width = self.logical_extent().map_or(0, |extent| extent.width);
        self.text.stamp_existing(vm, slot, width)?;
        Ok(())
    }
}

fn allocate_owner(vm: &mut CmvsPs2aVmState, table: u32, slot: u8) -> Result<(), CoreError> {
    let id = vm.next_slot_object_id;
    vm.next_slot_object_id = id
        .checked_add(1)
        .ok_or_else(|| invalid("native object identity exhausted"))?;
    vm.slot_objects.insert(table << 8 | u32::from(slot), id);
    Ok(())
}

/// Recover a representable native source reference from the bound atlas URI.
/// The equality check keeps same-named resources in other CPZs from aliasing.
fn channel_resource_name(
    vm: &CmvsPs2aVmState,
    archive: &CmvsArchive,
    uri: &str,
) -> Result<String, CoreError> {
    let bank = crate::CmvsResourceSearchBank::Texture;
    if let Some(relative) = uri.strip_prefix("cmvs:/native-files/") {
        if let Some(paths) = vm.resource_search_paths.get(&bank) {
            for (_, path) in paths.range(..10) {
                if path.is_empty() || path.ends_with(['/', '\\']) {
                    let prefix = path.replace('\\', "/");
                    if let Some(name) = relative.strip_prefix(&prefix)
                        && archive.resolve_native_resource(vm, bank, name)? == uri
                    {
                        return Ok(name.to_owned());
                    }
                }
            }
        }
    } else if let Some(name) = uri.rsplit('/').next()
        && archive.resolve_native_resource(vm, bank, name)? == uri
    {
        return Ok(name.to_owned());
    }
    Err(invalid(
        "native channel atlas URI has no lossless source reference",
    ))
}

fn encode_cp932(text: &str) -> Result<Vec<u8>, CoreError> {
    let (bytes, _, failed) = encoding_rs::SHIFT_JIS.encode(text);
    if failed || bytes.contains(&0) {
        return Err(invalid("native text cannot be represented as CP932"));
    }
    Ok(bytes.into_owned())
}

fn decode_cp932(bytes: &[u8]) -> Result<String, CoreError> {
    let text = encoding_rs::SHIFT_JIS
        .decode_without_bom_handling_and_without_replacement(bytes)
        .ok_or_else(|| invalid("native text contains invalid CP932"))?;
    // The VM stores Unicode; reject byte aliases that it cannot preserve on save.
    if encode_cp932(&text)?.as_slice() != bytes {
        return Err(invalid(
            "native CP932 alias has no lossless VM representation",
        ));
    }
    Ok(text.into_owned())
}

fn capture_text(
    surface: &CmvsNativeTextSurface,
    playback: Option<&CmvsEffectPlaybackRecord>,
) -> Result<SavedText, CoreError> {
    let font = encode_cp932(&surface.font)?;
    if font.len() >= 64 {
        return Err(invalid("native text font exceeds its buffer"));
    }
    let mut saved = SavedText {
        capacity: surface.capacity_hint,
        font,
        virtual_size: surface.virtual_size,
        parameters: [0; 39],
        layout: [0; 37],
        extended: [0; 52],
        glyphs: Vec::with_capacity(surface.glyphs.len()),
    };
    let p = &mut saved.parameters;
    p[0..2].copy_from_slice(&surface.position.map(|v| v as u32));
    p[2..6].copy_from_slice(&surface.rect.map(|v| v as u32));
    p[6] = surface.size;
    p[7] = surface.character_spacing;
    p[8] = surface.line_spacing;
    let mut font_buffer = [0u8; 64];
    font_buffer[..saved.font.len()].copy_from_slice(&saved.font);
    for (word, bytes) in p[9..25].iter_mut().zip(font_buffer.as_chunks::<4>().0) {
        *word = u32::from_le_bytes(*bytes);
    }
    p[25] = u32::from(surface.bold);
    p[26] = u32::from(surface.kinsoku);
    p[27..29].copy_from_slice(&surface.colors);
    p[29] = surface.style;
    p[31] = surface.color_mode;
    p[32..34].copy_from_slice(&surface.reveal_defaults);
    p[34] = surface.glyph_interval_ms;
    p[35] = u32::from(surface.visible);
    p[36] = surface.draw_order as u32;
    p[37..39].copy_from_slice(&surface.cursor.map(|v| v as u32));
    saved.layout[1] = 148;
    saved.layout[2] = u32::from(surface.reserve_ruby);
    saved.layout[3] = 12;
    if let Some(record) = playback {
        saved.layout[0] = record.flag;
        saved.layout[3] = record.value;
        saved.layout[4] = record.value2;
        saved.extended[1] = record.text_surface.value;
        if let Some(boundaries) = &record.text_surface.boundaries {
            saved.extended[0] = 1;
            let mut buffer = [0u8; 128];
            for (index, source) in boundaries.iter().enumerate() {
                if source.len() >= 64 || source.contains(&0) {
                    return Err(invalid("native kinsoku boundary exceeds its buffer"));
                }
                buffer[index * 64..index * 64 + source.len()].copy_from_slice(source);
            }
            for (word, bytes) in saved.extended[2..34]
                .iter_mut()
                .zip(buffer.as_chunks::<4>().0)
            {
                *word = u32::from_le_bytes(*bytes);
            }
        }
    }
    for (index, glyph) in surface.glyphs.iter().enumerate() {
        let code = glyph.code;
        let bytes = code.to_be_bytes();
        let source = if bytes[0] == 0 {
            &bytes[1..]
        } else {
            &bytes[..]
        };
        if glyph.text.chars().count() != 1 || decode_cp932(source)? != glyph.text {
            return Err(invalid("native glyph is not one CP932 character"));
        }
        saved.glyphs.push([
            0,
            (u32::from(code) << 16) | 40,
            index as u32 + 1,
            glyph.color_mode,
            glyph.position[0] as u32,
            glyph.position[1] as u32,
            glyph.size,
            glyph.colors[0],
            glyph.colors[1],
            glyph.style,
            0,
        ]);
    }
    Ok(saved)
}

fn restore_text(
    saved: &SavedText,
    physical_size: [u32; 2],
) -> Result<(CmvsNativeTextSurface, CmvsEffectPlaybackRecord), CoreError> {
    let p = &saved.parameters;
    if saved.virtual_size.contains(&0)
        || p[25] > 1
        || p[26] > 1
        || p[35] > 1
        || p[30] != 0
        || saved.layout[1] != 148
        || saved.layout[2] > 1
        || saved.extended[0] > 1
        || saved.extended[34..50].iter().any(|v| *v != 0)
    {
        return Err(invalid(
            "native text descriptor uses unsupported layout/effect fields",
        ));
    }
    let font = decode_cp932(&saved.font)?;
    let font_buffer = p[9..25]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<_>>();
    let font_end = font_buffer
        .iter()
        .position(|v| *v == 0)
        .ok_or_else(|| invalid("native text parameter font is unterminated"))?;
    if font_buffer[..font_end] != saved.font {
        return Err(invalid("native text font copies disagree"));
    }
    let mut glyphs = Vec::with_capacity(saved.glyphs.len());
    for (index, g) in saved.glyphs.iter().enumerate() {
        if g[1] as u16 != 40
            || g[2] != index as u32 + 1
            || g[10] != 0
            || !(1..=4096).contains(&g[6])
            || g[7..9].iter().any(|v| v & 0xff00_0000 != 0)
        {
            return Err(invalid("native glyph descriptor uses unsupported fields"));
        }
        let code = (g[1] >> 16) as u16;
        let bytes = code.to_be_bytes();
        let source = if bytes[0] == 0 {
            &bytes[1..]
        } else {
            &bytes[..]
        };
        let text = decode_cp932(source)?;
        if text.chars().count() != 1 {
            return Err(invalid(
                "native glyph code does not decode to one character",
            ));
        }
        glyphs.push(CmvsNativeTextGlyph {
            text,
            code,
            raster: crate::CmvsNativeFontRasterState::default(),
            position: [g[4] as i32, g[5] as i32],
            font: font.clone(),
            size: g[6],
            colors: [g[7], g[8]],
            style: g[9],
            color_mode: g[3],
        });
    }
    let boundaries = if saved.extended[0] == 0 {
        // 464D40 resets DWORD129/130 and its ruby-line census at entry.
        // Disabled boundary buffers and these parser caches are not active state.
        None
    } else {
        let bytes = saved.extended[2..34]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        let mut lists = [Vec::new(), Vec::new()];
        for (list, bytes) in lists.iter_mut().zip(bytes.as_chunks::<64>().0) {
            let length = bytes
                .iter()
                .position(|v| *v == 0)
                .ok_or_else(|| invalid("native kinsoku list is unterminated"))?;
            decode_cp932(&bytes[..length])?;
            list.extend_from_slice(&bytes[..length]);
        }
        Some(lists)
    };
    Ok((
        CmvsNativeTextSurface {
            font,
            capacity_hint: saved.capacity,
            visible: p[35] != 0,
            rect: [p[2] as i32, p[3] as i32, p[4] as i32, p[5] as i32],
            // 464AB0 copies the saved cursor, then 464C00 resets it to rect origin.
            cursor: [p[2] as i32, p[3] as i32],
            size: p[6],
            character_spacing: p[7],
            line_spacing: p[8],
            colors: [p[27], p[28]],
            kinsoku: p[26] != 0,
            bold: p[25] != 0,
            style: p[29],
            color_mode: p[31],
            position: [p[0] as i32, p[1] as i32],
            physical_size,
            virtual_size: saved.virtual_size,
            draw_order: p[36] as i32,
            glyph_interval_ms: p[34],
            reserve_ruby: saved.layout[2] != 0,
            reveal_defaults: [p[32], p[33]],
            glyphs,
        },
        CmvsEffectPlaybackRecord {
            flag: saved.layout[0],
            value: saved.layout[3],
            value2: saved.layout[4],
            text_surface: CmvsEffectTextSurface {
                value: saved.extended[1],
                boundaries,
            },
        },
    ))
}

pub(super) fn element_geometry(e: &CmvsEffectElement) -> crate::CmvsTextureParentState {
    crate::CmvsTextureParentState {
        surface_initialized: e.source.is_some(),
        hidden: e.visible == 0,
        screen_shake_disabled: e.inheritance_flags & 0x100 == 0,
        alpha: Some(e.alpha),
        scale_words: Some(e.scale_words),
        blend_mode: Some(e.blend_mode),
        basic_kind: e.basic_kind,
        float_position_words: e.float_position_words,
        projection_depth_word: e.projection_depth_word,
        projection_height_word: e.projection_height_word,
        rotation_hundredths: e.rotation_hundredths,
        reference_size: Some([e.width as u32, e.height as u32]),
        rect_words: e.source.map(|s| s.map(|v| v as u32)),
        position_words: Some(e.pivot.map(|v| v as u32)),
        auxiliary_pair_words: e.position.map(|v| v.map(|v| v as u32)),
        auxiliary_word: Some(e.order as u32),
        collision_geometry: e.collision_geometry.clone(),
        ..Default::default()
    }
}

pub(super) fn element_parent(e: &CmvsEffectElement) -> crate::CmvsTextureParentState {
    crate::CmvsTextureParentState {
        resource: e.image_resource.clone(),
        image_info: e.image_info,
        basic_effects: e.basic_effects.clone(),
        image_effect_words: e.image_effect_words,
        ..element_geometry(e)
    }
}

pub(super) fn element_child_geometry(e: &CmvsEffectElement) -> crate::CmvsTextureChildState {
    let p = element_geometry(e);
    crate::CmvsTextureChildState {
        resource: p.resource,
        image_info: p.image_info,
        basic_kind: p.basic_kind,
        float_position_words: p.float_position_words,
        projection_depth_word: p.projection_depth_word,
        projection_height_word: p.projection_height_word,
        rotation_hundredths: p.rotation_hundredths,
        surface_initialized: p.surface_initialized,
        hidden: p.hidden,
        parent_context_detached: e.inheritance_flags & 1 == 0,
        screen_shake_disabled: p.screen_shake_disabled,
        alpha: p.alpha,
        scale_words: p.scale_words,
        blend_mode: p.blend_mode,
        reference_size: p.reference_size,
        rect_words: p.rect_words,
        position_words: p.position_words,
        auxiliary_pair_words: p.auxiliary_pair_words,
        auxiliary_word: p.auxiliary_word,
        basic_effects: p.basic_effects,
        image_effect_words: p.image_effect_words,
        collision_geometry: p.collision_geometry,
        ..Default::default()
    }
}

fn element_child(e: &CmvsEffectElement) -> crate::CmvsTextureChildState {
    crate::CmvsTextureChildState {
        resource: e.image_resource.clone(),
        image_info: e.image_info,
        basic_effects: e.basic_effects.clone(),
        image_effect_words: e.image_effect_words,
        ..element_child_geometry(e)
    }
}

pub(super) fn parent_element(
    p: &crate::CmvsTextureParentState,
    flags: u32,
) -> Result<CmvsEffectElement, CoreError> {
    if !matches!(p.basic_kind, 0 | 1 | 2 | 3 | 5)
        || p.solid_color.is_some()
        || p.color_filter.is_some()
    {
        return Err(invalid(
            "native channel image uses unsupported projection/filter fields",
        ));
    }
    let size = p.reference_size.unwrap_or([0; 2]);
    Ok(CmvsEffectElement {
        visible: u32::from(!p.hidden),
        width: size[0] as i32,
        height: size[1] as i32,
        source: p
            .surface_initialized
            .then(|| p.rect_words.unwrap_or([0; 4]).map(|v| v as i32)),
        position: p
            .surface_initialized
            .then(|| p.auxiliary_pair_words.unwrap_or([0; 2]).map(|v| v as i32)),
        inheritance_flags: flags,
        pivot: p.position_words.unwrap_or([0; 2]).map(|v| v as i32),
        order: p.auxiliary_word.unwrap_or(0) as i32,
        alpha: p.alpha.unwrap_or(255),
        blend_mode: p.blend_mode.unwrap_or(1),
        scale_words: p.scale_words.unwrap_or([1.0f32.to_bits(); 2]),
        basic_kind: p.basic_kind,
        float_position_words: p.float_position_words,
        projection_depth_word: p.projection_depth_word,
        projection_height_word: p.projection_height_word,
        rotation_hundredths: p.rotation_hundredths,
        image_resource: p.resource.clone(),
        image_info: p.image_info,
        image_reload: None,
        basic_effects: p.basic_effects.clone(),
        image_effect_words: p.image_effect_words,
        collision_geometry: p.collision_geometry.clone(),
    })
}

fn child_element(
    c: &crate::CmvsTextureChildState,
    flags: u32,
) -> Result<CmvsEffectElement, CoreError> {
    let p = crate::CmvsTextureParentState {
        resource: c.resource.clone(),
        image_info: c.image_info,
        surface_initialized: c.surface_initialized,
        hidden: c.hidden,
        alpha: c.alpha,
        solid_color: c.solid_color,
        scale_words: c.scale_words,
        basic_kind: c.basic_kind,
        blend_mode: c.blend_mode,
        float_position_words: c.float_position_words,
        projection_depth_word: c.projection_depth_word,
        projection_height_word: c.projection_height_word,
        rotation_hundredths: c.rotation_hundredths,
        reference_size: c.reference_size,
        rect_words: c.rect_words,
        position_words: c.position_words,
        auxiliary_pair_words: c.auxiliary_pair_words,
        auxiliary_word: c.auxiliary_word,
        basic_effects: c.basic_effects.clone(),
        image_effect_words: c.image_effect_words,
        collision_geometry: c.collision_geometry.clone(),
        ..Default::default()
    };
    parent_element(&p, flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_native_channel_restore_reopens_atlas_and_text_font_owners() {
        use super::super::parent_load_tests::{fixture, write_image};
        let (root, archive, mut vm) = fixture();
        let mut scene = CmvsScene::new(super::super::tests::gpu_instance()).unwrap();
        scene.text = super::super::text::tests::fixture().0;
        write_image(root.path(), "channel.pb3", 8, 6, [240, 80, 24, 255]);
        vm.native_stage_size = [32, 24];
        vm.native_render_size = [32, 24];
        let mut surface = text();
        surface.physical_size = [32, 24];
        surface.virtual_size = [32, 24];
        surface.visible = true;
        vm.native_text_fonts.insert(3, surface.font.clone());
        vm.native_text.insert(3, surface);
        vm.slot_objects.insert((762 << 8) | 3, 1);
        vm.slot_objects.insert((750 << 8) | 3, 2);
        vm.slot_objects.insert(742 << 8, 3);
        crate::vm::native_text::font_owner::ensure_owner(&mut vm, 762, 3).unwrap();
        vm.native_font_owners
            .get_mut(&765)
            .unwrap()
            .set_family(&vm.native_text[&3].font)
            .unwrap();
        vm.stamp_native_text_glyphs(3, 0, Ok).unwrap();
        vm.native_text_reveals.insert(
            3,
            vec![crate::vm::CmvsNativeTextReveal {
                glyph_index: 0,
                delay_ms: 0,
                elapsed_ms: 17,
                effect: 1,
                duration_ms: 350,
                started: true,
            }],
        );
        let uri = archive
            .resolve_native_resource(&vm, crate::CmvsResourceSearchBank::Texture, "channel.pb3")
            .unwrap();
        let mut channel = CmvsEffectChannel::default();
        channel.write_dword(0, 3);
        channel.write_dword(1264, 7);
        channel.set_playback_mode(3, -4, 5);
        channel.image_resource = Some(uri.clone());
        vm.effect_channels.insert(0, channel);
        vm.effect_elements.insert(
            (0, u32::MAX),
            CmvsEffectElement {
                visible: 1,
                ..Default::default()
            },
        );
        vm.effect_elements.insert(
            (0, 7),
            CmvsEffectElement {
                visible: 1,
                width: 32,
                height: 24,
                source: Some([1, 1, 2, 2]),
                position: Some([4, 6]),
                order: 2,
                ..Default::default()
            },
        );
        let before_commands = scene.channel_image_commands(&vm, &archive).unwrap();
        let before = scene.draw(32, 24, before_commands).unwrap();
        let saved_text = scene.capture_native_text(&vm, 3).unwrap();
        let saved_channel = scene.capture_native_channel(&vm, 0, &archive).unwrap();
        let saved_channel = SavedChannel::decode(&saved_channel.encode().unwrap()).unwrap();
        let mut restored = CmvsPs2aVmState::new(0);
        restored.native_stage_size = [32, 24];
        restored.native_render_size = [32, 24];
        restored.resource_search_paths = vm.resource_search_paths.clone();
        scene
            .restore_native_text(&mut restored, 3, &saved_text)
            .unwrap();
        scene
            .restore_native_channel(&mut restored, 0, &saved_channel, &archive)
            .unwrap();
        assert_eq!(
            restored.effect_channels[&0].image_resource.as_deref(),
            Some(uri.as_str())
        );
        assert_eq!(restored.effect_channels[&0].read_dword(0), 3);
        assert_eq!(restored.effect_channels[&0].read_dword(1264), u32::MAX);
        assert_eq!(restored.effect_channels[&0].playback_mode(), 3);
        assert_eq!(
            restored.effect_elements[&(0, 7)],
            vm.effect_elements[&(0, 7)]
        );
        assert!(restored.slot_objects.contains_key(&((762 << 8) | 3)));
        assert!(!restored.native_text_reveals.contains_key(&3));
        assert!(!scene.text.frame(&restored, 32).unwrap().is_empty());
        let after_commands = scene.channel_image_commands(&restored, &archive).unwrap();
        let after = scene.draw(32, 24, after_commands).unwrap();
        assert_eq!(before.rgba8.as_slice(), after.rgba8.as_slice());
    }

    fn text() -> CmvsNativeTextSurface {
        CmvsNativeTextSurface {
            font: "Noto Sans JP".into(),
            capacity_hint: 196,
            visible: false,
            rect: [4, 8, 300, 100],
            cursor: [80, 90],
            size: 27,
            character_spacing: 1,
            line_spacing: 3,
            colors: [0x00112233, 0x00445566],
            kinsoku: true,
            bold: false,
            style: 2,
            color_mode: 1,
            position: [40, 50],
            physical_size: [1280, 720],
            virtual_size: [1280, 720],
            draw_order: 1,
            glyph_interval_ms: 30,
            reserve_ruby: false,
            reveal_defaults: [0; 2],
            glyphs: vec![CmvsNativeTextGlyph {
                text: "漢".into(),
                code: 0x8abf,
                raster: crate::CmvsNativeFontRasterState::default(),
                position: [5, 9],
                font: "Noto Sans JP".into(),
                size: 27,
                colors: [0x00112233, 0x00445566],
                style: 2,
                color_mode: 1,
            }],
        }
    }

    #[test]
    fn native_text_restores_cp932_style_visibility_and_native_cursor_reset() {
        let source = text();
        let saved = capture_text(&source, None).unwrap();
        let saved = SavedText::decode(&saved.encode().unwrap()).unwrap();
        let (restored, playback) = restore_text(&saved, [1280, 720]).unwrap();
        assert_eq!(restored.glyphs, source.glyphs);
        assert_eq!(restored.cursor, [4, 8]);
        assert_eq!(restored.position, source.position);
        assert_eq!(restored.style, source.style);
        assert_eq!(restored.visible, source.visible);
        assert_eq!(playback.value, 12);
    }

    #[test]
    fn native_text_bold_is_the_saved_parameter_boolean_not_outline_style() {
        let mut source = text();
        source.bold = true;
        source.style = 0;
        let saved = capture_text(&source, None).unwrap();
        assert_eq!(saved.parameters[25], 1);
        assert_eq!(saved.parameters[29], 0);
        let decoded = SavedText::decode(&saved.encode().unwrap()).unwrap();
        let (restored, _) = restore_text(&decoded, [1280, 720]).unwrap();
        assert!(restored.bold);
        assert_eq!(restored.style, 0);
        // Native wire stores no glyph font-manager weight; load regenerates
        // each texture from the live separately owned manager.
        assert!(!restored.glyphs[0].raster.bold);
        let encoded = capture_text(&restored, None).unwrap();
        assert_eq!(encoded.parameters[25], decoded.parameters[25]);
    }

    #[test]
    fn restored_native_text_uses_saved_ruby_reservation_and_glyph_interval() {
        let mut source = text();
        source.draw_order = 6;
        source.glyph_interval_ms = 50;
        source.reserve_ruby = true;
        source.reveal_defaults = [1, 350];
        let mut saved = capture_text(&source, None).unwrap();
        saved.layout[0] = 1;
        saved.layout[3] = 20;
        saved.layout[5] = 1;
        saved.extended[50] = 1;
        saved.extended[51] = 0x8142;
        let (restored, playback) = restore_text(&saved, [1280, 720]).unwrap();
        let mut vm = CmvsPs2aVmState::new(0);
        vm.native_text.insert(3, restored);
        vm.effect_playback.insert(3, playback);
        vm.write_interpreter_word(1488, 10);
        let options = crate::vm::native_text::dialogue::options(&vm, 3);
        assert_eq!(options.interval_ms, 50);
        let reveals = crate::vm::native_text::dialogue_layout::append(
            vm.native_text.get_mut(&3).unwrap(),
            "AB",
            options,
            None,
            None,
        )
        .unwrap();
        assert_eq!(vm.native_text[&3].glyphs[1].position[1], 28);
        assert_eq!(reveals[1].delay_ms, 50);
        let recaptured = capture_text(&vm.native_text[&3], vm.effect_playback.get(&3)).unwrap();
        assert_eq!(
            (
                recaptured.parameters[32],
                recaptured.parameters[33],
                recaptured.parameters[36]
            ),
            (1, 350, 6)
        );
    }

    #[test]
    fn native_text_rejects_unknown_semantics_instead_of_empty_glyphs() {
        let mut saved = capture_text(&text(), None).unwrap();
        saved.glyphs[0][10] = 1;
        assert!(restore_text(&saved, [1280, 720]).is_err());
        saved.glyphs[0][10] = 0;
        saved.extended[40] = 1;
        assert!(restore_text(&saved, [1280, 720]).is_err());
    }

    #[test]
    fn rebuilding_native_basic_resets_effect_owners_but_keeps_image_storage() {
        let mut element = CmvsEffectElement::default();
        element.basic_effects.alpha = Some(crate::vm::CmvsAlphaEffect {
            elapsed_ms: 625,
            ..Default::default()
        });
        element.basic_effects.sprite = Some(crate::vm::CmvsSpriteEffect {
            elapsed_ms: 981,
            ..Default::default()
        });
        element.image_effect_words[3] = 0xdeadbeef;
        element.reset_basic(true);
        assert_eq!(element.basic_effects, Default::default());
        assert_eq!(element.image_effect_words[3], 0xdeadbeef);
    }

    #[test]
    #[ignore = "requires a hardware GPU"]
    fn gpu_native_channel_sprite_restore_advances_hidden_owner_and_real_atlas_uv() {
        use crate::vm::{CmvsBasicEffects, CmvsSpriteEffect, CmvsSpriteFrame};
        let mut sprite = CmvsSpriteEffect {
            active: 1,
            current: 25,
            elapsed_ms: 81,
            ..Default::default()
        };
        sprite.frames[0][25] = CmvsSpriteFrame {
            next: 26,
            source: [0, 0, 1, 1],
            duration_ms: 100,
            ..Default::default()
        };
        sprite.frames[0][26] = CmvsSpriteFrame {
            next: 25,
            source: [1, 0, 1, 1],
            duration_ms: 100,
            ..Default::default()
        };
        let child = CmvsEffectElement {
            visible: 1,
            source: Some([0, 0, 1, 1]),
            basic_effects: CmvsBasicEffects {
                sprite: Some(sprite),
                ..Default::default()
            },
            ..Default::default()
        };
        let root = CmvsEffectElement {
            visible: 1,
            source: Some([0; 4]),
            ..Default::default()
        };
        let image = capture_image_state(
            &element_parent(&root),
            Some(&BTreeMap::from([(2, element_child(&child))])),
            false,
        )
        .unwrap();
        let encoded =
            crate::system_save::slot_image::SavedImage::decode(&image.encode().unwrap()).unwrap();
        let (parent, children) = restore_channel_image_state(&encoded).unwrap();
        let mut vm = CmvsPs2aVmState::new(0);
        vm.native_stage_size = [2, 1];
        vm.effect_channels.insert(0, CmvsEffectChannel::default());
        vm.effect_channels.get_mut(&0).unwrap().image_has_alpha = true;
        vm.effect_elements.insert(
            (0, u32::MAX),
            parent_element(&parent, encoded.header[2]).unwrap(),
        );
        vm.effect_elements.insert(
            (0, 2),
            child_element(&children[&2], encoded.children[&2].header[2]).unwrap(),
        );
        let mut scene = CmvsScene::new(super::super::tests::gpu_instance()).unwrap();
        let atlas = TextureFrame {
            width: 2,
            height: 1,
            rgba8: vec![255, 0, 0, 255, 0, 255, 0, 255].into(),
        };
        let mut commands = Vec::new();
        scene
            .append_channel_images(&vm, 0, "fixture:native-sprite", &atlas, &mut commands)
            .unwrap();
        let before = scene.draw(2, 1, commands).unwrap();
        assert_eq!(&before.rgba8[..4], &[255, 0, 0, 255]);
        vm.effect_elements.get_mut(&(0, 2)).unwrap().visible = 0;
        scene.advance_native_image_effects(&mut vm, 20).unwrap();
        let advanced = &vm.effect_elements[&(0, 2)];
        assert_eq!(
            advanced.basic_effects.sprite.as_ref().unwrap().elapsed_ms,
            1
        );
        assert_eq!(advanced.source, Some([1, 0, 1, 1]));
        vm.effect_elements.get_mut(&(0, 2)).unwrap().visible = 1;
        let mut commands = Vec::new();
        scene
            .append_channel_images(&vm, 0, "fixture:native-sprite", &atlas, &mut commands)
            .unwrap();
        let after = scene.draw(2, 1, commands).unwrap();
        assert_eq!(&after.rgba8[..4], &[0, 255, 0, 255]);
    }

    #[test]
    fn hardware_kind5_child_replacement_native_snapshot_cold_restore_and_missing_asset() {
        use super::super::parent_load_tests::{fixture, write_image};
        use crate::vm::{CmvsBasicEffects, CmvsFloatMotion, CmvsFloatSegment, native_timeline};
        let (directory, archive, mut vm) = fixture();
        write_image(directory.path(), "old-child.pb3", 4, 2, [240, 0, 0, 255]);
        write_image(directory.path(), "new-child.pb3", 4, 2, [0, 0, 240, 255]);
        write_image(directory.path(), "wide-child.pb3", 6, 2, [0, 240, 0, 255]);
        let mut scene = CmvsScene::new(super::super::tests::gpu_instance()).unwrap();
        scene.text = super::super::text::tests::fixture().0;
        vm.native_stage_size = [32, 24];
        vm.native_render_size = [32, 24];
        let mut surface = text();
        surface.physical_size = [32, 24];
        surface.virtual_size = [32, 24];
        vm.native_text_fonts.insert(0, surface.font.clone());
        vm.native_text.insert(0, surface);
        vm.slot_objects.insert(762 << 8, 1);
        vm.slot_objects.insert(750 << 8, 2);
        vm.slot_objects.insert(742 << 8, 3);
        crate::vm::native_text::font_owner::ensure_owner(&mut vm, 762, 0).unwrap();
        vm.native_font_owners
            .get_mut(&762)
            .unwrap()
            .set_family(&vm.native_text[&0].font)
            .unwrap();
        vm.stamp_native_text_glyphs(0, 0, Ok).unwrap();
        let uri = archive
            .resolve_native_resource(&vm, crate::CmvsResourceSearchBank::Texture, "old-child.pb3")
            .unwrap();
        vm.effect_channels.insert(
            0,
            CmvsEffectChannel {
                image_resource: Some(uri),
                ..Default::default()
            },
        );
        vm.effect_elements.insert(
            (0, u32::MAX),
            CmvsEffectElement {
                visible: 1,
                source: Some([0; 4]),
                ..Default::default()
            },
        );
        let mut motion = CmvsFloatMotion {
            active: 1,
            ..Default::default()
        };
        motion.segments[0] = CmvsFloatSegment {
            next: -1,
            target: [8.0f32.to_bits(), 0, 0],
            curve: 0,
            duration_ms: 400,
        };
        // The unused segment tail must survive native1941 exactly.
        motion.segments[7].next = i32::MIN;
        vm.effect_elements.insert(
            (0, 7),
            CmvsEffectElement {
                visible: 1,
                source: Some([0, 0, 4, 2]),
                position: Some([4, 6]),
                width: 32,
                height: 24,
                basic_effects: CmvsBasicEffects {
                    motion: Some(motion),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        assert!(
            scene
                .load_channel_image_reload(&mut vm, 7, "new-child.pb3".into(), 400, &archive)
                .unwrap()
        );
        scene.advance_native_image_effects(&mut vm, 100).unwrap();
        native_timeline::advance_channel_image_reloads(&mut vm, 100).unwrap();
        let commands = scene.channel_image_commands(&vm, &archive).unwrap();
        let mixed = scene.draw(32, 24, commands).unwrap();
        let sample = ((6 * 32 + 4) * 4) as usize;
        assert_eq!(&mixed.rgba8[sample..sample + 4], &[180, 0, 60, 255]);
        let saved_text = scene.capture_native_text(&vm, 0).unwrap();
        let saved = scene.capture_native_channel(&vm, 0, &archive).unwrap();
        let saved = SavedChannel::decode(&saved.encode().unwrap()).unwrap();
        assert!(saved.image.children[&7].secondary.is_none());
        let mut cold = CmvsPs2aVmState::new(0);
        cold.native_stage_size = vm.native_stage_size;
        cold.native_render_size = vm.native_render_size;
        cold.resource_search_paths = vm.resource_search_paths.clone();
        scene
            .restore_native_text(&mut cold, 0, &saved_text)
            .unwrap();
        scene
            .restore_native_channel(&mut cold, 0, &saved, &archive)
            .unwrap();
        let restored = &cold.effect_elements[&(0, 7)];
        assert!(restored.image_reload.is_none());
        let restored_motion = restored.basic_effects.motion.as_ref().unwrap();
        assert_eq!(
            (restored_motion.elapsed_ms, restored_motion.segments[7].next),
            (100, i32::MIN)
        );
        let commands = scene.channel_image_commands(&cold, &archive).unwrap();
        let final_image = scene.draw(32, 24, commands).unwrap();
        assert_eq!(&final_image.rgba8[sample..sample + 4], &[0, 0, 240, 255]);
        let before = cold.clone();
        let mut invalid = saved.clone();
        invalid
            .image
            .children
            .get_mut(&7)
            .unwrap()
            .basic
            .as_mut()
            .unwrap()
            .words[0] = 99;
        assert!(
            scene
                .restore_native_channel(&mut cold, 0, &invalid, &archive)
                .is_err()
        );
        assert_eq!(cold, before);
        assert!(
            !scene
                .load_channel_image_reload(&mut cold, 7, "missing-child.pb3".into(), 400, &archive)
                .unwrap()
        );
        assert!(cold.effect_elements[&(0, 7)].source.is_none());
        assert_eq!(
            cold.effect_elements[&(0, 7)]
                .image_reload
                .as_ref()
                .unwrap()
                .mode,
            crate::vm::texture_reload::ReloadMode::Failed
        );
        cold = before;
        assert!(
            scene
                .load_channel_image_reload(&mut cold, 7, "wide-child.pb3".into(), 400, &archive)
                .unwrap()
        );
        assert_eq!(
            cold.effect_elements[&(0, 7)]
                .image_reload
                .as_ref()
                .unwrap()
                .mode,
            crate::vm::texture_reload::ReloadMode::Composite
        );
        native_timeline::advance_channel_image_reloads(&mut cold, 100).unwrap();
        let commands = scene.channel_image_commands(&cold, &archive).unwrap();
        let composite = scene.draw(32, 24, commands).unwrap();
        let outside_old = ((6 * 32 + 8) * 4) as usize;
        assert_eq!(
            &composite.rgba8[outside_old..outside_old + 4],
            &[0, 60, 0, 255]
        );
        assert!(composite.rgba8[sample + 1] > 0 && composite.rgba8[sample + 2] > 0);
        // A second replacement discards the partial mix, retaining the last
        // real provider (wide green), not a captured mixed framebuffer.
        assert!(
            scene
                .load_channel_image_reload(&mut cold, 7, "old-child.pb3".into(), 0, &archive)
                .unwrap()
        );
        assert!(cold.effect_elements[&(0, 7)].image_reload.is_none());
        let commands = scene.channel_image_commands(&cold, &archive).unwrap();
        let skipped = scene.draw(32, 24, commands).unwrap();
        assert_eq!(&skipped.rgba8[sample..sample + 4], &[240, 0, 0, 255]);
    }

    #[test]
    fn hardware_channel_float1941_moves_projected_pixels_and_preserves_inheritance() {
        use crate::system_save::slot_projection::{SavedCamera, SavedPlane, SavedProjection};
        use crate::vm::{CmvsBasicEffects, CmvsFloatMotion, CmvsFloatSegment};
        let mut scene = CmvsScene::new(super::super::tests::gpu_instance()).unwrap();
        let mut vm = CmvsPs2aVmState::new(0);
        vm.native_stage_size = [32, 24];
        let mut projection = SavedProjection::default();
        projection.cameras.fill(SavedCamera {
            enabled: 1,
            reserved: 0,
            kind: 0,
            plane: Some(SavedPlane {
                mode: 1,
                position: [0.0; 3],
                auxiliary: [0.0; 2],
                depth: 10.0,
                extent: [32.0, 24.0],
                output: [32.0, 24.0],
                slope: 0.0,
                pitch: 0.0,
                yaw: 0.0,
                roll: 0.0,
            }),
            transition: None,
            center: [16.0, 12.0],
            scale: [1.0; 2],
        });
        crate::vm::native_slot_projection::apply(&mut vm, &projection, [0, 0]).unwrap();
        vm.slot_objects.insert(742 << 8, 1);
        vm.effect_channels.insert(0, Default::default());
        vm.effect_elements.insert(
            (0, u32::MAX),
            CmvsEffectElement {
                visible: 1,
                basic_kind: 2,
                float_position_words: [0, 0, 10.0f32.to_bits()],
                source: Some([0; 4]),
                projection_depth_word: 10.0f32.to_bits(),
                width: 32,
                height: 24,
                ..Default::default()
            },
        );
        let mut motion = CmvsFloatMotion {
            active: 1,
            ..Default::default()
        };
        motion.origin = [0, 0, 10.0f32.to_bits()];
        motion.segments[0] = CmvsFloatSegment {
            next: -1,
            target: [8.0f32.to_bits(), 0, 10.0f32.to_bits()],
            curve: 0,
            duration_ms: 400,
        };
        vm.effect_elements.insert(
            (0, 7),
            CmvsEffectElement {
                visible: 1,
                basic_kind: 2,
                source: Some([0, 0, 4, 2]),
                pivot: [2, 1],
                float_position_words: [0, 0, 10.0f32.to_bits()],
                projection_depth_word: 10.0f32.to_bits(),
                width: 32,
                height: 24,
                basic_effects: CmvsBasicEffects {
                    motion: Some(motion),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let atlas = TextureFrame::from_vec(4, 2, [240, 48, 24, 255].repeat(8)).unwrap();
        // 42C330 uses the root's XYZ while inherited; detached children use their own.
        for (inherits, delta, left) in [
            (true, 0, 14),
            (true, 200, 14),
            (false, 0, 18),
            (false, 200, 22),
        ] {
            if inherits {
                vm.effect_elements
                    .get_mut(&(0, 7))
                    .unwrap()
                    .inheritance_flags |= 1;
            } else {
                vm.effect_elements
                    .get_mut(&(0, 7))
                    .unwrap()
                    .inheritance_flags &= !1;
            }
            scene.advance_native_image_effects(&mut vm, delta).unwrap();
            let mut commands = Vec::new();
            scene
                .append_channel_images(&vm, 0, "fixture:channel-float", &atlas, &mut commands)
                .unwrap();
            let image = scene.draw(32, 24, commands).unwrap();
            for y in 0..24 {
                for x in 0..32 {
                    let expected = if (left..left + 4).contains(&x) && (11..13).contains(&y) {
                        [240, 48, 24, 255]
                    } else {
                        [0, 0, 0, 255]
                    };
                    let offset = ((y * 32 + x) * 4) as usize;
                    assert_eq!(
                        &image.rgba8[offset..offset + 4],
                        &expected,
                        "pixel {x},{y}, delta {delta}"
                    );
                }
            }
        }
    }
}
