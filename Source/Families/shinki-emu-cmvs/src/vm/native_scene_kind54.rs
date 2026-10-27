//! Ordinary660 / 448550: five growing rings and 512 rising additive billboards.
//! 44FD60 caps the common delta at 200ms. Requests replace state without resetting
//! any clock; requesting the current state leaves the native pending gate set.
use super::native_scene_frame::{NativeSceneFrame, NativeSceneSound};
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct Particle {
    mesh: CmvsNativeMesh,
    rise_time_word: u32,
    delay: u32,
    pulse_clock: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeSceneKind54 {
    rings: [CmvsNativeMesh; 5],
    particles: Vec<Particle>,
    rotation_clock: u32,
    growth_clock: u32,
    fade_clock: u32,
    state: u32,
    pending: u32,
    sound_started: bool,
    alternate_resource: bool,
    resource_reload_generation: u32,
}

fn words<const N: usize>(values: [f32; N]) -> [u32; N] {
    values.map(f32::to_bits)
}
fn random(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(214_013).wrapping_add(2_531_011);
    (*state >> 16) & 0x7fff
}
fn unit(state: &mut u32) -> f32 {
    random(state) as f32 / 32767.0_f32
}

fn ring(height: f32) -> CmvsNativeMesh {
    let one = 1.0_f32.to_bits();
    let positions = [
        [-1.0, height, 1.0],
        [1.0, height, 1.0],
        [-1.0, height, -1.0],
        [1.0, height, -1.0],
    ];
    let uv = [[0, 0], [one, 0], [0, one], [one, one]];
    let mut mesh = CmvsNativeMesh {
        kind: 3,
        alpha: 255,
        blend: 0,
        position_words: [0xbe23_d70a, 0x3f78_51ec, 0x4000_0000],
        angle_words: words([-90.0, 0.0, 0.0]),
        scale_words: [one; 3],
        billboard: false,
        force_blend: true,
        late_pass: true,
        vertices: std::array::from_fn(|index| CmvsNativeMeshVertex {
            position_words: words(positions[index]),
            rgba_words: [one; 4],
            uv_words: uv[index],
        }),
    };
    mesh.set_alpha(0);
    mesh
}

impl CmvsNativeSceneKind54 {
    pub const RESOURCES: [&'static [u8]; 6] = [
        b"try008a.pb3",
        b"try008b.pb3",
        b"try008c.pb3",
        b"try008d.pb3",
        b"try008e.pb3",
        b"try008z.pb3",
    ];
    pub const ALTERNATE_RESOURCE: &'static [u8] = b"try008z2.pb3";
    pub const SOUND_RESOURCES: [(u8, &'static [u8]); 1] = [(16, b"st03_magic1b.wav")];

    /// 44F010 consumes exactly five MSVCRT draws per particle: X, Z, Y, rise
    /// period, delay. No random numbers are consumed by per-frame updates.
    pub(crate) fn new(prng: &mut u32) -> Result<Self, CoreError> {
        let rings = [0.0, 0.1, 0.3, 0.4, 0.5].map(ring);
        let mut rng = *prng;
        let mut particles = Vec::with_capacity(512);
        for _ in 0..512 {
            let x = unit(&mut rng) * 4.0_f32 - 2.0_f32;
            let z_draw = unit(&mut rng);
            let z = (z_draw + z_draw) + 1.0_f32;
            let y = unit(&mut rng) * 10.0_f32 - 5.0_f32;
            let rise_time = (unit(&mut rng) * 2000.0_f32 - 1000.0_f32) + 8000.0_f32;
            let delay = random(&mut rng) % 1000;
            let mut mesh = CmvsNativeMesh::billboard(0.1, 0.1, [0.05, 0.05])?;
            mesh.position_words = words([x, y, z]);
            mesh.force_blend = true;
            mesh.late_pass = true;
            mesh.blend = 1;
            mesh.set_alpha(0);
            particles.push(Particle {
                mesh,
                rise_time_word: rise_time.to_bits(),
                delay,
                pulse_clock: 0,
            });
        }
        let owner = Self {
            rings,
            particles,
            rotation_clock: 0,
            growth_clock: 0,
            fade_clock: 0,
            state: 0,
            pending: u32::MAX,
            sound_started: false,
            resource_reload_generation: 0,
            alternate_resource: false,
        };
        owner.validate()?;
        *prng = rng;
        Ok(owner)
    }

    /// 44F360 writes position and the two view angles, retaining FOV and roll.
    pub(crate) fn initial_frame(&self) -> NativeSceneFrame {
        NativeSceneFrame {
            position_words: Some(words([0.0, 1.0, 0.0])),
            pitch_word: Some(0),
            yaw_word: Some(0),
            ..NativeSceneFrame::default()
        }
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if self.particles.len() != 512 {
            return Err(CoreError::invalid(
                "SHINKI_EMU_CMVS_NATIVE_SCENE_KIND54",
                "native particle count is not 512",
            ));
        }
        for mesh in &self.rings {
            mesh.validate()?;
        }
        for particle in &self.particles {
            particle.mesh.validate()?;
            let period = f32::from_bits(particle.rise_time_word);
            if !period.is_finite() || period <= 0.0 || period < f32::MIN_POSITIVE {
                return Err(CoreError::invalid(
                    "SHINKI_EMU_CMVS_NATIVE_SCENE_KIND54",
                    "native particle rise period is invalid",
                ));
            }
        }
        Ok(())
    }

    /// 44F220 ignores both additional request operands. Selector50 reloads the
    /// existing particle provider immediately; it does not touch the state gate.
    pub(crate) fn request(&mut self, selector: u32) {
        match selector {
            10 | 11 | 20 | 22 => self.pending = selector,
            50 => {
                self.alternate_resource = true;
                self.resource_reload_generation = self.resource_reload_generation.wrapping_add(1);
            }
            _ => {}
        }
    }

    pub(crate) fn status(&self) -> u32 {
        if self.pending == u32::MAX {
            self.state
        } else {
            u32::MAX
        }
    }

    /// 44FD60 uses a strict start boundary and all-axis scale. Completion of
    /// the automatic opening leaves state0; only explicitly requested11→12.
    fn grow(&mut self, elapsed: u32, volume: u32, frame: &mut NativeSceneFrame) -> usize {
        let mut complete = 0;
        for index in 0..5 {
            let start = 800 - index as u32 * 200;
            let duration = 500 + index as u32 * 50;
            if elapsed <= start {
                continue;
            }
            if !self.sound_started {
                frame.audio[0] = Some(NativeSceneSound::PlayStaticMain {
                    slot: 16,
                    volume,
                    looping: false,
                });
                self.sound_started = true;
            }
            let progress = elapsed.wrapping_sub(start);
            let mesh = &mut self.rings[index];
            if progress >= duration {
                mesh.scale_words = [1.0_f32.to_bits(); 3];
                mesh.set_alpha(256);
                complete += 1;
            } else {
                mesh.scale_words = [(progress as f32 / duration as f32).to_bits(); 3];
                mesh.set_alpha((progress.wrapping_shl(8) / duration) as i32);
            }
        }
        complete
    }

    pub(crate) fn advance(
        &mut self,
        delta_ms: u32,
        volume_word: u32,
    ) -> Result<NativeSceneFrame, CoreError> {
        self.validate()?;
        let delta = delta_ms.min(200);
        self.rotation_clock = self.rotation_clock.wrapping_add(delta);
        let yaw = (self.rotation_clock.wrapping_mul(3600) / 6000 % 3600) as f32 / 10.0_f32;
        for index in 0..5 {
            self.rings[index].angle_words[1] = if matches!(index, 0 | 2) {
                yaw
            } else {
                360.0_f32 - yaw
            }
            .to_bits();
        }
        if self.pending != u32::MAX && self.pending != self.state {
            self.state = self.pending;
            self.pending = u32::MAX;
        }
        let mut frame = NativeSceneFrame::default();
        if self.state < 10 {
            self.growth_clock = self.growth_clock.wrapping_add(delta);
            if self.growth_clock > 1500 {
                self.grow(self.growth_clock - 1500, volume_word, &mut frame);
            }
        } else if self.state == 11 {
            self.growth_clock = self.growth_clock.wrapping_add(delta);
            if self.grow(self.growth_clock, volume_word, &mut frame) == 5 {
                self.state = 12;
            }
        }
        if matches!(self.state, 20 | 22) {
            self.fade_clock = self.fade_clock.wrapping_add(delta);
        }
        for mesh in &mut self.rings {
            if self.state == 20 {
                if self.fade_clock >= 1000 {
                    self.state = 21;
                    mesh.set_alpha(0);
                } else {
                    mesh.set_alpha((256 - self.fade_clock.wrapping_shl(8) / 1000) as i32);
                }
            } else if self.state == 21 {
                mesh.set_alpha(0);
            }
        }
        let rise = delta as f32 * 10.0_f32;
        for particle in &mut self.particles {
            let mut y = rise / f32::from_bits(particle.rise_time_word)
                + f32::from_bits(particle.mesh.position_words[1]);
            if y >= 5.0_f32 {
                y -= 10.0_f32;
            }
            particle.mesh.position_words[1] = y.to_bits();
            if particle.delay != 0 {
                particle.delay = particle.delay.saturating_sub(delta);
            } else {
                particle.pulse_clock = particle.pulse_clock.wrapping_add(delta);
            }
            let clock = particle.pulse_clock;
            // The native zero-clock branch skips alpha entirely, including fades.
            if clock == 0 {
                continue;
            }
            let mut alpha = if clock < 500 {
                clock.wrapping_shl(7) / 500
            } else {
                let phase = (clock - 500) % 1000;
                if phase >= 500 {
                    256 - (phase.wrapping_shl(7) - 64000) / 500
                } else {
                    phase.wrapping_shl(7) / 500 + 128
                }
            };
            match self.state {
                20 => {
                    if self.fade_clock >= 1000 {
                        alpha = 0;
                        self.state = 21;
                    } else {
                        alpha -= alpha.wrapping_mul(self.fade_clock) / 1000;
                    }
                }
                21 | 23 => alpha = 0,
                22 => {
                    if self.fade_clock >= 1000 {
                        alpha = 0;
                        self.state = 23;
                    } else {
                        alpha -= alpha.wrapping_mul(self.fade_clock) / 1000;
                    }
                }
                _ => {}
            }
            particle.mesh.set_alpha(alpha as i32);
        }
        Ok(frame)
    }

    pub(crate) fn mesh_count(&self) -> usize {
        5 + self.particles.len()
    }
    pub(crate) fn mesh(&self, index: usize) -> Option<&CmvsNativeMesh> {
        if index < 5 {
            self.rings.get(index)
        } else {
            self.particles.get(index - 5).map(|p| &p.mesh)
        }
    }
    pub(crate) fn mesh_resource_index(&self, index: usize) -> usize {
        index.min(5)
    }
    pub(crate) fn resource_count(&self) -> usize {
        Self::RESOURCES.len()
    }
    pub(crate) fn resource(&self, index: usize) -> Option<&[u8]> {
        if index == 5 && self.alternate_resource {
            Some(Self::ALTERNATE_RESOURCE)
        } else {
            Self::RESOURCES.get(index).copied()
        }
    }
    pub(crate) fn resource_reload_generation(&self) -> u32 {
        self.resource_reload_generation
    }
}

#[cfg(test)]
mod tests;
