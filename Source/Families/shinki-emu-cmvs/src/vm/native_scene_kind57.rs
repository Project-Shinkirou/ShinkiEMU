//! Ordinary663 / 4486C0: eight fixed quads and six rings of 32 atlas quads.
//! 4520A0 caps delta at 200ms and quantizes descent before accumulating it.
//! Request10 replaces providers2/3 and reveals five quads without a state change.
use super::native_scene_frame::NativeSceneFrame;
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeSceneKind57 {
    meshes: Vec<CmvsNativeMesh>,
    rotation_clock: u32,
    orbit_clock: u32,
    height_words: [u32; 6],
    alternate_resources: bool,
    resource_reload_generation: u32,
}

fn words<const N: usize>(values: [f32; N]) -> [u32; N] {
    values.map(f32::to_bits)
}
fn random(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(214_013).wrapping_add(2_531_011);
    (*state >> 16) & 0x7fff
}
fn quad(positions: [[f32; 3]; 4], uv: [[f32; 2]; 4], position: [f32; 3]) -> CmvsNativeMesh {
    CmvsNativeMesh {
        kind: 3,
        alpha: 255,
        blend: 0,
        position_words: words(position),
        angle_words: [0; 3],
        scale_words: words([1.0; 3]),
        billboard: false,
        force_blend: false,
        late_pass: false,
        vertices: std::array::from_fn(|i| CmvsNativeMeshVertex {
            position_words: words(positions[i]),
            rgba_words: words([1.0; 4]),
            uv_words: words(uv[i]),
        }),
    }
}
fn rectangle(left: f32, right: f32, top: f32, bottom: f32) -> [[f32; 3]; 4] {
    [
        [left, top, 0.0],
        [right, top, 0.0],
        [left, bottom, -0.0],
        [right, bottom, -0.0],
    ]
}

/// 451EE3 / 4522B3 use two single-precision operations, then double sin/cos.
/// The radius product is rounded to f32 before truncation to hundredths.
fn orbit_position(degrees: f32, height: u32) -> [u32; 3] {
    let radians = (degrees / 180.0_f32) * f32::from_bits(0x4049_0fdb);
    let x = ((radians as f64).cos() * 1.5_f64) as f32;
    let z = ((radians as f64).sin() * 1.5_f64) as f32;
    [
        (((x * 100.0_f32) as i32 as f32 / 100.0_f32) - 0.17_f32).to_bits(),
        height,
        (((z * 100.0_f32) as i32 as f32 / 100.0_f32) + 1.2_f32).to_bits(),
    ]
}

impl CmvsNativeSceneKind57 {
    pub const RESOURCES: [&'static [u8]; 9] = [
        b"__ev-xx004a.pb3",
        b"__ev-xx004g.pb3",
        b"__ev-xx004f.pb3",
        b"__ev-xx004b1.pb3",
        b"it011d1.pb3",
        b"it011d2.pb3",
        b"it011d3.pb3",
        b"it011d4.pb3",
        b"it011c.pb3",
    ];

    /// The wrapper's constructor context pointers have no downstream consumer.
    /// The clock is not sampled during construction.
    pub(crate) fn new(prng: &mut u32) -> Result<Self, CoreError> {
        let uv = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        let mut meshes = Vec::with_capacity(200);
        for z in [32.0, f32::from_bits(0x41ff_3333)] {
            meshes.push(quad(rectangle(-15.0, 15.0, 9.0, -9.0), uv, [0.0, 0.0, z]));
        }
        meshes[1].force_blend = true;
        meshes[1].set_alpha(0);
        meshes.push(quad(
            rectangle(-0.5, 0.5, 0.5, -0.5),
            uv,
            [-0.17, 0.17, 1.2],
        ));
        meshes.push(quad(rectangle(-2.3, 1.7, 3.8, -0.2), uv, [2.6, -2.5, 10.0]));
        for _ in 0..4 {
            let mut mesh = quad(
                [
                    [-12.0, 0.0, 12.0],
                    [12.0, 0.0, 12.0],
                    [-12.0, 0.0, -12.0],
                    [12.0, 0.0, -12.0],
                ],
                uv,
                [2.6, -2.5, 10.1],
            );
            mesh.set_alpha(0);
            meshes.push(mesh);
        }
        for mesh in &mut meshes[2..] {
            mesh.force_blend = true;
            mesh.late_pass = true;
        }
        let mut rng = *prng;
        let mut height = -0.6_f32;
        let mut height_words = [0; 6];
        for row in &mut height_words {
            *row = height.to_bits();
            for column in 0..32 {
                let atlas = random(&mut rng) % 8;
                let u = (atlas & 3) as f32 * 0.25_f32;
                let v = (atlas >> 2) as f32 * 0.25_f32;
                let mut mesh = quad(
                    rectangle(-0.1, 0.1, 0.1, -0.1),
                    [[u, v], [u + 0.25, v], [u, v + 0.25], [u + 0.25, v + 0.25]],
                    [0.0; 3],
                );
                // Native 451E90 leaves the bottom atlas V in xmm1. 451EE3
                // converts that value, NOT the independently computed mesh yaw.
                mesh.position_words = orbit_position(v + 0.25_f32, *row);
                mesh.angle_words[1] = (((450 - column * 11) % 360) as f32).to_bits();
                mesh.force_blend = true;
                mesh.late_pass = true;
                meshes.push(mesh);
            }
            height += 0.3_f32;
        }
        let owner = Self {
            meshes,
            rotation_clock: 0,
            orbit_clock: 0,
            height_words,
            alternate_resources: false,
            resource_reload_generation: 0,
        };
        owner.validate()?;
        *prng = rng;
        Ok(owner)
    }

