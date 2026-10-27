//! Ordinary659 /4484A0: two hinged meshes and a screen-space backdrop.
//! 1033/1029 transition owners tick in render preparation, but4454D0 bypasses
//! them for these D3D meshes: pipeline8 samples only the retained base image.
use super::native_scene_frame::NativeSceneFrame;
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeSceneKind53 {
    meshes: [CmvsNativeMesh; 2],
    visible: [bool; 2],
    fallback: [bool; 2],
    transitions: [Option<u32>; 2],
    state: u32,
    pending: u32,
    effect_timer: u32,
    wobble_timer: u32,
    wobble_phase: u32,
    base_yaw_word: u32,
}

fn words<const N: usize>(values: [f32; N]) -> [u32; N] {
    values.map(f32::to_bits)
}

fn quad(right: bool) -> CmvsNativeMesh {
    let one = 1.0_f32.to_bits();
    let (left, right_edge, x) = if right {
        (0.0, 20.0, 0.1)
    } else {
        (-20.0, 0.0, -0.3)
    };
    let positions = [
        [left, 12.5, 0.0],
        [right_edge, 12.5, 0.0],
        [left, -7.5, 0.0],
        [right_edge, -7.5, 0.0],
    ];
    let uv = [[0, 0], [one, 0], [0, one], [one, one]];
    CmvsNativeMesh {
        kind: 3,
        alpha: 255,
        blend: 0,
        position_words: words([x, 0.0, 28.0]),
        angle_words: [0; 3],
        scale_words: [one; 3],
        billboard: false,
        force_blend: true,
        late_pass: true,
        vertices: std::array::from_fn(|index| CmvsNativeMeshVertex {
            position_words: words(positions[index]),
            rgba_words: [one; 4],
            uv_words: uv[index],
        }),
    }
}

