use super::*;
use crate::CmvsNativeGlyphGraph;

impl CmvsScene {
    /// 437D60/443FF0 replace the root provider, not its basic or linked children.
    /// The handler ignores the loader result; a failed provider owns no surfaces.
    pub(crate) fn load_glyph_image(
        &mut self,
        archive: &CmvsArchive,
        vm: &mut CmvsPs2aVmState,
        slot: u8,
        resource: &str,
    ) -> Result<(), CoreError> {
        load(archive, vm, slot, resource)
    }
}

fn load(
    archive: &CmvsArchive,
    vm: &mut CmvsPs2aVmState,
    slot: u8,
    resource: &str,
) -> Result<(), CoreError> {
    // Allocate/replace already occurred in the VM, before performing archive IO.
    let uri = archive.resolve_native_resource(vm, crate::CmvsResourceSearchBank::Texture, resource);
    let graph = vm
        .native_glyphs
        .get_mut(&slot)
        .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_OBJECT"))?;
    let provider = graph
        .auxiliary
        .as_mut()
        .filter(|provider| provider.resource == resource)
        .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_PROVIDER"))?;
    let loaded = uri.and_then(|uri| {
        provider.uri = Some(uri.clone());
        let info = archive.native_image_info(&uri)?;
        provider.image_info = info;
        graph.root.image_info = info;
        let image = archive.decode_pb_image(&uri)?;
        provider.rgba = image.into_raw();
        Ok(())
    });
    if let Err(cause) = loaded {
        // Native retains the newly allocated, surface-less CTexture even when
        // lookup or decoding fails. Never revive the old atlas/provider pixels.
        provider.rgba.clear();
        tracing::debug!(event = "cmvs.glyph.provider_load_failed", slot, resource, error = %cause);
    }
    Ok(())
}

/// The atlas address grid remains the constructor's grid after919; only the
/// actual provider extent and pixels change. A failed CTexture has no frame.
pub(super) fn provider_frame(
    graph: &CmvsNativeGlyphGraph,
) -> Result<Option<TextureFrame>, CoreError> {
    if let Some(provider) = &graph.auxiliary {
        if provider.rgba.is_empty() {
            return Ok(None);
        }
        let [width, height, _] = provider.image_info;
        if byte_count(width, height)? != provider.rgba.len() {
            return Err(error("SHINKI_EMU_CMVS_GLYPH_PROVIDER"));
        }
        return Ok(Some(TextureFrame {
            width,
            height,
            rgba8: provider.rgba.clone().into(),
        }));
    }
    let count = byte_count(graph.atlas.width, graph.atlas.height)?;
    Ok(Some(TextureFrame {
        width: graph.atlas.width,
        height: graph.atlas.height,
        rgba8: vec![0; count].into(),
    }))
}

