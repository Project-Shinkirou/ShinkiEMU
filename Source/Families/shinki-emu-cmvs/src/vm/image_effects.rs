//! CBasic owners restored by 42E260 and advanced by 42E5B0.
use super::*;

#[path = "float_motion.rs"]
mod float_motion;
pub use float_motion::{CmvsFloatMotion, CmvsFloatSegment};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsAlphaSegment {
    pub next: i32,
    pub target: u32,
    /// Only the low byte gates interpolation (432B40).
    pub step: u32,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsAlphaEffect {
    pub active: u32,
    pub origin: u32,
    pub start: u32,
    pub current: u32,
    pub elapsed_ms: u32,
    pub segments: [CmvsAlphaSegment; 8],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsSpriteFrame {
    pub next: i32,
    pub source: [i16; 4],
    /// A negative component preserves that component of the current pivot.
    pub pivot: [i16; 2],
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsSpriteEffect {
    pub active: u32,
    pub start: u32,
    pub current: u32,
    pub elapsed_ms: u32,
    /// Native v2 stores 64 frames; two fixed blocks keep serde's array support.
    pub frames: [[CmvsSpriteFrame; 32]; 2],
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsBasicEffects {
    pub alpha: Option<CmvsAlphaEffect>,
    pub sprite: Option<CmvsSpriteEffect>,
    pub motion: Option<CmvsFloatMotion>,
}

fn invalid_effect() -> CoreError {
    invalid(
        "SHINKI_EMU_CMVS_IMAGE_EFFECT",
        "native basic effect has an invalid active frame graph",
    )
}

fn validate_graph(
    mut current: u32,
    count: usize,
    mut frame: impl FnMut(usize) -> (i32, u32),
) -> Result<(), CoreError> {
    let mut visited = 0_u64;
    let mut zero_duration = 0_u64;
    loop {
        let index = current as usize;
        if index >= count {
            return Err(invalid_effect());
        }
        let bit = 1_u64 << index;
        if visited & bit != 0 {
            // A cycle is legal only if some member consumes clock time.
            let mut cycle = current;
            loop {
                if zero_duration & (1_u64 << cycle) == 0 {
                    return Ok(());
                }
                cycle = frame(cycle as usize).0 as u32;
                if cycle == current {
                    return Err(invalid_effect());
                }
            }
        }
        visited |= bit;
        let (next, duration) = frame(index);
        if duration == 0 {
            zero_duration |= bit;
        }
        if next < 0 {
            return Ok(());
        }
        current = next as u32;
    }
}

impl CmvsBasicEffects {
    pub(crate) fn active(&self) -> bool {
        self.alpha.as_ref().is_some_and(|effect| effect.active != 0)
            || self
                .sprite
                .as_ref()
                .is_some_and(|effect| effect.active != 0)
            || self
                .motion
                .as_ref()
                .is_some_and(|effect| effect.active != 0)
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if let Some(effect) = &self.motion {
            effect.validate()?;
        }
        if let Some(effect) = &self.alpha
            && effect.active != 0
        {
            validate_graph(effect.current, 8, |i| {
                let frame = effect.segments[i];
                (frame.next, frame.duration_ms)
            })?;
        }
        if let Some(effect) = &self.sprite
            && effect.active != 0
        {
            validate_graph(effect.current, 64, |i| {
                let frame = effect.frames[i / 32][i % 32];
                (frame.next, frame.duration_ms)
            })?;
        }
        Ok(())
    }

    pub(crate) fn advance_motion(
        &mut self,
        delta_ms: u32,
        position: &mut [u32; 3],
    ) -> Result<bool, CoreError> {
        let Some(effect) = &mut self.motion else {
            return Ok(false);
        };
        let result = effect.advance(delta_ms, position)?;
        if result < 0 {
            self.motion = None;
        }
        Ok(result == -2)
    }

    /// 42E5B0 retires a completed owner; -2 also requests image destruction.
    /// Both clocks retain their serialized remainder and cap each frame at 200ms.
    pub(crate) fn advance(
        &mut self,
        delta_ms: u32,
        alpha: &mut i32,
        source: &mut [u32; 4],
        pivot: &mut [u32; 2],
    ) -> Result<bool, CoreError> {
        let mut destroy = false;
        if let Some(effect) = &mut self.alpha
            && effect.active != 0
        {
            effect.elapsed_ms = effect.elapsed_ms.wrapping_add(delta_ms.min(200));
            let result = loop {
                let segment = *effect
                    .segments
                    .get(effect.current as usize)
                    .ok_or_else(invalid_effect)?;
                if segment.duration_ms > effect.elapsed_ms {
                    if segment.step as u8 == 0 {
                        let numerator = effect
                            .elapsed_ms
                            .wrapping_mul(segment.target.wrapping_sub(effect.origin))
                            as i32;
                        let quotient = numerator
                            .checked_div(segment.duration_ms as i32)
                            .ok_or_else(invalid_effect)?;
                        *alpha = effect.origin.wrapping_add(quotient as u32) as i32;
                    }
                    break 0;
                }
                effect.origin = segment.target;
                *alpha = segment.target as i32;
                if segment.next < 0 {
                    break segment.next;
                }
                effect.elapsed_ms = effect.elapsed_ms.wrapping_sub(segment.duration_ms);
                effect.current = segment.next as u32;
                let next = effect
                    .segments
                    .get(effect.current as usize)
                    .ok_or_else(invalid_effect)?;
                // Native does not interpolate the newly entered segment this tick.
                if next.duration_ms > effect.elapsed_ms {
                    break 0;
                }
            };
            if result < 0 {
                destroy |= result == -2;
                self.alpha = None;
            }
        }
        if let Some(effect) = &mut self.sprite
            && effect.active != 0
        {
            effect.elapsed_ms = effect.elapsed_ms.wrapping_add(delta_ms.min(200));
            let result = loop {
                let frame = *effect
                    .frames
                    .get(effect.current as usize / 32)
                    .and_then(|block| block.get(effect.current as usize % 32))
                    .ok_or_else(invalid_effect)?;
                if frame.duration_ms > effect.elapsed_ms {
                    break 0;
                }
                if frame.next < 0 {
                    break frame.next;
                }
                effect.elapsed_ms = effect.elapsed_ms.wrapping_sub(frame.duration_ms);
                effect.current = frame.next as u32;
                let frame = *effect
                    .frames
                    .get(effect.current as usize / 32)
                    .and_then(|block| block.get(effect.current as usize % 32))
                    .ok_or_else(invalid_effect)?;
                *source = frame.source.map(|word| word as i32 as u32);
                for (pivot, &frame_pivot) in pivot.iter_mut().zip(&frame.pivot) {
                    if frame_pivot >= 0 {
                        *pivot = frame_pivot as u32;
                    }
                }
            };
            if result < 0 {
                destroy |= result == -2;
                self.sprite = None;
            }
        }
        Ok(destroy)
    }
}

pub(crate) fn validate(state: &CmvsPs2aVmState) -> Result<(), CoreError> {
    for image in state.texture_parents.values() {
        if image.image_effect_words[0] != 0 {
            return Err(invalid_effect());
        }
        image.basic_effects.validate()?;
    }
    for image in state
        .texture_children
        .values()
        .flat_map(|children| children.values())
    {
        if image.image_effect_words[0] != 0 {
            return Err(invalid_effect());
        }
        image.basic_effects.validate()?;
    }
    for image in state.effect_elements.values() {
        if image.image_effect_words[0] != 0 {
            return Err(invalid_effect());
        }
        image.basic_effects.validate()?;
        if let Some(reload) = &image.image_reload {
            if reload.mode == super::texture_reload::ReloadMode::Pending
                || (reload.busy()
                    && (reload.duration_ms == 0
                        || reload.elapsed_ms >= reload.duration_ms
                        || reload.current_uri != image.image_resource))
            {
                return Err(invalid_effect());
            }
            reload.previous_parent.basic_effects.validate()?;
        }
    }
    Ok(())
}
pub(crate) fn advance_texture_fields(
    initialized: bool,
    effects: &mut CmvsBasicEffects,
    alpha: &mut Option<i32>,
    source: &mut Option<[u32; 4]>,
    pivot: &mut Option<[u32; 2]>,
    delta_ms: u32,
    position: &mut [u32; 3],
) -> Result<bool, CoreError> {
    if !initialized || !effects.active() {
        return Ok(false);
    }
    let mut a = alpha.unwrap_or(255);
    let mut s = source.unwrap_or([0; 4]);
    let mut p = pivot.unwrap_or([0; 2]);
    let destroy = effects.advance(delta_ms, &mut a, &mut s, &mut p)?
        | effects.advance_motion(delta_ms, position)?;
    if a != alpha.unwrap_or(255) {
        *alpha = Some(a);
    }
    if s != source.unwrap_or([0; 4]) {
        *source = Some(s);
    }
    if p != pivot.unwrap_or([0; 2]) {
        *pivot = Some(p);
    }
    Ok(destroy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_alpha_retains_clock_and_only_low_step_byte_gates_interpolation() {
        let mut effect = CmvsAlphaEffect {
            active: 1,
            elapsed_ms: 625,
            ..Default::default()
        };
        effect.segments[0] = CmvsAlphaSegment {
            next: 1,
            target: 255,
            step: 0x100,
            duration_ms: 750,
        };
        effect.segments[1] = CmvsAlphaSegment {
            next: -2,
            target: 0,
            step: 0,
            duration_ms: 750,
        };
        let mut effects = CmvsBasicEffects {
            alpha: Some(effect),
            ..Default::default()
        };
        let (mut alpha, mut source, mut pivot) = (212, [0; 4], [0; 2]);
        assert!(
            !effects
                .advance(25, &mut alpha, &mut source, &mut pivot)
                .unwrap()
        );
        assert_eq!(alpha, 221);
        assert!(
            !effects
                .advance(100, &mut alpha, &mut source, &mut pivot)
                .unwrap()
        );
        assert_eq!(alpha, 255);
        assert_eq!(effects.alpha.as_ref().unwrap().elapsed_ms, 0);
        assert!(
            !effects
                .advance(500, &mut alpha, &mut source, &mut pivot)
                .unwrap()
        );
        assert_eq!(effects.alpha.as_ref().unwrap().elapsed_ms, 200);
        assert_eq!(alpha, 187);
    }

    #[test]
    fn native_sprite_retains_remainder_and_negative_pivot_components() {
        let mut effect = CmvsSpriteEffect {
            active: 1,
            current: 1,
            elapsed_ms: 81,
            ..Default::default()
        };
        effect.frames[0][1] = CmvsSpriteFrame {
            next: 2,
            duration_ms: 100,
            ..Default::default()
        };
        effect.frames[0][2] = CmvsSpriteFrame {
            next: 1,
            source: [48, 690, 24, 24],
            pivot: [-1, 7],
            duration_ms: 100,
        };
        let mut effects = CmvsBasicEffects {
            sprite: Some(effect),
            ..Default::default()
        };
        let (mut alpha, mut source, mut pivot) = (255, [24, 690, 24, 24], [5, 6]);
        assert!(
            !effects
                .advance(20, &mut alpha, &mut source, &mut pivot)
                .unwrap()
        );
        assert_eq!(source, [48, 690, 24, 24]);
        assert_eq!(pivot, [5, 7]);
        assert_eq!(effects.sprite.as_ref().unwrap().elapsed_ms, 1);
    }

    #[test]
    fn inactive_native_owners_preserve_stale_indices_and_clock() {
        let effects = CmvsBasicEffects {
            alpha: Some(CmvsAlphaEffect {
                current: u32::MAX,
                elapsed_ms: 625,
                ..Default::default()
            }),
            sprite: Some(CmvsSpriteEffect {
                current: u32::MAX,
                elapsed_ms: 981,
                ..Default::default()
            }),
            ..Default::default()
        };
        effects.validate().unwrap();
        let mut advanced = effects.clone();
        let (mut alpha, mut source, mut pivot) = (42, [1; 4], [2; 2]);
        assert!(
            !advanced
                .advance(200, &mut alpha, &mut source, &mut pivot)
                .unwrap()
        );
        assert_eq!(advanced, effects);
        assert_eq!((alpha, source, pivot), (42, [1; 4], [2; 2]));
    }

    #[test]
    fn terminal_minus_two_removes_effect_and_requests_image_destruction() {
        let mut effect = CmvsAlphaEffect {
            active: 1,
            elapsed_ms: 749,
            ..Default::default()
        };
        effect.segments[0] = CmvsAlphaSegment {
            next: -2,
            target: 0,
            step: 0,
            duration_ms: 750,
        };
        let mut effects = CmvsBasicEffects {
            alpha: Some(effect),
            ..Default::default()
        };
        assert!(
            effects
                .advance(1, &mut 255, &mut [0; 4], &mut [0; 2])
                .unwrap()
        );
        assert!(effects.alpha.is_none());
    }
}
