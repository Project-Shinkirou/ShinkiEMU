//! Ordinary676: 4811A0 -> 448B90 -> 4542D0/454900/4549D0.
//! The 0x14044-byte owner has 2048 ten-DWORD slots, one CBasic per live slot.
//! Native2048 retains configuration; the private snapshot retains live phase.
use super::*;

const CAPACITY: usize = 2048;
const EMPTY: u16 = u16::MAX;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeParticleKind4Particle {
    pub active: bool,
    pub object_id: u32,
    pub last_tick_ms: u32,
    pub position_words: [u32; 3],
    /// Record DWORD6/7: copied at birth, with no trajectory consumer.
    pub unused_words: [u32; 2],
    pub angle_word: u32,
    pub speed_word: u32,
    /// CBasic properties are fixed at birth, independently of owner config.
    pub projection_depth_word: u32,
    pub rotation: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeParticleKind4 {
    pub resource: Vec<u8>,
    /// Flags, two unused words, angle(f32 degrees), speed(f32), interval(u32 ms).
    pub parameter_words: [u32; 6],
    pub particles: Vec<CmvsNativeParticleKind4Particle>,
    /// Wrapping native clock. The host seeds this at construction/restoration.
    /// Its absolute bits matter for the unrecognized X-distribution flag cases.
    pub clock_ms: u32,
    pub accumulator_ms: u32,
    pub next_object_id: u32,
    /// Preallocated equivalent of CImage's insertion-ordered linked child list.
    pub draw_order: Vec<u16>,
    pub draw_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SavedNativeParticleKind4 {
    pub parameter_words: [u32; 6],
    pub resource_field: [u8; 32],
}

fn invalid(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_PARTICLE_KIND4", message)
}

fn validate_parameters(words: &[u32; 6]) -> Result<(), CoreError> {
    // Native accepts arbitrary float words. Interval zero never exits4549D0.
    if words[5] == 0 {
        return Err(invalid(
            "native kind4 zero interval has a nonterminating spawn loop",
        ));
    }
    Ok(())
}

fn random(state: &mut u32) -> f32 {
    *state = state.wrapping_mul(214_013).wrapping_add(2_531_011);
    ((*state >> 16) & 0x7fff) as f32 / 32767.0_f32
}

fn native_rotation(angle: f32) -> u16 {
    // 454350 CVTTSS2SI, wrapping ADD360, signed remainder, then42E8A0 abs%360.
    let integer = if !(-2147483648.0_f32..2147483648.0_f32).contains(&angle) {
        i32::MIN
    } else {
        angle as i32
    };
    (integer.wrapping_add(360) % 360).unsigned_abs() as u16
}

fn displacement(angle: f32, distance: f32) -> [f32; 2] {
    // Shared native42ECD0: f32 degree conversion, f64 trig/product, f32 result.
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

impl CmvsNativeParticleKind4 {
    /// 454980 ignores the resource-load result;454900 consumes no CRT draws.
    /// Missing assets therefore leave spawning, motion and RNG unchanged.
    pub(crate) fn new(
        resource: Vec<u8>,
        parameter_words: [u32; 6],
        _prng: &mut u32,
    ) -> Result<Self, CoreError> {
        validate_parameters(&parameter_words)?;
        if resource.len() >= 32 || resource.contains(&0) {
            return Err(invalid(
                "native kind4 resource does not fit its NUL-terminated field",
            ));
        }
        Ok(Self {
            resource,
            parameter_words,
            particles: vec![CmvsNativeParticleKind4Particle::default(); CAPACITY],
            clock_ms: 0,
            accumulator_ms: 0,
            next_object_id: 0,
            draw_order: vec![EMPTY; CAPACITY],
            draw_count: 0,
        })
    }

    fn validate_header(&self) -> Result<(), CoreError> {
        validate_parameters(&self.parameter_words)?;
        if self.resource.len() >= 32
            || self.resource.contains(&0)
            || self.particles.len() != CAPACITY
            || self.draw_order.len() != CAPACITY
            || usize::from(self.draw_count) > CAPACITY
            || self.accumulator_ms > 1000
        {
            return Err(invalid("native kind4 owner layout or clock is invalid"));
        }
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        self.validate_header()?;
        let mut seen = [false; CAPACITY];
        for &index in self.draw_indices() {
            let index = usize::from(index);
            if index >= CAPACITY || seen[index] || !self.particles[index].active {
                return Err(invalid("native kind4 child ordering is inconsistent"));
            }
            seen[index] = true;
        }
        for (index, particle) in self.particles.iter().enumerate() {
            if particle.active != seen[index] {
                return Err(invalid(
                    "native kind4 active slot is absent from child ordering",
                ));
            }
            if particle.active
                && (particle.rotation >= 360
                    || ![20.5_f32, 30.5, 40.5, 50.5]
                        .map(f32::to_bits)
                        .contains(&particle.projection_depth_word))
            {
                return Err(invalid("native kind4 CBasic state is inconsistent"));
            }
        }
        if self.draw_order[usize::from(self.draw_count)..]
            .iter()
            .any(|index| *index != EMPTY)
        {
            return Err(invalid("native kind4 unused child ordering is not empty"));
        }
        Ok(())
    }

    fn spawn(&mut self, timestamp: u32, prng: &mut u32) {
        // 454350 searches the first inactive record before child allocation/rand.
        let Some(index) = self.particles.iter().position(|particle| !particle.active) else {
            return;
        };
        let x = match self.parameter_words[0] & 0xf0 {
            0x00 => random(prng) * 100.0_f32 - 50.0_f32,
            0x10 => random(prng) * 100.0_f32,
            0x20 => random(prng) * 100.0_f32 - 100.0_f32,
            0x30 => random(prng) * 100.0_f32 + 50.0_f32,
            0x40 => random(prng) * 100.0_f32 - 150.0_f32,
            0x50 => random(prng) * 100.0_f32 - 15.0_f32,
            0x60 => random(prng) * 100.0_f32 - 85.0_f32,
            0x70 => random(prng) * 100.0_f32 - 25.0_f32,
            0x80 => random(prng) * 100.0_f32 - 75.0_f32,
            0xa0 => random(prng) * 100.0_f32 - 78.0_f32,
            // This is the raw stack timestamp MOVSS, not a numeric conversion.
            _ => f32::from_bits(timestamp),
        };
        let z = match self.parameter_words[0] & 0xf00 {
            0x000 => random(prng) * 100.0_f32 + 0.0_f32,
            0x100 => random(prng) * 50.0_f32 + 0.0_f32,
            0x200 => random(prng) * 50.0_f32 + 10.0_f32,
            0x300 => random(prng) * 50.0_f32 + 20.0_f32,
            0x400 => random(prng) * 50.0_f32 + 30.0_f32,
            0x500 => random(prng) * 50.0_f32 + 40.0_f32,
            0x600 => random(prng) * 50.0_f32 + 50.0_f32,
            _ => x,
        };
        self.particles[index] = CmvsNativeParticleKind4Particle {
            active: true,
            object_id: self.next_object_id,
            last_tick_ms: timestamp,
            position_words: [x.to_bits(), 150.0_f32.to_bits(), z.to_bits()],
            unused_words: [self.parameter_words[1], self.parameter_words[2]],
            angle_word: self.parameter_words[3],
            speed_word: self.parameter_words[4],
            projection_depth_word: (20.5_f32 + 10.0_f32 * (self.parameter_words[0] & 3) as f32)
                .to_bits(),
            rotation: native_rotation(f32::from_bits(self.parameter_words[3])),
        };
        self.next_object_id = self.next_object_id.wrapping_add(1);
        self.draw_order[usize::from(self.draw_count)] = index as u16;
        self.draw_count += 1;
    }

    /// Spawn debt is capped at1000ms; live-particle elapsed time is uncapped.
    /// Spawning precedes motion/recycling, including both0x1000 births.
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
            if self.parameter_words[0] & 0x1000 != 0 {
                self.spawn(timestamp, prng);
            }
            self.accumulator_ms -= interval;
        }
        for particle in &mut self.particles {
            if !particle.active {
                continue;
            }
            let elapsed = self.clock_ms.wrapping_sub(particle.last_tick_ms) as i32;
            let distance = (elapsed as f32 * f32::from_bits(particle.speed_word)) / 250.0_f32;
            let [mut x, mut y, z] = particle.position_words.map(f32::from_bits);
            let angle = f32::from_bits(particle.angle_word);
            if angle == 0.0 {
                y -= distance;
            } else {
                let [dx, dy] = displacement(angle, distance);
                y -= dy;
                x += dx;
            }
            particle.position_words = [x.to_bits(), y.to_bits(), z.to_bits()];
            // Native COMISS/JBE also recycles unordered (NaN) Y.
            if y.partial_cmp(&-50.0_f32) != Some(std::cmp::Ordering::Greater) {
                particle.active = false;
            } else {
                particle.last_tick_ms = self.clock_ms;
            }
        }
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

    pub(crate) fn draw_indices(&self) -> &[u16] {
        &self.draw_order[..usize::from(self.draw_count)]
    }

    /// Index is a native fixed slot, not an ordinal in the linked child list.
    pub(crate) fn plane_position(&self, index: usize) -> Option<[u32; 3]> {
        self.particles
            .get(index)
            .filter(|particle| particle.active)
            .map(|particle| particle.position_words)
    }

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
            projection_height_word: 20.0_f32.to_bits(),
            rotation_hundredths: i32::from(particle.rotation) * 100,
            rect_words: Some([0, 0, 6, 112]),
            position_words: Some([3, 112]),
            ..Default::default()
        })
    }

    pub(crate) fn capture(&self) -> Result<SavedNativeParticleKind4, CoreError> {
        self.validate()?;
        let mut resource_field = [0; 32];
        resource_field[..self.resource.len()].copy_from_slice(&self.resource);
        Ok(SavedNativeParticleKind4 {
            parameter_words: self.parameter_words,
            resource_field,
        })
    }
}

