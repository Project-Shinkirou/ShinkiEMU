use super::*;
use crate::vm::texture_presentation::{CLEAR_SURFACE_URI, TexturePresentation};

pub(super) struct PresentationMask {
    uri: String,
    width: u32,
    height: u32,
    values: Vec<u8>,
}

impl CmvsScene {
    pub(super) fn cached_texture(
        &mut self,
        uri: &str,
        archive: &CmvsArchive,
    ) -> Result<TextureFrame, CoreError> {
        if let Some(frame) = self.textures.get(uri) {
            return Ok(frame);
        }
        let frame = if uri == CLEAR_SURFACE_URI {
            // Case 558 explicitly creates and clears parent zero.
            let size = self
                .logical_extent()
                .ok_or_else(|| error("SHINKI_EMU_CMVS_CANVAS"))?;
            TextureFrame {
                width: size.width,
                height: size.height,
                rgba8: [0, 0, 0, 255]
                    .repeat(size.width as usize * size.height as usize)
                    .into(),
            }
        } else {
            let image = archive.decode_pb_image(uri)?;
            TextureFrame {
                width: image.width(),
                height: image.height(),
                rgba8: image.into_raw().into(),
            }
        };
        self.textures
            .insert(uri.to_owned(), frame.clone())
            .map_err(|_| error("SHINKI_EMU_CMVS_TEXTURE_BUDGET"))?;
        Ok(frame)
    }

    pub(crate) fn prepare_image(
        &mut self,
        uri: &str,
        archive: &CmvsArchive,
    ) -> Result<(), CoreError> {
        self.cached_texture(uri, archive)?;
        Ok(())
    }

    pub(crate) fn bind_clear_surface(&mut self, archive: &CmvsArchive) -> Result<(), CoreError> {
        self.release_parent_surface(0);
        self.cached_texture(CLEAR_SURFACE_URI, archive)?;
        self.bindings
            .insert(CmvsTextureSlot::Parent(0), CLEAR_SURFACE_URI.into());
        Ok(())
    }

    pub(crate) fn release_parent_surface(&mut self, parent: u8) {
        self.release_owner_surface(CmvsTextureSlot::Parent(parent));
    }

    pub(crate) fn release_owner_surface(&mut self, owner: CmvsTextureSlot) {
        let keep = |slot: &CmvsTextureSlot| slot.root_slot() != owner;
        self.bindings.retain(|slot, _| keep(slot));
        self.dynamic_images.retain(|slot, _| keep(slot));
        self.slot_geometry.retain(|slot, _| keep(slot));
        self.surface_uploads.retain(|slot, upload| {
            if keep(slot) {
                return true;
            }
            self.reload_releases.push(upload.id.clone());
            false
        });
        self.filtered_images.remove(&owner);
        self.release_image_reload_owner(owner);
    }

    pub(super) fn mask_presentation_frame(
        &mut self,
        frame: &mut TextureFrame,
        presentation: &TexturePresentation,
        archive: &CmvsArchive,
    ) -> Result<(), CoreError> {
        let rebuild = self.presentation_mask.as_ref().is_none_or(|mask| {
            mask.uri != presentation.mask
                || mask.width != frame.width
                || mask.height != frame.height
        });
        if rebuild {
            let source = self.cached_texture(&presentation.mask, archive)?;
            let mut values = Vec::with_capacity(frame.width as usize * frame.height as usize);
            for y in 0..frame.height {
                let row = u64::from(y) * u64::from(source.height) / u64::from(frame.height);
                for x in 0..frame.width {
                    let column = u64::from(x) * u64::from(source.width) / u64::from(frame.width);
                    let offset = (row * u64::from(source.width) + column) as usize * 4;
                    values.push(source.rgba8[offset]);
                }
            }
            self.presentation_mask = Some(PresentationMask {
                uri: presentation.mask.clone(),
                width: frame.width,
                height: frame.height,
                values,
            });
        }
        apply_mask(
            frame,
            &self.presentation_mask.as_ref().unwrap().values,
            &presentation.alpha_lut(),
        );
        Ok(())
    }
}

pub(super) fn apply_mask(frame: &mut TextureFrame, mask: &[u8], alpha: &[u8; 256]) {
    for (pixel, value) in frame
        .rgba8
        .make_mut_for_update()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(mask)
    {
        pixel[3] =
            ((u16::from(pixel[3]) * u16::from(alpha[usize::from(*value)]) + 127) / 255) as u8;
    }
}
