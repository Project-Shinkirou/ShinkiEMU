use super::*;

fn load(
    vm: &CmvsPs2aVmState,
    archive: &CmvsArchive,
    slot: u8,
    index: usize,
    raw: &'static [u8],
) -> Option<crate::pb::PbDecodedImage> {
    let name = std::str::from_utf8(raw).expect("literal native scene53 resource");
    match archive
        .resolve_native_resource(vm, crate::CmvsResourceSearchBank::Texture, name)
        .and_then(|uri| archive.decode_pb_image(&uri))
    {
        Ok(image) => Some(image),
        Err(cause) => {
            tracing::warn!(
                event = "cmvs.session.scene53_provider_failed",
                slot,
                index,
                code = cause.code()
            );
            None
        }
    }
}

impl CmvsScene {
    ///44E960 checks each actually loaded c3/c4 width independently. Retain the
    ///decoded pixels until the first upload rather than decode both images twice.
    pub(crate) fn prepare_native_scene53(
        &mut self,
        vm: &CmvsPs2aVmState,
        archive: &CmvsArchive,
        slot: u8,
    ) -> Result<crate::CmvsNativeSceneKind53, CoreError> {
        if slot > 3 {
            return Err(invalid("native scene worker slot exceeds three"));
        }
        let initial: [Option<crate::pb::PbDecodedImage>; 2] = std::array::from_fn(|index| {
            load(
                vm,
                archive,
                slot,
                index,
                crate::CmvsNativeSceneKind53::RESOURCES[index],
            )
        });
        let widths =
            std::array::from_fn(|index| initial[index].as_ref().map_or(0, |image| image.width()));
        let owner = crate::CmvsNativeSceneKind53::new(widths)?;
        let mut resources = Vec::with_capacity(2);
        for (index, image) in initial.into_iter().enumerate() {
            let image = if widths[index] == 512 {
                image
            } else {
                drop(image);
                load(
                    vm,
                    archive,
                    slot,
                    index,
                    crate::CmvsNativeSceneKind53::FALLBACK_RESOURCES[index],
                )
            };
            let (texture_id, pending_frame) = if let Some(image) = image {
                self.reload_generation = self
                    .reload_generation
                    .checked_add(1)
                    .ok_or_else(|| invalid("native mesh texture generation overflowed"))?;
                let id = format!("cmvs.mesh.{slot}.{index}.{}", self.reload_generation);
                (
                    Some(id),
                    Some(TextureFrame {
                        width: image.width(),
                        height: image.height(),
                        rgba8: image.into_raw().into(),
                    }),
                )
            } else {
                (None, None)
            };
            resources.push(MeshTexture {
                resource: owner
                    .resource(index)
                    .expect("scene53 mesh provider")
                    .to_vec(),
                texture_id,
                revision: 0,
                pending_frame,
            });
        }
        self.invalidate_native_meshes(slot);
        self.mesh_uploads.insert(
            slot,
            NativeMeshUpload {
                resources,
                vertices: Vec::new(),
            },
        );
        Ok(owner)
    }
}

#[cfg(test)]
mod tests;
