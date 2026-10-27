use super::*;
use roukaku_media_core::{MeshMaterial2D, MeshVertex2D, TextureFilter2D, Transform2D};
use std::sync::{Arc, LazyLock};

/// Native 42EA90 computes the angle in single precision, calls double-precision
/// libm, then truncates each rotated coordinate independently to a hundredth.
#[derive(Clone, Copy)]
struct Rotation {
    sin: f32,
    cos: f32,
}

impl Rotation {
    fn new(degrees: f32) -> Option<Self> {
        if degrees == 0.0 {
            return None;
        }
        let radians = ((360.0 - degrees) * (std::f32::consts::PI / 180.0)) as f64;
        Some(Self {
            sin: radians.sin() as f32,
            cos: radians.cos() as f32,
        })
    }

    fn apply(self, [x, y]: [f32; 2]) -> [f32; 2] {
        let truncate = |value: f32| {
            let scaled = value * 100.0;
            // CVTTSS2SI returns the integer-indefinite value on overflow/NaN.
            let integer = if (-2_147_483_648.0..2_147_483_648.0).contains(&scaled) {
                scaled as i32
            } else {
                i32::MIN
            };
            integer as f32 / 100.0
        };
        [
            truncate(y * self.sin + x * self.cos),
            truncate(y * self.cos - x * self.sin),
        ]
    }
}

/// The two native geometry passes cannot be folded into affine coefficients:
/// the local rotation, inherited rotation and camera roll quantize separately.
#[derive(Clone, Copy)]
pub(super) struct BasicGeometry {
    pivot: [f32; 2],
    scale: [f32; 2],
    rotation: Option<Rotation>,
    parent_pivot: [f32; 2],
    parent_scale: [f32; 2],
    parent_rotation: Option<Rotation>,
    position: [f32; 2],
    camera_roll: Option<Rotation>,
    camera_center: [f32; 2],
    basis: f32,
    offset: [f32; 2],
}

