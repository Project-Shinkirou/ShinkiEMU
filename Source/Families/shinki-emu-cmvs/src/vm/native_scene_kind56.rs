//! Ordinary662 / 448600: a backdrop and seven independently phased horizontal
//! quads. The native malloc-backed owner never initializes its status, pending
//! or animation-clock words. Keep those words unknown until explicitly seeded;
//! the constructor geometry and resource bindings are nevertheless defined.
use super::native_scene_frame::NativeSceneFrame;
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeSceneKind56 {
    meshes: [CmvsNativeMesh; 8],
    /// Native owner DWORD34, read only when DWORD39 is all-ones.
    status_word: Option<u32>,
    /// Native owner DWORD35: read/modify/write by 450BC0 before any assignment.
    clock_ms: Option<u32>,
    /// Native owner DWORD39: read by 450B50; never written by this controller.
    pending_word: Option<u32>,
}

fn words<const N: usize>(values: [f32; N]) -> [u32; N] {
    values.map(f32::to_bits)
}

fn quad(
    positions: [[f32; 3]; 4],
    position_words: [u32; 3],
    angle_words: [u32; 3],
    foreground: bool,
) -> CmvsNativeMesh {
    let one = 1.0_f32.to_bits();
    let uv = [[0, 0], [one, 0], [0, one], [one, one]];
    CmvsNativeMesh {
        kind: 3,
        alpha: 255,
        blend: 0,
        position_words,
        angle_words,
        scale_words: [one; 3],
        billboard: false,
        force_blend: foreground,
        late_pass: foreground,
        vertices: std::array::from_fn(|index| CmvsNativeMeshVertex {
            position_words: words(positions[index]),
            rgba_words: [one; 4],
            uv_words: uv[index],
        }),
    }
}

fn indeterminate(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_SCENE_KIND56_INDETERMINATE", message)
}

impl CmvsNativeSceneKind56 {
    pub const RESOURCES: [&'static [u8]; 2] = [b"po_bg510c.pb3", b"it008d2.pb3"];

    /// 450360 stores a2 only at unused DWORD40. 450460 ignores a3 entirely.
    /// Neither operand supplies the uninitialized native heap phase; neither
    /// constructor consumes RNG, changes the camera, or touches an audio bank.
    pub(crate) fn new() -> Result<Self, CoreError> {
        // These immediate DWORDs are retained bit-for-bit from 450460.
        let positions = [
            [0xbf5c_28f6, 0x3f19_999a, 0x4040_0000],
            [0x3f4c_cccd, 0x3fb3_3333, 0x4080_0000],
            [0xbf19_999a, 0x400c_cccd, 0x40a0_0000],
            [0x3f19_999a, 0x4040_0000, 0x40c0_0000],
            [0xbecc_cccd, 0x4073_3333, 0x40e0_0000],
            [0x3ecc_cccd, 0x4093_3333, 0x4100_0000],
            [0xbe4c_cccd, 0x40ac_cccd, 0x4110_0000],
        ];
        let pitch = [
            0x40a0_0000,
            0xc170_0000,
            0xc170_0000,
            0xc190_0000,
            0xc190_0000,
            0xc190_0000,
            0xc190_0000,
        ];
        let owner = Self {
            meshes: std::array::from_fn(|index| {
                if index == 0 {
                    return quad(
                        [
                            [-7.5, 9.0, 0.0],
                            [7.5, 9.0, 0.0],
                            [-7.5, 0.0, 0.0],
                            [7.5, 0.0, 0.0],
                        ],
                        [0, 0xc040_0000, 0x410b_3333],
                        [0; 3],
                        false,
                    );
                }
                let ring = index - 1;
                quad(
                    [
                        [-1.0, 0.0, 1.0],
                        [1.0, 0.0, 1.0],
                        [-1.0, 0.0, -1.0],
                        [1.0, 0.0, -1.0],
                    ],
                    positions[ring],
                    [
                        pitch[ring],
                        0,
                        if ring % 2 == 0 {
                            0xc120_0000
                        } else {
                            0x4120_0000
                        },
                    ],
                    true,
                )
            }),
            status_word: None,
            clock_ms: None,
            pending_word: None,
        };
        owner.validate()?;
        Ok(owner)
    }

    /// Evidence/replay entry point for an explicitly captured native allocation.
    /// `heap_words` are DWORD34 (status), DWORD35 (clock), DWORD39 (pending),
    /// not script operands or an inferred zero-filled-heap default.
    #[cfg(test)]
    pub(crate) fn new_with_native_heap_phase(heap_words: [u32; 3]) -> Result<Self, CoreError> {
        let mut owner = Self::new()?;
        owner.status_word = Some(heap_words[0]);
        owner.clock_ms = Some(heap_words[1]);
        owner.pending_word = Some(heap_words[2]);
        Ok(owner)
    }

    pub(crate) fn initial_frame(&self) -> NativeSceneFrame {
        NativeSceneFrame::default()
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        for mesh in &self.meshes {
            mesh.validate()?;
        }
        // Arbitrary DWORD phase values, including all-ones, are valid native
        // heap contents. Unknown words are valid snapshots, not invented data.
        Ok(())
    }

    /// 450450 is `mov eax,1; ret 12`: all three request words are ignored.
    pub(crate) fn request(&mut self, _selector: u32) {}

    pub(crate) fn status(&self) -> Result<u32, CoreError> {
        match self.pending_word {
            Some(u32::MAX) => self.status_word.ok_or_else(|| {
                indeterminate("450B50 reads uninitialized native owner DWORD34 (status)")
            }),
            Some(_) => Ok(u32::MAX),
            None => Err(indeterminate(
                "450B50 reads uninitialized native owner DWORD39 (pending)",
            )),
        }
    }

    /// 450BC0 uses unsigned arithmetic: clamp delta before wrapping addition,
    /// then wrap clock*3600 before /120000. Only meshes1..7 change. No request
    /// or status transition exists, and no camera or audio side effect occurs.
    pub(crate) fn advance(
        &mut self,
        delta_ms: u32,
        _volume_word: u32,
    ) -> Result<NativeSceneFrame, CoreError> {
        self.validate()?;
        let clock = self
            .clock_ms
            .ok_or_else(|| {
                indeterminate("450BC0 reads uninitialized native owner DWORD35 (animation clock)")
            })?
            .wrapping_add(delta_ms.min(200));
        self.clock_ms = Some(clock);
        let yaw = clock.wrapping_mul(3600) / 120_000 % 3600;
        let phase = clock % 2000;
        let alpha = if phase < 1000 {
            255 - 63 * phase / 1000
        } else {
            (63 * phase - 63_000) / 1000 + 192
        };
        // Native xmmword4D2760, then4D2750; loop indices are1..=7.
        for (mesh, offset) in self.meshes[1..]
            .iter_mut()
            .zip([720, 0, 1720, 480, 1500, 640, 1280])
        {
            mesh.angle_words[1] = (((yaw + offset) % 3600) as f32 / 10.0_f32).to_bits();
            mesh.set_alpha(alpha as i32);
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
        usize::from(index != 0)
    }
    pub(crate) fn resource_count(&self) -> usize {
        Self::RESOURCES.len()
    }
    pub(crate) fn resource(&self, index: usize) -> Option<&[u8]> {
        Self::RESOURCES.get(index).copied()
    }
}

#[cfg(test)]
mod tests;
