//! Ordinary 673: 4812F0 -> 448C80 -> 453DA0/454060/454130.
//! The native 0xA044-byte owner contains 1024 ten-DWORD particle slots.
//! Native saves retain configuration only; serde retains the complete live phase.

use super::*;

pub const CMVS_NATIVE_PARTICLE_KIND1_CAPACITY: usize = 1024;
const EMPTY: u16 = u16::MAX;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeParticleKind1Particle {
    pub active: bool,
    pub object_id: u32,
    pub last_tick_ms: u32,
    pub position_words: [u32; 3],
    /// Native record DWORD6/7: copied but never read by this worker.
    pub unused_words: [u32; 2],
    pub angle_word: u32,
    pub speed_word: u32,
    /// Newborn CBasic state, independent of the subsequently retained config.
    pub projection_depth_word: u32,
    pub rotation: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeParticleKind1 {
    pub resource: Vec<u8>,
    /// Flags, two unused words, angle(float), speed(float), interval(unsigned ms).
    pub parameter_words: [u32; 6],
    pub particles: Vec<CmvsNativeParticleKind1Particle>,
    /// Clock relative to creation, with the original wrapping DWORD arithmetic.
    pub clock_ms: u32,
    pub accumulator_ms: u32,
    pub next_object_id: u32,
    /// Fixed-size storage for CImage's insertion-ordered linked child list.
    pub draw_order: Vec<u16>,
    pub draw_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SavedNativeParticleKind1 {
    pub parameter_words: [u32; 6],
    pub resource_field: [u8; 32],
}

fn invalid(message: &str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_PARTICLE_KIND1", message)
}
fn validate_parameters(words: &[u32; 6]) -> Result<(), CoreError> {
    // The unused words and upper flag bits have no native consumer.
    if !f32::from_bits(words[3]).is_finite() || !f32::from_bits(words[4]).is_finite() {
        return Err(invalid("native kind1 angle or speed is non-finite"));
    }
    if words[5] == 0 {
        return Err(invalid(
            "native kind1 zero interval would never finish its spawn loop",
        ));
    }
    Ok(())
}
fn random(state: &mut u32) -> f32 {
    *state = state.wrapping_mul(214_013).wrapping_add(2_531_011);
    ((*state >> 16) & 0x7fff) as f32 / 32767.0
}
fn native_rotation(angle: f32) -> u16 {
    // CVTTSS2SI followed by wrapping ADD, signed IDIV, then abs()%360.
    let integer = if !(-2147483648.0..2147483648.0).contains(&angle) {
        i32::MIN
    } else {
        angle as i32
    };
    (integer.wrapping_add(360) % 360).unsigned_abs() as u16
}
fn displacement(angle: f32, distance: f32) -> [f32; 2] {
    // 42ECD0 performs BOTH degree conversion operations in SSE single precision,
    // calls double-precision trig, multiplies in double, then rounds to f32.
    let degrees = angle + 90.0_f32;
    let radians = if degrees == 0.0 {
        0.0
    } else {
        (degrees / 180.0_f32) * std::f32::consts::PI
    } as f64;
    [
        (radians.cos() * f64::from(distance)) as f32,
        (radians.sin() * f64::from(distance)) as f32,
    ]
}

impl CmvsNativeParticleKind1 {
    pub(crate) fn new(
        resource: Vec<u8>,
        parameter_words: [u32; 6],
        _prng: &mut u32,
    ) -> Result<Self, CoreError> {
        validate_parameters(&parameter_words)?;
        if resource.len() >= 32 || resource.contains(&0) {
            return Err(invalid(
                "native kind1 resource does not fit its NUL-terminated field",
            ));
        }
        Ok(Self {
            resource,
            parameter_words,
            particles: vec![
                CmvsNativeParticleKind1Particle::default();
                CMVS_NATIVE_PARTICLE_KIND1_CAPACITY
            ],
            clock_ms: 0,
            accumulator_ms: 0,
            next_object_id: 0,
            draw_order: vec![EMPTY; CMVS_NATIVE_PARTICLE_KIND1_CAPACITY],
            draw_count: 0,
        })
    }

    fn validate_header(&self) -> Result<(), CoreError> {
        validate_parameters(&self.parameter_words)?;
        if self.resource.len() >= 32
            || self.resource.contains(&0)
            || self.particles.len() != CMVS_NATIVE_PARTICLE_KIND1_CAPACITY
            || self.draw_order.len() != CMVS_NATIVE_PARTICLE_KIND1_CAPACITY
            || usize::from(self.draw_count) > CMVS_NATIVE_PARTICLE_KIND1_CAPACITY
            || self.accumulator_ms > 1000
        {
            return Err(invalid("native kind1 owner layout or clock is invalid"));
        }
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        self.validate_header()?;
        let mut seen = [false; CMVS_NATIVE_PARTICLE_KIND1_CAPACITY];
        for &index in &self.draw_order[..usize::from(self.draw_count)] {
            let index = usize::from(index);
            if index >= self.particles.len() || seen[index] || !self.particles[index].active {
                return Err(invalid("native kind1 child ordering is inconsistent"));
            }
            seen[index] = true;
        }
        for (index, particle) in self.particles.iter().enumerate() {
            if particle.active != seen[index] {
                return Err(invalid(
                    "native kind1 active slot is absent from child ordering",
                ));
            }
            if particle.active
                && (particle
                    .position_words
                    .iter()
                    .any(|word| !f32::from_bits(*word).is_finite())
                    || !f32::from_bits(particle.angle_word).is_finite()
                    || !f32::from_bits(particle.speed_word).is_finite()
                    || ![
                        3.0_f32.to_bits(),
                        4.0_f32.to_bits(),
                        5.0_f32.to_bits(),
                        6.0_f32.to_bits(),
                    ]
                    .contains(&particle.projection_depth_word)
                    || particle.rotation >= 360)
            {
                return Err(invalid("native kind1 live particle geometry is invalid"));
            }
        }
        if self.draw_order[usize::from(self.draw_count)..]
            .iter()
            .any(|index| *index != EMPTY)
        {
            return Err(invalid("native kind1 unused child ordering is not empty"));
        }
        Ok(())
    }

    fn spawn(&mut self, timestamp: u32, prng: &mut u32) {
        // 453E50 finds the first inactive slot BEFORE allocating a child or rand().
        let Some(index) = self.particles.iter().position(|particle| !particle.active) else {
            return;
        };
        let x = random(prng) * 10.0_f32 - 5.0_f32;
        let z = random(prng) * 10.0_f32;
        self.particles[index] = CmvsNativeParticleKind1Particle {
            active: true,
            object_id: self.next_object_id,
            last_tick_ms: timestamp,
            position_words: [x.to_bits(), 6.0_f32.to_bits(), z.to_bits()],
            unused_words: [self.parameter_words[1], self.parameter_words[2]],
            angle_word: self.parameter_words[3],
            speed_word: self.parameter_words[4],
            projection_depth_word: (3.0_f32 + (self.parameter_words[0] & 3) as f32).to_bits(),
            rotation: native_rotation(f32::from_bits(self.parameter_words[3])),
        };
        self.next_object_id = self.next_object_id.wrapping_add(1);
        self.draw_order[usize::from(self.draw_count)] = index as u16;
        self.draw_count += 1;
    }

    /// 454130 caps accumulated spawn debt at 1000ms, NOT particle elapsed time.
    /// Slots freed by this frame become reusable only on the following frame.
    pub(crate) fn advance(&mut self, delta_ms: u32, prng: &mut u32) -> Result<(), CoreError> {
        self.validate_header()?;
        self.clock_ms = self.clock_ms.wrapping_add(delta_ms);
        self.accumulator_ms = self.accumulator_ms.wrapping_add(delta_ms).min(1000);
        let interval = self.parameter_words[5];
        while self.accumulator_ms > interval {
            let timestamp = self
                .clock_ms
                .wrapping_add(interval)
                .wrapping_sub(self.accumulator_ms);
            self.spawn(timestamp, prng);
            self.accumulator_ms -= interval;
        }
        for particle in &mut self.particles {
            if !particle.active {
                continue;
            }
            let elapsed = self.clock_ms.wrapping_sub(particle.last_tick_ms) as i32;
            let distance = (elapsed as f32 * f32::from_bits(particle.speed_word)) / 1000.0_f32;
            let [mut x, mut y, z] = particle.position_words.map(f32::from_bits);
            let angle = f32::from_bits(particle.angle_word);
            if angle == 0.0 {
                y -= distance;
            } else {
                let [dx, dy] = displacement(angle, distance);
                y -= dy;
                x += dx;
            }
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                return Err(invalid("native kind1 particle motion is non-finite"));
            }
            particle.position_words = [x.to_bits(), y.to_bits(), z.to_bits()];
            if y <= 0.0 {
                particle.active = false;
            } else {
                particle.last_tick_ms = self.clock_ms;
            }
        }
        // CImage unlinks deaths without changing the other children's order.
        let old_count = usize::from(self.draw_count);
        let mut count = 0;
        for cursor in 0..old_count {
            let index = self.draw_order[cursor];
            if self.particles[usize::from(index)].active {
                self.draw_order[count] = index;
                count += 1;
            }
        }
        self.draw_order[count..old_count].fill(EMPTY);
        self.draw_count = count as u16;
        Ok(())
    }

    /// Index is a native fixed particle slot, not a child-list ordinal.
    /// No resource name is copied; the manager supplies the inherited provider.
    pub(crate) fn draw_basic(&self, index: usize) -> Option<CmvsTextureParentState> {
        let particle = self
            .particles
            .get(index)
            .filter(|particle| particle.active)?;
        Some(CmvsTextureParentState {
            surface_initialized: true,
            alpha: Some(255),
            scale_words: Some([1.0_f32.to_bits(); 2]),
            basic_kind: 2,
            float_position_words: particle.position_words,
            projection_depth_word: particle.projection_depth_word,
            projection_height_word: 3.0_f32.to_bits(),
            rotation_hundredths: i32::from(particle.rotation) * 100,
            rect_words: Some([0, 0, 6, 112]),
            position_words: Some([3, 112]),
            ..Default::default()
        })
    }

    pub(crate) fn plane_position(&self, index: usize) -> Option<[u32; 3]> {
        self.particles
            .get(index)
            .filter(|particle| particle.active)
            .map(|particle| particle.position_words)
    }

    pub(crate) fn draw_indices(&self) -> &[u16] {
        &self.draw_order[..usize::from(self.draw_count)]
    }

    pub(crate) fn capture(&self) -> Result<SavedNativeParticleKind1, CoreError> {
        self.validate()?;
        let mut resource_field = [0; 32];
        resource_field[..self.resource.len()].copy_from_slice(&self.resource);
        Ok(SavedNativeParticleKind1 {
            parameter_words: self.parameter_words,
            resource_field,
        })
    }
}

impl SavedNativeParticleKind1 {
    /// 448FC0 returns 60 bytes. The containing 2048-byte save slot is not phase.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        if bytes.len() != 60 || bytes[..4] != [1, 0, 1, 0] {
            return Err(invalid("unsupported native kind1 configuration record"));
        }
        let parameter_words = std::array::from_fn(|index| {
            u32::from_le_bytes(bytes[4 + index * 4..8 + index * 4].try_into().unwrap())
        });
        validate_parameters(&parameter_words)?;
        let resource_field: [u8; 32] = bytes[28..60].try_into().unwrap();
        if !resource_field.contains(&0) {
            return Err(invalid("native kind1 resource is not NUL terminated"));
        }
        Ok(Self {
            parameter_words,
            resource_field,
        })
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        validate_parameters(&self.parameter_words)?;
        if !self.resource_field.contains(&0) {
            return Err(invalid("native kind1 resource is not NUL terminated"));
        }
        let mut bytes = Vec::with_capacity(60);
        bytes.extend_from_slice(&[1, 0, 1, 0]);
        for word in self.parameter_words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&self.resource_field);
        Ok(bytes)
    }

    pub(crate) fn rebuild(&self, prng: &mut u32) -> Result<CmvsNativeParticleKind1, CoreError> {
        let length = self
            .resource_field
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| invalid("native kind1 resource is not NUL terminated"))?;
        CmvsNativeParticleKind1::new(
            self.resource_field[..length].to_vec(),
            self.parameter_words,
            prng,
        )
    }
}

#[cfg(test)]
mod tests;
