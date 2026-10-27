//! Ordinary657's camera controller (452B70/452F00), without drawable geometry.
//! Native saves discard this transient controller; private snapshots keep its phase.
use super::native_scene_frame::{NativeSceneFrame, NativeSceneSound};
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeSceneKind51 {
    state: u32,
    base_position_words: [u32; 3],
    timer_ms: u32,
    event_fired: bool,
    pending_state: Option<u32>,
}

/// 452A60 divides degrees by the native 180.0 before multiplying by float PI.
/// Each arithmetic operation narrows to binary32 before the binary64 libm call.
fn radians(degrees: f32) -> f64 {
    if degrees == 0.0 {
        0.0
    } else {
        ((degrees / 180.0) * f32::from_bits(0x4049_0fdb)) as f64
    }
}

fn hundredths(value: f32) -> f32 {
    ((value * 100.0) as i32) as f32 / 100.0
}

/// Only the sine output of 452A60 is consumed by 453020/453250.
fn landing_sine(degrees: f32) -> f32 {
    hundredths((radians(degrees).sin() * f32::from_bits(0x3e4c_cccd) as f64) as f32)
}

impl CmvsNativeSceneKind51 {
    pub(crate) const SOUND_RESOURCES: [(u8, &[u8]); 3] = [
        (16, b"se001d.wav"),
        (17, b"se001e.wav"),
        (20, b"se003c.wav"),
    ];

    pub(crate) fn new(initial_clock: u32) -> Self {
        Self {
            state: 0,
            base_position_words: [0, 0x3fc0_0000, 0],
            timer_ms: initial_clock,
            event_fired: false,
            pending_state: None,
        }
    }

