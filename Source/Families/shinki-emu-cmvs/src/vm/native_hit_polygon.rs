//! CImage DWORD1031 (1865) is XYZ triangle soup for the kind-6 particle worker,
//! not a screen-space input polygon. Native anchors: 447C30..447D60, 455030,
//! 455410 and 455680. The kind word is stored but never read by the consumer.
use super::*;

const MAX_VERTICES: usize = 128;
const MAX_PARTICLES: usize = 8192;

fn invalid(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_COLLISION", message)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeCollisionGeometry {
    pub kind: u32,
    pub vertices: Vec<[u32; 3]>,
}

impl CmvsNativeCollisionGeometry {
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if self.vertices.len() > MAX_VERTICES {
            return Err(invalid("native collision geometry exceeds 128 vertices"));
        }
        Ok(())
    }

    /// 447C40 silently ignores additions once the native fixed array is full.
    pub(crate) fn append(&mut self, vertex: [u32; 3]) {
        if self.vertices.len() < MAX_VERTICES {
            self.vertices.push(vertex);
        }
    }

    /// 444FB0 requires both owners. 447C90 adds CBasic words11..13 only:
    /// integer position, pivot, scale, rotation and parent transforms are unused.
    pub(crate) fn translated(
        &self,
        translation: Option<[u32; 3]>,
        output: &mut [[f32; 4]],
    ) -> Result<usize, CoreError> {
        self.validate()?;
        let Some(translation) = translation else {
            return Ok(0);
        };
        if output.len() < self.vertices.len() {
            return Err(invalid("native collision vertex output exceeds capacity"));
        }
        let translation = translation.map(f32::from_bits);
        for (destination, vertex) in output.iter_mut().zip(&self.vertices) {
            for axis in 0..3 {
                destination[axis] = f32::from_bits(vertex[axis]) + translation[axis];
            }
            destination[3] = 1.0;
        }
        Ok(self.vertices.len())
    }
}

