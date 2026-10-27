//! Native 433620 generated D3D quads and the case-674/2048 snow producer.
//! Mesh pointer CImage DWORD1032 has no native image save record. The snow
//! manager saves its configuration and reconstructs its billboards on load.
use super::*;

pub(crate) const MESH_TRIANGLES: [u32; 6] = [0, 1, 2, 2, 1, 3];
// 4473C0 has 8192 shared draw records. Do not allocate a producer that
// cannot fit even an otherwise empty native scene.
const MAX_PARTICLES: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeMeshVertex {
    pub position_words: [u32; 3],
    pub rgba_words: [u32; 4],
    pub uv_words: [u32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeMesh {
    pub kind: u32,
    pub alpha: i32,
    pub blend: u32,
    pub position_words: [u32; 3],
    /// Degrees, composed in native yaw/pitch/roll order.
    pub angle_words: [u32; 3],
    pub scale_words: [u32; 3],
    pub billboard: bool,
    pub force_blend: bool,
    /// Native queue 5 bypasses depth testing; queue 1 writes LESS depth.
    pub late_pass: bool,
    pub vertices: [CmvsNativeMeshVertex; 4],
}

fn invalid(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_MESH", message)
}
fn finite(words: &[u32]) -> bool {
    words.iter().all(|word| f32::from_bits(*word).is_finite())
}
fn bits<const N: usize>(values: [f32; N]) -> [u32; N] {
    values.map(f32::to_bits)
}

impl CmvsNativeMesh {
    /// 432FA0: four XYZ/RGBA/UV vertices, 36-byte stride, triangle strip.
    pub(crate) fn billboard(width: f32, height: f32, pivot: [f32; 2]) -> Result<Self, CoreError> {
        let positions = [
            [-pivot[0], height - pivot[1], 0.0],
            [width - pivot[0], height - pivot[1], 0.0],
            [-pivot[0], -pivot[1], 0.0],
            [width - pivot[0], -pivot[1], 0.0],
        ];
        let uv = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        let mesh = Self {
            kind: 2,
            alpha: 255,
            blend: 0,
            position_words: [0; 3],
            angle_words: [0; 3],
            scale_words: [1.0_f32.to_bits(); 3],
            billboard: true,
            force_blend: false,
            late_pass: false,
            vertices: std::array::from_fn(|index| CmvsNativeMeshVertex {
                position_words: bits(positions[index]),
                rgba_words: [1.0_f32.to_bits(); 4],
                uv_words: bits(uv[index]),
            }),
        };
        mesh.validate()?;
        Ok(mesh)
    }

    /// 433B40 clamps only the upper bound and rewrites vertex alpha.
    pub(crate) fn set_alpha(&mut self, alpha: i32) {
        self.alpha = alpha.min(255);
        for vertex in &mut self.vertices {
            vertex.rgba_words[3] = (self.alpha as f32 / 255.0).to_bits();
        }
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if !matches!(self.kind, 2 | 3) || self.blend > 1 || self.alpha > 255 {
            return Err(invalid("unsupported native mesh kind or blend"));
        }
        if !finite(&self.position_words)
            || !finite(&self.angle_words)
            || !finite(&self.scale_words)
            || self.vertices.iter().any(|vertex| {
                !finite(&vertex.position_words)
                    || !finite(&vertex.rgba_words)
                    || !finite(&vertex.uv_words)
            })
        {
            return Err(invalid("native mesh contains non-finite geometry"));
        }
        Ok(())
    }

    /// Row-vector matrices, matching 412EC0 and 433620. Billboard orientation
    /// is the inverse orthonormal native view, with its translation replaced.
    pub(crate) fn world_matrix(&self, view: [f32; 16]) -> Result<[f32; 16], CoreError> {
        self.validate()?;
        if view.iter().any(|value| !value.is_finite()) {
            return Err(invalid("native mesh view is non-finite"));
        }
        let scale = self.scale_words.map(f32::from_bits);
        let mut rotation = identity();
        if self.billboard {
            for row in 0..3 {
                for column in 0..3 {
                    rotation[row * 4 + column] = view[column * 4 + row];
                }
            }
        } else {
            for axis in [1, 0, 2] {
                let angle = f32::from_bits(self.angle_words[axis]) * 0.017453292_f32;
                if angle == 0.0 {
                    continue;
                }
                let (sin, cos) = ((angle as f64).sin() as f32, (angle as f64).cos() as f32);
                let mut next = identity();
                match axis {
                    0 => {
                        next[5] = cos;
                        next[6] = sin;
                        next[9] = -sin;
                        next[10] = cos;
                    }
                    1 => {
                        next[0] = cos;
                        next[2] = -sin;
                        next[8] = sin;
                        next[10] = cos;
                    }
                    _ => {
                        next[0] = cos;
                        next[1] = sin;
                        next[4] = -sin;
                        next[5] = cos;
                    }
                }
                rotation = multiply(rotation, next);
            }
        }
        for row in 0..3 {
            for column in 0..3 {
                rotation[row * 4 + column] *= scale[row];
            }
        }
        rotation[12..15].copy_from_slice(&self.position_words.map(f32::from_bits));
        if rotation.iter().any(|value| !value.is_finite()) {
            return Err(invalid("native mesh world transform overflowed"));
        }
        Ok(rotation)
    }
}

pub(crate) fn identity() -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}
pub(crate) fn multiply(left: [f32; 16], right: [f32; 16]) -> [f32; 16] {
    std::array::from_fn(|index| {
        let row = index / 4;
        let column = index % 4;
        ((left[row * 4] * right[column] + left[row * 4 + 1] * right[4 + column])
            + left[row * 4 + 2] * right[8 + column])
            + left[row * 4 + 3] * right[12 + column]
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeSnowParticle {
    pub mesh: CmvsNativeMesh,
    pub fall_time_word: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeSnow {
    /// CP932 resource name, without its NUL terminator.
    pub resource: Vec<u8>,
    /// span, height, fall time, fade distance (float words), count, follow camera.
    pub parameter_words: [u32; 6],
    pub particles: Vec<CmvsNativeSnowParticle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SavedNativeSnow {
    pub(crate) parameter_words: [u32; 6],
    /// Preserve the entire native fixed field, including unused bytes after NUL.
    pub(crate) resource_field: [u8; 32],
}

fn random(state: &mut u32) -> f32 {
    *state = state.wrapping_mul(214_013).wrapping_add(2_531_011);
    ((*state >> 16) & 0x7fff) as f32 / 32767.0
}
fn validate_parameters(parameters: &[u32; 6]) -> Result<(), CoreError> {
    if !finite(&parameters[..4]) || parameters[4] as usize > MAX_PARTICLES {
        return Err(invalid(
            "native snow parameters exceed finite geometry or draw-record budget",
        ));
    }
    let fall_time = f32::from_bits(parameters[2]);
    if fall_time == 0.0 || fall_time.abs() < f32::MIN_POSITIVE {
        return Err(invalid("native snow fall time is zero or subnormal"));
    }
    Ok(())
}

impl CmvsNativeSnow {
    pub(crate) fn new(
        resource: Vec<u8>,
        parameter_words: [u32; 6],
        prng: &mut u32,
    ) -> Result<Self, CoreError> {
        validate_parameters(&parameter_words)?;
        if resource.len() >= 32 || resource.contains(&0) {
            return Err(invalid(
                "native snow resource does not fit its 32-byte field",
            ));
        }
        let span = f32::from_bits(parameter_words[0]);
        let height = f32::from_bits(parameter_words[1]);
        let fall_time = f32::from_bits(parameter_words[2]);
        let mut candidate_rng = *prng;
        let mut particles = Vec::with_capacity(parameter_words[4] as usize);
        for _ in 0..parameter_words[4] {
            let x = random(&mut candidate_rng) * span - span * 0.5;
            let z = random(&mut candidate_rng) * span - span * 0.5;
            let y = height * random(&mut candidate_rng);
            let jitter = fall_time / 5.0;
            let time = fall_time + (random(&mut candidate_rng) * jitter - jitter * 0.5);
            let mut mesh = CmvsNativeMesh::billboard(0.1, 0.1, [0.1, 0.1])?;
            mesh.position_words = bits([x, y, z]);
            mesh.force_blend = true;
            mesh.late_pass = true;
            particles.push(CmvsNativeSnowParticle {
                mesh,
                fall_time_word: time.to_bits(),
            });
        }
        let result = Self {
            resource,
            parameter_words,
            particles,
        };
        result.validate()?;
        *prng = candidate_rng;
        Ok(result)
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        validate_parameters(&self.parameter_words)?;
        if self.resource.len() >= 32
            || self.resource.contains(&0)
            || self.particles.len() != self.parameter_words[4] as usize
        {
            return Err(invalid(
                "native snow resource or particle count is inconsistent",
            ));
        }
        for particle in &self.particles {
            particle.mesh.validate()?;
            let time = f32::from_bits(particle.fall_time_word);
            if !time.is_finite() || time == 0.0 {
                return Err(invalid("native snow particle has invalid fall time"));
            }
        }
        Ok(())
    }

    /// 4563D0 takes unsigned elapsed milliseconds with its native 500ms cap.
    pub(crate) fn advance(&mut self, delta_ms: u32, camera: [f32; 3]) -> Result<(), CoreError> {
        self.validate()?;
        if camera.iter().any(|value| !value.is_finite()) {
            return Err(invalid("native snow camera is non-finite"));
        }
        let span = f32::from_bits(self.parameter_words[0]);
        let height = f32::from_bits(self.parameter_words[1]);
        let fade = f32::from_bits(self.parameter_words[3]);
        let delta = delta_ms.min(500) as f32;
        // Validate all candidate coordinates before publishing any particle.
        for particle in &self.particles {
            let position = snow_position(
                particle,
                span,
                height,
                delta,
                camera,
                self.parameter_words[5] != 0,
            );
            if position.iter().any(|value| !value.is_finite()) {
                return Err(invalid("native snow particle motion overflowed"));
            }
        }
        for particle in &mut self.particles {
            let position = snow_position(
                particle,
                span,
                height,
                delta,
                camera,
                self.parameter_words[5] != 0,
            );
            let difference = std::array::from_fn::<_, 3, _>(|i| (position[i] - camera[i]) as f64);
            let distance = ((difference[1] * difference[1] + difference[0] * difference[0])
                + difference[2] * difference[2])
                .sqrt() as f32;
            let alpha = if distance <= fade {
                if position[1] < 0.5 {
                    cvtt(position[1] * 510.0).min(255)
                } else {
                    255
                }
            } else if distance - fade <= 1.0 {
                cvtt((1.0 - (distance - fade)) * 255.0)
            } else {
                0
            };
            particle.mesh.position_words = bits(position);
            particle.mesh.set_alpha(alpha);
        }
        Ok(())
    }

    pub(crate) fn capture(&self) -> Result<SavedNativeSnow, CoreError> {
        self.validate()?;
        let mut resource_field = [0; 32];
        resource_field[..self.resource.len()].copy_from_slice(&self.resource);
        Ok(SavedNativeSnow {
            parameter_words: self.parameter_words,
            resource_field,
        })
    }
}
fn snow_position(
    particle: &CmvsNativeSnowParticle,
    span: f32,
    height: f32,
    delta: f32,
    camera: [f32; 3],
    follow: bool,
) -> [f32; 3] {
    let mut position = particle.mesh.position_words.map(f32::from_bits);
    position[1] -= span * delta / f32::from_bits(particle.fall_time_word);
    if position[1] <= 0.0 {
        position[1] += height;
    }
    if follow {
        for axis in [0, 2] {
            let difference = position[axis] - camera[axis];
            if difference.abs() > span * 0.5 {
                if difference < 0.0 {
                    position[axis] += span;
                } else {
                    position[axis] -= span;
                }
            }
        }
    }
    position
}
fn cvtt(value: f32) -> i32 {
    if !value.is_finite() || value >= 2147483648.0 || value < -2147483648.0 {
        i32::MIN
    } else {
        value as i32
    }
}

impl SavedNativeSnow {
    /// 448FC0 writes exactly 60 bytes for manager kind2; not particle positions.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        if bytes.len() != 60 || bytes[..4] != [1, 0, 2, 0] {
            return Err(invalid("unsupported native snow configuration record"));
        }
        let parameter_words = std::array::from_fn(|i| {
            u32::from_le_bytes(bytes[4 + i * 4..8 + i * 4].try_into().unwrap())
        });
        validate_parameters(&parameter_words)?;
        let resource_field: [u8; 32] = bytes[28..60].try_into().unwrap();
        if !resource_field.contains(&0) {
            return Err(invalid("native snow resource is not NUL terminated"));
        }
        Ok(Self {
            parameter_words,
            resource_field,
        })
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        let mut bytes = Vec::with_capacity(60);
        bytes.extend_from_slice(&[1, 0, 2, 0]);
        for word in self.parameter_words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&self.resource_field);
        Self::decode(&bytes)?;
        Ok(bytes)
    }
    pub(crate) fn rebuild(&self, prng: &mut u32) -> Result<CmvsNativeSnow, CoreError> {
        let length = self
            .resource_field
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| invalid("native snow resource is not NUL terminated"))?;
        CmvsNativeSnow::new(
            self.resource_field[..length].to_vec(),
            self.parameter_words,
            prng,
        )
    }
}

#[cfg(test)]
mod tests;
