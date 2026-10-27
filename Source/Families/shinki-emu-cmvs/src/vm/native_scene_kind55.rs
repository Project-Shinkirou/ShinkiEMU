//! Ordinary661 / 448320: three atlas-animated CBasic2 plane sprites.
//! Evidence: 449D50..44CB05, exported in .tmp/cmvs-port/ida_kind55_restore.
//! Owner clocks use the uncapped native delta; CBasic controllers cap it at200.
//! Rotation432840 retains signed hundredths, including exact negative completion targets.
use super::native_scene_frame::{NativeSceneFrame, NativeSceneSound};
use super::*;

fn words(values: [f32; 3]) -> [u32; 3] {
    values.map(f32::to_bits)
}
fn floats(values: [u32; 3]) -> [f32; 3] {
    values.map(f32::from_bits)
}
fn hundredth(value: f32) -> f32 {
    (value * 100.0) as i32 as f32 / 100.0
}
fn sine(degrees: f32, radius: f32) -> f32 {
    let radians = degrees / 180.0_f32 * f32::from_bits(0x4049_0fdb);
    hundredth(((radians as f64).sin() * radius as f64) as f32)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct Motion {
    start: [u32; 3],
    target: [u32; 3],
    elapsed: u32,
    duration: u32,
    ease: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct RotationStep {
    next: i32,
    mode: u32,
    duration: u32,
    target: i32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct Rotation {
    steps: [Option<RotationStep>; 5],
    index: usize,
    elapsed: u32,
    start: i32,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct Sprite {
    initialized: bool,
    position: [u32; 3],
    atlas_index: u32,
    atlas_clock: u32,
    // CBasic DWORD20: raw bits of signed hundredths of degrees.
    rotation: u32,
    motion: Option<Motion>,
    rotation_controller: Option<Rotation>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct Actor {
    target: [u32; 3],
    baseline: [u32; 3],
    phase: u32,
    clock: u32,
    segment: u32,
    sound: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeSceneKind55 {
    sprites: [Sprite; 3],
    actors: [Actor; 3],
    state: u32,
    transition_clock: u32,
    previous_segment: u32,
    finish_clock: u32,
    pending: u32,
}

impl Sprite {
    fn tick(&mut self, delta: u32) {
        if !self.initialized {
            return;
        }
        let delta = delta.min(200);
        if let Some(rotation) = &mut self.rotation_controller {
            rotation.elapsed = rotation.elapsed.wrapping_add(delta);
            let step = rotation.steps[rotation.index].unwrap();
            if rotation.elapsed >= step.duration {
                loop {
                    let step = rotation.steps[rotation.index].unwrap();
                    if (step.mode as u8) < 128 {
                        rotation.start = step.target;
                        self.rotation = step.target as u32;
                    }
                    if step.next < 0 {
                        self.rotation_controller = None;
                        break;
                    }
                    rotation.elapsed -= step.duration;
                    rotation.index = step.next as usize;
                    if rotation.elapsed < rotation.steps[rotation.index].unwrap().duration {
                        break;
                    }
                }
            } else if matches!(step.mode as u8, 0 | 1) {
                let reverse = step.mode & 0x8000 != 0;
                let mut start = rotation.start;
                let mut target = step.target;
                if reverse {
                    if target >= start {
                        start += 36000;
                    }
                } else if target <= start {
                    target += 36000;
                }
                let distance = if reverse {
                    start - target
                } else {
                    target - start
                };
                let amount = if step.mode as u8 == 0 {
                    (rotation.elapsed.wrapping_mul(distance as u32) / step.duration) as i32
                } else {
                    let angle = rotation.elapsed.wrapping_mul(9000) / step.duration;
                    //419B50 converts hundredths of degrees to radians in double precision.
                    ((angle as f64 / 18000.0 * std::f64::consts::PI).sin() * distance as f64) as i32
                };
                self.rotation = if reverse {
                    (rotation.start - amount + 36000) % 36000
                } else {
                    (rotation.start + amount) % 36000
                } as u32;
            }
        }
        self.atlas_clock = self.atlas_clock.wrapping_add(delta);
        let steps = self.atlas_clock / 100;
        self.atlas_clock %= 100;
        self.atlas_index = self.atlas_index / 4 * 4 + (self.atlas_index % 4 + steps) % 4;
        if let Some(motion) = &mut self.motion {
            motion.elapsed = motion.elapsed.wrapping_add(delta);
            if motion.elapsed >= motion.duration {
                self.position = motion.target;
                self.motion = None;
            } else {
                let start = floats(motion.start);
                let target = floats(motion.target);
                let elapsed = motion.elapsed as f32;
                let duration = motion.duration as f32;
                self.position = words(std::array::from_fn(|i| {
                    if motion.ease {
                        let remain = (1.0_f64 - (elapsed / duration) as f64).powi(2);
                        target[i]
                            - ((target[i] - start[i]) as f64 * remain * 100.0) as i32 as f32 / 100.0
                    } else {
                        start[i] + hundredth((target[i] - start[i]) * elapsed / duration)
                    }
                }));
            }
        }
    }
}

impl CmvsNativeSceneKind55 {
    pub const RESOURCES: [&'static [u8]; 3] = [b"ci001a.pb3", b"ci001b.pb3", b"ci001c.pb3"];
    pub const SOUND_RESOURCES: [(u8, &'static [u8]); 2] =
        [(16, b"SE035a.ogg"), (17, b"SE037b.ogg")];
    pub const RETIRED_STATIC_SOUNDS: u32 = (1 << 16) | (1 << 17);

    pub(crate) fn new() -> Result<Self, CoreError> {
        Ok(Self {
            sprites: std::array::from_fn(|_| Sprite::default()),
            actors: std::array::from_fn(|_| Actor::default()),
            state: 0,
            transition_clock: 0,
            previous_segment: 0,
            finish_clock: 0,
            pending: 0,
        })
    }
    pub(crate) fn initial_frame(&self) -> NativeSceneFrame {
        NativeSceneFrame::default()
    }
    pub(crate) fn status(&self) -> u32 {
        if self.pending == u32::MAX {
            self.state
        } else {
            u32::MAX
        }
    }
    ///449F00 consumes selector only; both extra ordinary737 operands are ignored.
    pub(crate) fn request(&mut self, selector: u32) {
        match selector {
            1..=4 | 6..=8 | 10..=14 | 17 => self.pending = selector,
            5 => self.select_atlas(1, 4),
            15 => self.select_atlas(1, 8),
            16 => self.select_atlas(0, 20),
            _ => {}
        }
    }
    fn select_atlas(&mut self, index: usize, entry: u32) {
        if self.sprites[index].initialized {
            self.sprites[index].atlas_index = entry;
            self.sprites[index].atlas_clock = 0;
        }
    }
    fn set_position(&mut self, index: usize, value: [f32; 3]) {
        self.actors[index].target = words(value);
        self.sprites[index].position = words(value);
    }
    fn move_to(&mut self, index: usize, target: [f32; 3], ease: bool, duration: u32) {
        let sprite = &mut self.sprites[index];
        sprite.motion = Some(Motion {
            start: sprite.position,
            target: words(target),
            elapsed: 0,
            duration,
            ease,
        });
        self.actors[index].target = words(target);
    }
    fn setup(&mut self, index: usize) {
        self.sprites[index] = Sprite {
            initialized: true,
            ..Sprite::default()
        };
        let position = match index {
            0 => [-4.0, 1.5, 7.0],
            1 => [3.3, 4.0, 5.0],
            _ => [3.0, 3.5, 3.0],
        };
        self.set_position(index, position);
        if index == 1 {
            self.rotate(1, &[(1, 1, 500, 5), (2, 32769, 1000, -5), (1, 1, 1000, 5)]);
        }
    }
    fn rotate(&mut self, index: usize, steps: &[(i32, u32, u32, i32)]) {
        let mut entries = [None; 5];
        for (entry, &(next, mode, duration, target)) in entries.iter_mut().zip(steps) {
            *entry = Some(RotationStep {
                next,
                mode,
                duration,
                target: 100 * (target % 360),
            });
        }
        self.sprites[index].rotation_controller = Some(Rotation {
            steps: entries,
            index: 0,
            elapsed: 0,
            // Every kind55 call to432760 supplies zero, not the current CBasic angle.
            start: 0,
        });
    }
    fn stop_rotation(&mut self, index: usize) {
        if let Some(rotation) = self.sprites[index].rotation_controller.take() {
            self.sprites[index].rotation = rotation.steps[rotation.index].unwrap().target as u32;
        }
        self.sprites[index].rotation = 0;
    }
    fn sound(frame: &mut NativeSceneFrame, slot: u8, volume: u32) {
        let event = frame
            .audio
            .iter_mut()
            .find(|event| event.is_none())
            .expect("kind55 emits at most two sounds per callback");
        *event = Some(NativeSceneSound::PlayStatic { slot, volume });
    }
    fn recoil_rotation(&mut self, reverse: bool) {
        let flag = if reverse { 0x8000 } else { 0 };
        self.rotate(
            1,
            &[
                (1, flag, 400, 0),
                (2, flag, 400, 0),
                (3, 1, 500, 5),
                (4, 32769, 1000, -5),
                (3, 1, 1000, 5),
            ],
        );
    }
    fn begin_recoil(&mut self, selector: u32, volume: u32, frame: &mut NativeSceneFrame) {
        self.state = 95;
        self.transition_clock = 0;
        let (start, target, second) = match selector {
            7 => ([-6.0, 4.0, 11.0], [1.0, 2.5, -1.0], [-0.5, 3.2, 5.0]),
            12 => ([6.0, 4.5, 11.0], [-1.0, 2.5, -1.0], [-1.0, 3.2, 5.0]),
            _ => ([6.0, 4.5, 11.0], [-6.0, 2.5, -1.0], [-1.5, 3.8, 5.0]),
        };
        if selector == 7 {
            self.select_atlas(0, 4);
        }
        self.set_position(0, start);
        self.move_to(0, target, false, 300);
        self.recoil_rotation(selector != 7);
        self.move_to(1, second, true, 1000);
        if selector == 12 {
            Self::sound(frame, 16, volume);
        }
        Self::sound(frame, 17, volume);
    }

    ///44AE40 executes before44A0A0 advances each CBasic controller.
    pub(crate) fn advance(
        &mut self,
        delta_ms: u32,
        volume: u32,
    ) -> Result<NativeSceneFrame, CoreError> {
        self.validate()?;
        let mut frame = NativeSceneFrame::default();
        if self.state == 0 && self.pending != 0 && self.pending != u32::MAX {
            self.state = self.pending;
            self.pending = u32::MAX;
        }
        match self.state {
            1..=3 => {
                let index = (self.state - 1) as usize;
                self.state = 90 + index as u32;
                self.transition_clock = 0;
                self.setup(index);
                let (target, ease, duration) = match index {
                    0 => ([-0.7, 3.2, 5.0], true, 500),
                    1 => ([1.0, 4.0, 5.0], false, 3000),
                    _ => ([-1.6, 2.5, 5.0], true, 1000),
                };
                self.move_to(index, target, ease, duration);
            }
            4 => {
                self.state = 90;
                self.transition_clock = 0;
                self.move_to(0, [1.0, 2.8, -1.0], false, 500);
                self.move_to(2, [-2.5, 3.0, -2.0], false, 500);
                self.move_to(1, [-1.0, 2.7, 5.0], false, 5000);
            }
            6 => {
                self.state = 95;
                self.transition_clock = 0;
                self.select_atlas(0, 8);
                self.set_position(0, [6.0, 4.0, -1.0]);
                self.move_to(0, [-6.0, 4.7, 11.0], false, 1000);
                self.rotate(0, &[(-1, 0, 1000, 0)]);
                Self::sound(&mut frame, 17, volume);
            }
            7 | 12 | 13 => self.begin_recoil(self.state, volume, &mut frame),
            8 => {
                for index in 0..2 {
                    self.actors[index].phase = 0;
                    self.actors[index].clock = 0;
                }
                self.state = 9;
                self.walk(delta_ms, volume, &mut frame);
            }
            9 => self.walk(delta_ms, volume, &mut frame),
            10 => self.attack(volume, &mut frame),
            14 => {
                self.state = 20;
                for index in 0..2 {
                    self.actors[index].phase = 0;
                    self.actors[index].clock = 0;
                    self.actors[index].segment = 0;
                    self.actors[index].sound = 0;
                }
                self.previous_segment = u32::MAX;
                self.orbit(delta_ms, volume, &mut frame);
            }
            17 | 20 => self.orbit(delta_ms, volume, &mut frame),
            90..=92 => {
                if self.sprites[(self.state - 90) as usize].motion.is_none() {
                    self.state = 0;
                }
            }
            95 if self.sprites.iter().all(|sprite| sprite.motion.is_none()) => self.state = 0,
            _ => {}
        }
        for sprite in &mut self.sprites {
            sprite.tick(delta_ms);
        }
        Ok(frame)
    }

    fn walk(&mut self, delta: u32, volume: u32, frame: &mut NativeSceneFrame) {
        if self.actors[0].phase == 0 {
            self.set_position(0, [6.0, 3.3, 5.0]);
            self.move_to(0, [0.5, 3.3, 5.0], false, 800);
            self.actors[0].phase = 1;
            self.actors[0].sound = 0;
            self.actors[1].sound = 0;
        }
        if self.actors[0].phase == 1 && self.sprites[0].motion.is_none() {
            self.actors[0].phase = 2;
            self.rotate(
                0,
                &[
                    (1, 0, 100, 5),
                    (2, 0x8000, 100, 0),
                    (3, 0, 100, 5),
                    (4, 0x8000, 100, 0),
                    (0, 255, 300, 0),
                ],
            );
            self.actors[0].baseline = self.actors[0].target;
        }
        if self.actors[0].phase == 2 {
            self.actors[0].clock = self.actors[0].clock.wrapping_add(delta);
            let clock = self.actors[0].clock;
            let phase = clock % 700;
            let moving = f32::from_bits(self.actors[0].target[0]) > -1.0;
            if phase < 100 && self.actors[1].sound == 0 {
                Self::sound(frame, 16, volume);
                self.actors[1].sound = 1;
            }
            if phase.wrapping_sub(200) <= 99 && self.actors[1].sound == 1 {
                Self::sound(frame, 16, volume);
                self.actors[1].sound = 0;
            }
            if moving {
                let first = if phase < 100 {
                    phase as f32 * 0.1 / 100.0
                } else {
                    0.1
                };
                let second = if phase.wrapping_sub(200) <= 99 {
                    (phase - 200) as f32 * 0.1 / 100.0
                } else {
                    0.1
                };
                let distance = clock.wrapping_div(700) as f32 * 0.2 + first + second;
                let base = floats(self.actors[0].baseline);
                self.set_position(0, [base[0] - distance, base[1], base[2]]);
            }
            if self.pending == 10 {
                self.state = 10;
                self.pending = u32::MAX;
                self.stop_rotation(0);
                let p = floats(self.actors[0].target);
                self.move_to(0, [p[0] - 3.0, p[1] + 3.0, p[2]], false, 300);
                self.actors[0].phase = 0;
                self.actors[0].clock = 0;
                self.actors[1].segment = 0;
                self.select_atlas(0, 16);
            }
        }
        if self.state != 10 && self.actors[1].phase == 0 {
            let p = floats(self.actors[0].target);
            self.set_position(1, [p[0] - 1.0, p[1] - 0.1, 5.0]);
        }
    }

    fn attack(&mut self, volume: u32, frame: &mut NativeSceneFrame) {
        let phase = self.actors[0].phase;
        if phase <= 3 && self.sprites[0].motion.is_none() {
            let p = floats(self.actors[1].target);
            if matches!(phase, 0 | 2) && self.pending == 11 {
                self.pending = u32::MAX;
                self.set_position(0, [6.0, 0.5, -1.0]);
                self.move_to(0, [-6.0, 3.5, 11.0], false, 300);
                self.recoil_rotation(false);
                self.move_to(1, [-0.5, 2.8, 5.0], true, 1000);
                self.state = 95;
                Self::sound(frame, 17, volume);
            } else {
                match phase {
                    0 | 2 => {
                        let offset = if phase == 0 { -4.0 } else { 4.0 };
                        self.set_position(0, [p[0] + offset, p[1] + 5.0, p[2]]);
                        self.move_to(0, [p[0], p[1] + 0.8, p[2]], false, 300);
                        self.actors[0].phase = phase + 1;
                    }
                    _ => {
                        let offset = if phase == 1 { 4.0 } else { -4.0 };
                        self.move_to(0, [p[0] + offset, p[1] + 5.0, p[2]], false, 300);
                        self.actors[0].phase = if phase == 1 { 2 } else { 0 };
                        self.actors[1].segment = 1;
                    }
                }
            }
        }
        //The native test is state !=11, including the95 transition above.
        if self.state != 11 && self.actors[1].phase == 0 && self.actors[1].segment != 0 {
            Self::sound(frame, 16, volume);
            let p = floats(self.actors[1].target);
            if p[1] > 2.0 {
                self.set_position(1, [p[0], p[1] - 0.1, p[2]]);
            }
            self.actors[1].segment = 0;
        }
    }

    fn orbit(&mut self, delta: u32, volume: u32, frame: &mut NativeSceneFrame) {
        self.actors[0].clock = self.actors[0].clock.wrapping_add(delta);
        if self.state == 17 {
            if self.actors[0].sound == 0 {
                Self::sound(frame, 17, volume);
                self.actors[0].sound = 1;
            }
            self.actors[1].segment = 1;
            self.finish_clock = self.finish_clock.wrapping_add(delta);
            let t = self.finish_clock;
            self.set_position(
                0,
                if t >= 300 {
                    [0.0, 3.1, 0.5]
                } else {
                    [0.0, 3.05, 11.0 - t as f32 * 10.5 / 300.0]
                },
            );
        } else {
            let clock = self.actors[0].clock;
            let segment = clock % 2000 / 500;
            let t = (clock % 500) as f32;
            if segment != self.actors[0].segment {
                Self::sound(frame, 17, volume);
            }
            let x = t * 9.0 / 500.0 - if matches!(segment, 0 | 2) { 6.0 } else { 3.0 };
            let y = if segment < 2 {
                6.5 - t * 6.0 / 500.0
            } else {
                t * 6.0 / 500.0 + 0.5
            };
            let z = if matches!(segment, 0 | 2) {
                11.0 - t * 12.0 / 500.0
            } else {
                t * 12.0 / 500.0 - 1.0
            };
            self.set_position(0, [x, y, z]);
            self.actors[0].segment = segment;
        }
        self.actors[1].clock = self.actors[1].clock.wrapping_add(delta);
        if self.state == 17 {
            let t = self.finish_clock;
            let base = floats(self.actors[0].baseline);
            if t < 500 {
                if self.actors[1].sound < 2 {
                    Self::sound(frame, 16, volume);
                    self.actors[1].sound = 3;
                }
                self.sprites[1].rotation = (t.wrapping_mul(720) / 500 % 360) * 100;
                self.set_position(
                    1,
                    [
                        base[0] + t as f32 * 4.0 / 500.0,
                        base[1] + t as f32 / 500.0,
                        base[2] - t as f32 * 6.0 / 500.0,
                    ],
                );
            } else {
                self.set_position(1, [base[0] + 4.0, base[1] + 1.0, base[2] - 6.0]);
            }
        } else {
            if self.actors[1].phase == 0 && self.actors[1].clock >= 300 {
                self.actors[1].clock -= 300;
                self.stop_rotation(1);
                self.actors[1].phase = 1;
            }
            if self.actors[1].phase == 1 {
                let clock = self.actors[1].clock % 1000;
                let reverse = clock >= 500;
                if (!reverse && self.actors[1].sound == 0) || (reverse && self.actors[1].sound == 1)
                {
                    Self::sound(frame, 16, volume);
                    self.actors[1].sound ^= 1;
                }
                let t = (clock % 500) as f32;
                let travel = t * 3.0 / 500.0;
                self.set_position(
                    1,
                    [
                        if reverse { 1.5 - travel } else { travel - 1.5 },
                        sine(t * 180.0 / 500.0, 0.8) + 3.8,
                        5.0,
                    ],
                );
                let angle = (clock % 500).wrapping_mul(720) / 500 % 360;
                self.sprites[1].rotation = (if reverse { 360 - angle } else { angle }) % 360 * 100;
            }
        }
        if self.pending == 17 {
            if self.previous_segment == u32::MAX {
                self.previous_segment = self.actors[0].segment;
            }
            if self.previous_segment != self.actors[0].segment {
                self.previous_segment = self.actors[0].segment;
                self.state = 17;
                self.finish_clock = self.actors[0].clock % 500;
                self.actors[1].baseline = self.actors[1].target;
                //44CAF6 copies actor1's current target to actor1 baseline (DWORD16..18).
            }
        }
    }

    pub(crate) fn particle_capacity(&self) -> usize {
        3
    }
    pub(crate) fn plane_position(&self, index: usize) -> Option<[u32; 3]> {
        self.sprites
            .get(index)
            .filter(|sprite| sprite.initialized)
            .map(|sprite| sprite.position)
    }
    pub(crate) fn basic_resource_index(&self, index: usize) -> usize {
        index
    }
    pub(crate) fn resource_count(&self) -> usize {
        Self::RESOURCES.len()
    }
    pub(crate) fn resource(&self, index: usize) -> Option<&[u8]> {
        Self::RESOURCES.get(index).copied()
    }
    pub(crate) fn draw_basic(&self, index: usize) -> Option<CmvsTextureParentState> {
        let sprite = self.sprites.get(index)?;
        if !sprite.initialized {
            return None;
        }
        Some(CmvsTextureParentState {
            resource_frame: Some(0),
            surface_initialized: true,
            basic_kind: 2,
            blend_mode: Some(1),
            rect_words: Some([
                sprite.atlas_index / 4 * 256,
                sprite.atlas_index % 4 * 192,
                256,
                192,
            ]),
            position_words: Some([128, 96]),
            auxiliary_pair_words: Some([0, 0]),
            float_position_words: sprite.position,
            projection_depth_word: 5.0_f32.to_bits(),
            projection_height_word: 0,
            rotation_hundredths: sprite.rotation as i32,
            alpha: Some(255),
            scale_words: Some([1.0_f32.to_bits(); 2]),
            ..Default::default()
        })
    }
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        let invalid = || {
            CoreError::invalid(
                "SHINKI_EMU_CMVS_NATIVE_SCENE_KIND55",
                "invalid native plane or controller state",
            )
        };
        for (index, sprite) in self.sprites.iter().enumerate() {
            if !floats(sprite.position).iter().all(|v| v.is_finite())
                || !(-35999..=35999).contains(&(sprite.rotation as i32))
                || sprite.atlas_index >= [24, 12, 8][index]
                || sprite.atlas_clock >= 100
            {
                return Err(invalid());
            }
            if let Some(motion) = &sprite.motion
                && (motion.duration == 0
                    || !floats(motion.start)
                        .iter()
                        .chain(floats(motion.target).iter())
                        .all(|v| v.is_finite()))
            {
                return Err(invalid());
            }
            if let Some(rotation) = &sprite.rotation_controller {
                if rotation.index >= 5
                    || rotation.steps[rotation.index].is_none()
                    || !(-35999..=35999).contains(&rotation.start)
                {
                    return Err(invalid());
                }
                for step in rotation.steps.iter().flatten() {
                    if step.duration == 0
                        || step.next < -1
                        || step.next >= 5
                        || (step.next >= 0 && rotation.steps[step.next as usize].is_none())
                        || !(-35999..=35999).contains(&step.target)
                        || step.target % 100 != 0
                    {
                        return Err(invalid());
                    }
                }
            }
        }
        for actor in &self.actors {
            if !floats(actor.target)
                .iter()
                .chain(floats(actor.baseline).iter())
                .all(|v| v.is_finite())
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "native_scene_kind55/tests.rs"]
mod tests;