    /// 458AD0 writes pitch (+0C), 458AB0 writes yaw (+10); neither writes FOV.
    pub(crate) fn initial_frame(&self) -> NativeSceneFrame {
        NativeSceneFrame {
            position_words: Some([0, 0x3fc0_0000, 0]),
            pitch_word: Some(0x420c_0000),
            yaw_word: Some(0),
            ..NativeSceneFrame::default()
        }
    }
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if !matches!(self.state, 0 | 1 | 2 | 3 | 90)
            || self
                .pending_state
                .is_some_and(|state| !matches!(state, 1..=3))
            || self
                .base_position_words
                .iter()
                .any(|word| !f32::from_bits(*word).is_finite())
        {
            return Err(CoreError::invalid(
                "SHINKI_EMU_CMVS_NATIVE_SCENE_51",
                "invalid camera controller state",
            ));
        }
        Ok(())
    }

    /// 452B20 ignores selectors outside 1..=3 and overwrites the single queue slot.
    pub(crate) fn request(&mut self, selector: u32) {
        if matches!(selector, 1..=3) {
            self.pending_state = Some(selector);
        }
    }

    pub(crate) fn status(&self) -> u32 {
        if self.state == 0 {
            u32::MAX
        } else {
            self.state
        }
    }

    fn transition(&mut self, default_state: u32) {
        self.state = self.pending_state.take().unwrap_or(default_state);
        self.timer_ms = 0;
        self.event_fired = false;
    }

    fn position_frame(&self, vertical: f32, forward: f32) -> NativeSceneFrame {
        let [x, y, z] = self.base_position_words.map(f32::from_bits);
        NativeSceneFrame {
            position_words: Some([
                (x + 0.0).to_bits(),
                (y + vertical).to_bits(),
                (z + forward).to_bits(),
            ]),
            ..NativeSceneFrame::default()
        }
    }

    /// 419AA0 is capped only inside the three movement callbacks. Cooldown90
    /// adds the full unsigned frame span; all native timer additions wrap.
    pub(crate) fn advance(
        &mut self,
        frame_delta_ms: u32,
        volume_word: u32,
    ) -> Result<NativeSceneFrame, CoreError> {
        let frame = match self.state {
            0 => {
                if self.pending_state.is_some() {
                    self.transition(0);
                }
                NativeSceneFrame::default()
            }
            90 => {
                self.timer_ms = self.timer_ms.wrapping_add(frame_delta_ms);
                if self.pending_state.is_some() || self.timer_ms > 600 {
                    self.transition(1);
                }
                NativeSceneFrame::default()
            }
            1 | 2 => {
                self.timer_ms = self.timer_ms.wrapping_add(frame_delta_ms.min(200));
                self.advance_step(volume_word)
            }
            3 => {
                self.timer_ms = self.timer_ms.wrapping_add(frame_delta_ms.min(200));
                self.advance_descent(volume_word)
            }
            _ => NativeSceneFrame::default(), // 452F00's native default arm.
        };
        Ok(frame)
    }

    fn advance_step(&mut self, volume_word: u32) -> NativeSceneFrame {
        let fast = self.state == 2;
        let (walk_ms, landing_ms, finish_ms, step_word, slot) = if fast {
            (800_u32, 50.0_f32, 900_u32, 0x3ecc_cccd, 17)
        } else {
            (1000_u32, 250.0_f32, 1500_u32, 0x3e4c_cccd, 16)
        };
        let step = f32::from_bits(step_word);
        if self.timer_ms >= finish_ms {
            self.base_position_words[2] =
                (f32::from_bits(self.base_position_words[2]) + step).to_bits();
            let frame = NativeSceneFrame {
                position_words: Some(self.base_position_words),
                ..NativeSceneFrame::default()
            };
            self.transition(if fast { 2 } else { 90 });
            return frame;
        }

        let elapsed = self.timer_ms as f32;
        let forward = if self.timer_ms < walk_ms {
            (elapsed * step) / walk_ms as f32
        } else {
            step
        };
        let vertical = if self.timer_ms < walk_ms {
            let degrees = (elapsed * 180.0) / walk_ms as f32;
            // State2 multiplies by the promoted native float 0.8 before narrowing.
            let sine = radians(degrees).sin();
            let sine = if fast {
                sine * f32::from_bits(0x3f4c_cccd) as f64
            } else {
                sine
            };
            hundredths(sine as f32) / 10.0
        } else {
            let landing_start = walk_ms as f32;
            let circle_start = landing_start + landing_ms;
            if elapsed < circle_start {
                0.0 - (((elapsed - landing_start) * f32::from_bits(0x3ca3_d70a)) / landing_ms)
            } else {
                let degrees = ((elapsed - circle_start) * 90.0) / landing_ms;
                landing_sine(degrees) / 10.0 - f32::from_bits(0x3ca3_d70a)
            }
        };
        let mut frame = self.position_frame(vertical, forward);
        if self.timer_ms >= walk_ms && !self.event_fired {
            frame.audio[0] = Some(NativeSceneSound::PlayStatic {
                slot,
                volume: volume_word,
            });
            self.event_fired = true;
        }
        frame
    }

    fn advance_descent(&mut self, volume_word: u32) -> NativeSceneFrame {
        if self.timer_ms >= 2000 {
            self.base_position_words[1] = 0xbf00_0000;
            self.base_position_words[2] =
                (f32::from_bits(self.base_position_words[2]) + 1.0).to_bits();
            // 452ED0 resets to idle without consuming the pending selector.
            self.state = 0;
            self.timer_ms = 0;
            self.event_fired = false;
            return NativeSceneFrame {
                position_words: Some(self.base_position_words),
                pitch_word: Some(0x42a0_0000),
                audio: [
                    Some(NativeSceneSound::StopStream { slot: 10 }),
                    None,
                    None,
                    None,
                ],
                ..NativeSceneFrame::default()
            };
        }
        let elapsed = self.timer_ms as f32;
        let (vertical, forward, pitch_word) = if self.timer_ms < 1000 {
            let degrees = (elapsed * 180.0) / 1000.0;
            (
                hundredths(radians(degrees).sin() as f32) / 10.0,
                (elapsed * 0.5) / 1000.0,
                None,
            )
        } else {
            let elapsed = elapsed - 1000.0;
            let degrees = 90.0 - (elapsed * 90.0) / 1000.0;
            (
                hundredths((radians(degrees).sin() * 2.0) as f32) - 2.0,
                ((elapsed * 0.5) / 1000.0) + 0.5,
                Some((((elapsed * 25.0) / 1000.0) + 35.0).to_bits()),
            )
        };
        let mut frame = self.position_frame(vertical, forward);
        frame.pitch_word = pitch_word;
        if self.timer_ms >= 1800 && !self.event_fired {
            frame.audio[0] = Some(NativeSceneSound::PlayStatic {
                slot: 20,
                volume: volume_word,
            });
            frame.audio[1] = Some(NativeSceneSound::StopStream { slot: 10 });
            self.event_fired = true;
        }
        frame
    }
}

#[cfg(test)]
#[path = "native_scene_kind51/tests.rs"]
mod tests;
