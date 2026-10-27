//! Ordinary681: native448780/415E40 flat CBasic0 particles.
//! Native2048 stores configuration; private snapshots retain fixed-point phase.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeParticleKind9Particle {
    /// Native particle12/16: wrapping signed 22.10 pixel coordinates.
    pub fixed_position_words: [u32; 2],
    /// Native particle20, including the birth jitter and atlas speed multiplier.
    pub speed_word: u32,
    pub atlas_variant: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeParticleKind9 {
    pub resource: Vec<u8>,
    /// Flags, unsigned speed, draw order and particle count. Flags are retained
    /// for native capture;415E40/4160B0 do not use them to configure CBasic.
    pub parameter_words: [u32; 4],
    ///416060 queries432F00/432EF0 before resource loading, independently of
    ///the texture's dimensions. Private restore retains this birth extent.
    pub extent: [u32; 2],
    pub particles: Vec<CmvsNativeParticleKind9Particle>,
    /// Parent seeds the current41A000 session clock after construction.
    pub clock_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SavedNativeParticleKind9 {
    /// Native worker12..32: flags, width, height, speed, order, count.
    pub parameter_words: [u32; 6],
    pub resource_field: [u8; 32],
}

fn invalid(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_PARTICLE_KIND9", message)
}

fn random(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(214_013).wrapping_add(2_531_011);
    (*state >> 16) & 0x7fff
}

fn validate_configuration(
    resource: &[u8],
    parameters: [u32; 4],
    extent: [u32; 2],
) -> Result<(), CoreError> {
    if resource.len() >= 32 || resource.contains(&0) {
        return Err(invalid("native kind9 resource exceeds its fixed field"));
    }
    //415E40 skips every modulus for count0. For nonempty births the original
    //DIV instructions fault on these inputs; do not invent a divisor fallback.
    if parameters[3] != 0 && (extent.contains(&0) || (parameters[1] / 5) >> 4 == 0) {
        return Err(invalid("native kind9 birth has a zero random modulus"));
    }
    Ok(())
}

impl CmvsNativeParticleKind9 {
    ///448780 ignores416060's provider-load result and still runs all births.
    ///Each particle consumes four CRT draws in X,Y,variant,speed-jitter order.
    pub(crate) fn new(
        resource: Vec<u8>,
        parameter_words: [u32; 4],
        extent: [u32; 2],
        prng: &mut u32,
    ) -> Result<Self, CoreError> {
        validate_configuration(&resource, parameter_words, extent)?;
        let [_, speed, _, count] = parameter_words;
        let mut particles = Vec::new();
        particles
            .try_reserve_exact(count as usize)
            .map_err(|_| invalid("native kind9 particle allocation cannot be represented"))?;
        for _ in 0..count {
            let x = random(prng) % extent[0];
            let y = random(prng) % extent[1];
            let atlas_variant = (random(prng) % 3) as u8;
            let jitter = random(prng) % ((speed / 5) >> 4);
            let speed = speed.wrapping_add(jitter << 7);
            let speed_word = match atlas_variant {
                0 => speed.wrapping_mul(3) >> 1,
                1 => speed,
                _ => speed.wrapping_mul(2) / 3,
            };
            particles.push(CmvsNativeParticleKind9Particle {
                fixed_position_words: [x << 10, y << 10],
                speed_word,
                atlas_variant,
            });
        }
        Ok(Self {
            resource,
            parameter_words,
            extent,
            particles,
            clock_ms: 0,
        })
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        validate_configuration(&self.resource, self.parameter_words, self.extent)?;
        if self.particles.len() != self.parameter_words[3] as usize
            || self
                .particles
                .iter()
                .any(|particle| particle.atlas_variant >= 3)
        {
            return Err(invalid(
                "native kind9 particle count or atlas variant is inconsistent",
            ));
        }
        Ok(())
    }

    ///4160B0 uses wrapping IMUL, logical SHR4, then signed SAR10. Its JL at
    ///41611F compares against signed wrapping(height+32); reset discards all
    ///overshoot and consumes no random draws, including at delta0.
    pub(crate) fn advance(&mut self, delta_ms: u32, _prng: &mut u32) -> Result<(), CoreError> {
        self.validate()?;
        self.clock_ms = self.clock_ms.wrapping_add(delta_ms);
        let delta = delta_ms.min(500);
        let bottom = self.extent[1].wrapping_add(32) as i32;
        for particle in &mut self.particles {
            let step = delta.wrapping_mul(particle.speed_word) >> 4;
            particle.fixed_position_words[1] = particle.fixed_position_words[1].wrapping_add(step);
            if (particle.fixed_position_words[1] as i32 >> 10) >= bottom {
                particle.fixed_position_words[1] = (-16384_i32) as u32;
            }
        }
        Ok(())
    }

    pub(crate) fn particle_capacity(&self) -> usize {
        self.particles.len()
    }

    pub(crate) fn draw_order(&self, index: usize) -> Option<u32> {
        let particle = self.particles.get(index)?;
        Some(self.parameter_words[2].wrapping_add(2 * u32::from(particle.atlas_variant)))
    }

    pub(crate) fn draw_basic(&self, index: usize) -> Option<CmvsTextureParentState> {
        let particle = self.particles.get(index)?;
        let (x, size, pivot) = match particle.atlas_variant {
            0 => (0, 24, 12),
            1 => (24, 16, 8),
            2 => (48, 8, 4),
            _ => return None,
        };
        Some(CmvsTextureParentState {
            resource_frame: Some(0),
            surface_initialized: true,
            screen_shake_disabled: true,
            basic_kind: 0,
            blend_mode: Some(0),
            rect_words: Some([x, 0, size, size]),
            position_words: Some([pivot, pivot]),
            auxiliary_pair_words: Some(
                particle
                    .fixed_position_words
                    .map(|word| (word as i32 >> 10) as u32),
            ),
            auxiliary_word: self.draw_order(index),
            alpha: Some(255),
            scale_words: Some([1.0_f32.to_bits(); 2]),
            ..Default::default()
        })
    }

    pub(crate) fn capture(&self) -> Result<SavedNativeParticleKind9, CoreError> {
        self.validate()?;
        let [flags, speed, order, count] = self.parameter_words;
        let [width, height] = self.extent;
        let mut resource_field = [0; 32];
        resource_field[..self.resource.len()].copy_from_slice(&self.resource);
        Ok(SavedNativeParticleKind9 {
            parameter_words: [flags, width, height, speed, order, count],
            resource_field,
        })
    }
}

impl SavedNativeParticleKind9 {
    ///448FC0 kind9 copies56 bytes from453AC0's worker+12 after its header.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        if bytes.len() != 60 || bytes[..4] != [1, 0, 9, 0] {
            return Err(invalid("unsupported native kind9 configuration record"));
        }
        let saved = Self {
            parameter_words: std::array::from_fn(|index| {
                u32::from_le_bytes(bytes[4 + index * 4..8 + index * 4].try_into().unwrap())
            }),
            resource_field: bytes[28..60].try_into().unwrap(),
        };
        saved.validate()?;
        Ok(saved)
    }

    fn validate(&self) -> Result<(), CoreError> {
        if !self.resource_field.contains(&0) {
            return Err(invalid("native kind9 resource is not NUL terminated"));
        }
        Ok(())
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(60);
        bytes.extend_from_slice(&[1, 0, 9, 0]);
        for word in self.parameter_words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&self.resource_field);
        Ok(bytes)
    }

    ///449710 case8 reruns416060 using the current owner extent. It does NOT
    ///pass saved width/height to416010, so saved extent bytes are wire data,
    ///not authority for native reconstruction. All positions and RNG restart.
    pub(crate) fn rebuild(
        &self,
        extent: [u32; 2],
        prng: &mut u32,
    ) -> Result<CmvsNativeParticleKind9, CoreError> {
        self.validate()?;
        let length = self
            .resource_field
            .iter()
            .position(|byte| *byte == 0)
            .unwrap();
        let [flags, _, _, speed, order, count] = self.parameter_words;
        CmvsNativeParticleKind9::new(
            self.resource_field[..length].to_vec(),
            [flags, speed, order, count],
            extent,
            prng,
        )
    }
}

#[cfg(test)]
mod tests;
