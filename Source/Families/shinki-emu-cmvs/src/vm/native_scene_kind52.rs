//! Ordinary658 / 4483E0: nine fixed providers, 27 child meshes and two queued effects.
//! 44DCC0 rotates every ring before advancing the effect. Native saves retain only
//! the kind marker; private snapshots retain the clocks, event gate and queued request.
use super::native_scene_frame::{NativeSceneFrame, NativeSceneSound};
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeSceneKind52 {
    /// Root-major submission order: a1, a2, four children per c1..c6, then d.
    meshes: [CmvsNativeMesh; 27],
    rotation_clock: u32,
    state: u32,
    state_timer: u32,
    event_gate: bool,
    pending: u32,
}

const GROUP_START: [usize; 12] = [0, 1, 3, 5, 6, 7, 9, 11, 12, 13, 15, 17];
const GROUP_COUNT: [usize; 12] = [1, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2, 1];

fn words<const N: usize>(values: [f32; N]) -> [u32; N] {
    values.map(f32::to_bits)
}

fn quad(positions: [[f32; 3]; 4], position: [f32; 3], late: bool) -> CmvsNativeMesh {
    let one = 1.0_f32.to_bits();
    let uv = [[0, 0], [one, 0], [0, one], [one, one]];
    CmvsNativeMesh {
        kind: 3,
        alpha: 255,
        blend: 0,
        position_words: words(position),
        angle_words: [0; 3],
        scale_words: [one; 3],
        billboard: false,
        force_blend: late,
        late_pass: late,
        vertices: std::array::from_fn(|index| CmvsNativeMeshVertex {
            position_words: words(positions[index]),
            rgba_words: [one; 4],
            uv_words: uv[index],
        }),
    }
}