fn byte_count(width: u32, height: u32) -> Result<usize, CoreError> {
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(error("SHINKI_EMU_CMVS_GLYPH_PROVIDER"));
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_PROVIDER"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::parent_load_tests::{command, fixture, write_image};

    fn create(vm: &mut CmvsPs2aVmState) {
        vm.native_stage_size = [128, 96];
        vm.native_render_size = [128, 96];
        vm.process_string_slots.insert(0, b"Arial".to_vec());
        command(vm, 896, &[0, 0x80000000, 24, 64]);
    }

    fn replace(archive: &CmvsArchive, vm: &mut CmvsPs2aVmState, name: &str) {
        vm.process_string_slots.insert(1, name.as_bytes().to_vec());
        assert_eq!(
            command(vm, 919, &[0, 0x80000001]),
            Some(crate::CmvsPs2aVmAction::NativeText(
                crate::CmvsNativeTextAction::GlyphLoadImage {
                    slot: 0,
                    resource: name.into()
                }
            ))
        );
        load(archive, vm, 0, name).unwrap();
    }

    #[test]
    fn real_archive_provider_replaces_pixels_on_every_load_and_survives_private_resume() {
        let (root, archive, mut vm) = fixture();
        create(&mut vm);
        let identity = vm.native_glyphs[&0].identity;
        {
            let graph = vm.native_glyphs.get_mut(&0).unwrap();
            graph.atlas.occupancy[11] = 1;
            graph.cursor = [13.25f32.to_bits(), (-7.5f32).to_bits()];
            graph.draw_order = 17;
            graph.root.auxiliary_word = Some(17);
            graph.root.hidden = true;
            graph.root.alpha = Some(64);
        }
        write_image(root.path(), "aux.pb3", 16, 8, [17, 81, 149, 213]);
        replace(&archive, &mut vm, "aux.pb3");
        let graph = &vm.native_glyphs[&0];
        let frame = provider_frame(graph).unwrap().unwrap();
        assert_eq!((frame.width, frame.height), (16, 8));
        assert!(
            frame
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [17, 81, 149, 213])
        );
        assert_eq!(graph.identity, identity);
        assert_eq!(graph.root.basic_kind, 5);
        assert_eq!(graph.root.blend_mode, Some(4));
        assert_eq!(graph.atlas.width, 512);
        assert_eq!(graph.atlas.occupancy[11], 1);
        assert_eq!(graph.cursor.map(f32::from_bits), [13.25, -7.5]);
        assert_eq!(graph.root.auxiliary_word, Some(17));
        assert!(graph.root.hidden);
        assert_eq!(graph.root.alpha, Some(64));
        let saved = postcard::to_allocvec(&vm).unwrap();
        let restored: CmvsPs2aVmState = postcard::from_bytes(&saved).unwrap();
        assert_eq!(
            provider_frame(&restored.native_glyphs[&0])
                .unwrap()
                .unwrap()
                .rgba8,
            frame.rgba8
        );
        write_image(root.path(), "aux.pb3", 4, 4, [223, 31, 77, 255]);
        replace(&archive, &mut vm, "aux.pb3");
        let graph = &vm.native_glyphs[&0];
        assert_eq!(graph.provider_generation, 2);
        assert_eq!(
            provider_frame(graph).unwrap().unwrap().rgba8.as_ref(),
            [223, 31, 77, 255].repeat(16)
        );
    }

    #[test]
    fn missing_or_invalid_load_discards_old_pixels_without_fabricating_a_frame() {
        let (root, archive, mut vm) = fixture();
        create(&mut vm);
        write_image(root.path(), "aux.pb3", 4, 4, [255, 0, 0, 255]);
        replace(&archive, &mut vm, "aux.pb3");
        let owner = vm.native_font_owners[&783].raster.owner;
        replace(&archive, &mut vm, "missing.pb3");
        let graph = &vm.native_glyphs[&0];
        assert!(provider_frame(graph).unwrap().is_none());
        assert_eq!(graph.provider_generation, 2);
        assert_eq!(graph.root.image_info, [0; 3]);
        assert_eq!(
            vm.interpreter_flag_words.get(&10532).copied().unwrap_or(0) & 0x1000,
            0
        );
        std::fs::write(root.path().join("invalid.pb3"), b"not an image").unwrap();
        replace(&archive, &mut vm, "invalid.pb3");
        assert!(provider_frame(&vm.native_glyphs[&0]).unwrap().is_none());
        command(&mut vm, 898, &[0]);
        assert!(vm.native_glyphs.is_empty());
        assert_eq!(vm.native_font_owners[&783].raster.owner, owner);
        create(&mut vm);
        assert!(vm.native_glyphs[&0].auxiliary.is_none());
        assert_eq!(vm.native_glyphs[&0].provider_generation, 0);
    }

    #[test]
    #[ignore = "requires a hardware GPU"]
    fn gpu_919_replaces_root_and_resident_child_pixels_without_repainting_old_records() {
        let (root, archive, mut vm) = fixture();
        vm.native_stage_size = [128, 96];
        vm.native_render_size = [128, 96];
        vm.process_string_slots.insert(0, b"Noto Sans JP".to_vec());
        command(&mut vm, 896, &[0, 0x80000000, 24, 64]);
        let font_owner = vm.native_font_owners[&783].raster.owner;
        let mut scene = CmvsScene::new(crate::scene::tests::gpu_instance()).unwrap();
        scene.text = crate::scene::text::tests::fixture().0;
        scene.bind_glyph_owner(&vm, 0).unwrap();
        command(&mut vm, 901, &[0, 0, 0, 128, 96]);
        command(&mut vm, 899, &[0, 10, 10]);
        command(&mut vm, 908, &[0, 30, 20]);
        vm.process_string_slots.insert(2, b"W".to_vec());
        let Some(crate::CmvsPs2aVmAction::NativeText(crate::CmvsNativeTextAction::GlyphAppend {
            slot,
            source,
        })) = command(&mut vm, 907, &[0, 0x80000002])
        else {
            panic!("907 did not reach its actual glyph consumer");
        };
        scene.append_glyph(&mut vm, slot, &source).unwrap();
        let render = |scene: &mut CmvsScene, vm: &mut CmvsPs2aVmState| {
            scene
                .render(vm, CmvsTextureSlot::Parent(0), &archive, (128, 96), None)
                .unwrap()
                .unwrap()
        };
        let _ = render(&mut scene, &mut vm);
        let resident = vm.native_glyphs[&0].records[0].resident.clone().unwrap();
        let painted = vm.native_glyphs[&0].records[0].painted.clone().unwrap();
        let local = resident
            .child
            .auxiliary_pair_words
            .unwrap()
            .map(|word| word as i32);
        let child_pixel = [(10 + local[0] + 2) as u32, (10 + local[1] + 2) as u32];
        let pixel = |frame: &TextureFrame, x: u32, y: u32| {
            let offset = ((y * frame.width + x) * 4) as usize;
            <[u8; 4]>::try_from(&frame.rgba8[offset..offset + 4]).unwrap()
        };
        for color in [[231, 17, 41, 255], [23, 71, 219, 255]] {
            write_image(root.path(), "aux.pb3", 128, 64, color);
            replace(&archive, &mut vm, "aux.pb3");
            let frame = render(&mut scene, &mut vm);
            assert_eq!(pixel(&frame, 11, 11), color);
            assert_eq!(pixel(&frame, child_pixel[0], child_pixel[1]), color);
            assert_eq!(
                vm.native_glyphs[&0].records[0].resident.as_ref(),
                Some(&resident)
            );
            assert_eq!(
                vm.native_glyphs[&0].records[0].painted.as_ref(),
                Some(&painted)
            );
            assert!(
                vm.native_glyphs[&0]
                    .auxiliary
                    .as_ref()
                    .unwrap()
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|value| *value == color)
            );
        }
        replace(&archive, &mut vm, "missing.pb3");
        //43AF30 still creates a child when43D6A0's zero-extent upload returns0.
        vm.process_string_slots.insert(2, b"B".to_vec());
        let Some(crate::CmvsPs2aVmAction::NativeText(crate::CmvsNativeTextAction::GlyphAppend {
            slot,
            source,
        })) = command(&mut vm, 907, &[0, 0x80000002])
        else {
            panic!("907 did not reach its actual glyph consumer");
        };
        scene.append_glyph(&mut vm, slot, &source).unwrap();
        let missing = render(&mut scene, &mut vm);
        assert_eq!(pixel(&missing, 11, 11), [0, 0, 0, 255]);
        assert_eq!(
            pixel(&missing, child_pixel[0], child_pixel[1]),
            [0, 0, 0, 255]
        );
        assert!(vm.native_glyphs[&0].records[1].resident.is_some());
        let color = [19, 197, 73, 255];
        write_image(root.path(), "aux.pb3", 128, 64, color);
        replace(&archive, &mut vm, "aux.pb3");
        let restored = render(&mut scene, &mut vm);
        assert_eq!(pixel(&restored, child_pixel[0], child_pixel[1]), color);
        assert!(
            vm.native_glyphs[&0]
                .auxiliary
                .as_ref()
                .unwrap()
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|value| *value == color)
        );
        command(&mut vm, 898, &[0]);
        let destroyed = render(&mut scene, &mut vm);
        assert_eq!(pixel(&destroyed, 11, 11), [0, 0, 0, 255]);
        assert_eq!(
            pixel(&destroyed, child_pixel[0], child_pixel[1]),
            [0, 0, 0, 255]
        );
        assert_eq!(vm.native_font_owners[&783].raster.owner, font_owner);
    }
}
