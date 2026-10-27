use super::*;
use roukaku_media_core::{MeshDepth2D, MeshMaterial2D, ProjectiveMeshVertex2D, TextureFilter2D};
use std::sync::{Arc, LazyLock};

mod scene53;

pub(super) struct NativeMeshUpload {
    resources: Vec<MeshTexture>,
    vertices: Vec<Option<Arc<[ProjectiveMeshVertex2D]>>>,
}

struct MeshTexture {
    resource: Vec<u8>,
    texture_id: Option<String>,
    revision: u32,
    pending_frame: Option<TextureFrame>,
}

pub(super) struct MeshOrder {
    pass: u8,
    slot: u8,
    index: usize,
    distance: f32,
}

fn invalid(message: &str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_MESH", message)
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalized(v: [f32; 3]) -> Result<[f32; 3], CoreError> {
    let length = (f64::from(dot(v, v))).sqrt() as f32;
    if !length.is_finite() || length == 0.0 {
        return Err(invalid("native primary view has a singular basis"));
    }
    let reciprocal = 1.0 / length;
    Ok(v.map(|value| value * reciprocal))
}
fn transform(v: [f32; 4], matrix: [f32; 16]) -> [f32; 4] {
    std::array::from_fn(|column| {
        ((v[0] * matrix[column] + v[1] * matrix[4 + column]) + v[2] * matrix[8 + column])
            + v[3] * matrix[12 + column]
    })
}

/// 457E80 quantizes target/up to thousandths before 412C00 LookAtLH.
/// Roll changes the global up vector, not the forward vector.
fn view_projection(vm: &CmvsPs2aVmState, aspect: f32) -> Result<([f32; 16], [f32; 16]), CoreError> {
    let state = &vm.independent_views[0];
    let eye = state.position_words.map(f32::from_bits);
    let angles = state.angle_words.map(f32::from_bits);
    let radians = angles.map(|angle| f64::from((angle / 180.0) * std::f32::consts::PI));
    let (pitch_sin, pitch_cos) = radians[0].sin_cos();
    let (yaw_sin, yaw_cos) = radians[1].sin_cos();
    let quantize = |value: f64| (value * 1000.0).trunc() as i32 as f32 / 1000.0;
    let direction = [yaw_sin * pitch_cos, -pitch_sin, yaw_cos * pitch_cos].map(quantize);
    let target = std::array::from_fn::<_, 3, _>(|axis| eye[axis] + direction[axis]);
    let up = if angles[2] == 0.0 {
        [0.0, 1.0, 0.0]
    } else {
        [-radians[2].sin(), radians[2].cos(), 0.0].map(quantize)
    };
    let forward = normalized(std::array::from_fn(|axis| target[axis] - eye[axis]))?;
    let right = normalized(cross(up, forward))?;
    let up = cross(forward, right);
    let view = [
        right[0],
        up[0],
        forward[0],
        0.0,
        right[1],
        up[1],
        forward[1],
        0.0,
        right[2],
        up[2],
        forward[2],
        0.0,
        -dot(right, eye),
        -dot(up, eye),
        -dot(forward, eye),
        1.0,
    ];
    let fov = f32::from_bits(state.field_of_view_word) * 0.017453292_f32;
    let tan_half = f64::from(fov * 0.5).tan() as f32;
    let y = 1.0 / tan_half;
    let near = 0.1_f32;
    let far = 100.0_f32;
    let projection = [
        y / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        y,
        0.0,
        0.0,
        0.0,
        0.0,
        far / (far - near),
        1.0,
        0.0,
        0.0,
        -(near * far / (far - near)),
        0.0,
    ];
    if view
        .iter()
        .chain(&projection)
        .any(|value| !value.is_finite())
    {
        return Err(invalid("native primary view transform overflowed"));
    }
    Ok((view, projection))
}

impl CmvsScene {
    pub(crate) fn invalidate_native_meshes(&mut self, slot: u8) {
        if let Some(upload) = self.mesh_uploads.remove(&slot) {
            for resource in upload.resources {
                if resource.pending_frame.is_none()
                    && let Some(id) = resource.texture_id
                {
                    self.reload_releases.push(id);
                }
            }
        }
    }

    pub(crate) fn native_mesh_commands(
        &mut self,
        vm: &CmvsPs2aVmState,
        archive: &CmvsArchive,
        stage: (u32, u32),
        slot_budgets: [usize; 4],
    ) -> Result<Vec<SceneCommand>, CoreError> {
        self.mesh_uploads.retain(|slot, upload| {
            let live = vm
                .native_scene_workers
                .get(slot)
                .is_some_and(|owner| owner.mesh_resource_count() == upload.resources.len());
            if !live {
                for resource in &upload.resources {
                    if resource.pending_frame.is_none()
                        && let Some(id) = &resource.texture_id
                    {
                        self.reload_releases.push(id.clone());
                    }
                }
            }
            live
        });
        let mut commands = Vec::new();
        if !vm
            .native_scene_workers
            .values()
            .any(|owner| owner.mesh_resource_count() != 0)
        {
            return Ok(commands);
        }
        let viewport = native_viewport::NativeViewport::from_vm(vm)?;
        let bounds = viewport.map_or(
            RectI {
                x: 0,
                y: 0,
                width: stage.0,
                height: stage.1,
            },
            |viewport| viewport.clip_rect(),
        );
        let (view, projection) = view_projection(vm, bounds.width as f32 / bounds.height as f32)?;
        for (&slot, owner) in &vm.native_scene_workers {
            let count = owner.mesh_resource_count();
            if count == 0 {
                continue;
            }
            let upload = self
                .mesh_uploads
                .entry(slot)
                .or_insert_with(|| NativeMeshUpload {
                    resources: Vec::with_capacity(count),
                    vertices: Vec::new(),
                });
            for index in 0..count {
                let raw = owner.mesh_resource(index).expect("mesh provider index");
                let revision = owner.mesh_resource_revision(index);
                if let Some(resource) = upload.resources.get_mut(index)
                    && resource.resource.as_slice() == raw
                    && resource.revision == revision
                {
                    if let Some(frame) = resource.pending_frame.take() {
                        commands.push(SceneCommand::UploadTexture {
                            resource_id: resource
                                .texture_id
                                .as_ref()
                                .expect("prepared texture identity")
                                .clone(),
                            frame,
                        });
                    }
                    continue;
                }
                let name = encoding_rs::SHIFT_JIS
                    .decode_without_bom_handling_and_without_replacement(raw)
                    .ok_or_else(|| invalid("native mesh resource is not valid CP932"))?;
                let loaded = archive
                    .resolve_native_resource(vm, crate::CmvsResourceSearchBank::Texture, &name)
                    .and_then(|uri| archive.decode_pb_image(&uri));
                let texture_id = match loaded {
                    Ok(image) => {
                        self.reload_generation = self
                            .reload_generation
                            .checked_add(1)
                            .ok_or_else(|| invalid("native mesh texture generation overflowed"))?;
                        let id = format!("cmvs.mesh.{slot}.{index}.{}", self.reload_generation);
                        commands.push(SceneCommand::UploadTexture {
                            resource_id: id.clone(),
                            frame: TextureFrame {
                                width: image.width(),
                                height: image.height(),
                                rgba8: image.into_raw().into(),
                            },
                        });
                        Some(id)
                    }
                    Err(cause) => {
                        tracing::warn!(
                            event = "cmvs.session.native_mesh_provider_failed",
                            slot,
                            index,
                            code = cause.code()
                        );
                        None
                    }
                };
                let resource = MeshTexture {
                    resource: raw.to_vec(),
                    texture_id,
                    revision,
                    pending_frame: None,
                };
                if let Some(current) = upload.resources.get_mut(index) {
                    let previous = std::mem::replace(current, resource);
                    if previous.pending_frame.is_none()
                        && let Some(id) = previous.texture_id
                    {
                        self.reload_releases.push(id);
                    }
                } else {
                    upload.resources.push(resource);
                }
            }
        }
        self.mesh_order.clear();
        let eye = vm.independent_views[0].position_words.map(f32::from_bits);
        for (&slot, owner) in &vm.native_scene_workers {
            let mut admitted = 0;
            for index in 0..owner.mesh_count() {
                if !owner.mesh_visible(index) {
                    continue;
                }
                if admitted >= slot_budgets[slot as usize] {
                    break;
                }
                admitted += 1;
                let mesh = owner.mesh(index).expect("mesh index");
                //445130 excludes hidden roots before queueing;433620 also
                // returns before matrix calculation or depth writes at alpha0.
                if mesh.alpha == 0 {
                    continue;
                }
                let position = mesh.position_words.map(f32::from_bits);
                let delta =
                    std::array::from_fn::<_, 3, _>(|axis| f64::from(position[axis] - eye[axis]));
                let distance = ((delta[1] * delta[1] + delta[0] * delta[0]) + delta[2] * delta[2])
                    .sqrt() as f32;
                self.mesh_order.push(MeshOrder {
                    pass: u8::from(mesh.late_pass),
                    slot,
                    index,
                    distance,
                });
            }
        }
        self.mesh_order.sort_by(|left, right| {
            left.pass
                .cmp(&right.pass)
                .then_with(|| right.distance.total_cmp(&left.distance))
                .then_with(|| {
                    let order = (left.slot, left.index).cmp(&(right.slot, right.index));
                    if vm.native_mesh_equal_prepend {
                        order.reverse()
                    } else {
                        order
                    }
                })
        });
        static INDICES: LazyLock<Arc<[u32]>> =
            LazyLock::new(|| Arc::from(crate::vm::native_mesh::MESH_TRIANGLES));
        for entry in &self.mesh_order {
            let owner = &vm.native_scene_workers[&entry.slot];
            let mesh = owner.mesh(entry.index).expect("queued mesh index");
            let resource_index = owner
                .mesh_resource_index(entry.index)
                .expect("mesh provider index");
            let upload = self
                .mesh_uploads
                .get_mut(&entry.slot)
                .expect("mesh provider cache");
            let resource = &upload.resources[resource_index];
            let Some(texture_id) = resource.texture_id.as_ref() else {
                continue;
            };
            let matrix = crate::vm::native_mesh::multiply(
                crate::vm::native_mesh::multiply(mesh.world_matrix(view)?, view),
                projection,
            );
            let vertices = std::array::from_fn::<_, 4, _>(|vertex| {
                let raw = &mesh.vertices[vertex];
                let position = raw.position_words.map(f32::from_bits);
                let [x, y, z, w] = transform([position[0], position[1], position[2], 1.0], matrix);
                let color = raw.rgba_words.map(f32::from_bits);
                let alpha = color[3].clamp(0.0, 1.0);
                ProjectiveMeshVertex2D {
                    position: [
                        (x + w) * bounds.width as f32 * 0.5 + bounds.x as f32 * w,
                        (w - y) * bounds.height as f32 * 0.5 + bounds.y as f32 * w,
                        z,
                        w,
                    ],
                    uv: raw.uv_words.map(f32::from_bits),
                    premultiplied_rgba: [
                        color[0] * alpha,
                        color[1] * alpha,
                        color[2] * alpha,
                        alpha,
                    ],
                }
            });
            if vertices
                .iter()
                .any(|vertex| vertex.position.iter().any(|word| !word.is_finite()))
            {
                return Err(invalid("native projected mesh overflowed"));
            }
            if upload.vertices.len() <= entry.index {
                upload.vertices.resize_with(entry.index + 1, || None);
            }
            if let Some(cached) = &mut upload.vertices[entry.index] {
                Arc::make_mut(cached).copy_from_slice(&vertices);
            } else {
                upload.vertices[entry.index] = Some(Arc::from(vertices));
            }
            let cached = upload.vertices[entry.index].as_ref().unwrap();
            commands.push(SceneCommand::ProjectiveMesh2D {
                id: format!("cmvs.mesh.{}.{}", entry.slot, entry.index),
                vertices: Arc::clone(cached),
                indices: Arc::clone(&INDICES),
                //4454D0 draw-kind2 bypasses secondary/shader owners.433C30
                // selects pipeline8:4CE188 samples only the retained base.
                material: MeshMaterial2D::ColorTexture,
                texture_id: Some(texture_id.clone()),
                texture_filter: TextureFilter2D::Linear,
                opacity: 1.0,
                blend: if mesh.force_blend || mesh.alpha < 255 {
                    if mesh.blend == 1 {
                        BlendMode::Add
                    } else {
                        BlendMode::Alpha
                    }
                } else {
                    BlendMode::Opaque
                },
                depth: if mesh.late_pass {
                    MeshDepth2D::Disabled
                } else {
                    MeshDepth2D::ReadWriteLess
                },
            });
        }
        Ok(commands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn primary_view_projection_preserves_homogeneous_depth_and_billboards() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.independent_views[0].position_words = [1.0_f32, 2.0, -3.0].map(f32::to_bits);
        vm.independent_views[0].field_of_view_word = 90.0_f32.to_bits();
        let (view, projection) = view_projection(&vm, 2.0).unwrap();
        let eye = transform([1.0, 2.0, -3.0, 1.0], view);
        assert_eq!(eye, [0.0, 0.0, 0.0, 1.0]);
        let clip = transform(transform([3.0, 3.0, 1.0, 1.0], view), projection);
        assert!((clip[0] - 1.0).abs() < 0.00001);
        assert!((clip[1] - 1.0).abs() < 0.00001);
        assert_eq!(clip[3], 4.0);
        assert!(clip[2] > 0.0 && clip[2] < clip[3]);
        vm.independent_views[0].angle_words = [20.0_f32, 30.0, 40.0].map(f32::to_bits);
        let (view, _) = view_projection(&vm, 1.0).unwrap();
        let mut mesh =
            crate::vm::native_mesh::CmvsNativeMesh::billboard(0.1, 0.1, [0.1, 0.1]).unwrap();
        mesh.position_words = [1.0_f32, 2.0, -3.0].map(f32::to_bits);
        let world_view = crate::vm::native_mesh::multiply(mesh.world_matrix(view).unwrap(), view);
        for row in 0..3 {
            for column in 0..3 {
                assert!(
                    (world_view[row * 4 + column] - if row == column { 1.0 } else { 0.0 }).abs()
                        < 0.00001
                );
            }
        }
    }
}

#[cfg(test)]
mod depth_tests {
    use super::*;
    use crate::scene::parent_load_tests::{command, fixture, write_image};

    #[test]
    #[ignore = "requires a hardware GPU"]
    fn gpu_force_blended_floor_still_occludes_later_far_geometry() {
        let (root, archive, mut vm) = fixture();
        write_image(root.path(), "strip.pb3", 16, 16, [255, 0, 0, 255]);
        write_image(root.path(), "floor.pb3", 16, 16, [0, 0, 255, 128]);
        vm.native_stage_size = [32, 32];
        vm.native_render_size = [32, 32];
        command(&mut vm, 608, &[0, 1.5_f32.to_bits(), 0]);
        command(&mut vm, 609, &[20.0_f32.to_bits()]);
        command(&mut vm, 612, &[35.0_f32.to_bits()]);
        crate::vm::native_scene_worker::start_scene50(
            &mut vm,
            0,
            [b"strip.pb3".to_vec(), b"floor.pb3".to_vec()],
        )
        .unwrap();
        let mut scene = CmvsScene::new(crate::scene::tests::gpu_instance()).unwrap();
        scene.clear(32, 32).unwrap();
        let mut commands = scene
            .native_mesh_commands(&vm, &archive, (32, 32), [4, 0, 0, 0])
            .unwrap();
        commands.push(SceneCommand::UploadTexture {
            resource_id: "far-red".to_owned(),
            frame: TextureFrame::from_vec(1, 1, vec![255, 0, 0, 255]).unwrap(),
        });
        let positions = [[0.0, 0.0], [32.0, 0.0], [0.0, 32.0], [32.0, 32.0]];
        let vertices: Arc<[ProjectiveMeshVertex2D]> =
            Arc::from(std::array::from_fn::<_, 4, _>(|index| {
                ProjectiveMeshVertex2D {
                    position: [positions[index][0], positions[index][1], 0.99999, 1.0],
                    uv: [0.5, 0.5],
                    premultiplied_rgba: [1.0; 4],
                }
            }));
        commands.push(SceneCommand::ProjectiveMesh2D {
            id: "later-far-quad".to_owned(),
            vertices,
            indices: Arc::from(crate::vm::native_mesh::MESH_TRIANGLES),
            material: MeshMaterial2D::ColorTexture,
            texture_id: Some("far-red".to_owned()),
            texture_filter: TextureFilter2D::Linear,
            opacity: 1.0,
            blend: BlendMode::Opaque,
            depth: MeshDepth2D::ReadWriteLess,
        });
        let frame = scene.draw(32, 32, commands).unwrap();
        let top = (16 * 4) as usize;
        assert_eq!(&frame.rgba8[top..top + 3], &[255, 0, 0]);
        let floor = ((24 * 32 + 16) * 4) as usize;
        for (actual, expected) in frame.rgba8[floor..floor + 3].iter().zip([127_u8, 0, 128]) {
            assert!(
                actual.abs_diff(expected) <= 2,
                "floor color {actual}, expected {expected}"
            );
        }
    }
}

#[cfg(test)]
#[path = "native_mesh/provider_tests.rs"]
mod provider_tests;
