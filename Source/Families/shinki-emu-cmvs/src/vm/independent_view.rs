//! Native 457E20/4583E0 view owners. These are separate from plane cameras.
use super::*;
use crate::system_save::slot_projection::SavedView;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsIndependentViewMotion {
    pub active: u32,
    pub curve: u32,
    pub target_position: [u32; 3],
    /// Only pitch/yaw participate; roll is never animated by 4583E0.
    pub target_angles: [u32; 2],
    pub duration_ms: u32,
    pub move_position: u32,
    pub move_angles: u32,
    pub origin_position: [u32; 3],
    pub origin_angles: [u32; 2],
    pub elapsed_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsIndependentView {
    pub enabled: u32,
    /// IEEE754 words preserve native fields and private snapshot equality.
    pub position_words: [u32; 3],
    /// Pitch, yaw, roll in degrees (native DWORD3,4,5).
    pub angle_words: [u32; 3],
    pub field_of_view_word: u32,
    pub motion: CmvsIndependentViewMotion,
}

impl Default for CmvsIndependentView {
    fn default() -> Self {
        Self {
            enabled: 0,
            position_words: [0, 1.0_f32.to_bits(), 0],
            angle_words: [0; 3],
            field_of_view_word: 45.0_f32.to_bits(),
            motion: CmvsIndependentViewMotion::default(),
        }
    }
}

fn invalid() -> CoreError {
    CoreError::invalid(
        "SHINKI_EMU_CMVS_INDEPENDENT_VIEW",
        "native view contains non-finite geometry",
    )
}

pub(crate) fn default_pair() -> [CmvsIndependentView; 2] {
    let mut pair = [
        CmvsIndependentView::default(),
        CmvsIndependentView::default(),
    ];
    pair[0].enabled = 1;
    pair
}

fn finite(words: &[u32]) -> bool {
    words.iter().all(|word| f32::from_bits(*word).is_finite())
}

impl CmvsIndependentView {
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if !finite(&self.position_words)
            || !finite(&self.angle_words)
            || !f32::from_bits(self.field_of_view_word).is_finite()
        {
            return Err(invalid());
        }
        let motion = &self.motion;
        if motion.active != 0
            && (motion.move_position != 0
                && (!finite(&motion.origin_position) || !finite(&motion.target_position))
                || motion.move_angles != 0
                    && (!finite(&motion.origin_angles) || !finite(&motion.target_angles)))
        {
            return Err(invalid());
        }
        Ok(())
    }

    /// 458A10 cancels without changing the visible instant unless snap=true.
    pub(crate) fn commit(&mut self, snap: bool) {
        if self.motion.active == 0 {
            return;
        }
        self.motion.active = 0;
        if snap {
            if self.motion.move_position != 0 {
                self.position_words = self.motion.target_position;
            }
            if self.motion.move_angles != 0 {
                self.angle_words[..2].copy_from_slice(&self.motion.target_angles);
            }
        }
    }

    fn start(&mut self, command: u16, values: &[u32]) {
        let mut motion = self.motion.clone();
        motion.active = 1;
        motion.curve = values[0];
        motion.origin_position = self.position_words;
        motion.origin_angles = [self.angle_words[0], self.angle_words[1]];
        motion.move_position = u32::from(command != 628);
        motion.move_angles = u32::from(command != 627);
        let mut index = 1;
        if motion.move_position != 0 {
            motion
                .target_position
                .copy_from_slice(&values[index..index + 3]);
            index += 3;
        }
        if motion.move_angles != 0 {
            motion
                .target_angles
                .copy_from_slice(&values[index..index + 2]);
            index += 2;
        }
        motion.duration_ms = values[index];
        motion.elapsed_ms = 0;
        self.motion = motion;
    }

    fn advance(&mut self, delta_ms: u32) {
        if self.motion.active == 0 {
            return;
        }
        self.motion.elapsed_ms = self.motion.elapsed_ms.wrapping_add(delta_ms);
        if self.motion.elapsed_ms >= self.motion.duration_ms {
            self.commit(true);
            return;
        }
        // Unknown curves leave the current values untouched until completion.
        if self.motion.curve > 2 {
            return;
        }
        let elapsed = self.motion.elapsed_ms as f32;
        let duration = self.motion.duration_ms as f32;
        let curve = self.motion.curve;
        let interpolate = |origin: u32, target: u32| {
            let origin = f32::from_bits(origin);
            let delta = f32::from_bits(target) - origin;
            let offset = match curve {
                0 => hundredths(delta * elapsed / duration),
                1 => sine(elapsed * 90.0 / duration, delta, false),
                2 => delta - sine(hundredths(elapsed * 90.0 / duration), delta, true),
                _ => unreachable!(),
            };
            (origin + offset).to_bits()
        };
        if self.motion.move_position != 0 {
            self.position_words = std::array::from_fn(|i| {
                interpolate(
                    self.motion.origin_position[i],
                    self.motion.target_position[i],
                )
            });
        }
        if self.motion.move_angles != 0 {
            for i in 0..2 {
                self.angle_words[i] =
                    interpolate(self.motion.origin_angles[i], self.motion.target_angles[i]);
            }
        }
    }
}

