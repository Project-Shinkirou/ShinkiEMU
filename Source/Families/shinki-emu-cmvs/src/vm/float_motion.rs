//! CBasic DWORD33: native 4315C0/4316C0, serialized verbatim as tag1941.
use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsFloatSegment {
    pub next: i32,
    pub target: [u32; 3],
    pub curve: u32,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsFloatMotion {
    pub active: u32,
    pub origin: [u32; 3],
    pub start: u32,
    pub current: u32,
    pub elapsed_ms: u32,
    pub segments: [CmvsFloatSegment; 8],
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
fn sine(degrees: f32, radius: f32) -> f32 {
    let radians = (degrees / 180.0) * std::f32::consts::PI;
    hundredths((f64::from(radians).sin() * f64::from(radius)) as f32)
}

impl CmvsFloatMotion {
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if self.active == 0 {
            return Ok(());
        }
        // Native 431680/4316C0 retain raw IEEE words. Arithmetic applies native
        // CVTT where specified, and a boundary copies the literal target bits.
        validate_graph(self.current, 8, |i| {
            let segment = self.segments[i];
            (segment.next, segment.duration_ms)
        })
    }

    pub(crate) fn advance(
        &mut self,
        delta_ms: u32,
        position: &mut [u32; 3],
    ) -> Result<i32, CoreError> {
        self.validate()?;
        if self.active == 0 {
            return Ok(0);
        }
        let elapsed_ms = self.elapsed_ms.wrapping_add(delta_ms.min(200));
        self.elapsed_ms = elapsed_ms;
        loop {
            let segment = self.segments[self.current as usize];
            if self.elapsed_ms < segment.duration_ms {
                self.interpolate(segment, position);
                return Ok(0);
            }
            // Even relative curves store their literal target at a boundary (4316C0).
            self.origin = segment.target;
            *position = segment.target;
            if segment.next < 0 {
                return Ok(segment.next);
            }
            self.elapsed_ms = self.elapsed_ms.wrapping_sub(segment.duration_ms);
            self.current = segment.next as u32;
            if self.segments[self.current as usize].duration_ms > self.elapsed_ms {
                return Ok(0); // No interpolation on the newly-entered segment this tick.
            }
        }
    }

    fn interpolate(&self, segment: CmvsFloatSegment, position: &mut [u32; 3]) {
        let origin = self.origin.map(f32::from_bits);
        let target = segment.target.map(f32::from_bits);
        let mut value = position.map(f32::from_bits);
        let e = self.elapsed_ms as f32;
        let d = segment.duration_ms as f32;
        let relative = segment.curve & 0x8000 != 0;
        let mode = segment.curve as u8;
        let linear = |axis: usize| {
            let delta = if relative {
                target[axis]
            } else {
                target[axis] - origin[axis]
            };
            origin[axis] + hundredths(delta * e / d)
        };
        match mode {
            0 => value = std::array::from_fn(linear),
            1 => {
                value = std::array::from_fn(|axis| {
                    if relative {
                        origin[axis] + sine(e * 90.0 / d, target[axis])
                    } else {
                        // 43196C/43196F promote both rounded f32 operands before DIVSD.
                        let remaining = 1.0 - f64::from(e) / f64::from(d);
                        target[axis]
                            - cvtt(
                                f64::from(target[axis] - origin[axis])
                                    * (remaining * remaining)
                                    * 100.0,
                            ) as f32
                                / 100.0
                    }
                });
            }
            16 | 18 => {
                if !relative {
                    value = std::array::from_fn(linear);
                }
                let degrees = (self.elapsed_ms % 2000) as f32 * 360.0 / 2000.0;
                value[0] += sine(degrees, if mode == 16 { 0.2 } else { 1.0 });
            }
            32 | 33 | 34 | 49 => {
                let radius = match mode {
                    32 => 1.0,
                    33 => 0.25,
                    _ => (segment.curve >> 16) as f32 / 100.0,
                };
                value = origin;
                value[1] += sine(
                    (self.elapsed_ms % segment.duration_ms) as f32 * 180.0 / d,
                    radius,
                );
            }
            48 | 56 | 57 | 64 | 65 | 72 => {
                let oscillating = matches!(mode, 56 | 57 | 72);
                let elapsed = if oscillating {
                    self.elapsed_ms.wrapping_mul(segment.curve >> 28)
                } else {
                    self.elapsed_ms
                };
                let radius = if oscillating {
                    ((segment.curve >> 16) & 0xfff) as f32
                } else {
                    (segment.curve >> 16) as f32
                } / 100.0;
                let degrees = (elapsed % segment.duration_ms) as f32
                    * if mode >= 64 { 360.0 } else { 180.0 }
                    / d;
                let offset = sine(degrees, radius);
                value = origin;
                if matches!(mode, 64 | 72) {
                    value[0] += offset;
                } else if mode == 57 || mode == 65 {
                    value[1] += offset;
                } else {
                    value[1] -= offset;
                }
            }
            80 | 81 => {
                if !relative {
                    value = std::array::from_fn(|axis| {
                        if mode == 80 {
                            linear(axis)
                        } else {
                            origin[axis] + sine(e * 90.0 / d, target[axis] - origin[axis])
                        }
                    });
                }
                let degrees = (self.elapsed_ms % segment.duration_ms) as f32
                    * ((segment.curve >> 28) as f32 * 360.0)
                    / d;
                value[0] += sine(degrees, ((segment.curve >> 16) & 0xfff) as f32 / 100.0);
            }
            // 432314: unknown low bytes leave the actual current XYZ untouched.
            _ => return,
        }
        *position = value.map(f32::to_bits);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn motion(curve: u32, target: [f32; 3], duration_ms: u32) -> CmvsFloatMotion {
        let mut effect = CmvsFloatMotion {
            active: 1,
            ..Default::default()
        };
        effect.segments[0] = CmvsFloatSegment {
            next: -1,
            target: target.map(f32::to_bits),
            curve,
            duration_ms,
        };
        effect
    }

    #[test]
    fn native_float_linear_quadratic_and_relative_sine_have_distinct_progress() {
        for (curve, expected) in [(0, 4.0), (1, 6.0), (0x8000, 4.0), (0x8001, 5.65)] {
            let mut effect = motion(curve, [8.0, 0.0, 0.0], 400);
            let mut position = [0; 3];
            assert_eq!(effect.advance(999, &mut position).unwrap(), 0);
            assert_eq!(effect.elapsed_ms, 200);
            assert_eq!(f32::from_bits(position[0]), expected, "curve {curve}");
            assert_eq!(effect.advance(200, &mut position).unwrap(), -1);
            assert_eq!(position, [8.0f32.to_bits(), 0, 0]);
        }
    }

    #[test]
    fn native_float_oscillation_curves_write_the_recovered_axes() {
        for (curve, expected) in [
            (16, [0.19, 0.0, 0.0]),
            (18, [0.95, 0.0, 0.0]),
            (32, [0.0, 1.0, 0.0]),
            (33, [0.0, 0.25, 0.0]),
            ((100 << 16) | 34, [0.0, 1.0, 0.0]),
            ((100 << 16) | 48, [0.0, -1.0, 0.0]),
            ((100 << 16) | 49, [0.0, 1.0, 0.0]),
            ((1 << 28) | (100 << 16) | 56, [0.0, -1.0, 0.0]),
            ((1 << 28) | (100 << 16) | 57, [0.0, 1.0, 0.0]),
            ((100 << 16) | 64, [0.0; 3]),
            ((100 << 16) | 65, [0.0; 3]),
            ((1 << 28) | (100 << 16) | 72, [0.0; 3]),
            ((1 << 28) | (100 << 16) | 80, [0.0; 3]),
            ((1 << 28) | (100 << 16) | 81, [0.0; 3]),
        ] {
            let mut effect = motion(curve, [0.0; 3], 400);
            // 16/18 use the independent 2000ms period: start at300, advance200.
            if curve == 16 || curve == 18 {
                effect.elapsed_ms = 200;
            }
            let mut position = [0; 3];
            if curve == 16 || curve == 18 {
                effect.segments[0].duration_ms = 1000;
            }
            effect.advance(200, &mut position).unwrap();
            let actual = position.map(f32::from_bits);
            for axis in 0..3 {
                assert!(
                    (actual[axis] - expected[axis]).abs() < 0.011,
                    "curve {curve} axis {axis}: {actual:?}"
                );
            }
        }
    }

    #[test]
    fn native_float_segment_switch_preserves_remainder_without_interpolating_new_segment() {
        let mut effect = motion(0, [4.0, 0.0, 0.0], 100);
        effect.segments[0].next = 1;
        effect.segments[1] = CmvsFloatSegment {
            next: -2,
            target: [8.0f32.to_bits(), 0, 0],
            curve: 0,
            duration_ms: 100,
        };
        let mut position = [0; 3];
        assert_eq!(effect.advance(150, &mut position).unwrap(), 0);
        assert_eq!((effect.current, effect.elapsed_ms), (1, 50));
        assert_eq!(position[0], 4.0f32.to_bits());
        assert_eq!(effect.advance(25, &mut position).unwrap(), 0);
        assert_eq!(position[0], 7.0f32.to_bits());
        assert_eq!(effect.advance(25, &mut position).unwrap(), -2);
    }

    #[test]
    fn invalid_graph_fails_before_motion_changes() {
        for malformed in 0..2 {
            let mut effect = motion(0, [8.0, 0.0, 0.0], 400);
            match malformed {
                0 => effect.segments[0].next = 0,
                _ => effect.segments[0].next = 8,
            }
            if malformed == 0 {
                effect.segments[0].duration_ms = 0;
            }
            let before = effect.clone();
            let mut position = [0; 3];
            assert!(effect.advance(200, &mut position).is_err());
            assert_eq!(effect, before);
            assert_eq!(position, [0; 3]);
        }
    }

    #[test]
    fn unknown_segment_chains_into_known_motion_and_destroys_only_at_final_minus_two() {
        let mut effect = motion(0xffff_ff02, [4.0, 0.0, 0.0], 100);
        effect.origin = [f32::NAN.to_bits(); 3];
        effect.segments[0].next = 1;
        effect.segments[1] = CmvsFloatSegment {
            next: 2,
            target: [8.0f32.to_bits(), 0, 0],
            curve: 0,
            duration_ms: 100,
        };
        effect.segments[2] = CmvsFloatSegment {
            next: -2,
            target: [12.0f32.to_bits(), 0, 0],
            curve: 0x8002,
            duration_ms: 100,
        };
        let mut effects = CmvsBasicEffects {
            motion: Some(effect),
            ..Default::default()
        };
        let mut position = [0; 3];
        assert!(!effects.advance_motion(150, &mut position).unwrap());
        assert_eq!(position[0], 4.0f32.to_bits());
        assert!(!effects.advance_motion(25, &mut position).unwrap());
        assert_eq!(position[0], 7.0f32.to_bits());
        assert!(!effects.advance_motion(25, &mut position).unwrap());
        assert_eq!(position[0], 8.0f32.to_bits());
        assert!(!effects.advance_motion(50, &mut position).unwrap());
        assert_eq!(position[0], 8.0f32.to_bits());
        assert!(effects.advance_motion(50, &mut position).unwrap());
        assert_eq!(position[0], 12.0f32.to_bits());
        assert!(effects.motion.is_none());
    }

    #[test]
    fn relative_additive_curves_read_current_coordinates_without_consuming_stale_origin() {
        for (curve, expected_x) in [
            (0x8000 | 16, 4.06f32),
            (0x8000 | 18, 4.3),
            ((1 << 28) | (100 << 16) | 0x8000 | 80, 5.0),
            ((1 << 28) | (100 << 16) | 0x8000 | 81, 5.0),
        ] {
            let mut effect = motion(curve, [9.0, 7.0, 5.0], 400);
            effect.origin = [0x7fc0_1234; 3];
            let mut position = [4.0f32.to_bits(), 6.0f32.to_bits(), 8.0f32.to_bits()];
            effect.advance(100, &mut position).unwrap();
            assert_eq!(
                position,
                [expected_x.to_bits(), 6.0f32.to_bits(), 8.0f32.to_bits()]
            );
        }
    }

    #[test]
    fn high_word_is_full_amplitude_or_frequency_plus_masked_amplitude_by_mode() {
        for (mode, elapsed, expected) in [
            (34, 200, [0.0f32, 615.4, 0.0]),
            (49, 200, [0.0, 615.4, 0.0]),
            (48, 200, [0.0, -615.4, 0.0]),
            (64, 100, [615.4, 0.0, 0.0]),
            (65, 100, [0.0, 615.4, 0.0]),
            (56, 100, [0.0, -0.7, 0.0]),
            (57, 100, [0.0, 0.7, 0.0]),
            (72, 100, [-1.0, 0.0, 0.0]),
        ] {
            let mut effect = motion((0xf064 << 16) | 0x7f00 | mode, [0.0; 3], 400);
            let mut position = [0; 3];
            effect.advance(elapsed, &mut position).unwrap();
            assert_eq!(position, expected.map(f32::to_bits), "mode {mode}");
        }
    }

    #[test]
    fn inactive_native_float_owner_retains_unchecked_stale_words() {
        let effect = CmvsFloatMotion {
            current: u32::MAX,
            elapsed_ms: 981,
            origin: [f32::NAN.to_bits(); 3],
            ..Default::default()
        };
        effect.validate().unwrap();
        let mut saved = crate::system_save::slot_image::SavedBasic::default();
        let effects = CmvsBasicEffects {
            motion: Some(effect.clone()),
            ..Default::default()
        };
        saved.capture_effects(&effects).unwrap();
        assert_eq!(saved.restore_effects().unwrap(), effects);
        let mut advanced = effect.clone();
        let mut position = [0; 3];
        assert_eq!(advanced.advance(200, &mut position).unwrap(), 0);
        assert_eq!(advanced, effect);
        assert_eq!(position, [0; 3]);
    }
}