impl CmvsNativeSceneKind53 {
    /// 44E960 probes the first two providers independently. A width other than
    /// 512 (including an allocated failed-load provider's zero width) selects
    /// its actual native alternate; height and request operands are not read.
    pub const RESOURCES: [&'static [u8]; 3] = [b"try007c3.pb3", b"try007c4.pb3", b"__try007a.pb3"];
    pub const FALLBACK_RESOURCES: [&'static [u8]; 2] = [b"try007c1.pb3", b"try007c2.pb3"];

    pub(crate) fn new(probe_widths: [u32; 2]) -> Result<Self, CoreError> {
        let owner = Self {
            meshes: [quad(false), quad(true)],
            visible: [false; 2],
            fallback: probe_widths.map(|width| width != 512),
            transitions: [None, None],
            state: 0,
            pending: u32::MAX,
            effect_timer: 0,
            wobble_timer: 0,
            wobble_phase: 0,
            base_yaw_word: 0,
        };
        owner.validate()?;
        Ok(owner)
    }

    /// 44E550 resets position, pitch and yaw, but retains the live camera roll.
    pub(crate) fn initial_frame(&self) -> NativeSceneFrame {
        NativeSceneFrame {
            position_words: Some(words([0.0, 3.2, 0.0])),
            pitch_word: Some(0),
            yaw_word: Some(0),
            ..Default::default()
        }
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        for mesh in &self.meshes {
            mesh.validate()?;
        }
        if !matches!(self.state, 0..=5 | 10..=12)
            || !matches!(self.pending, 1..=5 | u32::MAX)
            || self.wobble_phase > 3
            || !f32::from_bits(self.base_yaw_word).is_finite()
            || self
                .transitions
                .iter()
                .flatten()
                .any(|elapsed| *elapsed >= 5000)
        {
            return Err(CoreError::invalid(
                "SHINKI_EMU_CMVS_NATIVE_SCENE_KIND53",
                "invalid native kind53 controller state",
            ));
        }
        Ok(())
    }

    /// 44E390 ignores the other two operands and overwrites the pending request.
    pub(crate) fn request(&mut self, selector: u32) {
        if matches!(selector, 1..=5) {
            self.pending = selector;
        }
    }

    pub(crate) fn status(&self) -> u32 {
        if self.pending == u32::MAX {
            self.state
        } else {
            u32::MAX
        }
    }

    /// 44EA40 uses the full unsigned419AA0 delta, without the200ms cap used
    /// by kind52. Accepted requests execute in this callback; states11/12 only
    /// start accumulating on the next callback. No audio bank is touched.
    pub(crate) fn advance(
        &mut self,
        frame_delta_ms: u32,
        _volume_word: u32,
    ) -> Result<NativeSceneFrame, CoreError> {
        self.validate()?;
        if self.state == 0 && self.pending != u32::MAX {
            self.state = self.pending;
            self.pending = u32::MAX;
        }
        match self.state {
            1 => {
                self.transitions = [Some(0); 2];
                self.visible = [true; 2];
                self.state = 10;
            }
            2 => {
                self.visible = [false; 2];
                self.state = 0;
            }
            3 => {
                self.visible = [true; 2];
                self.state = 0;
            }
            4 => {
                self.effect_timer = 0;
                self.state = 11;
            }
            5 => {
                self.effect_timer = 0;
                self.state = 12;
            }
            10 => {
                if self.transitions.iter().all(Option::is_none) {
                    self.state = 0;
                }
            }
            11 => {
                self.effect_timer = self.effect_timer.wrapping_add(frame_delta_ms);
                if self.effect_timer >= 800 {
                    self.base_yaw_word = 5.0_f32.to_bits();
                    self.state = 0;
                } else {
                    // 452A60: round every SSE float operation before the double
                    // sine call, then truncate to hundredths, not nearest.
                    let degrees = (self.effect_timer as f32 * 90.0_f32) / 800.0_f32;
                    let radians = (degrees / 180.0_f32) * f32::from_bits(0x4049_0fdb);
                    let radius = ((radians as f64).sin() * 5.0_f64) as f32;
                    let hundredths = (radius * 100.0_f32) as i32;
                    self.base_yaw_word = (hundredths as f32 / 100.0_f32).to_bits();
                }
            }
            12 => {
                self.effect_timer = self.effect_timer.wrapping_add(frame_delta_ms);
                if self.effect_timer >= 500 {
                    // Native hides without restoring alpha, including the
                    // zero-alpha avoidance caused by completing at>=500ms.
                    self.visible = [false; 2];
                    self.state = 0;
                } else {
                    let alpha = 256 - (self.effect_timer << 8) / 500;
                    for mesh in &mut self.meshes {
                        mesh.set_alpha(alpha as i32);
                    }
                }
            }
            _ => {}
        }
        self.advance_wobble(frame_delta_ms);
        Ok(NativeSceneFrame::default())
    }

    fn advance_wobble(&mut self, delta_ms: u32) {
        self.wobble_timer = self.wobble_timer.wrapping_add(delta_ms);
        let base = f32::from_bits(self.base_yaw_word);
        let (left, right) = match self.wobble_phase {
            0 => {
                let amount = if self.wobble_timer >= 2000 {
                    self.wobble_timer = 0;
                    self.wobble_phase = 1;
                    1.0_f32
                } else {
                    self.wobble_timer as f32 / 2000.0_f32
                };
                (base + amount, base - amount)
            }
            1 => {
                if self.wobble_timer > 200 {
                    self.wobble_timer = 0;
                    self.wobble_phase = 2;
                }
                (base + 1.0_f32, base - 1.0_f32)
            }
            2 => {
                let amount = if self.wobble_timer >= 2000 {
                    self.wobble_timer = 0;
                    self.wobble_phase = 3;
                    1.0_f32
                } else {
                    self.wobble_timer as f32 / 2000.0_f32
                };
                ((1.0_f32 - amount) + base, (amount - 1.0_f32) + base)
            }
            _ => {
                if self.wobble_timer > 200 {
                    self.wobble_timer = 0;
                    self.wobble_phase = 0;
                }
                (base + 0.0_f32, base + 0.0_f32)
            }
        };
        self.meshes[0].angle_words[1] = left.to_bits();
        self.meshes[1].angle_words[1] = right.to_bits();
    }

    /// 44E410 /4597C0 is a separate render-preparation phase, not part of737.
    /// Call after advance and before reading draw state, even for hidden meshes.
    /// Completion releases shader+secondary texture; it does not replace the
    /// retained c-provider. State10 observes that release on the next advance.
    pub(crate) fn prepare_draw(&mut self, frame_delta_ms: u32) -> Result<(), CoreError> {
        self.validate()?;
        for slot in &mut self.transitions {
            if let Some(elapsed) = slot {
                let next = elapsed.wrapping_add(frame_delta_ms);
                *slot = (next < 5000).then_some(next);
            }
        }
        Ok(())
    }

    pub(crate) fn mesh_count(&self) -> usize {
        2
    }
    pub(crate) fn mesh(&self, index: usize) -> Option<&CmvsNativeMesh> {
        self.meshes.get(index)
    }
    pub(crate) fn mesh_visible(&self, index: usize) -> bool {
        self.visible.get(index).copied().unwrap_or(false)
    }
    pub(crate) fn mesh_resource_index(&self, index: usize) -> usize {
        index
    }
    // Shader uniforms and secondary images have no consumer on pipeline8.
    pub(crate) fn resource(&self, index: usize) -> Option<&[u8]> {
        match index {
            0 | 1 => Some(if self.fallback[index] {
                Self::FALLBACK_RESOURCES[index]
            } else {
                Self::RESOURCES[index]
            }),
            2 => Some(Self::RESOURCES[2]),
            _ => None,
        }
    }

    /// Third native root: basic surface, queue3/order64; never a world mesh.
    /// Resource index2 uses446C20 flag0; the c/d providers use load flag1.
    pub(crate) fn backdrop_basic(&self) -> CmvsTextureParentState {
        CmvsTextureParentState {
            surface_initialized: true,
            basic_kind: 0,
            blend_mode: Some(0),
            alpha: Some(255),
            scale_words: Some([1.0_f32.to_bits(); 2]),
            rect_words: Some([0, 0, 1024, 614]),
            position_words: Some([0, 0]),
            auxiliary_pair_words: Some([0, 0]),
            auxiliary_word: Some(64),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests;
