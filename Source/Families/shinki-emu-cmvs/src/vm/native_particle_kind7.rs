//! Ordinary679: native4489F0/455AA0 plane particles and455ED0 flash clock.
//! Native2048 retains configuration; private snapshots retain both live clocks.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeParticleKind7Particle {
    pub position_words: [u32; 3],
    pub fall_time_word: u32,
    /// CBasic starts at255 even when the owner's fade-in alpha starts at0.
    pub basic_alpha: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeParticleKind7 {
    pub resource: Vec<u8>,
    /// Flags, base Z, fall time, particle count; floats retain their raw bits.
    pub parameter_words: [u32; 4],
    pub particles: Vec<CmvsNativeParticleKind7Particle>,
    /// Native+8: parent seeds the current41A000 session clock after construction.
    pub clock_ms: u32,
    /// Wrapping now-minus-native+28, independent of the capped motion delta.
    pub flash_elapsed_ms: u32,
    pub flash_mode: u32,
    pub alpha: u32,
    pub flash_delay_ms: u32,
    pub flash_duration_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SavedNativeParticleKind7 {
    pub parameter_words: [u32; 4],
    pub resource_field: [u8; 32],
}

fn invalid(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_PARTICLE_KIND7", message)
}

fn random(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(214_013).wrapping_add(2_531_011);
    (*state >> 16) & 0x7fff
}

fn unit_random(state: &mut u32) -> f32 {
    random(state) as f32 / 32767.0
}

/// CVTTSS2SI's indefinite result is INT_MIN, including NaN and overflow.
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

impl CmvsNativeParticleKind7 {
    ///455AA0 consumes X/Z/Y, an unused draw, then fall-time jitter per birth.
    ///455E90's resource-load return never gates particle construction.
    pub(crate) fn new(
        resource: Vec<u8>,
        parameter_words: [u32; 4],
        prng: &mut u32,
    ) -> Result<Self, CoreError> {
        if resource.len() >= 32 || resource.contains(&0) {
            return Err(invalid("native kind7 resource exceeds its fixed field"));
        }
        let [flags, base_z, fall_time, count] = parameter_words;
        let base_z = f32::from_bits(base_z);
        let fall_time = f32::from_bits(fall_time);
        let spread = depth_spread(flags);
        let mut particles = Vec::new();
        particles
            .try_reserve_exact(count as usize)
            .map_err(|_| invalid("native kind7 particle allocation cannot be represented"))?;
        for _ in 0..count {
            let x = unit_random(prng) * 90.0 - 45.0;
            let z = base_z + unit_random(prng) * spread;
            let y = unit_random(prng) * 75.0;
            random(prng);
            let jitter = fall_time * 0.25;
            let time = fall_time + (unit_random(prng) * jitter - jitter * 0.5);
            particles.push(CmvsNativeParticleKind7Particle {
                position_words: [hundredths(x), hundredths(y), hundredths(z)].map(f32::to_bits),
                fall_time_word: time.to_bits(),
                basic_alpha: 255,
            });
        }
        let flash_mode = ((flags >> 16) & 255) % 3;
        Ok(Self {
            resource,
            parameter_words,
            particles,
            clock_ms: 0,
            flash_elapsed_ms: 0,
            flash_mode,
            alpha: if flash_mode == 1 { 0 } else { 255 },
            flash_delay_ms: ((flags >> 8) & 255) * 100,
            flash_duration_ms: (flags & 255) * 100,
        })
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if self.resource.len() >= 32
            || self.resource.contains(&0)
            || self.particles.len() != self.parameter_words[3] as usize
        {
            return Err(invalid(
                "native kind7 resource or particle count is inconsistent",
            ));
        }
        Ok(())
    }

    ///455ED0 updates alpha even at zero motion. Motion is capped at120ms;
    ///flash compares full wrapping elapsed time with unsigned inclusive delay.
    pub(crate) fn advance(&mut self, delta_ms: u32, prng: &mut u32) -> Result<(), CoreError> {
        self.validate()?;
        self.clock_ms = self.clock_ms.wrapping_add(delta_ms);
        self.flash_elapsed_ms = self.flash_elapsed_ms.wrapping_add(delta_ms);
        match self.flash_mode {
            1 | 2 => {
                let fade_in = self.flash_mode == 1;
                self.alpha = if self.flash_elapsed_ms < self.flash_delay_ms {
                    if fade_in { 0 } else { 255 }
                } else {
                    let elapsed = self.flash_elapsed_ms.wrapping_sub(self.flash_delay_ms);
                    if elapsed < self.flash_duration_ms {
                        let alpha = 255_u32.wrapping_mul(elapsed) / self.flash_duration_ms;
                        if fade_in {
                            alpha
                        } else {
                            255_u32.wrapping_sub(alpha)
                        }
                    } else if fade_in {
                        255
                    } else {
                        0
                    }
                };
            }
            _ => {}
        }
        let distance = delta_ms.min(120) as f32 * 10.0;
        let base_z = f32::from_bits(self.parameter_words[1]);
        let spread = depth_spread(self.parameter_words[0]);
        for particle in &mut self.particles {
            let mut y = f32::from_bits(particle.position_words[1])
                - distance / f32::from_bits(particle.fall_time_word);
            if y < 0.0 {
                particle.position_words[0] = hundredths(unit_random(prng) * 90.0 - 45.0).to_bits();
                particle.position_words[2] =
                    hundredths(base_z + unit_random(prng) * spread).to_bits();
                // Native adds once, even if a tiny fall time leaves Y negative.
                y += 75.0;
            }
            particle.position_words[1] = y.to_bits();
            particle.basic_alpha = self.alpha;
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

    ///42B7A0 defaults: rotation0, scale1, blend1;445E30(0) clears shake bit100.
    ///446A30 selects CBasic2,446B30 crops32²,446AD0 sets pivot16².
    pub(crate) fn draw_basic(&self, index: usize) -> Option<CmvsTextureParentState> {
        let particle = self.particles.get(index)?;
        let depth = match self.parameter_words[0] >> 28 {
            4 => 60.0,
            5 => 50.0,
            6 => 40.0,
            7 => 30.0,
            8 => 20.0,
            _ => f32::from_bits(self.parameter_words[1]) + 5.0,
        };
        Some(CmvsTextureParentState {
            resource_frame: Some(0),
            surface_initialized: true,
            screen_shake_disabled: true,
            basic_kind: 2,
            blend_mode: Some(1),
            rect_words: Some([0, 0, 32, 32]),
            position_words: Some([16, 16]),
            float_position_words: particle.position_words,
            projection_depth_word: depth.to_bits(),
            projection_height_word: 0,
            alpha: Some(particle.basic_alpha as i32),
            scale_words: Some([1.0_f32.to_bits(); 2]),
            ..Default::default()
        })
    }

    pub(crate) fn capture(&self) -> Result<SavedNativeParticleKind7, CoreError> {
        self.validate()?;
        let mut resource_field = [0; 32];
        resource_field[..self.resource.len()].copy_from_slice(&self.resource);
        Ok(SavedNativeParticleKind7 {
            parameter_words: self.parameter_words,
            resource_field,
        })
    }
}

impl SavedNativeParticleKind7 {
    ///448FC0 case7 copies48 bytes from455EC0's native+32, following version/kind.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        if bytes.len() != 52 || bytes[..4] != [1, 0, 7, 0] {
            return Err(invalid("unsupported native kind7 configuration record"));
        }
        let parameter_words = std::array::from_fn(|index| {
            u32::from_le_bytes(bytes[4 + index * 4..8 + index * 4].try_into().unwrap())
        });
        let saved = Self {
            parameter_words,
            resource_field: bytes[20..52].try_into().unwrap(),
        };
        saved.validate()?;
        Ok(saved)
    }

    fn validate(&self) -> Result<(), CoreError> {
        if !self.resource_field.contains(&0) {
            return Err(invalid("native kind7 resource is not NUL terminated"));
        }
        Ok(())
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(52);
        bytes.extend_from_slice(&[1, 0, 7, 0]);
        for word in self.parameter_words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&self.resource_field);
        Ok(bytes)
    }

    ///449710 case6 reruns resource setup/births and resets both flash/motion phase.
    pub(crate) fn rebuild(&self, prng: &mut u32) -> Result<CmvsNativeParticleKind7, CoreError> {
        self.validate()?;
        let length = self
            .resource_field
            .iter()
            .position(|byte| *byte == 0)
            .unwrap();
        CmvsNativeParticleKind7::new(
            self.resource_field[..length].to_vec(),
            self.parameter_words,
            prng,
        )
    }
}

#[cfg(test)]
mod tests;