impl CmvsNativeSceneKind52 {
    /// 44D920 loads these nine providers once, in this order.
    pub const RESOURCES: [&'static [u8]; 9] = [
        b"try002a1.pb3",
        b"try002a2.pb3",
        b"try002c1.pb3",
        b"try002c2.pb3",
        b"try002c3.pb3",
        b"try002c4.pb3",
        b"try002c5.pb3",
        b"try002c6.pb3",
        b"try002d.pb3",
    ];
    pub const SOUND_RESOURCES: [(u8, &'static [u8]); 2] = [(16, b"SE006.ogg"), (17, b"SE006c.wav")];

    /// 44CEB0/44D1C0/44D3C0/44D5F0 use literal vertices, not texture dimensions.
    pub(crate) fn new() -> Result<Self, CoreError> {
        let owner = Self {
            meshes: std::array::from_fn(|index| match index {
                0 => quad(
                    [
                        [-30.0, 36.0, 0.0],
                        [30.0, 36.0, 0.0],
                        [-30.0, 0.0, 0.0],
                        [30.0, 0.0, 0.0],
                    ],
                    [0.0, -12.0, 39.0],
                    false,
                ),
                1 => quad(
                    [
                        [-5.0, 6.0, 0.0],
                        [5.0, 6.0, 0.0],
                        [-5.0, 0.0, 0.0],
                        [5.0, 0.0, 0.0],
                    ],
                    [0.0, 2.0, f32::from_bits(0x40e4_cccd)],
                    true,
                ),
                26 => quad(
                    [
                        [-6.0, 20.0, 0.0],
                        [6.0, 20.0, 0.0],
                        [-6.0, 0.0, 0.0],
                        [6.0, 0.0, 0.0],
                    ],
                    [2.0, -20.0, 12.0],
                    true,
                ),
                _ => {
                    let provider = (index - 2) / 4;
                    let ring = (index - 2) % 4;
                    // Both loops skip their Y increment after provider1/provider3.
                    // 44D3C0 does not reset Y between its three six-child rounds.
                    let height_step = [0, 1, 1, 2, 2, 3][provider];
                    let y = if ring == 0 {
                        1.25_f32 + height_step as f32 * 0.25_f32
                    } else {
                        2.5_f32 + ((ring - 1) * 4 + height_step) as f32 * 0.75_f32
                    };
                    quad(
                        [
                            [-3.0, 0.0, 3.0],
                            [3.0, 0.0, 3.0],
                            [-3.0, 0.0, -3.0],
                            [3.0, 0.0, -3.0],
                        ],
                        [2.0, y, 13.0],
                        true,
                    )
                }
            }),
            rotation_clock: 0,
            state: 0,
            state_timer: 0,
            event_gate: false,
            pending: u32::MAX,
        };
        owner.validate()?;
        Ok(owner)
    }

    pub(crate) fn initial_frame(&self) -> NativeSceneFrame {
        NativeSceneFrame::default()
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        for mesh in &self.meshes {
            mesh.validate()?;
        }
        Ok(())
    }

    /// 44D7A0 ignores every selector except 1/2 and overwrites an existing request.
    pub(crate) fn request(&mut self, selector: u32) {
        if matches!(selector, 1 | 2) {
            self.pending = selector;
        }
    }

    /// 44D900 reports -1 while any request is waiting, even during an active effect.
    pub(crate) fn status(&self) -> u32 {
        if self.pending == u32::MAX {
            self.state
        } else {
            u32::MAX
        }
    }

    /// 44DF20 addresses only the eighteen upper rings. The first six are untouched.
    fn set_group_alpha(&mut self, group: usize, alpha: u32) {
        for child in GROUP_START[group]..GROUP_START[group] + GROUP_COUNT[group] {
            let mesh = 2 + (child % 6) * 4 + 1 + child / 6;
            self.meshes[mesh].set_alpha(alpha.min(255) as i32);
        }
    }

    /// Rotation uses the full 419AA0 delta; the effect timer independently caps it
    /// at 200ms. The DWORD multiply wraps before unsigned /5000 and %360.
    pub(crate) fn advance(
        &mut self,
        frame_delta_ms: u32,
        volume_word: u32,
    ) -> Result<NativeSceneFrame, CoreError> {
        self.validate()?;
        self.rotation_clock = self.rotation_clock.wrapping_add(frame_delta_ms);
        let angle = (self.rotation_clock.wrapping_mul(360) / 5000 % 360) as f32;
        for provider in 0..6 {
            let yaw = if matches!(provider, 0 | 1 | 4) {
                angle
            } else {
                360.0_f32 - angle
            };
            for ring in 0..4 {
                self.meshes[2 + provider * 4 + ring].angle_words[1] = yaw.to_bits();
            }
        }
        let mut frame = NativeSceneFrame::default();
        match self.state {
            0 => {
                if self.pending != u32::MAX {
                    self.state = self.pending;
                    self.pending = u32::MAX;
                    self.state_timer = 0;
                    self.event_gate = false;
                }
            }
            1 | 2 => {
                let disappearing = self.state == 2;
                if !self.event_gate {
                    frame.audio[0] = Some(NativeSceneSound::PlayStaticMain {
                        slot: if disappearing { 17 } else { 16 },
                        volume: volume_word,
                        looping: false,
                    });
                    self.event_gate = true;
                }
                self.state_timer = self.state_timer.wrapping_add(frame_delta_ms.min(200));
                let end_time = if disappearing { 7000 } else { 6500 };
                if self.state_timer >= end_time {
                    for group in 0..12 {
                        self.set_group_alpha(group, if disappearing { 0 } else { 255 });
                    }
                    if disappearing {
                        self.meshes[26].position_words = words([2.0, -20.0, 12.0]);
                        frame.audio[1] = Some(NativeSceneSound::StopStatic { slot: 17 });
                    }
                    self.state = 0;
                    self.state_timer = 0;
                    self.event_gate = false;
                } else {
                    if disappearing {
                        let lift = if self.state_timer >= 1000 {
                            32.0_f32
                        } else {
                            (self.state_timer as f32 * 32.0_f32) / 1000.0_f32
                        };
                        self.meshes[26].position_words = words([2.0, lift - 20.0_f32, 12.0]);
                    }
                    for group in 0..12 {
                        let start = group as u32 * 300 + if disappearing { 1500 } else { 0 };
                        if self.state_timer >= start {
                            let elapsed = self.state_timer - start;
                            let alpha = if disappearing {
                                if elapsed >= 2000 {
                                    0
                                } else {
                                    255 - 255 * elapsed / 2000
                                }
                            } else if elapsed >= 3000 {
                                255
                            } else {
                                255 * elapsed / 3000
                            };
                            self.set_group_alpha(group, alpha);
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(frame)
    }

    pub(crate) fn mesh_count(&self) -> usize {
        self.meshes.len()
    }
    pub(crate) fn mesh(&self, index: usize) -> Option<&CmvsNativeMesh> {
        self.meshes.get(index)
    }
    pub(crate) fn mesh_resource_index(&self, index: usize) -> usize {
        match index {
            0 | 1 => index,
            26 => 8,
            _ => 2 + (index - 2) / 4,
        }
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