impl SavedNativeParticleKind4 {
    /// 448FC0 copies56 config bytes from4549C0 after version/kind WORDs.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        if bytes.len() != 60 || bytes[..4] != [1, 0, 4, 0] {
            return Err(invalid("unsupported native kind4 configuration record"));
        }
        let parameter_words = std::array::from_fn(|index| {
            u32::from_le_bytes(bytes[4 + index * 4..8 + index * 4].try_into().unwrap())
        });
        validate_parameters(&parameter_words)?;
        let resource_field: [u8; 32] = bytes[28..60].try_into().unwrap();
        if !resource_field.contains(&0) {
            return Err(invalid("native kind4 resource is not NUL terminated"));
        }
        Ok(Self {
            parameter_words,
            resource_field,
        })
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        validate_parameters(&self.parameter_words)?;
        if !self.resource_field.contains(&0) {
            return Err(invalid("native kind4 resource is not NUL terminated"));
        }
        let mut bytes = Vec::with_capacity(60);
        bytes.extend_from_slice(&[1, 0, 4, 0]);
        for word in self.parameter_words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&self.resource_field);
        Ok(bytes)
    }

    /// 449710 reconstructs an empty pool and resets debt without consuming RNG.
    /// The host seeds the new native clock; private serde restores it verbatim.
    pub(crate) fn rebuild(&self, prng: &mut u32) -> Result<CmvsNativeParticleKind4, CoreError> {
        let length = self
            .resource_field
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| invalid("native kind4 resource is not NUL terminated"))?;
        CmvsNativeParticleKind4::new(
            self.resource_field[..length].to_vec(),
            self.parameter_words,
            prng,
        )
    }
}

#[cfg(test)]
mod tests;
