use crate::{
    Graphics, Surface,
    graphics::{
        BackgroundRotation, BackgroundTransform, BackgroundWarp, SceneOffset, SpriteGeometry,
    },
    random::Random,
};
use std::{collections::BTreeMap, io, sync::Arc};

#[derive(Clone, Copy)]
enum Ease {
    Sine,
    Power(f64),
}

impl Ease {
    fn progress(self, fraction: f64) -> f64 {
        match self {
            Self::Sine => (1.0 - (fraction * std::f64::consts::PI).cos()) * 0.5,
            Self::Power(power) if power < 0.0 => 1.0 - (1.0 - fraction).powf(-power),
            Self::Power(power) if power > 0.0 => fraction.powf(power),
            Self::Power(_) => fraction,
        }
    }
}

#[derive(Clone, Copy)]
struct Step {
    start: [i32; 2],
    end: [i32; 2],
    amplitude: [i32; 2],
    phase: [i32; 2],
    milliseconds: u64,
    ease: Ease,
}

impl Step {
    fn position(&self, elapsed: u64) -> [i32; 2] {
        let progress = self
            .ease
            .progress(elapsed as f64 / self.milliseconds as f64);
        std::array::from_fn(|axis| {
            let amp = f64::from(self.amplitude[axis]);
            let phase = f64::from(self.phase[axis]);
            let wave = 2.0 * amp * progress;
            let wave = if phase != 0.0 && wave <= -2.0 * phase {
                if wave <= -phase {
                    -wave
                } else {
                    wave + 2.0 * phase
                }
            } else {
                let midpoint = amp + phase;
                let wave = wave + 2.0 * phase;
                if wave <= midpoint {
                    wave
                } else {
                    2.0 * midpoint - wave
                }
            };
            let start = f64::from(self.start[axis]);
            let end = f64::from(self.end[axis]);
            (start + (end - start) * progress + wave) as i32
        })
    }
}

struct BackgroundMotion {
    target: i32,
    started: u32,
    paused_at: Option<u32>,
    path: Vec<Step>,
    initial: [i32; 2],
    end: [i32; 2],
    period: u64,
    repeat: bool,
}