impl BasicGeometry {
    pub(super) fn flat(
        pivot: [f32; 2],
        scale: [f32; 2],
        rotation: f32,
        position: [f32; 2],
        basis: f32,
    ) -> Self {
        Self {
            pivot,
            scale,
            rotation: Rotation::new(rotation),
            parent_pivot: [0.0; 2],
            parent_scale: [1.0; 2],
            parent_rotation: None,
            position,
            camera_roll: None,
            camera_center: [0.0; 2],
            basis,
            offset: [0.0; 2],
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn camera(
        pivot: [f32; 2],
        scale: [f32; 2],
        rotation: f32,
        parent_pivot: [f32; 2],
        parent_scale: [f32; 2],
        parent_rotation: f32,
        projected: super::projection::CameraProjection,
        basis: f32,
    ) -> Self {
        let roll = ((projected.rotation * 100.0) as i32) % 36_000;
        Self {
            parent_pivot,
            parent_scale,
            parent_rotation: Rotation::new(parent_rotation),
            camera_roll: if roll > 0 {
                Rotation::new(roll as f32 / 100.0)
            } else {
                None
            },
            camera_center: projected.center,
            ..Self::flat(pivot, scale, rotation, projected.position, basis)
        }
    }

    /// 42C330 adds the gated global displacement after reference-size scaling.
    pub(super) fn with_offset(mut self, offset: [f32; 2]) -> Self {
        self.offset = offset;
        self
    }

    /// Native strip order: top-left, top-right, bottom-left, bottom-right.
    pub(super) fn corners(self, size: [u32; 2]) -> [[f32; 2]; 4] {
        let [width, height] = size.map(|value| value as f32);
        [[0.0, 0.0], [width, 0.0], [0.0, height], [width, height]].map(|point| {
            let mut point = [
                (point[0] - self.pivot[0]) * self.scale[0],
                (point[1] - self.pivot[1]) * self.scale[1],
            ];
            if let Some(rotation) = self.rotation {
                point = rotation.apply(point);
            }
            point = [
                (point[0] - self.parent_pivot[0]) * self.parent_scale[0],
                (point[1] - self.parent_pivot[1]) * self.parent_scale[1],
            ];
            if let Some(rotation) = self.parent_rotation {
                point = rotation.apply(point);
            }
            point[0] += self.position[0];
            point[1] += self.position[1];
            if let Some(rotation) = self.camera_roll {
                point = rotation.apply([
                    (point[0] + 0.5).floor() - self.camera_center[0],
                    (point[1] + 0.5).floor() - self.camera_center[1],
                ]);
                point[0] += self.camera_center[0];
                point[1] += self.camera_center[1];
            }
            [
                point[0] * self.basis + self.offset[0],
                point[1] * self.basis + self.offset[1],
            ]
        })
    }

    /// Apply native corners to retained texture, solid and reload draws.
    /// Resource commands pass through without modifying their payloads.
    pub(super) fn append(
        self,
        target: &mut Vec<SceneCommand>,
        command: SceneCommand,
        texture_size: Option<[u32; 2]>,
    ) -> Result<(), CoreError> {
        match command {
            SceneCommand::Sprite {
                id,
                texture_id,
                source,
                destination,
                opacity,
                blend,
            } => {
                let texture_size =
                    texture_size.ok_or_else(|| error("SHINKI_EMU_CMVS_GEOMETRY_TEXTURE_SIZE"))?;
                let source = source.unwrap_or(RectI::new(0, 0, texture_size[0], texture_size[1]));
                if source.width != destination.width || source.height != destination.height {
                    return Err(error("SHINKI_EMU_CMVS_GEOMETRY_TEXTURE_EXTENT"));
                }
                self.append_sprite(target, id, texture_id, source, texture_size, opacity, blend);
            }
            SceneCommand::Rect {
                id,
                width,
                height,
                rgba,
                ..
            } => {
                self.append_solid(target, id, [width, height], rgba);
            }
            SceneCommand::CrossfadeSprite {
                id,
                previous_texture_id,
                current_texture_id,
                source,
                destination,
                progress,
                opacity,
            } => {
                let size = [destination.width, destination.height];
                let positions = self.corners(size);
                if let Some(transform) = axis_transform(positions, size) {
                    super::surface::extend_transformed(
                        target,
                        transform,
                        [SceneCommand::CrossfadeSprite {
                            id,
                            previous_texture_id,
                            current_texture_id,
                            source,
                            destination: RectI::new(0, 0, size[0], size[1]),
                            progress,
                            opacity,
                        }],
                    );
                } else {
                    target.push(SceneCommand::CrossfadeQuad {
                        id,
                        previous_texture_id,
                        current_texture_id,
                        source,
                        positions,
                        progress,
                        opacity,
                    });
                }
            }
            resource @ (SceneCommand::UploadTexture { .. }
            | SceneCommand::ReleaseResource { .. }) => {
                target.push(resource);
            }
            _ => return Err(error("SHINKI_EMU_CMVS_GEOMETRY_COMMAND")),
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_sprite(
        self,
        target: &mut Vec<SceneCommand>,
        id: String,
        texture_id: String,
        source: RectI,
        texture_size: [u32; 2],
        opacity: f32,
        blend: BlendMode,
    ) {
        let size = [source.width, source.height];
        let corners = self.corners(size);
        if let Some(transform) = axis_transform(corners, size) {
            super::surface::extend_transformed(
                target,
                transform,
                [SceneCommand::Sprite {
                    id,
                    texture_id,
                    source: Some(source),
                    destination: RectI::new(0, 0, size[0], size[1]),
                    opacity,
                    blend,
                }],
            );
            return;
        }
        let [width, height] = texture_size.map(|value| value as f32);
        let left = source.x as f32 / width;
        let top = source.y as f32 / height;
        let right = (source.x as f32 + source.width as f32) / width;
        let bottom = (source.y as f32 + source.height as f32) / height;
        target.push(mesh(
            id,
            corners,
            [[left, top], [right, top], [left, bottom], [right, bottom]],
            [255; 4],
            MeshMaterial2D::ColorTexture,
            Some(texture_id),
            opacity,
            blend,
        ));
    }

    pub(super) fn append_solid(
        self,
        target: &mut Vec<SceneCommand>,
        id: String,
        size: [u32; 2],
        rgba: [u8; 4],
    ) {
        let corners = self.corners(size);
        if let Some(transform) = axis_transform(corners, size) {
            super::surface::extend_transformed(
                target,
                transform,
                [SceneCommand::Rect {
                    id,
                    x: 0,
                    y: 0,
                    width: size[0],
                    height: size[1],
                    rgba,
                }],
            );
            return;
        }
        let alpha = u32::from(rgba[3]);
        let premultiplied = [
            ((u32::from(rgba[0]) * alpha + 127) / 255) as u8,
            ((u32::from(rgba[1]) * alpha + 127) / 255) as u8,
            ((u32::from(rgba[2]) * alpha + 127) / 255) as u8,
            rgba[3],
        ];
        target.push(mesh(
            id,
            corners,
            [[0.0; 2]; 4],
            premultiplied,
            MeshMaterial2D::Solid,
            None,
            1.0,
            BlendMode::Alpha,
        ));
    }
}

fn axis_transform(corners: [[f32; 2]; 4], size: [u32; 2]) -> Option<Transform2D> {
    if size.contains(&0)
        || corners[0][1] != corners[1][1]
        || corners[2][1] != corners[3][1]
        || corners[0][0] != corners[2][0]
        || corners[1][0] != corners[3][0]
    {
        return None;
    }
    let scale_x = (corners[1][0] - corners[0][0]) / size[0] as f32;
    let scale_y = (corners[2][1] - corners[0][1]) / size[1] as f32;
    let determinant = scale_x * scale_y;
    // Shared PushTransform requires an invertible affine matrix. Native
    // camera-local zero/tiny scales remain valid degenerate vertex geometry,
    // so send their exact corners through Mesh2D instead of a singular push.
    if !determinant.is_finite() || determinant.abs() < f32::EPSILON {
        return None;
    }
    Some(Transform2D {
        m11: scale_x,
        m22: scale_y,
        tx: corners[0][0],
        ty: corners[0][1],
        ..Transform2D::IDENTITY
    })
}

#[allow(clippy::too_many_arguments)]
fn mesh(
    id: String,
    positions: [[f32; 2]; 4],
    uvs: [[f32; 2]; 4],
    premultiplied_rgba: [u8; 4],
    material: MeshMaterial2D,
    texture_id: Option<String>,
    opacity: f32,
    blend: BlendMode,
) -> SceneCommand {
    static INDICES: LazyLock<Arc<[u32]>> = LazyLock::new(|| Arc::from([0, 1, 2, 2, 1, 3]));
    SceneCommand::Mesh2D {
        id,
        vertices: std::array::from_fn::<_, 4, _>(|index| MeshVertex2D {
            position: positions[index],
            uv: uvs[index],
            premultiplied_rgba,
        })
        .into(),
        indices: Arc::clone(&INDICES),
        material,
        texture_id,
        texture_filter: TextureFilter2D::Linear,
        opacity,
        blend,
    }
}
