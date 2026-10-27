//! Actual680:448270/415940 append flat shards to the transferred CImage.
//! The owner retains that image/provider and its old children; this model owns
//! only the new children's geometry and415C60's private continuation phase.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeImageShardParticle {
    pub delay_ms: u32,
    /// Native particle12/16; integer destinations wrap when motion is added.
    pub base_position_words: [u32; 2],
    pub speed_word: u32,
    /// Native particle28, accumulated once per eligible415C60 call.
    pub drift_word: u32,
    pub drift_per_call_word: u32,
    /// Live CBasic8/9, retained when the wrapping elapsed clock waits again.
    pub destination_words: [u32; 2],
    pub rotation: u16,
    pub scale_word: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeImageShardGrid {
    /// Actual source-provider dimensions, also written into root/child1038/1039.
    pub extent: [u32; 2],
    /// Tile side and an unused native word; grid counts always divide by20.
    pub parameter_words: [u32; 2],
    pub particles: Vec<CmvsNativeImageShardParticle>,
    /// Parent seeds41A000's absolute session clock after construction.
    pub clock_ms: u32,
    /// Wrapping now-minus-birth, independent of the parent's absolute seed.
    elapsed_ms: u32,
    finished: bool,
}

fn invalid(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_IMAGE_SHARDS", message)
}

fn random(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(214_013).wrapping_add(2_531_011);
    (*state >> 16) & 0x7fff
}

fn grid_dimensions(extent: [u32; 2]) -> Result<([u32; 2], usize), CoreError> {
    let dimensions = extent.map(|side| side / 20 + u32::from(side % 20 != 0));
    let count = u64::from(dimensions[0]) * u64::from(dimensions[1]);
    //4159DA..4159E0 allocate count*36 using32-bit arithmetic. Reject its
    //undersized-allocation overflow, not tile sizes or a renderer queue limit.
    if count > u64::from(u32::MAX / 36) {
        return Err(invalid("native shard grid overflows its32-bit allocation"));
    }
    let count = usize::try_from(count)
        .map_err(|_| invalid("native shard grid count cannot be represented"))?;
    Ok((dimensions, count))
}

/// SSE CVTTSS2SI returns integer-indefinite for NaN or out-of-range input;
/// Rust's saturating float cast has different positive-overflow behavior.
fn cvttss2si(value: f32) -> i32 {
    if !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        i32::MIN
    } else {
        value as i32
    }
}

impl CmvsNativeImageShardGrid {
    pub fn new(
        extent: [u32; 2],
        parameter_words: [u32; 2],
        prng: &mut u32,
    ) -> Result<Self, CoreError> {
        let ([columns, rows], count) = grid_dimensions(extent)?;
        let mut particles = Vec::new();
        particles
            .try_reserve_exact(count)
            .map_err(|_| invalid("native shard particle allocation cannot be represented"))?;
        let tile = parameter_words[0];
        let pivot = tile >> 1;
        //A zero column count consumes no random numbers, even with a nonzero
        //row extent. Tile0 still creates the complete finite20-based grid.
        if count != 0 {
            for row in 0..rows {
                for column in 0..columns {
                    let base = [
                        column.wrapping_mul(tile).wrapping_add(pivot),
                        row.wrapping_mul(tile).wrapping_add(pivot),
                    ];
                    //415AF8's heading is never read by415C60. Its CRT draw is
                    //observable by every subsequent birth and must survive.
                    let _ = random(prng);
                    let delay_ms = (random(prng) % 60)
                        .wrapping_add(70_u32.wrapping_mul(rows.wrapping_sub(row)));
                    let speed = ((random(prng) % 100) as f32 / 100.0 - 0.5) + 4.0;
                    let drift_per_call = (random(prng) % 100) as f32 / 50.0 - 1.0;
                    particles.push(CmvsNativeImageShardParticle {
                        delay_ms,
                        base_position_words: base,
                        speed_word: speed.to_bits(),
                        drift_word: 0,
                        drift_per_call_word: drift_per_call.to_bits(),
                        destination_words: base,
                        rotation: 0,
                        scale_word: 1.0_f32.to_bits(),
                    });
                }
            }
        }
        Ok(Self {
            extent,
            parameter_words,
            particles,
            clock_ms: 0,
            elapsed_ms: 0,
            finished: false,
        })
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        let (_, count) = grid_dimensions(self.extent)?;
        if self.particles.len() != count {
            return Err(invalid(
                "native shard particle count does not match the source grid",
            ));
        }
        for particle in &self.particles {
            let speed = f32::from_bits(particle.speed_word);
            let drift = f32::from_bits(particle.drift_word);
            let drift_per_call = f32::from_bits(particle.drift_per_call_word);
            let scale = f32::from_bits(particle.scale_word);
            if !(3.5..=4.49).contains(&speed)
                || !drift.is_finite()
                || !(-1.0..=0.98).contains(&drift_per_call)
                || !(0.0..=1.0).contains(&scale)
                || particle.rotation >= 360
            {
                return Err(invalid(
                    "native shard continuation has invalid floating or rotation state",
                ));
            }
        }
        Ok(())
    }