impl BackgroundMotion {
    fn parse(
        target: i32,
        spec: &str,
        milliseconds: u32,
        now: u32,
        initial: [i32; 2],
        image: [i32; 2],
        viewport: [i32; 2],
    ) -> io::Result<Self> {
        let mut motion = Self {
            target,
            started: now,
            paused_at: None,
            path: Vec::new(),
            initial,
            end: initial,
            period: 0,
            repeat: false,
        };
        let mut duration = u64::from(milliseconds.max(1));
        let mut ease = Ease::Sine;
        let mut input = spec.as_bytes();
        let mut step = motion.zero_step(duration, ease);
        let mut pending = false;
        while let Some((&token, rest)) = input.split_first() {
            input = rest;
            match token {
                b'!' | b'l' | b'r' | b'u' | b'd' | b'c' | b'<' | b'>' | b'+' | b'-' | b'$'
                | b'%' | b'[' | b']' | b'^' | b'_' => {
                    pending = true;
                    let axis = usize::from(matches!(
                        token,
                        b'u' | b'd' | b'+' | b'-' | b'%' | b'^' | b'_'
                    ));
                    let far = image[axis].saturating_sub(viewport[axis]);
                    match token {
                        b'l' | b'u' => step.end[axis] = 0,
                        b'r' | b'd' => step.end[axis] = far,
                        b'c' => {
                            step.end = std::array::from_fn(|axis| {
                                image[axis].saturating_sub(viewport[axis]) / 2
                            });
                        }
                        b'!' => {
                            if let Some((&edge, rest)) = input.split_first() {
                                match edge {
                                    b'l' => step.end[0] = -viewport[0],
                                    b'r' => step.end[0] = image[0],
                                    b'u' => step.end[1] = -viewport[1],
                                    b'd' => step.end[1] = image[1],
                                    b'c' => step.end = [0, viewport[1]],
                                    _ => continue,
                                }
                                input = rest;
                            }
                        }
                        _ => {
                            let units = unsigned(&mut input)?.unwrap_or(1);
                            let pixels = units.saturating_mul(25);
                            match token {
                                b'<' | b'-' => {
                                    step.end[axis] = step.end[axis].saturating_sub(pixels)
                                }
                                b'>' | b'+' => {
                                    step.end[axis] = step.end[axis].saturating_add(pixels)
                                }
                                _ => {
                                    let span = pixels.saturating_mul(2);
                                    step.amplitude[axis] =
                                        step.amplitude[axis].saturating_add(span);
                                    if matches!(token, b'$' | b'%') {
                                        step.phase[axis] = step.phase[axis].saturating_sub(pixels);
                                    } else if matches!(token, b'[' | b'^') {
                                        step.phase[axis] = step.phase[axis].saturating_sub(span);
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {
                    if pending {
                        motion.end = step.end;
                        motion.period += step.milliseconds;
                        motion.path.push(step);
                        pending = false;
                    }
                    match token {
                        b' ' | b'\t' | b'\n' | b'\r' | b'i' | b'o' | b'f' | b'b' => {}
                        b'L' => {
                            motion.repeat = true;
                            break;
                        }
                        b'a' => ease = easing(&mut input)?,
                        b'0'..=b'9' => {
                            input = &spec.as_bytes()[spec.len() - input.len() - 1..];
                            duration = unsigned(&mut input)?.unwrap().max(1) as u64;
                        }
                        _ => return Err(invalid()),
                    }
                    step = motion.zero_step(duration, ease);
                }
            }
        }
        if pending {
            motion.end = step.end;
            motion.period += step.milliseconds;
            motion.path.push(step);
        }
        if motion.path.is_empty() {
            return Err(invalid());
        }
        Ok(motion)
    }

    fn zero_step(&self, milliseconds: u64, ease: Ease) -> Step {
        Step {
            start: self.end,
            end: self.end,
            amplitude: [0; 2],
            phase: [0; 2],
            milliseconds,
            ease,
        }
    }

    fn draw(&self, now: u32, graphics: &mut Graphics) -> bool {
        if self.paused_at.is_some() {
            return true;
        }
        let elapsed = u64::from(now.wrapping_sub(self.started));
        let cycles = elapsed / self.period;
        if cycles != 0 && !self.repeat {
            graphics.shift_background(self.target, self.end);
            return false;
        }
        let mut time = elapsed % self.period;
        for step in &self.path {
            if time < step.milliseconds {
                let position = step.position(time);
                let position = std::array::from_fn(|axis| {
                    let step = i64::from(self.end[axis]) - i64::from(self.initial[axis]);
                    (i64::from(position[axis]) + step.saturating_mul(cycles as i64))
                        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
                });
                return graphics.shift_background(self.target, position);
            }
            time -= step.milliseconds;
        }
        false
    }
}

fn unsigned(input: &mut &[u8]) -> io::Result<Option<i32>> {
    let length = input
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if length == 0 {
        return Ok(None);
    }
    let digits = std::str::from_utf8(&input[..length]).unwrap();
    *input = &input[length..];
    digits.parse().map(Some).map_err(|_| invalid())
}

fn easing(input: &mut &[u8]) -> io::Result<Ease> {
    let mut used = usize::from(matches!(input.first(), Some(b'+' | b'-')));
    let left = input[used..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if left == 0 {
        return Ok(Ease::Sine);
    }
    used += left;
    if input.get(used) == Some(&b'.') {
        used += 1;
        used += input[used..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
    }
    let value = std::str::from_utf8(&input[..used])
        .unwrap()
        .parse::<f64>()
        .map_err(|_| invalid())?;
    *input = &input[used..];
    if value.is_finite() {
        Ok(Ease::Power(value))
    } else {
        Err(invalid())
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid Lucifen background motion",
    )
}

pub(crate) struct SpritePlacement {
    pub(crate) position: [i32; 2],
    pub(crate) opacity: i32,
    pub(crate) plane: i32,
    amplitude: [i32; 2],
    phase: [i32; 2],
}

impl SpritePlacement {
    pub(crate) fn parse(spec: &str, geometry: SpriteGeometry) -> io::Result<Self> {
        let mut placement = Self {
            position: geometry.position,
            opacity: -1,
            plane: 0,
            amplitude: [0, 0],
            phase: [0, 0],
        };
        let [width, height] = geometry.viewport;
        let [sprite_width, sprite_height] = geometry.size;
        let [anchor_x, anchor_y] = geometry.origin;
        let mut input = spec.as_bytes();
        while let Some((&token, rest)) = input.split_first() {
            if token.is_ascii_digit() {
                unsigned(&mut input)?;
                continue;
            }
            input = rest;
            match token {
                b' ' | b'\t' | b'\n' | b'\r' | b'L' => {}
                b'a' => {
                    easing(&mut input)?;
                }
                b'f' => placement.plane = 1,
                b'b' => placement.plane = 2,
                b'i' => placement.opacity = 255,
                b'o' => placement.opacity = 0,
                b'l' => placement.position = [anchor_x - width / 4, anchor_y],
                b'r' => placement.position = [anchor_x + width / 4, anchor_y],
                b'c' => placement.position = geometry.origin,
                b'u' => placement.position[1] = 0,
                b'd' => placement.position[1] = sprite_height - height,
                b'!' => {
                    match input.first() {
                        Some(b'l') => placement.position = [-sprite_width, anchor_y],
                        Some(b'r') => placement.position = [width, anchor_y],
                        Some(b'c') => placement.position = [anchor_x, height],
                        Some(b'u') => placement.position[1] = -height,
                        Some(b'd') => placement.position[1] = sprite_height,
                        _ => {
                            placement.position[1] = height;
                            continue;
                        }
                    }
                    input = &input[1..];
                }
                b'<' | b'>' | b'-' | b'+' => {
                    let axis = usize::from(matches!(token, b'-' | b'+'));
                    let delta = unsigned(&mut input)?.unwrap_or(1).saturating_mul(25);
                    let delta = if matches!(token, b'<' | b'-') {
                        -delta
                    } else {
                        delta
                    };
                    placement.position[axis] = placement.position[axis].saturating_add(delta);
                }
                b'$' | b'%' | b'[' | b']' | b'^' | b'_' => {
                    let axis = usize::from(matches!(token, b'%' | b'^' | b'_'));
                    let span = unsigned(&mut input)?.unwrap_or(1).saturating_mul(50);
                    placement.amplitude[axis] = placement.amplitude[axis].saturating_add(span);
                    let phase = match token {
                        b'$' | b'%' => span / 2,
                        b'[' | b'^' => span,
                        _ => 0,
                    };
                    placement.phase[axis] = placement.phase[axis].saturating_sub(phase);
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid Lucifen sprite placement",
                    ));
                }
            }
        }
        Ok(placement)
    }
}

struct SpriteStep {
    motion: Step,
    fade: Option<bool>,
    plane: i32,
}

struct SpriteMotion {
    texture: i32,
    steps: Vec<SpriteStep>,
    started: u32,
    paused_at: Option<u32>,
    initial: [i32; 2],
    end: [i32; 2],
    opacity: i32,
    plane: i32,
    period: u64,
    repeat: bool,
    active: Option<(u64, usize)>,
}

impl SpriteMotion {
    fn parse(
        texture: i32,
        spec: &str,
        milliseconds: u32,
        started: u32,
        mut geometry: SpriteGeometry,
    ) -> io::Result<Self> {
        let initial = geometry.position;
        let mut steps = Vec::new();
        let mut duration = u64::from(milliseconds.max(1));
        let mut ease = Ease::Sine;
        let mut fade = None;
        let mut plane = 0;
        let mut last_opacity = -1;
        let mut last_plane = 0;
        let mut repeat = false;
        let mut input = spec.as_bytes();
        while let Some((&token, tail)) = input.split_first() {
            match token {
                b' ' | b'\t' | b'\n' | b'\r' => input = tail,
                b'L' => {
                    repeat = true;
                    break;
                }
                b'a' => {
                    input = tail;
                    ease = easing(&mut input)?;
                }
                b'i' | b'o' => {
                    input = tail;
                    fade = Some(token == b'i');
                    last_opacity = if token == b'i' { 255 } else { 0 };
                }
                b'f' | b'b' => {
                    input = tail;
                    plane = if token == b'f' { 1 } else { 2 };
                    last_plane = plane;
                }
                b'0'..=b'9' => {
                    duration = unsigned(&mut input)?.unwrap().max(1) as u64;
                }
                _ => {
                    let original = input;
                    input = sprite_segment_tail(input)?;
                    let segment = std::str::from_utf8(&original[..original.len() - input.len()])
                        .map_err(|_| invalid())?;
                    let position = geometry.position;
                    let placement = SpritePlacement::parse(segment, geometry)?;
                    geometry.position = placement.position;
                    steps.push(SpriteStep {
                        motion: Step {
                            start: position,
                            end: placement.position,
                            amplitude: placement.amplitude,
                            phase: placement.phase,
                            milliseconds: duration,
                            ease,
                        },
                        fade: fade.take(),
                        plane: std::mem::take(&mut plane),
                    });
                }
            }
        }
        if steps.is_empty() || fade.is_some() || plane != 0 {
            steps.push(SpriteStep {
                motion: Step {
                    start: geometry.position,
                    end: geometry.position,
                    amplitude: [0; 2],
                    phase: [0; 2],
                    milliseconds: duration,
                    ease,
                },
                fade,
                plane,
            });
        }
        let period = steps.iter().map(|step| step.motion.milliseconds).sum();
        Ok(Self {
            texture,
            steps,
            started,
            paused_at: None,
            initial,
            end: geometry.position,
            opacity: last_opacity,
            plane: last_plane,
            period,
            repeat,
            active: None,
        })
    }

    fn tick(&mut self, now: u32, graphics: &mut Graphics) -> bool {
        if graphics.sprite_geometry(self.texture).is_none() {
            return false;
        }
        if self.paused_at.is_some() {
            return true;
        }
        let elapsed = u64::from(now.wrapping_sub(self.started));
        let cycle = elapsed / self.period;
        if cycle != 0 && !self.repeat {
            self.finish(graphics);
            return false;
        }
        let mut time = elapsed % self.period;
        for (index, step) in self.steps.iter().enumerate() {
            if time >= step.motion.milliseconds {
                time -= step.motion.milliseconds;
                continue;
            }
            if self.active != Some((cycle, index)) {
                graphics.set_sprite_plane(self.texture, step.plane);
                self.active = Some((cycle, index));
            }
            let position = step.motion.position(time);
            let [x, y] = std::array::from_fn(|axis| {
                let drift = i64::from(self.end[axis]) - i64::from(self.initial[axis]);
                i64::from(position[axis])
                    .saturating_add(drift.saturating_mul(cycle as i64))
                    .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
            });
            graphics.texture_move(self.texture, x, y, false);
            if let Some(fade_in) = step.fade {
                let opacity = (time * 255 / step.motion.milliseconds) as u8;
                graphics
                    .texture_opacity(self.texture, if fade_in { opacity } else { 255 - opacity });
            }
            return true;
        }
        false
    }

    fn finish(&self, graphics: &mut Graphics) {
        graphics.texture_move(self.texture, self.end[0], self.end[1], false);
        if self.opacity >= 0 {
            graphics.texture_opacity(self.texture, self.opacity as u8);
        }
        graphics.set_sprite_plane(self.texture, self.plane);
    }

    fn pause(&mut self, now: u32, paused: bool) {
        if paused {
            self.paused_at.get_or_insert(now);
        } else if let Some(since) = self.paused_at.take() {
            self.started = self.started.wrapping_add(now.wrapping_sub(since));
        }
    }
}

fn sprite_segment_tail(mut input: &[u8]) -> io::Result<&[u8]> {
    let start = input.len();
    while let Some((&token, rest)) = input.split_first() {
        match token {
            b'l' | b'r' | b'u' | b'd' | b'c' => input = rest,
            b'!' => {
                input = rest;
                if matches!(input.first(), Some(b'l' | b'r' | b'u' | b'd' | b'c')) {
                    input = &input[1..];
                }
            }
            b'<' | b'>' | b'-' | b'+' | b'$' | b'%' | b'[' | b']' | b'^' | b'_' => {
                input = rest;
                unsigned(&mut input)?;
            }
            _ => break,
        }
    }
    if input.len() == start {
        Err(invalid())
    } else {
        Ok(input)
    }
}

pub(crate) struct HazeTiming {
    pub(crate) period: u32,
    pub(crate) sustain: u32,
    pub(crate) attack: u32,
    pub(crate) decay: u32,
    pub(crate) fill_edges: bool,
}

struct Haze {
    target: i32,
    table: Arc<[i32]>,
    started: u32,
    paused_at: Option<u32>,
    timing: HazeTiming,
    decay_started: Option<(u32, i32)>,
}

impl Haze {
    fn gain(&self, elapsed: u32) -> i32 {
        if let Some((start, gain)) = self.decay_started {
            let remaining = self
                .timing
                .decay
                .saturating_sub(elapsed.saturating_sub(start));
            if remaining == 0 {
                return 0;
            }
            return (i64::from(gain) * i64::from(remaining) / i64::from(self.timing.decay)) as i32;
        }
        if elapsed < self.timing.attack {
            return (u64::from(elapsed) * 512 / u64::from(self.timing.attack)) as i32;
        }
        512
    }

    fn end(&mut self, now: u32) -> bool {
        if self.decay_started.is_some() {
            return false;
        }
        let elapsed = now.wrapping_sub(self.started);
        self.decay_started = Some((elapsed, self.gain(elapsed)));
        true
    }

    fn tick(&mut self, now: u32, graphics: &mut Graphics) -> bool {
        if graphics.background_geometry(self.target).is_none() {
            graphics.clear_background_warp(self.target);
            return false;
        }
        if self.paused_at.is_some() {
            return true;
        }
        let elapsed = now.wrapping_sub(self.started);
        let full = self.timing.attack.saturating_add(self.timing.sustain);
        if self.decay_started.is_none() && self.timing.sustain != 0 && elapsed >= full {
            self.decay_started = Some((full, 512));
        }
        if self
            .decay_started
            .is_some_and(|(start, _)| elapsed.saturating_sub(start) >= self.timing.decay)
        {
            graphics.clear_background_warp(self.target);
            return false;
        }
        let phase = self.table.len() as u64 * u64::from(elapsed % self.timing.period)
            / u64::from(self.timing.period);
        graphics.set_background_warp(
            self.target,
            BackgroundWarp {
                table: self.table.clone(),
                phase: phase as usize,
                gain: self.gain(elapsed),
                fill_edges: self.timing.fill_edges,
            },
        );
        true
    }
}

fn haze_table(spec: &str, size: u32) -> io::Result<Arc<[i32]>> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid Lucifen background haze",
        )
    };
    if !(1..=65_536).contains(&size) {
        return Err(invalid());
    }
    let mut waves = Vec::new();
    for tuple in spec.split('(').skip(1) {
        let (parameters, _) = tuple.split_once(')').ok_or_else(invalid)?;
        let mut values = parameters
            .split(',')
            .map(|value| value.trim().parse::<i32>());
        let wave = [
            values.next().ok_or_else(invalid)?.map_err(|_| invalid())?,
            values.next().ok_or_else(invalid)?.map_err(|_| invalid())?,
            values.next().ok_or_else(invalid)?.map_err(|_| invalid())?,
        ];
        if values.next().is_some() {
            return Err(invalid());
        }
        waves.push(wave);
    }
    if waves.is_empty() {
        return Err(invalid());
    }
    let step = 0x2000000u64 / u64::from(size);
    Ok((0..size)
        .map(|index| {
            let coordinate = (step * u64::from(index) >> 16) as i32;
            let sum: f64 = waves
                .iter()
                .map(|&[frequency, phase, amplitude]| {
                    let position = phase.wrapping_add(frequency.wrapping_mul(coordinate)) & 511;
                    (f64::from(position) * std::f64::consts::TAU / 512.0).cos()
                        * f64::from(amplitude)
                })
                .sum();
            (sum / waves.len() as f64).floor() as i32
        })
        .collect::<Vec<_>>()
        .into())
}

#[derive(Default)]
pub(crate) struct Effects {
    background: BTreeMap<i32, BackgroundMotion>,
    haze: BTreeMap<i32, Haze>,
    quakes: BTreeMap<i32, Quake>,
    noise: BTreeMap<i32, Noise>,
    particles: BTreeMap<i32, Particles>,
    sprites: BTreeMap<i32, SpriteMotion>,
    pulses: BTreeMap<i32, SpritePulse>,
    zoom: BTreeMap<i32, BackgroundZoom>,
    rotations: BTreeMap<i32, BackgroundRotationTask>,
    background_transforms: BTreeMap<i32, BackgroundTransformTask>,
}

struct BackgroundRotationTask {
    target: i32,
    start: BackgroundRotation,
    end: BackgroundRotation,
    started: u32,
    paused_at: Option<u32>,
    duration: u32,
    ease: Ease,
}

impl BackgroundRotationTask {
    fn tick(&self, now: u32, graphics: &mut Graphics) -> bool {
        let elapsed = self.paused_at.unwrap_or(now).wrapping_sub(self.started);
        if elapsed >= self.duration {
            graphics.rotate_background(self.target, self.end);
            return false;
        }
        let fraction = self
            .ease
            .progress(f64::from(elapsed) / f64::from(self.duration));
        let interpolate = |start: f64, end: f64| start + (end - start) * fraction;
        graphics.rotate_background(
            self.target,
            BackgroundRotation {
                offset: std::array::from_fn(|axis| {
                    interpolate(
                        f64::from(self.start.offset[axis]),
                        f64::from(self.end.offset[axis]),
                    ) as i32
                }),
                turns: interpolate(f64::from(self.start.turns), f64::from(self.end.turns)) as i32,
                scale: interpolate(self.start.scale, self.end.scale),
            },
        )
    }

    fn pause(&mut self, now: u32, paused: bool) {
        if paused {
            self.paused_at.get_or_insert(now);
        } else if let Some(since) = self.paused_at.take() {
            self.started = self.started.wrapping_add(now.wrapping_sub(since));
        }
    }
}

struct SpritePulse {
    target: i32,
    position: [i32; 2],
    size: [i32; 2],
    peak: [i32; 2],
    bottom_anchor: bool,
    kind: i32,
    started: u32,
    duration: u32,
    ease: Ease,
    paused_at: Option<u32>,
}

pub(crate) struct SpritePulseSettings {
    pub(crate) duration: u32,
    pub(crate) scale: f64,
    pub(crate) centered: bool,
    pub(crate) ease: f64,
}

impl SpritePulse {
    fn tick(&self, now: u32, graphics: &mut Graphics) -> bool {
        let elapsed = self.paused_at.unwrap_or(now).wrapping_sub(self.started);
        if elapsed >= self.duration {
            graphics.stretch_texture(self.target, None);
            return false;
        }
        let progress = self
            .ease
            .progress(f64::from(elapsed) / f64::from(self.duration));
        let progress = 1.0 - (progress * 2.0 - 1.0).abs();
        let size = std::array::from_fn(|axis| {
            let initial = f64::from(self.size[axis]);
            (initial + (f64::from(self.peak[axis]) - initial) * progress) as i32
        });
        let x = self.position[0].saturating_add((self.size[0] - size[0]) >> 1);
        let y = self.position[1]
            .saturating_add((self.size[1] - size[1]) >> u32::from(!self.bottom_anchor));
        graphics.stretch_texture(self.target, Some(([x, y], size)))
    }

    fn pause(&mut self, now: u32, paused: bool) {
        if paused {
            self.paused_at.get_or_insert(now);
        } else if let Some(since) = self.paused_at.take() {
            self.started = self.started.wrapping_add(now.wrapping_sub(since));
        }
    }
}

pub(crate) enum BackgroundTransformPath {
    Centered {
        center: [i32; 2],
        scale: [f64; 2],
        turns: [f64; 2],
    },
    Points(Vec<[f64; 3]>, Vec<[f64; 2]>),
}

impl BackgroundTransformPath {
    pub(crate) fn parse(spec: &str, smooth: bool) -> io::Result<Self> {
        let points = spec
            .split('(')
            .skip(1)
            .map(|part| {
                let (coordinates, _) = part.split_once(')').ok_or_else(invalid)?;
                let mut values = coordinates
                    .split(|c: char| c == ',' || c.is_ascii_whitespace())
                    .filter(|s| !s.is_empty());
                let mut point = [0.0, 0.0, 1.0];
                for field in &mut point[..2] {
                    *field = f64::from(
                        values
                            .next()
                            .and_then(|value| value.parse::<i32>().ok())
                            .ok_or_else(invalid)?,
                    );
                }
                if let Some(scale) = values.next() {
                    point[2] = scale
                        .parse::<f64>()
                        .ok()
                        .filter(|scale| scale.is_finite())
                        .ok_or_else(invalid)?;
                }
                Ok(point)
            })
            .collect::<io::Result<Vec<_>>>()?;
        if points.len() < 2 {
            return Err(invalid());
        }
        let mut curvature = vec![[0.0; 2]; points.len()];
        if smooth && points.len() > 2 {
            let mut diagonal = vec![4.0; points.len()];
            for i in 1..points.len() - 1 {
                let weight = if i == 1 { 0.0 } else { 1.0 / diagonal[i - 1] };
                diagonal[i] -= weight;
                for axis in 0..2 {
                    curvature[i][axis] = points[i + 1][axis] - 2.0 * points[i][axis]
                        + points[i - 1][axis]
                        - weight * curvature[i - 1][axis];
                }
            }
            for i in (1..points.len() - 1).rev() {
                for axis in 0..2 {
                    curvature[i][axis] =
                        (curvature[i][axis] - curvature[i + 1][axis]) / diagonal[i];
                }
            }
        }
        Ok(Self::Points(points, curvature))
    }

    fn at(&self, progress: f64) -> ([i32; 2], f64, f64) {
        let lerp = |[start, end]: [f64; 2], weight| start + (end - start) * weight;
        match self {
            Self::Centered {
                center,
                scale,
                turns,
            } => (*center, lerp(*scale, progress), lerp(*turns, progress)),
            Self::Points(points, curvature) => {
                let time = progress * (points.len() - 1) as f64;
                let segment = (time as usize).min(points.len() - 2);
                let t = time - segment as f64;
                let u = 1.0 - t;
                let [start, end] = [points[segment], points[segment + 1]];
                let center = std::array::from_fn(|axis| {
                    (lerp([start[axis], end[axis]], t)
                        + (u * u * u - u) * curvature[segment][axis]
                        + (t * t * t - t) * curvature[segment + 1][axis]) as i32
                });
                (center, lerp([start[2], end[2]], t), 0.0)
            }
        }
    }
}

pub(crate) struct BackgroundTransformSettings {
    pub(crate) path: BackgroundTransformPath,
    pub(crate) bitmap: Option<Arc<Surface>>,
    pub(crate) duration: u32,
    pub(crate) opacity: u8,
    pub(crate) sampling: u8,
    pub(crate) flip: [bool; 2],
    pub(crate) ease: f64,
    pub(crate) in_place: bool,
    pub(crate) remove_on_finish: bool,
}

struct BackgroundTransformTask {
    target: i32,
    settings: BackgroundTransformSettings,
    bitmap: Arc<Surface>,
    started: u32,
    last: u32,
    paused_at: Option<u32>,
    finished: bool,
}

impl BackgroundTransformTask {
    fn draw(&self, progress: f64, graphics: &mut Graphics) -> bool {
        let (center, scale, turns) = self.settings.path.at(progress);
        graphics.transform_background(
            self.target,
            &self.bitmap,
            BackgroundTransform {
                center,
                offset: [0; 2],
                scale,
                turns,
                opacity: self.settings.opacity,
                sampling: self.settings.sampling,
                alpha: self.settings.bitmap.is_some(),
                in_place: self.settings.in_place,
            },
        )
    }

    fn tick(&mut self, now: u32, graphics: &mut Graphics) -> bool {
        if graphics.background_image(self.target).is_none() {
            return false;
        }
        if self.paused_at.is_some() || self.finished || now.wrapping_sub(self.last) < 16 {
            return true;
        }
        self.last = now;
        let elapsed = now.wrapping_sub(self.started);
        self.finished = elapsed >= self.settings.duration;
        let progress = if self.finished {
            1.0
        } else {
            Ease::Power(self.settings.ease)
                .progress(f64::from(elapsed) / f64::from(self.settings.duration))
        };
        self.draw(progress, graphics);
        if self.finished && self.settings.remove_on_finish {
            self.clear(graphics);
            return false;
        }
        true
    }

    fn clear(&self, graphics: &mut Graphics) {
        if !self.settings.in_place {
            graphics.clear_background_transform(self.target);
        }
    }
}

pub(crate) struct ZoomSettings {
    pub(crate) center: [i32; 2],
    pub(crate) scale: f64,
    pub(crate) ease: f64,
    pub(crate) duration: u32,
}

struct BackgroundZoom {
    target: i32,
    start: [i32; 4],
    end: [i32; 4],
    ease: Ease,
    duration: u32,
    started: u32,
    paused_at: Option<u32>,
}

impl BackgroundZoom {
    fn tick(&self, now: u32, graphics: &mut Graphics) -> bool {
        if self.paused_at.is_some() {
            return true;
        }
        let elapsed = now.wrapping_sub(self.started);
        if elapsed >= self.duration {
            graphics.set_background_window(self.target, self.end);
            return false;
        }
        let progress = self
            .ease
            .progress(f64::from(elapsed) / f64::from(self.duration));
        let window = std::array::from_fn(|axis| {
            let start = f64::from(self.start[axis]);
            (start + (f64::from(self.end[axis]) - start) * progress) as i32
        });
        graphics.set_background_window(self.target, window)
    }

    fn pause(&mut self, now: u32, paused: bool) {
        if paused {
            self.paused_at.get_or_insert(now);
        } else if let Some(since) = self.paused_at.take() {
            self.started = self.started.wrapping_add(now.wrapping_sub(since));
        }
    }
}

pub(crate) struct NoiseSettings {
    pub(crate) opacity: u8,
    pub(crate) priority: u32,
    pub(crate) visible: [bool; 2],
}

struct Noise {
    target: i32,
    bitmap: i32,
    sample: Arc<Surface>,
    layers: Vec<i32>,
    updated: u32,
    paused: bool,
}

pub(crate) struct ParticleSettings {
    pub(crate) kind: ParticleKind,
    pub(crate) count: usize,
    pub(crate) priority: i32,
    pub(crate) priority_step: i32,
    pub(crate) prefill: bool,
    pub(crate) visible: [bool; 2],
}

#[derive(Clone, Copy)]
pub(crate) enum ParticleKind {
    Light,
    Paper,
}

impl ParticleKind {
    pub(crate) fn atlases(self) -> usize {
        match self {
            Self::Light => 3,
            Self::Paper => 8,
        }
    }

    pub(crate) fn atlas_name(self, index: usize) -> String {
        match self {
            Self::Light => format!("sys/gr_lt{index}.elg"),
            Self::Paper => format!("sys/paper_{index}.elg"),
        }
    }

    fn frames(self) -> i32 {
        match self {
            Self::Light => 5,
            Self::Paper => 24,
        }
    }

    fn velocity(self, variant: i32, random: &mut Random) -> [f64; 2] {
        let variant = f64::from(variant);
        match self {
            Self::Light => {
                let speed = 1.9 + variant * 0.8;
                let speed_x = speed + random_fraction(random) * 0.2;
                [
                    if random.below(2) == 0 {
                        speed_x
                    } else {
                        -speed_x
                    },
                    -speed - random_fraction(random) * 2.0,
                ]
            }
            Self::Paper => [
                0.0,
                7.9 + variant * 0.8
                    + if variant >= 4.0 { 4.3 } else { 0.0 }
                    + random_fraction(random) * 4.0,
            ],
        }
    }
}

struct Particles {
    target: i32,
    particles: Vec<Particle>,
    frame: ParticleFrame,
    updated: u32,
    paused: bool,
}

struct Particle {
    kind: ParticleKind,
    layers: [i32; 2],
    size: [i32; 2],
    position: [f64; 2],
    velocity: [f64; 2],
    drift: f64,
    frame: i32,
    phase: u16,
    active: bool,
}

impl Particle {
    fn starting_y(&self, height: f64) -> f64 {
        match self.kind {
            ParticleKind::Light => height,
            ParticleKind::Paper => -f64::from(self.size[1]),
        }
    }

    fn spawn(&mut self, extent: [u32; 2], prefill: bool, random: &mut Random) {
        let [width, height] = extent.map(f64::from);
        self.position = [width * random_fraction(random), self.starting_y(height)];
        if prefill {
            self.position[1] += self.velocity[1].signum() * height * random_fraction(random);
        }
        self.frame = random.below(self.kind.frames() as u32);
        if matches!(self.kind, ParticleKind::Light) {
            self.phase = random.below(510) as u16;
        }
        self.active = true;
    }

    fn place(&self, graphics: &mut Graphics, visible: [bool; 2]) {
        let [x, y] = self.position.map(|coordinate| coordinate as i32);
        for (layer, visible) in self.layers.into_iter().zip(visible) {
            graphics.show_texture(layer, self.active && visible);
            if self.active {
                graphics.texture_move(layer, x, y, false);
                graphics.move_texture_source(layer, [self.frame * self.size[0], 0]);
                if matches!(self.kind, ParticleKind::Light) {
                    graphics.texture_opacity(layer, (510 - self.phase) as u8);
                }
            }
        }
    }

    fn tick(&mut self, particles: &ParticleFrame, graphics: &mut Graphics, random: &mut Random) {
        if self.active {
            self.position[0] += self.velocity[0];
            self.position[1] += self.velocity[1];
            let max_speed = match self.kind {
                ParticleKind::Light => 5.0,
                ParticleKind::Paper => 3.0,
            };
            self.velocity[0] = (self.velocity[0] + self.drift).clamp(-max_speed, max_speed);
            self.drift = (self.drift + (random_fraction(random) - 0.5) * 0.6).clamp(-0.5, 0.5);
            let height = f64::from(particles.extent[1]);
            let outside = match self.kind {
                ParticleKind::Light => self.position[1] < -f64::from(self.size[1]),
                ParticleKind::Paper => self.position[1] >= height,
            };
            if outside {
                if particles.draining {
                    self.active = false;
                } else {
                    self.position = [
                        f64::from(particles.extent[0]) * random_fraction(random),
                        self.starting_y(height),
                    ];
                }
            }
            if !particles.draining {
                self.frame = (self.frame + 1) % self.kind.frames();
                self.phase += 32;
                if self.phase >= 510 {
                    self.phase = 0;
                }
            }
        } else if !particles.draining && random_fraction(random) < 0.1 {
            self.spawn(particles.extent, false, random);
        }
        self.place(graphics, particles.visible);
    }
}

#[derive(Clone, Copy)]
struct ParticleFrame {
    visible: [bool; 2],
    extent: [u32; 2],
    draining: bool,
}

impl Particles {
    fn free(&self, graphics: &mut Graphics) {
        for particle in &self.particles {
            for layer in particle.layers {
                graphics.free_texture(layer);
            }
        }
    }

    fn tick(&mut self, now: u32, graphics: &mut Graphics, random: &mut Random) -> bool {
        if graphics.surface(self.target).is_err() {
            return false;
        }
        if !self.paused && now.wrapping_sub(self.updated) >= 80 {
            self.updated = now;
            for particle in &mut self.particles {
                particle.tick(&self.frame, graphics, random);
            }
        }
        !self.frame.draining || self.particles.iter().any(|particle| particle.active)
    }
}

fn random_fraction(random: &mut Random) -> f64 {
    f64::from(random.below(1_000_000_000)) / 1_000_000_000.0
}

impl Noise {
    fn update(&self, graphics: &mut Graphics, random: &mut Random) {
        let pixels = self.sample.width * self.sample.height;
        graphics.repeat_bitmap_pixels(self.bitmap, &self.sample, random.below(pixels) as usize);
    }

    fn free(&self, graphics: &mut Graphics) {
        for &layer in &self.layers {
            graphics.free_texture(layer);
        }
        graphics.free(self.bitmap);
    }
}

pub(crate) struct QuakeSettings {
    pub(crate) duration: u32,
    pub(crate) frequency: i32,
    pub(crate) amplitude: [i32; 2],
    pub(crate) end_gain: f64,
    pub(crate) fill_edges: bool,
    pub(crate) primary: bool,
    pub(crate) overlay: bool,
}

struct Quake {
    target: i32,
    settings: QuakeSettings,
    started: u32,
    paused_at: Option<u32>,
    positive: bool,
}

impl Quake {
    fn tick(&mut self, now: u32, graphics: &mut Graphics, random: &mut Random) -> bool {
        if graphics.surface(self.target).is_err() {
            return false;
        }
        if self.paused_at.is_some() {
            return true;
        }
        let elapsed = now.wrapping_sub(self.started);
        let settings = &self.settings;
        if elapsed >= settings.duration && settings.duration != u32::MAX {
            graphics.set_scene_offset(self.target, SceneOffset::default());
            return false;
        }
        let progress = if settings.duration == u32::MAX {
            0.0
        } else {
            f64::from(elapsed) / f64::from(settings.duration)
        };
        let gain = 1.0 - (1.0 - settings.end_gain) * progress;
        let offset = if settings.frequency != 0 && settings.duration != u32::MAX {
            let phase = (256.0 * f64::from(settings.frequency) * progress) as i64 - 128;
            let wave = (phase as f64 * std::f64::consts::TAU / 512.0).cos();
            settings
                .amplitude
                .map(|amplitude| (f64::from(amplitude) * wave * gain) as i32)
        } else {
            self.positive = !self.positive;
            random_quake_offset(settings.amplitude, gain, self.positive, random)
        };
        graphics.set_scene_offset(
            self.target,
            SceneOffset {
                primary: if settings.primary { offset } else { [0, 0] },
                overlay: if settings.overlay { offset } else { [0, 0] },
                fill_edges: settings.fill_edges,
            },
        );
        true
    }

    fn pause(&mut self, now: u32, paused: bool) {
        if paused {
            self.paused_at.get_or_insert(now);
        } else if let Some(since) = self.paused_at.take() {
            self.started = self.started.wrapping_add(now.wrapping_sub(since));
        }
    }
}

pub(crate) fn random_quake_offset(
    amplitude: [i32; 2],
    gain: f64,
    reverse: bool,
    random: &mut Random,
) -> [i32; 2] {
    let direction = if reverse { -1.0 } else { 1.0 };
    std::array::from_fn(|axis| {
        let sample = f64::from(random.below(1_000_000_000)) / 1_000_000_000.0;
        let relative = if amplitude[axis] > amplitude[1 - axis] {
            sample * direction
        } else {
            sample - 0.5
        };
        (f64::from(amplitude[axis]) * relative * gain) as i32
    })
}

impl Effects {
    pub(crate) fn start_background_rotation(
        &mut self,
        target: i32,
        end: BackgroundRotation,
        duration: u32,
        ease: f64,
        now: u32,
        graphics: &mut Graphics,
    ) -> io::Result<i32> {
        let Some(start) = graphics.background_rotation(target) else {
            return Ok(0);
        };
        if end.scale == 0.0 {
            return Ok(0);
        }
        if duration == 0 {
            graphics.rotate_background(target, end);
            return Ok(0);
        }
        let handle = graphics.next_surface()?;
        self.rotations.insert(
            handle,
            BackgroundRotationTask {
                target,
                start,
                end,
                duration,
                started: now,
                paused_at: None,
                ease: Ease::Power(ease),
            },
        );
        Ok(handle)
    }

    pub(crate) fn start_sprite_pulse(
        &mut self,
        target: i32,
        settings: SpritePulseSettings,
        now: u32,
        graphics: &mut Graphics,
    ) -> io::Result<i32> {
        let SpritePulseSettings {
            duration,
            scale,
            centered,
            ease,
        } = settings;
        let Some([left, top, right, bottom]) = graphics.texture_bounds(target) else {
            return Ok(0);
        };
        if duration < 2 || scale < 0.0 {
            return Ok(0);
        }
        let size = [right.saturating_sub(left), bottom.saturating_sub(top)];
        let task = graphics.next_surface()?;
        self.pulses.insert(
            task,
            SpritePulse {
                target,
                position: [left, top],
                size,
                peak: size.map(|extent| (f64::from(extent) * scale) as i32),
                bottom_anchor: !centered && scale < 1.0,
                kind: 12007 + i32::from(graphics.is_overlay(target)),
                started: now,
                duration: (duration / 2) * 2,
                ease: Ease::Power(ease),
                paused_at: None,
            },
        );
        Ok(task)
    }

    pub(crate) fn stop_sprite_pulses(&mut self, kind: i32, graphics: &mut Graphics) {
        self.pulses.retain(|_, pulse| {
            if kind >= 0 && kind != pulse.kind {
                return true;
            }
            graphics.stretch_texture(pulse.target, None);
            false
        });
    }

    pub(crate) fn pause_sprite_pulses(&mut self, kind: i32, now: u32, paused: bool) {
        for pulse in self
            .pulses
            .values_mut()
            .filter(|pulse| kind < 0 || kind == pulse.kind)
        {
            pulse.pause(now, paused);
        }
    }

    pub(crate) fn start_particles(
        &mut self,
        target: i32,
        samples: &[i32],
        settings: ParticleSettings,
        now: u32,
        graphics: &mut Graphics,
        random: &mut Random,
    ) -> io::Result<i32> {
        let Ok(surface) = graphics.surface(target) else {
            return Ok(0);
        };
        let extent = [surface.width, surface.height];
        let mut particles = Particles {
            target,
            particles: Vec::new(),
            frame: ParticleFrame {
                extent,
                visible: settings.visible,
                draining: false,
            },
            updated: now,
            paused: false,
        };
        let result = (|| {
            for index in 0..settings.count {
                let variant = random.below(samples.len() as u32);
                let sample = samples[variant as usize];
                let image = graphics.surface(sample)?;
                let size = [
                    image.width as i32 / settings.kind.frames(),
                    image.height as i32,
                ];
                let mut particle = Particle {
                    kind: settings.kind,
                    layers: [0; 2],
                    size,
                    position: [0.0; 2],
                    velocity: settings.kind.velocity(variant, random),
                    drift: random_fraction(random),
                    frame: random.below(settings.kind.frames() as u32),
                    phase: 0,
                    active: false,
                };
                if settings.prefill || index == 0 {
                    particle.spawn(extent, settings.prefill, random);
                }
                particles.particles.push(particle);
                let particle = particles.particles.last_mut().unwrap();
                let priority = settings.priority.wrapping_add(
                    settings
                        .priority_step
                        .wrapping_mul(samples.len() as i32 / 2 - variant),
                ) as u32;
                for (pass, layer) in particle.layers.iter_mut().enumerate() {
                    *layer = graphics.texture_from_surface(
                        sample,
                        Some([0, 0, size[0], size[1]]),
                        [0, 0],
                    )?;
                    if graphics.attach_layer(target, *layer, priority, 0, pass == 1, false) != 0 {
                        return Err(invalid());
                    }
                }
                particle.place(graphics, settings.visible);
            }
            graphics.next_surface()
        })();
        match result {
            Ok(id) => {
                self.particles.insert(id, particles);
                Ok(id)
            }
            Err(error) => {
                particles.free(graphics);
                Err(error)
            }
        }
    }

    pub(crate) fn configure_particles(
        &mut self,
        id: i32,
        visible: Option<[bool; 2]>,
        graphics: &mut Graphics,
    ) -> Option<i32> {
        let particles = self.particles.get_mut(&id)?;
        if let Some(visible) = visible {
            particles.frame.visible = visible;
            for particle in &particles.particles {
                particle.place(graphics, visible);
            }
        } else {
            particles.frame.draining = true;
        }
        Some(1)
    }

    pub(crate) fn is_particles(&self, id: i32) -> bool {
        self.particles.contains_key(&id)
    }

    pub(crate) fn start_background_transform(
        &mut self,
        target: i32,
        settings: BackgroundTransformSettings,
        now: u32,
        graphics: &mut Graphics,
    ) -> io::Result<i32> {
        let Some(background) = graphics.background_image(target) else {
            return Ok(0);
        };
        let mut bitmap = settings.bitmap.clone().unwrap_or(background);
        let region = [0, 0, bitmap.width as i32, bitmap.height as i32];
        for (axis, flip) in settings.flip.into_iter().enumerate() {
            if flip {
                Arc::make_mut(&mut bitmap).flip(region, axis == 0);
            }
        }
        let effect = BackgroundTransformTask {
            target,
            settings,
            bitmap,
            started: now,
            last: now,
            paused_at: None,
            finished: false,
        };
        let id = graphics.next_surface()?;
        if !effect.draw(0.0, graphics) {
            return Ok(0);
        }
        self.background_transforms.insert(id, effect);
        Ok(id)
    }

    pub(crate) fn zoom_window(
        target: i32,
        center: [i32; 2],
        scale: f64,
        graphics: &Graphics,
    ) -> Option<[i32; 4]> {
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let (_, bitmap, viewport) = graphics.background_geometry(target)?;
        let size: [i32; 2] = std::array::from_fn(|axis| {
            ((f64::from(viewport[axis]) / scale) as i32).clamp(1, bitmap[axis])
        });
        let position: [i32; 2] = std::array::from_fn(|axis| {
            center[axis]
                .saturating_sub(size[axis] / 2)
                .clamp(0, bitmap[axis] - size[axis])
        });
        Some([position[0], position[1], size[0], size[1]])
    }

    pub(crate) fn start_zoom(
        &mut self,
        target: i32,
        settings: ZoomSettings,
        now: u32,
        graphics: &mut Graphics,
    ) -> io::Result<i32> {
        let Some(start) = graphics.background_window(target) else {
            return Ok(0);
        };
        let Some(end) = Self::zoom_window(target, settings.center, settings.scale, graphics) else {
            return Ok(0);
        };
        if settings.duration == 0 {
            graphics.set_background_window(target, end);
            return Ok(0);
        }
        let handle = graphics.next_surface()?;
        self.zoom.insert(
            handle,
            BackgroundZoom {
                target,
                start,
                end,
                ease: Ease::Power(settings.ease),
                duration: settings.duration,
                started: now,
                paused_at: None,
            },
        );
        Ok(handle)
    }

    pub(crate) fn zoom_end(&self, handle: i32) -> Option<[i32; 4]> {
        self.zoom.get(&handle).map(|zoom| zoom.end)
    }
    pub(crate) fn start_sprite(
        &mut self,
        texture: i32,
        path: &str,
        duration: u32,
        now: u32,
        graphics: &mut Graphics,
    ) -> io::Result<i32> {
        let Some(geometry) = graphics.sprite_geometry(texture) else {
            return Ok(0);
        };
        let motion = SpriteMotion::parse(texture, path, duration, now, geometry)?;
        if duration == 0 {
            motion.finish(graphics);
            return Ok(0);
        }
        let id = graphics.next_surface()?;
        self.sprites.insert(id, motion);
        Ok(id)
    }

    pub(crate) fn sprite_end(&self, handle: i32) -> Option<[i32; 4]> {
        self.sprites
            .get(&handle)
            .map(|motion| [motion.end[0], motion.end[1], motion.opacity, motion.plane])
    }

    pub(crate) fn stop_sprites(&mut self, graphics: &mut Graphics) {
        for motion in std::mem::take(&mut self.sprites).into_values() {
            motion.finish(graphics);
        }
    }

    pub(crate) fn pause_sprites(&mut self, now: u32, paused: bool) {
        for motion in self.sprites.values_mut() {
            motion.pause(now, paused);
        }
    }

    pub(crate) fn position_background(
        target: i32,
        spec: &str,
        graphics: &mut Graphics,
    ) -> io::Result<Option<[i32; 2]>> {
        let Some((initial, image, viewport)) = graphics.background_geometry(target) else {
            return Ok(None);
        };
        let motion = BackgroundMotion::parse(target, spec, 0, 0, initial, image, viewport)?;
        graphics.shift_background(target, motion.end);
        Ok(Some(motion.end))
    }

    pub(crate) fn start_background(
        &mut self,
        target: i32,
        spec: &str,
        milliseconds: u32,
        now: u32,
        graphics: &mut Graphics,
    ) -> io::Result<i32> {
        let Some((initial, image, viewport)) = graphics.background_geometry(target) else {
            return Ok(0);
        };
        let motion =
            BackgroundMotion::parse(target, spec, milliseconds, now, initial, image, viewport)?;
        if milliseconds == 0 {
            graphics.shift_background(target, motion.end);
            return Ok(0);
        }
        let id = graphics.next_surface()?;
        self.background.insert(id, motion);
        Ok(id)
    }

    pub(crate) fn start_quake(
        &mut self,
        target: i32,
        settings: QuakeSettings,
        now: u32,
        graphics: &mut Graphics,
    ) -> io::Result<i32> {
        if settings.duration == 0 || graphics.surface(target).is_err() {
            return Ok(0);
        }
        let id = graphics.next_surface()?;
        self.quakes.insert(
            id,
            Quake {
                target,
                settings,
                started: now,
                paused_at: None,
                positive: false,
            },
        );
        Ok(id)
    }

    pub(crate) fn start_noise(
        &mut self,
        target: i32,
        sample: Arc<Surface>,
        settings: NoiseSettings,
        now: u32,
        graphics: &mut Graphics,
        random: &mut Random,
    ) -> io::Result<i32> {
        let Ok(surface) = graphics.surface(target) else {
            return Ok(0);
        };
        let bitmap = graphics.create(surface.width, surface.height)?;
        let mut noise = Noise {
            target,
            bitmap,
            sample,
            layers: Vec::new(),
            updated: now,
            paused: false,
        };
        let result = (|| {
            for (overlay, visible) in [false, true].into_iter().zip(settings.visible) {
                let layer = graphics.texture_from_surface(bitmap, None, [0, 0])?;
                noise.layers.push(layer);
                graphics.texture_opacity(layer, settings.opacity);
                if graphics.attach_layer(target, layer, settings.priority, 0, overlay, false) != 0 {
                    return Err(invalid());
                }
                graphics.show_texture(layer, visible);
            }
            graphics.next_surface()
        })();
        match result {
            Ok(id) => {
                noise.update(graphics, random);
                self.noise.insert(id, noise);
                Ok(id)
            }
            Err(error) => {
                noise.free(graphics);
                Err(error)
            }
        }
    }

    pub(crate) fn configure_noise(
        &self,
        id: i32,
        visible: [bool; 2],
        graphics: &mut Graphics,
    ) -> i32 {
        let Some(noise) = self.noise.get(&id) else {
            return -2;
        };
        for (&layer, visible) in noise.layers.iter().zip(visible) {
            graphics.show_texture(layer, visible);
        }
        1
    }

    pub(crate) fn is_noise(&self, id: i32) -> bool {
        self.noise.contains_key(&id)
    }

    pub(crate) fn tick(&mut self, now: u32, graphics: &mut Graphics, random: &mut Random) {
        self.sprites.retain(|_, motion| motion.tick(now, graphics));
        self.pulses.retain(|_, pulse| pulse.tick(now, graphics));
        self.zoom.retain(|_, zoom| zoom.tick(now, graphics));
        self.rotations
            .retain(|_, rotation| rotation.tick(now, graphics));
        self.background_transforms
            .retain(|_, effect| effect.tick(now, graphics));
        self.background
            .retain(|_, motion| motion.draw(now, graphics));
        self.haze.retain(|_, effect| effect.tick(now, graphics));
        self.particles.retain(|_, particles| {
            let playing = particles.tick(now, graphics, random);
            if !playing {
                particles.free(graphics);
            }
            playing
        });
        self.quakes
            .retain(|_, effect| effect.tick(now, graphics, random));
        self.noise.retain(|_, effect| {
            if graphics.surface(effect.target).is_err() || graphics.surface(effect.bitmap).is_err()
            {
                effect.free(graphics);
                return false;
            }
            if !effect.paused && now.wrapping_sub(effect.updated) >= 100 {
                effect.updated = now;
                effect.update(graphics, random);
            }
            true
        });
    }

    pub(crate) fn playing(&self, handle: i32) -> bool {
        self.background.contains_key(&handle)
            || self.haze.contains_key(&handle)
            || self.quakes.contains_key(&handle)
            || self.noise.contains_key(&handle)
            || self.particles.contains_key(&handle)
            || self.sprites.contains_key(&handle)
            || self.pulses.contains_key(&handle)
            || self.zoom.contains_key(&handle)
            || self.rotations.contains_key(&handle)
            || self
                .background_transforms
                .get(&handle)
                .is_some_and(|effect| !effect.finished)
    }

    pub(crate) fn pause(&mut self, handle: i32, now: u32, paused: bool) -> bool {
        if let Some(rotation) = self.rotations.get_mut(&handle) {
            rotation.pause(now, paused);
            return true;
        }
        if let Some(pulse) = self.pulses.get_mut(&handle) {
            pulse.pause(now, paused);
            return true;
        }
        if let Some(particles) = self.particles.get_mut(&handle) {
            particles.paused = paused;
            return true;
        }
        if let Some(effect) = self.background_transforms.get_mut(&handle) {
            if paused {
                effect.paused_at.get_or_insert(now);
            } else if let Some(since) = effect.paused_at.take() {
                effect.started = effect.started.wrapping_add(now.wrapping_sub(since));
                effect.last = now;
            }
            return true;
        }
        if let Some(zoom) = self.zoom.get_mut(&handle) {
            zoom.pause(now, paused);
            return true;
        }
        if let Some(motion) = self.sprites.get_mut(&handle) {
            motion.pause(now, paused);
            return true;
        }
        if let Some(effect) = self.noise.get_mut(&handle) {
            effect.paused = paused;
            return true;
        }
        if let Some(effect) = self.quakes.get_mut(&handle) {
            effect.pause(now, paused);
            return true;
        }
        if let Some(effect) = self.haze.get_mut(&handle) {
            if paused {
                effect.paused_at.get_or_insert(now);
            } else if let Some(since) = effect.paused_at.take() {
                effect.started = effect.started.wrapping_add(now.wrapping_sub(since));
            }
            return true;
        }
        let Some(motion) = self.background.get_mut(&handle) else {
            return false;
        };
        if paused {
            motion.paused_at.get_or_insert(now);
        } else if let Some(since) = motion.paused_at.take() {
            motion.started = motion.started.wrapping_add(now.wrapping_sub(since));
        }
        true
    }

    pub(crate) fn stop(&mut self, handle: i32, graphics: &mut Graphics) -> bool {
        if let Some(rotation) = self.rotations.remove(&handle) {
            graphics.rotate_background(rotation.target, rotation.end);
            return true;
        }
        if let Some(pulse) = self.pulses.remove(&handle) {
            graphics.stretch_texture(pulse.target, None);
            return true;
        }
        if let Some(particles) = self.particles.remove(&handle) {
            particles.free(graphics);
            return true;
        }
        if let Some(effect) = self.background_transforms.remove(&handle) {
            if !effect.settings.remove_on_finish && !effect.finished {
                effect.draw(1.0, graphics);
            }
            effect.clear(graphics);
            return true;
        }
        if let Some(zoom) = self.zoom.remove(&handle) {
            graphics.set_background_window(zoom.target, zoom.end);
            return true;
        }
        if let Some(motion) = self.sprites.remove(&handle) {
            motion.finish(graphics);
            return true;
        }
        if let Some(effect) = self.noise.remove(&handle) {
            effect.free(graphics);
            return true;
        }
        if let Some(effect) = self.quakes.remove(&handle) {
            graphics.set_scene_offset(effect.target, SceneOffset::default());
            return true;
        }
        if let Some(effect) = self.haze.remove(&handle) {
            graphics.clear_background_warp(effect.target);
            return true;
        }
        let Some(motion) = self.background.remove(&handle) else {
            return false;
        };
        graphics.shift_background(motion.target, motion.end);
        true
    }

    pub(crate) fn stop_scene_effects(&mut self, graphics: &mut Graphics) {
        for particles in std::mem::take(&mut self.particles).into_values() {
            particles.free(graphics);
        }
        for effect in std::mem::take(&mut self.quakes).into_values() {
            graphics.set_scene_offset(effect.target, SceneOffset::default());
        }
        for effect in std::mem::take(&mut self.noise).into_values() {
            effect.free(graphics);
        }
    }

    pub(crate) fn pause_scene_effects(&mut self, now: u32, paused: bool) {
        for particles in self.particles.values_mut() {
            particles.paused = paused;
        }
        for effect in self.quakes.values_mut() {
            effect.pause(now, paused);
        }
        for effect in self.noise.values_mut() {
            effect.paused = paused;
        }
    }

    pub(crate) fn stop_background(&mut self, graphics: &mut Graphics) {
        for rotation in std::mem::take(&mut self.rotations).into_values() {
            graphics.rotate_background(rotation.target, rotation.end);
        }
        for effect in std::mem::take(&mut self.background_transforms).into_values() {
            effect.clear(graphics);
        }
        for zoom in std::mem::take(&mut self.zoom).into_values() {
            graphics.set_background_window(zoom.target, zoom.end);
        }
        for (_, motion) in std::mem::take(&mut self.background) {
            graphics.shift_background(motion.target, motion.end);
        }
        for (_, effect) in std::mem::take(&mut self.haze) {
            graphics.clear_background_warp(effect.target);
        }
    }

    pub(crate) fn end(&self, handle: i32) -> Option<[i32; 2]> {
        self.background.get(&handle).map(|motion| motion.end)
    }

    pub(crate) fn start_haze(
        &mut self,
        target: i32,
        spec: &str,
        samples: u32,
        timing: HazeTiming,
        now: u32,
        graphics: &mut Graphics,
    ) -> io::Result<i32> {
        if timing.period == 0 || graphics.background_geometry(target).is_none() {
            return Ok(0);
        }
        let table = haze_table(spec, samples)?;
        let id = graphics.next_surface()?;
        self.haze.insert(
            id,
            Haze {
                target,
                table,
                started: now,
                paused_at: None,
                timing,
                decay_started: None,
            },
        );
        Ok(id)
    }

    pub(crate) fn fade_haze(&mut self, handle: i32, now: u32) -> Option<i32> {
        self.haze
            .get_mut(&handle)
            .map(|effect| i32::from(!effect.end(now)))
    }

    pub(crate) fn pause_background(&mut self, now: u32, paused: bool) {
        let handles: Vec<_> = self
            .background
            .keys()
            .chain(self.haze.keys())
            .chain(self.zoom.keys())
            .chain(self.rotations.keys())
            .chain(self.background_transforms.keys())
            .copied()
            .collect();
        for handle in handles {
            self.pause(handle, now, paused);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_pan_advances_the_bitmap_source_coordinates() {
        let mut graphics = Graphics::open(1, 1).unwrap();
        let display = graphics.display();
        let bitmap = graphics.create(3, 1).unwrap();
        let colors = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
        for (x, color) in colors.into_iter().enumerate() {
            graphics.fill(bitmap, Some([x as i32, 0, x as i32 + 1, 1]), color);
        }
        graphics
            .set_background_image(display, bitmap, [0, 0], false)
            .unwrap();
        let mut effects = Effects::default();
        let handle = effects
            .start_background(display, "a0 r", 100, 0, &mut graphics)
            .unwrap();
        assert_ne!(handle, 0);
        let mut random = Random::seeded(0);
        for (now, expected) in [0, 50, 100].into_iter().zip(colors) {
            effects.tick(now, &mut graphics, &mut random);
            graphics.present_display().unwrap();
            assert_eq!(graphics.presented().unwrap().pixels(), &expected);
        }
        assert!(!effects.playing(handle));
    }

    #[test]
    fn background_zoom_scales_the_crop_around_its_requested_center() {
        let mut graphics = Graphics::open(2, 1).unwrap();
        let display = graphics.display();
        let bitmap = graphics.create(4, 1).unwrap();
        let green = [0, 255, 0, 255];
        graphics.fill(bitmap, None, [255, 0, 0, 255]);
        graphics.fill(bitmap, Some([1, 0, 2, 1]), green);
        graphics
            .set_background_image(display, bitmap, [0, 0], false)
            .unwrap();
        let mut effects = Effects::default();
        let handle = effects
            .start_zoom(
                display,
                ZoomSettings {
                    center: [1, 0],
                    scale: 2.0,
                    ease: 0.0,
                    duration: 100,
                },
                0,
                &mut graphics,
            )
            .unwrap();
        assert_eq!(effects.zoom_end(handle), Some([1, 0, 1, 1]));
        effects.tick(100, &mut graphics, &mut Random::seeded(0));
        graphics.present_display().unwrap();
        assert_eq!(
            graphics.presented().unwrap().pixels(),
            [green, green].concat()
        );
    }

    #[test]
    fn background_rotation_keeps_the_final_pose_and_clips_to_the_canvas() {
        let mut graphics = Graphics::open(3, 1).unwrap();
        let display = graphics.display();
        let bitmap = graphics.create(3, 1).unwrap();
        let black = [0, 0, 0, 255];
        let red = [255, 0, 0, 255];
        graphics.fill(bitmap, None, black);
        graphics.fill(bitmap, Some([0, 0, 1, 1]), red);
        graphics
            .set_background_image(display, bitmap, [0, 0], false)
            .unwrap();
        let mut effects = Effects::default();
        let pose = BackgroundRotation {
            offset: [2, 0],
            turns: 0,
            scale: 1.0,
        };
        let handle = effects
            .start_background_rotation(display, pose, 100, 0.0, 0, &mut graphics)
            .unwrap();
        let mut random = Random::seeded(0);
        effects.tick(50, &mut graphics, &mut random);
        graphics.present_display().unwrap();
        assert_eq!(
            graphics.presented().unwrap().pixels(),
            [black, red, black].concat()
        );
        effects.tick(100, &mut graphics, &mut random);
        graphics.present_display().unwrap();
        assert_eq!(
            graphics.presented().unwrap().pixels(),
            [black, black, red].concat()
        );
        assert!(!effects.playing(handle));
    }

    #[test]
    fn background_transform_preserves_the_overlay_and_clears_its_buffer() {
        let red = [255, 0, 0, 255];
        let green = [0, 255, 0, 255];
        let blue = [0, 0, 255, 255];
        let mut graphics = Graphics::open(3, 1).unwrap();
        let target = graphics.display();
        let background = graphics.create(3, 1).unwrap();
        graphics.fill(background, None, red);
        graphics
            .set_background_image(target, background, [0, 0], false)
            .unwrap();
        let bitmap = graphics.create(1, 1).unwrap();
        graphics.fill(bitmap, None, green);
        let overlay = graphics.texture_from_surface(bitmap, None, [0, 0]).unwrap();
        graphics.attach_layer(target, overlay, 0, 0, true, false);
        let image = graphics.create(1, 1).unwrap();
        graphics.fill(image, None, blue);
        let mut effects = Effects::default();
        let task = effects
            .start_background_transform(
                target,
                BackgroundTransformSettings {
                    path: BackgroundTransformPath::parse("(0,0,1)(0,0,3)", false).unwrap(),
                    bitmap: Some(graphics.surface(image).unwrap().clone()),
                    duration: 100,
                    opacity: 255,
                    sampling: 0,
                    flip: [false, false],
                    ease: 0.0,
                    in_place: false,
                    remove_on_finish: true,
                },
                0,
                &mut graphics,
            )
            .unwrap();
        let mut random = Random::seeded(0);
        for (time, expected) in [(50, [green, blue, red]), (100, [green, red, red])] {
            effects.tick(time, &mut graphics, &mut random);
            graphics.present_display().unwrap();
            assert_eq!(graphics.presented().unwrap().pixels(), expected.concat());
        }
        assert!(!effects.playing(task));
    }

    #[test]
    fn sprite_motion_advances_each_timed_placement() {
        let mut graphics = Graphics::open(100, 8).unwrap();
        let target = graphics.display();
        let image = graphics.create(1, 1).unwrap();
        let sprite = graphics.texture_from_surface(image, None, [0, 0]).unwrap();
        assert_eq!(graphics.attach_layer(target, sprite, 0, 0, false, false), 0);
        let mut effects = Effects::default();
        let mut random = Random::seeded(0);
        let handle = effects
            .start_sprite(sprite, "a0 100>1 50<1", 10, 0, &mut graphics)
            .unwrap();
        assert_ne!(handle, 0);
        effects.tick(50, &mut graphics, &mut random);
        assert_eq!(graphics.texture_bounds(sprite).unwrap()[0], 12);
        effects.tick(100, &mut graphics, &mut random);
        assert_eq!(graphics.texture_bounds(sprite).unwrap()[0], 25);
        effects.tick(150, &mut graphics, &mut random);
        assert_eq!(graphics.texture_bounds(sprite).unwrap()[0], 0);
        assert!(!effects.playing(handle));
    }

    #[test]
    fn shrinking_sprite_pulse_keeps_its_bottom_edge_and_restores_its_size() {
        let mut graphics = Graphics::open(5, 5).unwrap();
        let display = graphics.display();
        let image = graphics.create(2, 2).unwrap();
        let color = [0, 0, 255, 255];
        graphics.fill(image, None, color);
        let sprite = graphics.texture_from_surface(image, None, [2, 1]).unwrap();
        graphics.attach_layer(display, sprite, 0, 0, false, false);
        let mut effects = Effects::default();
        let handle = effects
            .start_sprite_pulse(
                sprite,
                SpritePulseSettings {
                    duration: 100,
                    scale: 0.5,
                    centered: false,
                    ease: 0.0,
                },
                0,
                &mut graphics,
            )
            .unwrap();
        let mut random = Random::seeded(1);
        effects.tick(50, &mut graphics, &mut random);
        assert_eq!(graphics.texture_bounds(sprite), Some([2, 2, 3, 3]));
        graphics.present_display().unwrap();
        assert_eq!(&graphics.presented().unwrap().pixels()[48..52], &color);
        effects.tick(100, &mut graphics, &mut random);
        assert_eq!(graphics.texture_bounds(sprite), Some([2, 1, 4, 3]));
        assert!(!effects.playing(handle));
    }

    #[test]
    fn quake_moves_the_primary_pass_without_moving_the_overlay() {
        let red = [255, 0, 0, 255];
        let green = [0, 255, 0, 255];
        let blue = [0, 0, 255, 255];
        let mut graphics = Graphics::open(3, 1).unwrap();
        let display = graphics.display();
        graphics.fill(display, None, red);
        graphics.fill(display, Some([1, 0, 2, 1]), blue);
        let bitmap = graphics.create(1, 1).unwrap();
        graphics.fill(bitmap, None, green);
        let overlay = graphics.texture_from_surface(bitmap, None, [0, 0]).unwrap();
        assert_eq!(
            graphics.attach_layer(display, overlay, 0, 0, true, false),
            0
        );
        let mut effects = Effects::default();
        let mut random = Random::seeded(0);
        let settings = QuakeSettings {
            duration: 100,
            frequency: 1,
            amplitude: [1, 0],
            end_gain: 1.0,
            fill_edges: true,
            primary: true,
            overlay: false,
        };
        let task = effects
            .start_quake(display, settings, 0, &mut graphics)
            .unwrap();
        effects.tick(50, &mut graphics, &mut random);
        graphics.present_display().unwrap();
        assert_eq!(
            graphics.presented().unwrap().pixels(),
            [green, red, blue].concat()
        );
        effects.tick(100, &mut graphics, &mut random);
        graphics.present_display().unwrap();
        assert_eq!(
            graphics.presented().unwrap().pixels(),
            [green, blue, red].concat()
        );
        assert!(!effects.playing(task));
    }

    #[test]
    fn haze_distorts_background_pixels_and_stops_at_the_fade_boundary() {
        let mut graphics = Graphics::open(2, 2).unwrap();
        let display = graphics.display();
        let bitmap = graphics.create(2, 2).unwrap();
        let red = [255, 0, 0, 255];
        let blue = [0, 0, 255, 255];
        let black = [0, 0, 0, 255];
        graphics.fill(bitmap, Some([0, 0, 2, 1]), red);
        graphics.fill(bitmap, Some([0, 1, 2, 2]), blue);
        graphics
            .set_background_image(display, bitmap, [0, 0], false)
            .unwrap();
        let mut effects = Effects::default();
        let handle = effects
            .start_haze(
                display,
                "(1,0,1)",
                2,
                HazeTiming {
                    period: 100,
                    sustain: 20,
                    attack: 0,
                    decay: 10,
                    fill_edges: true,
                },
                0,
                &mut graphics,
            )
            .unwrap();
        let mut random = Random::seeded(0);
        effects.tick(0, &mut graphics, &mut random);
        graphics.present_display().unwrap();
        assert_eq!(
            graphics.presented().unwrap().pixels(),
            [blue, black, black, red].concat()
        );
        effects.tick(30, &mut graphics, &mut random);
        graphics.present_display().unwrap();
        assert_eq!(
            graphics.presented().unwrap().pixels(),
            [red, red, blue, blue].concat()
        );
        assert!(!effects.playing(handle));
    }
}