pub(crate) fn execute(
    state: &mut CmvsPs2aVmState,
    command_id: u16,
    values: &[u32],
) -> Result<(), CoreError> {
    let expected = match command_id {
        816 | 817 => 1,
        818 => 4,
        _ => return Err(invalid("unknown native collision geometry command")),
    };
    if values.len() != expected {
        return Err(invalid("invalid native collision geometry operand count"));
    }
    let slot = values[0];
    if slot >= 256 || !texture_parent_is_live(state, 1924, 255, slot)? {
        raise_error_flag(state, 10532, 0x10)?;
        return Ok(());
    }
    let parent = state
        .texture_parents
        .get_mut(&(slot as u8))
        .ok_or_else(|| invalid("native collision geometry requires a modeled live image owner"))?;
    match command_id {
        816 => parent.collision_geometry = Some(CmvsNativeCollisionGeometry::default()),
        817 => parent.collision_geometry = None,
        818 => {
            if let Some(geometry) = &mut parent.collision_geometry {
                geometry.append([values[1], values[2], values[3]]);
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
/// 4135A0 uses x87 products/sums, rounded to float by each 455410 caller.
fn dot(a: [f32; 3], b: [f32; 3]) -> f64 {
    (a[1] as f64 * b[1] as f64 + a[0] as f64 * b[0] as f64) + a[2] as f64 * b[2] as f64
}

/// 455410: first intersected disjoint triangle, inclusive edges, either winding.
/// The query's Y is ignored: the vertical line starts at [X,35,Z]. Native reads
/// uninitialized storage after an incomplete final triple; reject that unsafe
/// input explicitly rather than manufacturing an extra vertex.
pub(crate) fn vertical_hit(
    point: [f32; 3],
    vertices: &[[f32; 4]],
) -> Result<Option<[f32; 3]>, CoreError> {
    if !vertices.len().is_multiple_of(3) {
        return Err(invalid(
            "native collision query requires complete vertex triples",
        ));
    }
    let origin = [point[0], 35.0, point[2]];
    for triangle in vertices.as_chunks::<3>().0 {
        let [a, b, c] =
            std::array::from_fn::<_, 3, _>(|i| [triangle[i][0], triangle[i][1], triangle[i][2]]);
        let mut normal = cross(subtract(c, a), subtract(b, a));
        let square = (normal[0] * normal[0] + normal[1] * normal[1]) + normal[2] * normal[2];
        let length = (square as f64).sqrt() as f32;
        let multiplier = if length > 0.0 { 1.0 / length } else { length };
        normal = normal.map(|value| value * multiplier);
        let numerator = dot(subtract(a, origin), normal) as f32;
        let distance = (numerator as f64 / dot([0.0, 1.0, 0.0], normal)) as f32;
        // Preserve MULSS/ADDSS even for zero components: 0 * infinity is NaN.
        let hit = [
            0.0 * distance + origin[0],
            distance + origin[1],
            0.0 * distance + origin[2],
        ];
        let mut inside = true;
        for (start, end) in [(a, b), (b, c), (c, a)] {
            let edge = dot(cross(subtract(hit, start), subtract(end, start)), normal) as f32;
            // COMISS zero,edge / JA rejects strictly negative values; unordered
            // values pass. This is important for native degenerate triangles.
            if edge < 0.0 {
                inside = false;
                break;
            }
        }
        if inside {
            return Ok(Some(hit));
        }
    }
    Ok(None)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeCollisionParticle {
    pub position_words: [u32; 3],
    pub fall_time_word: u32,
    pub collision_height_word: u32,
    pub alpha: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeCollisionParticles {
    pub resource: Vec<u8>,
    /// 453AC0's four words: flags, base Z, fall time, particle count.
    pub parameter_words: [u32; 4],
    pub particles: Vec<CmvsNativeCollisionParticle>,
    projection_height_word: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SavedNativeCollisionParticles {
    pub parameter_words: [u32; 4],
    pub resource_field: [u8; 32],
}

fn random(state: &mut u32) -> f32 {
    *state = state.wrapping_mul(214_013).wrapping_add(2_531_011);
    ((*state >> 16) & 0x7fff) as f32 / 32767.0
}
fn hundredths(value: f32) -> f32 {
    cvtt(value * 100.0) as f32 / 100.0
}
fn cvtt(value: f32) -> i32 {
    if !value.is_finite() || value >= 2147483648.0 || value < -2147483648.0 {
        i32::MIN
    } else {
        value as i32
    }
}
fn z_span(flags: u32) -> f32 {
    match (flags >> 24) & 15 {
        1 => 15.0,
        2 => 10.0,
        3 => 5.0,
        4 => 2.5,
        5 => 1.0,
        _ => 20.0,
    }
}
fn projection_height(parameters: &[u32; 4]) -> u32 {
    match parameters[0] >> 28 {
        4 => 60.0_f32,
        5 => 50.0,
        6 => 40.0,
        7 => 30.0,
        8 => 20.0,
        _ => f32::from_bits(parameters[1]) + 5.0,
    }
    .to_bits()
}
fn validate_parameters(parameters: &[u32; 4]) -> Result<(), CoreError> {
    if parameters[3] as usize > MAX_PARTICLES
        || !f32::from_bits(parameters[1]).is_finite()
        || !f32::from_bits(parameters[2]).is_finite()
        || f32::from_bits(parameters[2]) == 0.0
    {
        return Err(invalid(
            "native collision particle parameters exceed finite geometry or draw budget",
        ));
    }
    Ok(())
}

impl CmvsNativeCollisionParticles {
    /// 455030 consumes five CRT rand calls per particle, including one unused
    /// call between initial Y and fall-time jitter. Native save reload rebuilds.
    pub(crate) fn new(
        resource: Vec<u8>,
        parameter_words: [u32; 4],
        prng: &mut u32,
    ) -> Result<Self, CoreError> {
        validate_parameters(&parameter_words)?;
        if resource.len() >= 32 || resource.contains(&0) {
            return Err(invalid(
                "native collision particle resource exceeds its native buffer",
            ));
        }
        let mut candidate_rng = *prng;
        let base_z = f32::from_bits(parameter_words[1]);
        let fall_time = f32::from_bits(parameter_words[2]);
        let span = z_span(parameter_words[0]);
        let mut particles = Vec::with_capacity(parameter_words[3] as usize);
        for _ in 0..parameter_words[3] {
            let x = random(&mut candidate_rng) * 90.0 - 45.0;
            let z = base_z + random(&mut candidate_rng) * span;
            let y = random(&mut candidate_rng) * 75.0;
            random(&mut candidate_rng);
            let jitter = fall_time * 0.25;
            let time = fall_time + (random(&mut candidate_rng) * jitter - jitter * 0.5);
            particles.push(CmvsNativeCollisionParticle {
                position_words: [hundredths(x), hundredths(y), hundredths(z)].map(f32::to_bits),
                fall_time_word: time.to_bits(),
                collision_height_word: 0,
                alpha: 255,
            });
        }
        let result = Self {
            resource,
            parameter_words,
            projection_height_word: projection_height(&parameter_words),
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
            || self.particles.len() != self.parameter_words[3] as usize
        {
            return Err(invalid(
                "native collision particle owner has invalid resource or count",
            ));
        }
        for particle in &self.particles {
            if particle
                .position_words
                .iter()
                .any(|word| !f32::from_bits(*word).is_finite())
                || !f32::from_bits(particle.fall_time_word).is_finite()
                || f32::from_bits(particle.fall_time_word) == 0.0
                || particle.alpha > 255
            {
                return Err(invalid(
                    "native collision particle contains invalid motion storage",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn projection_height_word(&self) -> u32 {
        self.projection_height_word
    }

    /// 455680, unsigned elapsed cap120ms. The collision snapshot is shared by
    /// the whole frame, before any particle moves. No allocations in this loop.
    pub(crate) fn advance(
        &mut self,
        delta_ms: u32,
        vertices: &[[f32; 4]],
        prng: &mut u32,
    ) -> Result<(), CoreError> {
        self.validate()?;
        if !vertices.len().is_multiple_of(3) {
            return Err(invalid(
                "native collision query requires complete vertex triples",
            ));
        }
        let delta = delta_ms.min(120) as f32 * 10.0;
        let base_z = f32::from_bits(self.parameter_words[1]);
        let span = z_span(self.parameter_words[0]);
        for particle in &mut self.particles {
            let mut position = particle.position_words.map(f32::from_bits);
            position[1] -= delta / f32::from_bits(particle.fall_time_word);
            if !vertices.is_empty() {
                let cached = f32::from_bits(particle.collision_height_word);
                if cached < 1.0 {
                    if let Some(hit) = vertical_hit(position, vertices)? {
                        let difference = position[1] - hit[1];
                        if difference > -0.5 && difference <= 0.5 {
                            particle.alpha = cvtt((difference + 0.5) * 255.0);
                            particle.collision_height_word = hit[1].to_bits();
                        }
                    }
                } else {
                    let difference = position[1] - cached;
                    if difference > -0.5 {
                        if difference <= 0.5 {
                            particle.alpha = cvtt((difference + 0.5) * 255.0);
                        }
                    } else {
                        position[0] = hundredths(random(prng) * 90.0 - 45.0);
                        position[2] = hundredths(base_z + random(prng) * span);
                        position[1] += 75.0;
                        particle.collision_height_word = 0;
                        particle.alpha = 255;
                    }
                }
            }
            if position[1] < 0.0 {
                position[0] = hundredths(random(prng) * 90.0 - 45.0);
                position[2] = hundredths(base_z + random(prng) * span);
                position[1] += 75.0;
                particle.collision_height_word = 0;
                particle.alpha = 255;
            }
            particle.position_words = position.map(f32::to_bits);
        }
        Ok(())
    }

    pub(crate) fn capture(&self) -> Result<SavedNativeCollisionParticles, CoreError> {
        self.validate()?;
        let mut resource_field = [0; 32];
        resource_field[..self.resource.len()].copy_from_slice(&self.resource);
        Ok(SavedNativeCollisionParticles {
            parameter_words: self.parameter_words,
            resource_field,
        })
    }
}

impl SavedNativeCollisionParticles {
    /// 448FC0 kind6: version/kind WORDs, four parameters, 32-byte resource.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        if bytes.len() != 52 || bytes[..4] != [1, 0, 6, 0] {
            return Err(invalid(
                "unsupported native collision particle configuration",
            ));
        }
        let parameter_words = std::array::from_fn(|i| {
            u32::from_le_bytes(bytes[4 + i * 4..8 + i * 4].try_into().unwrap())
        });
        validate_parameters(&parameter_words)?;
        let resource_field: [u8; 32] = bytes[20..52].try_into().unwrap();
        resource_field
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| invalid("native collision particle resource is not NUL terminated"))?;
        Ok(Self {
            parameter_words,
            resource_field,
        })
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        let mut bytes = Vec::with_capacity(52);
        bytes.extend_from_slice(&[1, 0, 6, 0]);
        for word in self.parameter_words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&self.resource_field);
        Self::decode(&bytes)?;
        Ok(bytes)
    }
    pub(crate) fn rebuild(
        &self,
        prng: &mut u32,
    ) -> Result<CmvsNativeCollisionParticles, CoreError> {
        let length = self
            .resource_field
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| invalid("native collision particle resource is not NUL terminated"))?;
        CmvsNativeCollisionParticles::new(
            self.resource_field[..length].to_vec(),
            self.parameter_words,
            prng,
        )
    }
}

pub(crate) fn validate(state: &CmvsPs2aVmState) -> Result<(), CoreError> {
    for parent in state.texture_parents.values() {
        if let Some(geometry) = &parent.collision_geometry {
            geometry.validate()?;
        }
    }
    for children in state.texture_children.values() {
        for child in children.values() {
            if let Some(geometry) = &child.collision_geometry {
                geometry.validate()?;
            }
        }
    }
    for element in state.effect_elements.values() {
        if let Some(geometry) = &element.collision_geometry {
            geometry.validate()?;
        }
    }
    Ok(())
}

/// 4262A0 passes the parent table to 455680; its eight owner pointers at
/// byte80 are parent IDs20..27, including hidden images. Child geometry is
/// serialized independently but is not visited by this particular consumer.
pub(crate) fn collect_vertices(
    state: &CmvsPs2aVmState,
    vertices: &mut [[f32; 4]],
) -> Result<usize, CoreError> {
    let mut count = 0;
    for slot in 20..28 {
        if let Some(parent) = state.texture_parents.get(&slot)
            && let Some(geometry) = &parent.collision_geometry
        {
            count += geometry.translated(
                parent
                    .surface_initialized
                    .then_some(parent.float_position_words),
                &mut vertices[count..],
            )?;
        }
    }
    if count % 3 != 0 {
        return Err(invalid(
            "native collision query requires complete vertex triples",
        ));
    }
    Ok(count)
}

#[cfg(test)]
mod tests;
