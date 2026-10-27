//! Ordinary677: native448840/453490 animated CBasic particles.
//! Native2048 stores configuration; private snapshots also retain live phase.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeParticleKind5Particle {
    /// Native particle12/16/20, retained even though453AD0 only moves28/32/36.
    pub initial_position_words: [u32; 3],
    pub position_words: [u32; 3],
    /// Native24 and40. This heading is not the CBasic rendering rotation.
    pub initial_heading_word: u32,
    pub heading_word: u32,
    pub travel_time_word: u32,
    pub atlas_variant: u8,
    /// CBasic sprite program4325F0's current record and residual milliseconds.
    pub animation_frame: u8,
    pub animation_elapsed_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeParticleKind5 {
    pub resource: Vec<u8>,
    /// Flags, base X, base Z, heading offset, travel time, particle count.
    pub parameter_words: [u32; 6],
    pub particles: Vec<CmvsNativeParticleKind5Particle>,
    /// Native41A000 baseline; parent seeds the current session clock after births.
    pub clock_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SavedNativeParticleKind5 {
    pub parameter_words: [u32; 6],
    pub resource_field: [u8; 32],
}

fn invalid(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_PARTICLE_KIND5", message)
}

fn random(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(214_013).wrapping_add(2_531_011);
    (*state >> 16) & 0x7fff
}

fn unit_random(state: &mut u32) -> f32 {
    random(state) as f32 / 32767.0
}

/// SSE CVTTSS2SI returns the indefinite integer for NaN/overflow, unlike
/// Rust's saturating float cast. Every intermediate here is native SSE f32.
fn hundredths(value: f32) -> f32 {
    let scaled = value * 100.0;
    let integer = if (-2147483648.0..2147483648.0).contains(&scaled) {
        scaled as i32
    } else {
        i32::MIN
    };
    integer as f32 / 100.0
}

fn depth_spread(flags: u32) -> f32 {
    match (flags >> 24) & 15 {
        1 => 15.0,
        2 => 10.0,
        3 => 5.0,
        4 => 2.5,
        5 => 1.0,
        _ => 20.0,
    }
}

impl CmvsNativeParticleKind5 {
    ///453A90 loads the provider first, but its return value never gates453490.
    ///Each birth consumes eight shared CRT draws, including the two original
    ///position draws whose values do not feed the visible starting position.
    pub(crate) fn new(
        resource: Vec<u8>,
        parameter_words: [u32; 6],
        prng: &mut u32,
    ) -> Result<Self, CoreError> {
        if resource.len() >= 32 || resource.contains(&0) {
            return Err(invalid("native kind5 resource exceeds its fixed field"));
        }
        let [flags, x, z, heading, time, count] = parameter_words;
        let x = f32::from_bits(x);
        let z = f32::from_bits(z);
        let heading = f32::from_bits(heading);
        let time = f32::from_bits(time);
        let spread = depth_spread(flags);
        let mut particles = Vec::new();
        //8192 bounds the shared draw queue, not native453490's allocation.
        particles
            .try_reserve_exact(count as usize)
            .map_err(|_| invalid("native kind5 particle allocation cannot be represented"))?;
        for _ in 0..count {
            let original_x = (x + unit_random(prng) * 120.0) - 60.0;
            let original_z = z + unit_random(prng) * 15.0;
            let live_x = (x + unit_random(prng) * 120.0) - 60.0;
            let live_z = z + unit_random(prng) * spread;
            let live_y = unit_random(prng) * 100.0;
            let mut angle = (heading + ((random(prng) % 100) as f32 / 10.0 - 5.0)) + 270.0;
            if angle >= 360.0 {
                angle -= 360.0;
            }
            let jitter = time / 5.0;
            let travel_time = time + (unit_random(prng) * jitter - jitter * 0.5);
            let atlas_variant = (random(prng) % 3) as u8;
            particles.push(CmvsNativeParticleKind5Particle {
                initial_position_words: [original_x, 100.0, original_z].map(f32::to_bits),
                position_words: [hundredths(live_x), hundredths(live_y), hundredths(live_z)]
                    .map(f32::to_bits),
                initial_heading_word: angle.to_bits(),
                heading_word: angle.to_bits(),
                travel_time_word: travel_time.to_bits(),
                atlas_variant,
                animation_frame: 0,
                animation_elapsed_ms: 0,
            });
        }
        Ok(Self {
            resource,
            parameter_words,
            particles,
            clock_ms: 0,
        })
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if self.resource.len() >= 32
            || self.resource.contains(&0)
            || self.particles.len() != self.parameter_words[5] as usize
            || self.particles.iter().any(|particle| {
                particle.atlas_variant >= 3
                    || particle.animation_frame >= 10
                    || particle.animation_elapsed_ms >= 100
            })
        {
            return Err(invalid(
                "native kind5 resource, count or sprite phase is inconsistent",
            ));
        }
        Ok(())
    }

    ///453AD0 uses the worker clock41A000 (cap500);4325F0 uses the separate
    ///global frame delta419AA0 (cap200). Parent owns both clock domains.
    pub(crate) fn advance(
        &mut self,
        delta_ms: u32,
        animation_delta_ms: u32,
        prng: &mut u32,
    ) -> Result<(), CoreError> {
        self.validate()?;
        self.clock_ms = self.clock_ms.wrapping_add(delta_ms);
        let distance = delta_ms.min(500) as f32 * 10.0;
        let base_x = f32::from_bits(self.parameter_words[1]);
        let base_z = f32::from_bits(self.parameter_words[2]);
        let spread = depth_spread(self.parameter_words[0]);
        for particle in &mut self.particles {
            let step = distance / f32::from_bits(particle.travel_time_word);
            let heading = f32::from_bits(particle.heading_word);
            //453BAE/453BB6 round radians to f32 before the precise double
            //libm calls. The product is double, then narrowed before CVTTSS2SI.
            let radians = ((heading / 180.0) * f32::from_bits(0x4049_0fdb)) as f64;
            let mut x = f32::from_bits(particle.position_words[0]);
            let mut y = f32::from_bits(particle.position_words[1]);
            let mut z = f32::from_bits(particle.position_words[2]);
            x += hundredths((radians.cos() * step as f64) as f32);
            y += hundredths((radians.sin() * step as f64) as f32);
            if y < 0.0 {
                x = hundredths((base_x + unit_random(prng) * 120.0) - 60.0);
                z = hundredths(base_z + unit_random(prng) * spread);
                y = 100.0;
            }
            particle.position_words = [x, y, z].map(f32::to_bits);
            let elapsed = particle.animation_elapsed_ms + animation_delta_ms.min(200);
            particle.animation_frame = (particle.animation_frame + (elapsed / 100) as u8) % 10;
            particle.animation_elapsed_ms = elapsed % 100;
        }
        Ok(())
    }

    pub(crate) fn particle_capacity(&self) -> usize {
        self.particles.len()
    }

    pub(crate) fn plane_position(&self, index: usize) -> Option<[u32; 3]> {
        self.particles
            .get(index)
            .map(|particle| particle.position_words)
    }

    ///CBasic rotation stays zero; the fractional heading only controls motion.
    pub(crate) fn draw_basic(&self, index: usize) -> Option<CmvsTextureParentState> {
        let particle = self.particles.get(index)?;
        let frame = u32::from(particle.animation_frame);
        let (column, row) = if frame < 6 {
            (0, frame)
        } else {
            (20, frame - 6)
        };
        let projection_depth = match self.parameter_words[0] >> 28 {
            4 => 60.0,
            5 => 50.0,
            6 => 40.0,
            7 => 30.0,
            8 => 20.0,
            _ => f32::from_bits(self.parameter_words[2]) + 5.0,
        };
        Some(CmvsTextureParentState {
            resource_frame: Some(0),
            surface_initialized: true,
            basic_kind: 2,
            blend_mode: Some(1),
            rect_words: Some([
                40 * u32::from(particle.atlas_variant) + column,
                row * 20,
                20,
                20,
            ]),
            position_words: Some([10, 10]),
            float_position_words: particle.position_words,
            projection_depth_word: projection_depth.to_bits(),
            projection_height_word: 0,
            alpha: Some(255),
            scale_words: Some([1.0_f32.to_bits(); 2]),
            ..Default::default()
        })
    }

    pub(crate) fn capture(&self) -> Result<SavedNativeParticleKind5, CoreError> {
        self.validate()?;
        let mut resource_field = [0; 32];
        resource_field[..self.resource.len()].copy_from_slice(&self.resource);
        Ok(SavedNativeParticleKind5 {
            parameter_words: self.parameter_words,
            resource_field,
        })
    }
}

impl SavedNativeParticleKind5 {
    ///448FC0 emits version1/kind5, six raw DWORDs and a32-byte name field.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        if bytes.len() != 60 || bytes[..4] != [1, 0, 5, 0] {
            return Err(invalid("unsupported native kind5 configuration record"));
        }
        let parameter_words = std::array::from_fn(|index| {
            u32::from_le_bytes(bytes[4 + index * 4..8 + index * 4].try_into().unwrap())
        });
        let resource_field = bytes[28..60].try_into().unwrap();
        let saved = Self {
            parameter_words,
            resource_field,
        };
        saved.validate()?;
        Ok(saved)
    }

    fn validate(&self) -> Result<(), CoreError> {
        if !self.resource_field.contains(&0) {
            return Err(invalid("native kind5 resource is not NUL terminated"));
        }
        Ok(())
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(60);
        bytes.extend_from_slice(&[1, 0, 5, 0]);
        for word in self.parameter_words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&self.resource_field);
        Ok(bytes)
    }

    ///449710 reruns resource setup and all births against current shared RNG;
    ///configuration records never restore saved positions or sprite phase.
    pub(crate) fn rebuild(&self, prng: &mut u32) -> Result<CmvsNativeParticleKind5, CoreError> {
        self.validate()?;
        let length = self
            .resource_field
            .iter()
            .position(|byte| *byte == 0)
            .unwrap();
        CmvsNativeParticleKind5::new(
            self.resource_field[..length].to_vec(),
            self.parameter_words,
            prng,
        )
    }
}

#[cfg(test)]
mod tests;
