//! Ordinary675: native 448AC0/454B90 CBasic planes, advanced by 454EC0.
//! Native2048 saves configuration; the private VM snapshot retains live phase.
use super::*;

const MAX_PARTICLES: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeParticleKind3Particle {
    /// Unquantized SSE float storage at native particle offsets12/16/20.
    pub position_words: [u32; 3],
    pub fall_time_word: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeParticleKind3 {
    /// CP932 resource name without the native NUL terminator.
    pub resource: Vec<u8>,
    /// Flags, base Z, fall time, count; only flags&3 affects this worker.
    pub parameter_words: [u32; 4],
    pub particles: Vec<CmvsNativeParticleKind3Particle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SavedNativeParticleKind3 {
    pub parameter_words: [u32; 4],
    /// Native serialization preserves bytes after the first NUL as well.
    pub resource_field: [u8; 32],
}

fn invalid(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_PARTICLE_KIND3", message)
}

fn random(state: &mut u32) -> f32 {
    *state = state.wrapping_mul(214_013).wrapping_add(2_531_011);
    ((*state >> 16) & 0x7fff) as f32 / 32767.0
}

fn validate_parameters(parameters: &[u32; 4]) -> Result<(), CoreError> {
    // Native accepts every float bit pattern, including zero/negative time.
    // The only port-side bound is the shared4473C0 draw-record capacity.
    if parameters[3] as usize > MAX_PARTICLES {
        return Err(invalid(
            "native kind3 count exceeds the shared draw-record capacity",
        ));
    }
    Ok(())
}

impl CmvsNativeParticleKind3 {
    /// 454B90 consumes four CRT draws per particle, in X/Z/Y/time order.
    /// 4553E0's resource-load result is ignored: even a missing provider has
    /// the same particles and RNG consumption as a successfully loaded one.
    pub(crate) fn new(
        resource: Vec<u8>,
        parameter_words: [u32; 4],
        prng: &mut u32,
    ) -> Result<Self, CoreError> {
        validate_parameters(&parameter_words)?;
        if resource.len() >= 32 || resource.contains(&0) {
            return Err(invalid("native kind3 resource exceeds its fixed field"));
        }
        let base_z = f32::from_bits(parameter_words[1]);
        let fall_time = f32::from_bits(parameter_words[2]);
        let mut particles = Vec::with_capacity(parameter_words[3] as usize);
        for _ in 0..parameter_words[3] {
            let x = random(prng) * 10.0 - 5.0;
            let z = base_z + random(prng) * 10.0;
            let y = random(prng) * 10.0;
            let jitter = fall_time / 5.0;
            let time = fall_time + (random(prng) * jitter - jitter * 0.5);
            particles.push(CmvsNativeParticleKind3Particle {
                position_words: [x, y, z].map(f32::to_bits),
                fall_time_word: time.to_bits(),
            });
        }
        Ok(Self {
            resource,
            parameter_words,
            particles,
        })
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        validate_parameters(&self.parameter_words)?;
        if self.resource.len() >= 32
            || self.resource.contains(&0)
            || self.particles.len() != self.parameter_words[3] as usize
        {
            return Err(invalid(
                "native kind3 resource or particle count is inconsistent",
            ));
        }
        Ok(())
    }

    /// 454EC0 uses unsigned elapsed milliseconds capped at500. Wrapping is
    /// one strict-negative add, not modulo and not a respawn. X/Z, alpha,
    /// rotation, sourceframe and the CRT state never change here.
    pub(crate) fn advance(&mut self, delta_ms: u32) -> Result<(), CoreError> {
        self.validate()?;
        let distance = delta_ms.min(500) as f32 * 10.0;
        for particle in &mut self.particles {
            let mut y = f32::from_bits(particle.position_words[1]);
            y -= distance / f32::from_bits(particle.fall_time_word);
            if y < 0.0 {
                y += 10.0;
            }
            particle.position_words[1] = y.to_bits();
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

    /// Publish the CBasic created by446900/446A30, borrowing the resource
    /// separately. No allocation or resource-name copying on the frame path.
    pub(crate) fn draw_basic(&self, index: usize) -> Option<CmvsTextureParentState> {
        Some(CmvsTextureParentState {
            resource_frame: Some(0),
            surface_initialized: true,
            basic_kind: 2,
            blend_mode: Some(1),
            rect_words: Some([0, 0, 32, 32]),
            position_words: Some([16, 16]),
            float_position_words: self.plane_position(index)?,
            projection_depth_word: (3.0_f32 + (self.parameter_words[0] & 3) as f32).to_bits(),
            projection_height_word: 3.0_f32.to_bits(),
            alpha: Some(255),
            scale_words: Some([1.0_f32.to_bits(); 2]),
            ..Default::default()
        })
    }

    pub(crate) fn capture(&self) -> Result<SavedNativeParticleKind3, CoreError> {
        self.validate()?;
        let mut resource_field = [0; 32];
        resource_field[..self.resource.len()].copy_from_slice(&self.resource);
        Ok(SavedNativeParticleKind3 {
            parameter_words: self.parameter_words,
            resource_field,
        })
    }
}

impl SavedNativeParticleKind3 {
    /// 448FC0: version1/kind3 WORDs, four words, then32 resource bytes.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        if bytes.len() != 52 || bytes[..4] != [1, 0, 3, 0] {
            return Err(invalid("unsupported native kind3 configuration record"));
        }
        let parameter_words = std::array::from_fn(|index| {
            u32::from_le_bytes(bytes[4 + index * 4..8 + index * 4].try_into().unwrap())
        });
        validate_parameters(&parameter_words)?;
        let resource_field: [u8; 32] = bytes[20..52].try_into().unwrap();
        if !resource_field.contains(&0) {
            return Err(invalid("native kind3 resource is not NUL terminated"));
        }
        Ok(Self {
            parameter_words,
            resource_field,
        })
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        validate_parameters(&self.parameter_words)?;
        if !self.resource_field.contains(&0) {
            return Err(invalid("native kind3 resource is not NUL terminated"));
        }
        let mut bytes = Vec::with_capacity(52);
        bytes.extend_from_slice(&[1, 0, 3, 0]);
        for word in self.parameter_words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&self.resource_field);
        Ok(bytes)
    }

    /// 449710 reconstructs all particles using the shared current CRT state;
    /// it does not restore the pre-save particle positions or clock phase.
    pub(crate) fn rebuild(&self, prng: &mut u32) -> Result<CmvsNativeParticleKind3, CoreError> {
        let length = self
            .resource_field
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| invalid("native kind3 resource is not NUL terminated"))?;
        CmvsNativeParticleKind3::new(
            self.resource_field[..length].to_vec(),
            self.parameter_words,
            prng,
        )
    }
}

#[cfg(test)]
mod tests;
