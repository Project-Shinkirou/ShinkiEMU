use super::*;
use crate::{CmvsPs2aVmState, CmvsResourceSearchBank};

impl CmvsArchive {
    /// 4140C0 ignores load failure and rejects every decoded type except 8.
    /// 43F7B0 routes MSK0 to 43F9E0; PB2/PB3, BMP and PNG produce type32,
    /// even when their original files contain grayscale or 8-bit samples.
    pub(crate) fn load_native_custom_glyph_sheet(
        &self,
        vm: &mut CmvsPs2aVmState,
        resource: &str,
        cell_size: u32,
    ) -> Result<bool, CoreError> {
        let uri = match self.resolve_native_resource(vm, CmvsResourceSearchBank::Texture, resource)
        {
            Ok(uri) => uri,
            Err(_) => return Ok(false),
        };
        let bytes = match self.resource_bytes(&uri) {
            Ok(source) => source,
            Err(_) => return Ok(false),
        };
        let Some((width, height, coverage)) = decode_mask(&bytes) else {
            return Ok(false);
        };
        vm.native_custom_glyphs
            .load_sheet(uri, cell_size, width, height, coverage)?;
        Ok(true)
    }
}

/// Native MSK0 layout: magic, pixel-data offset:u32, width:u32, height:u32,
/// then tightly packed width*height coverage bytes at the indicated offset.
fn decode_mask(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if bytes.len() < 16 || !bytes.starts_with(b"MSK0") {
        return None;
    }
    let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let offset = word(4) as usize;
    let width = word(8);
    let height = word(12);
    let count = (width as usize).checked_mul(height as usize)?;
    if count == 0 {
        return None;
    }
    let coverage = bytes.get(offset..offset.checked_add(count)?)?.to_vec();
    Some((width, height, coverage))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mask(width: u32, height: u32, coverage: &[u8]) -> Vec<u8> {
        let mut bytes = b"MSK0".to_vec();
        for word in [20u32, width, height, 0x12345678] {
            bytes.extend(word.to_le_bytes());
        }
        bytes.extend(coverage);
        bytes
    }
    #[test]
    fn mask_reader_honors_real_data_offset_and_truncation() {
        assert_eq!(
            decode_mask(&mask(2, 2, &[0, 17, 128, 255])),
            Some((2, 2, vec![0, 17, 128, 255]))
        );
        assert!(decode_mask(&mask(2, 2, &[0, 17, 128])).is_none());
        assert!(decode_mask(&mask(u32::MAX, u32::MAX, &[])).is_none());
        assert!(decode_mask(b"PB3Bnot-a-mask").is_none());
    }
    #[test]
    fn real_loose_resource_load_reaches_native_glyph_pixels_and_failed_load_preserves_manager() {
        let (root, mut archive) = crate::archive::cache_tests::fixture();
        archive.archives[0].path = archive.archives[0].path.canonicalize().unwrap();
        archive
            .bind_game_root(root.path().canonicalize().unwrap())
            .unwrap();
        std::fs::write(
            root.path().join("glyph.msk"),
            mask(4, 2, &[1, 2, 91, 92, 3, 4, 93, 94]),
        )
        .unwrap();
        std::fs::write(root.path().join("wrong.pb3"), b"PB3Bnot-a-mask").unwrap();
        let mut vm = CmvsPs2aVmState::new(0);
        vm.resource_search_paths.insert(
            CmvsResourceSearchBank::Texture,
            BTreeMap::from([(0, String::new())]),
        );
        vm.process_string_slots.insert(0, b"glyph.msk".to_vec());
        let action =
            crate::scene::parent_load_tests::command(&mut vm, 960, &[0x8000_0000, 2]).unwrap();
        let crate::CmvsPs2aVmAction::LoadCustomGlyphResource {
            resource,
            cell_size,
        } = action
        else {
            panic!("960 did not dispatch a custom sheet load");
        };
        assert_eq!((resource.as_str(), cell_size), ("glyph.msk", 2));
        assert!(
            archive
                .load_native_custom_glyph_sheet(&mut vm, &resource, cell_size)
                .unwrap()
        );
        crate::scene::parent_load_tests::command(&mut vm, 961, &[1, 0x8140]);
        assert_eq!(
            vm.native_custom_glyphs
                .lookup(0x8140, 2)
                .unwrap()
                .unwrap()
                .coverage,
            [91, 92, 93, 94]
        );
        let original = vm.native_custom_glyphs.clone();
        assert!(
            !archive
                .load_native_custom_glyph_sheet(&mut vm, "absent.msk", 2)
                .unwrap()
        );
        assert!(
            !archive
                .load_native_custom_glyph_sheet(&mut vm, "wrong.pb3", 2)
                .unwrap()
        );
        assert_eq!(vm.native_custom_glyphs, original);
    }
}
