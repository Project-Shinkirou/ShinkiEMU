//! Ordinary656: two provider roots, three recycled XZ strips and a camera-following floor.
//! 452890 binds both providers; 452900 advances by camera Z, not by elapsed time.
//! Native2048 writes only its kind marker. Private snapshots retain the live phase.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeSceneKind50 {
    /// Root0's inherited strip provider, then root1's floor provider (CP932).
    pub resources: [Vec<u8>; 2],
    meshes: [CmvsNativeMesh; 4],
    /// Native+20 indexes the three fixed child IDs; recycling never reorders them.
    next_plane_index: u32,
    /// Native+24, accumulated with ADDSS independently of the camera position.
    phase_z_word: u32,
}

fn invalid(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_SCENE_KIND50", message)
}

fn quad(half_extent_word: u32, y_word: u32, z_word: u32, force_blend: bool) -> CmvsNativeMesh {
    let negative_extent = half_extent_word | 0x8000_0000;
    let positions = [
        [negative_extent, y_word, half_extent_word],
        [half_extent_word, y_word, half_extent_word],
        [negative_extent, y_word, negative_extent],
        [half_extent_word, y_word, negative_extent],
    ];
    let one = 1.0_f32.to_bits();
    let uv = [[0, 0], [one, 0], [0, one], [one, one]];
    CmvsNativeMesh {
        kind: 3,
        alpha: 255,
        blend: 0,
        position_words: [0, 0, z_word],
        angle_words: [0; 3],
        scale_words: [one; 3],
        billboard: false,
        force_blend,
        late_pass: false,
        vertices: std::array::from_fn(|index| CmvsNativeMeshVertex {
            position_words: positions[index],
            rgba_words: [one; 4],
            uv_words: uv[index],
        }),
    }
}

impl CmvsNativeSceneKind50 {
    /// 4524F0 creates children at Z=0/10/20, then a separate root floor.
    /// Its child Y immediate is BDCCCCCD (-0.1), not -0.05.
    pub(crate) fn new(resources: [Vec<u8>; 2]) -> Result<Self, CoreError> {
        let owner = Self {
            resources,
            meshes: std::array::from_fn(|index| match index {
                0 => quad(0x40a0_0000, 0xbdcc_cccd, 0, false),
                1 => quad(0x40a0_0000, 0xbdcc_cccd, 0x4120_0000, false),
                2 => quad(0x40a0_0000, 0xbdcc_cccd, 0x41a0_0000, false),
                _ => quad(0x4170_0000, 0, 0, true),
            }),
            next_plane_index: 0,
            phase_z_word: 0,
        };
        owner.validate()?;
        Ok(owner)
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if self
            .resources
            .iter()
            .any(|name| name.len() >= 32 || name.contains(&0))
        {
            return Err(invalid("native kind50 resource exceeds its fixed field"));
        }
        if self.next_plane_index >= 3 || !f32::from_bits(self.phase_z_word).is_finite() {
            return Err(invalid("native kind50 recycling phase is invalid"));
        }
        for mesh in &self.meshes {
            mesh.validate()?;
        }
        Ok(())
    }

    /// 411C60/4582F0 read the primary camera's raw position. Y is unused.
    /// 452900 follows X/Z on every callback, including stationary frames, and
    /// recycles at most one child after a strict Z > phase+10 comparison.
    pub(crate) fn advance(&mut self, camera_xz_words: [u32; 2]) -> Result<(), CoreError> {
        self.validate()?;
        let [camera_x, camera_z] = camera_xz_words.map(f32::from_bits);
        if !camera_x.is_finite() || !camera_z.is_finite() {
            return Err(invalid("native kind50 camera position is non-finite"));
        }
        self.meshes[3].position_words = [camera_xz_words[0], 0, camera_xz_words[1]];
        let phase_z = f32::from_bits(self.phase_z_word);
        if camera_z > phase_z + 10.0_f32 {
            self.meshes[self.next_plane_index as usize].position_words =
                [0, 0, (phase_z + 30.0_f32).to_bits()];
            self.next_plane_index = (self.next_plane_index + 1) % 3;
            self.phase_z_word = (phase_z + 10.0_f32).to_bits();
        }
        Ok(())
    }

    pub(crate) fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    /// Submission order is fixed child0/1/2, then the independent floor root.
    pub(crate) fn mesh(&self, index: usize) -> Option<&CmvsNativeMesh> {
        self.meshes.get(index)
    }

    /// Valid mesh indices are 0..mesh_count(). Children inherit root0's provider.
    pub(crate) fn mesh_resource_index(&self, index: usize) -> usize {
        [0, 0, 0, 1][index]
    }
}

#[cfg(test)]
mod tests;