fn cvtt(value: f64) -> i32 {
    if !value.is_finite() || !(-2147483648.0..2147483648.0).contains(&value) {
        i32::MIN
    } else {
        value as i32
    }
}
fn hundredths(value: f32) -> f32 {
    cvtt(f64::from(value * 100.0)) as f32 / 100.0
}
fn sine(degrees: f32, radius: f32, cosine: bool) -> f32 {
    let radians = f64::from((degrees / 180.0) * std::f32::consts::PI);
    let factor = if cosine { radians.cos() } else { radians.sin() };
    hundredths((factor * f64::from(radius)) as f32)
}

pub(crate) fn validate_pair(pair: &[CmvsIndependentView; 2]) -> Result<(), CoreError> {
    for view in pair {
        view.validate()?;
    }
    Ok(())
}

pub(crate) fn tick(vm: &mut CmvsPs2aVmState, delta_ms: u32) -> Result<(), CoreError> {
    validate_pair(&vm.independent_views)?;
    for view in &mut vm.independent_views {
        view.advance(delta_ms);
    }
    Ok(())
}

/// 458A60 resets body/worker for both owners, preserving native DWORD23.
pub(crate) fn reset(vm: &mut CmvsPs2aVmState) {
    for view in &mut vm.independent_views {
        let enabled = view.enabled;
        *view = CmvsIndependentView {
            enabled,
            ..Default::default()
        };
    }
}

pub(crate) fn pending(vm: &CmvsPs2aVmState) -> bool {
    vm.independent_views[0].motion.active != 0
}

pub(crate) fn commit(vm: &mut CmvsPs2aVmState, snap: bool) {
    vm.independent_views[0].commit(snap);
}

pub(crate) fn execute(
    vm: &mut CmvsPs2aVmState,
    command: u16,
    values: &[u32],
) -> Result<(), CoreError> {
    let count = match command {
        609..=612 | 624 | 625 => 1,
        608 => 3,
        626 => 0,
        627 => 5,
        628 => 4,
        629 => 7,
        _ => usize::MAX,
    };
    if values.len() != count {
        return Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
            "invalid independent view operand count",
        ));
    }
    if command == 626 {
        let active = vm.independent_views[0].motion.active != 0;
        vm.write_interpreter_word(81220, u32::from(active));
        return Ok(());
    }
    let view = &mut vm.independent_views[0];
    // Construct before publishing so invalid commands preserve the live owner.
    let mut candidate = view.clone();
    match command {
        608 => candidate.position_words.copy_from_slice(values),
        609 => candidate.angle_words[0] = values[0],
        610 => candidate.angle_words[1] = values[0],
        611 => candidate.angle_words[2] = values[0],
        612 => candidate.field_of_view_word = values[0],
        624 => candidate.commit(values[0] != 0),
        625 => return Ok(()), // Native A000/C004 gate is handled before normal dispatch.
        627..=629 => candidate.start(command, values),
        _ => unreachable!(),
    }
    candidate.validate()?;
    *view = candidate;
    Ok(())
}

/// 458380 saves the current visible values, even while its worker is active.
pub(crate) fn capture_pair(vm: &CmvsPs2aVmState) -> Result<[SavedView; 2], CoreError> {
    let pair = &vm.independent_views;
    validate_pair(pair)?;
    Ok(std::array::from_fn(|i| SavedView {
        enabled: pair[i].enabled,
        position: pair[i].position_words.map(f32::from_bits),
        angles: pair[i].angle_words.map(f32::from_bits),
        field_of_view: f32::from_bits(pair[i].field_of_view_word),
    }))
}

/// 458B70 discards the live worker. No projection payload is stashed.
pub(crate) fn restore_pair(
    vm: &mut CmvsPs2aVmState,
    saved: &[SavedView; 2],
) -> Result<(), CoreError> {
    let pair = std::array::from_fn(|i| CmvsIndependentView {
        enabled: saved[i].enabled,
        position_words: saved[i].position.map(f32::to_bits),
        angle_words: saved[i].angles.map(f32::to_bits),
        field_of_view_word: saved[i].field_of_view.to_bits(),
        motion: CmvsIndependentViewMotion::default(),
    });
    validate_pair(&pair)?;
    vm.independent_views = pair;
    Ok(())
}

#[cfg(test)]
#[path = "independent_view_tests.rs"]
mod tests;