    pub(crate) fn initial_frame(&self) -> NativeSceneFrame {
        NativeSceneFrame {
            position_words: Some([0; 3]),
            pitch_word: Some(0),
            yaw_word: Some(0),
            ..NativeSceneFrame::default()
        }
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if self.meshes.len() != 200
            || self
                .height_words
                .iter()
                .any(|w| !f32::from_bits(*w).is_finite())
        {
            return Err(CoreError::invalid(
                "SHINKI_EMU_CMVS_NATIVE_SCENE_KIND57",
                "invalid native mesh count or ring height",
            ));
        }
        for mesh in &self.meshes {
            mesh.validate()?;
        }
        Ok(())
    }

    /// 451030 reads only selector; both additional request words are ignored.
    /// Reloads are immediate, including repeated selector10, and never pend.
    pub(crate) fn request(&mut self, selector: u32) {
        if selector != 10 {
            return;
        }
        self.alternate_resources = true;
        self.resource_reload_generation = self.resource_reload_generation.wrapping_add(1);
        for index in [4, 5, 6, 7, 1] {
            self.meshes[index].set_alpha(255);
        }
    }

    pub(crate) fn status(&self) -> u32 {
        0
    }

    pub(crate) fn advance(
        &mut self,
        delta_ms: u32,
        _volume_word: u32,
    ) -> Result<NativeSceneFrame, CoreError> {
        self.validate()?;
        let delta = delta_ms.min(200);
        self.rotation_clock = self.rotation_clock.wrapping_add(delta);
        let yaw = (self.rotation_clock.wrapping_mul(3600) / 30000 % 3600) as f32 / 10.0_f32;
        for index in 4..8 {
            self.meshes[index].angle_words[1] =
                if index & 1 == 0 { yaw } else { 360.0_f32 - yaw }.to_bits();
        }
        self.orbit_clock = self.orbit_clock.wrapping_add(delta);
        let descent = (delta.wrapping_mul(2000) / 3000) as f32 / 1000.0_f32;
        let phase = self.orbit_clock.wrapping_mul(36000) / 4000 % 36000;
        for row in 0..6 {
            let mut height = f32::from_bits(self.height_words[row]) - descent;
            if height <= -0.9_f32 {
                height += 1.8_f32;
            }
            self.height_words[row] = height.to_bits();
            for column in 0..32 {
                let mut angle = (column as u32 * 1125 + phase) % 36000;
                if row & 1 == 0 {
                    angle = (36000 - angle) % 36000;
                }
                let mesh = &mut self.meshes[8 + row * 32 + column];
                mesh.position_words = orbit_position(angle as f32 / 100.0_f32, height.to_bits());
                mesh.angle_words[1] = (((45000 - angle) % 36000 / 100) as f32).to_bits();
            }
        }
        Ok(NativeSceneFrame::default())
    }

    pub(crate) fn mesh_count(&self) -> usize {
        self.meshes.len()
    }
    pub(crate) fn mesh(&self, index: usize) -> Option<&CmvsNativeMesh> {
        self.meshes.get(index)
    }
    pub(crate) fn mesh_resource_index(&self, index: usize) -> usize {
        index.min(8)
    }
    pub(crate) fn resource_count(&self) -> usize {
        Self::RESOURCES.len()
    }
    pub(crate) fn resource(&self, index: usize) -> Option<&[u8]> {
        match (self.alternate_resources, index) {
            (true, 2) => Some(b"__ev-xx004e.pb3"),
            (true, 3) => Some(b"__ev-xx004b2.pb3"),
            _ => Self::RESOURCES.get(index).copied(),
        }
    }
    /// Consumers caching providers can observe repeated native reloads, even
    /// when the resource names were already changed by an earlier request10.
    pub(crate) fn resource_reload_generation(&self) -> u32 {
        self.resource_reload_generation
    }
}

#[cfg(test)]
mod tests;