    ///415C60 ignores its delta argument: all vertical motion, rotation and scale
    ///use full wrapping(now-birth)-delay. Drift alone advances once per call,
    ///including at delta0 and after the completion latch has already been set.
    pub fn advance(&mut self, delta_ms: u32) -> Result<(), CoreError> {
        self.validate()?;
        self.clock_ms = self.clock_ms.wrapping_add(delta_ms);
        self.elapsed_ms = self.elapsed_ms.wrapping_add(delta_ms);
        let mut any_remaining = false;
        let bottom = self.extent[1].wrapping_add(32) as i32;
        for particle in &mut self.particles {
            //415CA9 uses unsigned JB; equality performs the first drift step.
            if self.elapsed_ms < particle.delay_ms {
                any_remaining = true;
                continue;
            }
            let elapsed = self.elapsed_ms.wrapping_sub(particle.delay_ms);
            let drift =
                f32::from_bits(particle.drift_word) + f32::from_bits(particle.drift_per_call_word);
            particle.drift_word = drift.to_bits();
            let x = particle.base_position_words[0].wrapping_add(cvttss2si(drift) as u32);
            let y = particle.base_position_words[1].wrapping_add(cvttss2si(
                elapsed as f32 / f32::from_bits(particle.speed_word),
            ) as u32);
            particle.destination_words = [x, y];
            //415D02's CMOVLE compares signed wrapping destinations and bottom.
            any_remaining |= (y as i32) <= bottom;
            //IMUL wraps BEFORE the two unsigned divisions at415D19/415D27.
            particle.rotation = ((elapsed.wrapping_mul(360) / 10_000) % 360) as u16;
            let scale = 1.0 - (elapsed as f32 / 7500.0).clamp(0.0, 1.0);
            particle.scale_word = scale.to_bits();
        }
        if !any_remaining {
            self.finished = true;
        }
        Ok(())
    }

    pub fn particle_capacity(&self) -> usize {
        self.particles.len()
    }

    ///The parent's transferred image supplies the provider; these CImage
    ///children have no resource of their own and explicitly clear1036 flags.
    pub fn draw_basic(&self, index: usize) -> Option<CmvsTextureChildState> {
        let particle = self.particles.get(index)?;
        let tile = self.parameter_words[0];
        let pivot = tile >> 1;
        Some(CmvsTextureChildState {
            surface_initialized: true,
            parent_context_detached: true,
            screen_shake_disabled: true,
            alpha: Some(255),
            scale_words: Some([particle.scale_word; 2]),
            basic_kind: 0,
            blend_mode: Some(1),
            rotation_hundredths: i32::from(particle.rotation) * 100,
            reference_size: Some(self.extent),
            rect_words: Some([
                particle.base_position_words[0].wrapping_sub(pivot),
                particle.base_position_words[1].wrapping_sub(pivot),
                tile,
                tile,
            ]),
            position_words: Some([pivot; 2]),
            auxiliary_pair_words: Some(particle.destination_words),
            auxiliary_word: Some(16),
            ..Default::default()
        })
    }

    pub fn finished(&self) -> bool {
        self.finished
    }

    ///415C50 returns0 while pending and-1 after completion.
    pub fn status(&self) -> i32 {
        -i32::from(self.finished)
    }
}

#[cfg(test)]
mod tests;
