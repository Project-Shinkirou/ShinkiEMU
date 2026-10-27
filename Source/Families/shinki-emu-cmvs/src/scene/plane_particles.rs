use super::*;

pub(super) struct PlaneUpload {
    resource: Vec<u8>,
    texture: Option<(String, [u32; 2])>,
}
impl CmvsScene {
    pub(crate) fn invalidate_plane_particles(&mut self, slot: u8) {
        self.plane_uploads.retain(|&(owner_slot, _), upload| {
            if owner_slot != slot {
                return true;
            }
            if let Some((id, _)) = &upload.texture {
                self.reload_releases.push(id.clone());
            }
            false
        });
    }
    pub(super) fn prepare_plane_particles(
        &mut self,
        vm: &CmvsPs2aVmState,
        archive: &CmvsArchive,
        commands: &mut Vec<SceneCommand>,
    ) -> Result<[usize; 4], CoreError> {
        self.plane_uploads.retain(|&(slot, provider), upload| {
            let live = vm.native_scene_workers.get(&slot).is_some_and(|owner| {
                owner.basic_resource(provider) == Some(upload.resource.as_slice())
            });
            if !live && let Some((id, _)) = &upload.texture {
                self.reload_releases.push(id.clone());
            }
            live
        });
        self.plane_order.clear();
        let mut counts = [0; 4];
        let camera_z = vm
            .screen_words
            .get(&3)
            .copied()
            .map(f32::from_bits)
            .unwrap_or(0.0);
        for (&slot, owner) in &vm.native_scene_workers {
            for provider in 0..owner.basic_resource_count() {
                let resource = owner
                    .basic_resource(provider)
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_NATIVE_PARTICLE_RESOURCE"))?;
                if self.plane_uploads.contains_key(&(slot, provider)) {
                    continue;
                }
                let name = encoding_rs::SHIFT_JIS
                    .decode_without_bom_handling_and_without_replacement(resource)
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_NATIVE_PARTICLE_RESOURCE"))?;
                let loaded = archive
                    .resolve_native_resource(vm, crate::CmvsResourceSearchBank::Texture, &name)
                    .and_then(|uri| archive.decode_pb_image(&uri));
                let texture = match loaded {
                    Ok(image) => {
                        self.reload_generation = self
                            .reload_generation
                            .checked_add(1)
                            .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXTURE_GENERATION"))?;
                        let id = format!("cmvs.plane.{slot}.{provider}.{}", self.reload_generation);
                        let size = [image.width(), image.height()];
                        commands.push(SceneCommand::UploadTexture {
                            resource_id: id.clone(),
                            frame: TextureFrame {
                                width: size[0],
                                height: size[1],
                                rgba8: image.into_raw().into(),
                            },
                        });
                        Some((id, size))
                    }
                    Err(cause) => {
                        tracing::warn!(
                            event = "cmvs.session.plane_particle_provider_failed",
                            slot,
                            provider,
                            code = cause.code()
                        );
                        None
                    }
                };
                self.plane_uploads.insert(
                    (slot, provider),
                    PlaneUpload {
                        resource: resource.to_vec(),
                        texture,
                    },
                );
            }
            for ordinal in 0..owner.particle_count() {
                let index = owner.particle_index(ordinal);
                let Some((pass, depth)) = owner.basic_queue_depth(index, camera_z) else {
                    continue;
                };
                counts[slot as usize] += 1;
                let rank = self.plane_order.len();
                self.plane_order.push((
                    (pass, surface::descending_float_key(depth)),
                    slot,
                    index,
                    rank,
                ));
            }
        }
        Ok(counts)
    }
    pub(super) fn limit_plane_particles(&mut self, vm: &CmvsPs2aVmState, budgets: [usize; 4]) {
        let mut counts = [0; 4];
        self.plane_order.retain(|(_, slot, _, _)| {
            counts[*slot as usize] += 1;
            counts[*slot as usize] <= budgets[*slot as usize]
        });
        self.plane_order.sort_by(|left, right| {
            left.0.cmp(&right.0).then_with(|| {
                if vm.native_mesh_equal_prepend {
                    right.3.cmp(&left.3)
                } else {
                    left.3.cmp(&right.3)
                }
            })
        });
    }
    pub(super) fn append_plane_particles_before(
        &self,
        vm: &CmvsPs2aVmState,
        cursor: &mut usize,
        before: Option<(u8, u32)>,
        stage: (u32, u32),
        commands: &mut Vec<SceneCommand>,
    ) -> Result<(), CoreError> {
        while let Some(&(key, slot, index, _)) = self.plane_order.get(*cursor) {
            if before.is_some_and(|before| {
                key > before || (key == before && !vm.native_mesh_equal_prepend)
            }) {
                break;
            }
            *cursor += 1;
            let owner = &vm.native_scene_workers[&slot];
            let Some(provider) = owner.basic_resource_index(index) else {
                continue;
            };
            let Some(upload) = self.plane_uploads.get(&(slot, provider)) else {
                continue;
            };
            let Some((texture_id, size)) = &upload.texture else {
                continue;
            };
            let Some(image) = owner.particle_basic(index) else {
                continue;
            };
            let alpha = image.alpha.unwrap_or(255);
            let opaque = image.basic_kind == 0 && image.blend_mode == Some(0);
            if alpha <= 0 {
                continue;
            }
            let Some(geometry) = surface::geometry_with_surfaces(vm, &image, None, stage.0, true)
            else {
                continue;
            };
            let source = image
                .rect_words
                .ok_or_else(|| error("SHINKI_EMU_CMVS_NATIVE_PARTICLE_SOURCE"))?;
            geometry.append_sprite(
                commands,
                format!("cmvs.plane.{slot}.{index}"),
                texture_id.clone(),
                RectI::new(source[0] as i32, source[1] as i32, source[2], source[3]),
                *size,
                if opaque { 1.0 } else { alpha as f32 / 255.0 },
                if opaque {
                    BlendMode::Opaque
                } else {
                    BlendMode::Alpha
                },
            );
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "plane_particles/provider_tests.rs"]
mod provider_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::parent_load_tests::{command, fixture};

    #[test]
    #[ignore = "requires a hardware GPU"]
    fn gpu_flat_particles_obey_queue_ties_opaque_rgb_and_shake_gate() {
        let (_, archive, mut vm) = fixture();
        vm.native_stage_size = [64, 48];
        vm.native_render_size = [64, 48];
        vm.native_frame_delta_ms = 10;
        crate::vm::native_scene_worker::start(
            &mut vm,
            681,
            &[0, 0, 0, 160, 120, 1],
            b"flat.pb3".to_vec(),
        )
        .unwrap();
        command(&mut vm, 32, &[0]);
        command(&mut vm, 64, &[0]);
        command(&mut vm, 67, &[0, u32::MAX, 64, 48, 255]);
        command(&mut vm, 69, &[0, u32::MAX, 0, 0]);
        command(&mut vm, 46, &[0, u32::MAX]);
        command(&mut vm, 549, &[0, 1, 10, u32::MAX]);
        crate::vm::screen_shake::advance(&mut vm).unwrap();
        let mut scene = CmvsScene::new(crate::scene::tests::gpu_instance()).unwrap();
        scene.clear(64, 48).unwrap();
        let mut rgba = vec![0; 64 * 24 * 4];
        for pixel in rgba.as_chunks_mut::<4>().0 {
            *pixel = [255, 0, 0, 0];
        }
        scene
            .draw(
                64,
                48,
                vec![SceneCommand::UploadTexture {
                    resource_id: "flat-atlas".to_owned(),
                    frame: TextureFrame::from_vec(64, 24, rgba).unwrap(),
                }],
            )
            .unwrap();
        scene.plane_uploads.insert(
            (0, 0),
            PlaneUpload {
                resource: b"flat.pb3".to_vec(),
                texture: Some(("flat-atlas".to_owned(), [64, 24])),
            },
        );
        for (order, prepend, visible) in [
            (200, false, true),
            (100, false, false),
            (122, false, true),
            (122, true, false),
        ] {
            command(&mut vm, 71, &[0, u32::MAX, order]);
            vm.native_mesh_equal_prepend = prepend;
            let frame = scene
                .render(
                    &mut vm,
                    CmvsTextureSlot::Parent(0),
                    &archive,
                    (64, 48),
                    None,
                )
                .unwrap()
                .unwrap();
            for y in 0..48 {
                for x in 0..64 {
                    let red = visible && (33..49).contains(&x) && (27..43).contains(&y);
                    let expected = if red { [255, 0, 0] } else { [0, 0, 255] };
                    let offset = (y * 64 + x) * 4;
                    assert_eq!(
                        &frame.rgba8[offset..offset + 3],
                        &expected,
                        "order={order} prepend={prepend} pixel={x},{y}"
                    );
                }
            }
        }
        let crate::CmvsNativeSceneWorker::Kind9(owner) =
            vm.native_scene_workers.get_mut(&0).unwrap()
        else {
            panic!("flat worker");
        };
        owner.parameter_words[2] = 0x8000_0000;
        let frame = scene
            .render(
                &mut vm,
                CmvsTextureSlot::Parent(0),
                &archive,
                (64, 48),
                None,
            )
            .unwrap()
            .unwrap();
        for pixel in frame.rgba8.as_chunks::<4>().0 {
            assert_eq!(&pixel[..3], &[0, 0, 255]);
        }
    }
}
