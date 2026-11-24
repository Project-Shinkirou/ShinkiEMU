use crate::Graphics;
use shinki_emu_lucifen_formats::Lag;
use std::{collections::BTreeMap, io};

struct Player {
    clip: i32,
    sequence: usize,
    frame: usize,
    started: u32,
    repeats: i32,
    catch_up: bool,
    detach_on_end: bool,
    finished: bool,
    paused_at: Option<u32>,
}

pub(crate) struct Playback {
    pub target: i32,
    pub clip: i32,
    pub sequence: usize,
    pub repeats: i32,
    pub priority: u32,
    pub catch_up: bool,
    pub detach_on_end: bool,
}

pub(crate) struct MotionPoint {
    pub(crate) position: [i32; 2],
    pub(crate) opacity: u8,
}

pub(crate) enum MotionMode {
    Opacity,
    Position,
    SourceOffset,
}

pub(crate) struct Motion {
    pub(crate) texture: i32,
    pub(crate) interval: u32,
    pub(crate) delay: u32,
    pub(crate) hide_after: bool,
    pub(crate) path: Vec<MotionPoint>,
    pub(crate) mode: MotionMode,
    pub(crate) curve: bool,
    pub(crate) ease: f64,
    pub(crate) repeat: bool,
}

struct MovingSprite {
    motion: Motion,
    started: u32,
    paused_at: Option<u32>,
    curve: Vec<[f64; 2]>,
    task_type: i32,
}

impl MovingSprite {
    fn finish(&self, gfx: &mut Graphics) {
        let last = self.motion.path.last().unwrap();
        self.apply(last.position, last.opacity, gfx);
        if self.motion.hide_after {
            gfx.show_texture(self.motion.texture, false);
        }
    }

    fn apply(&self, position: [i32; 2], opacity: u8, gfx: &mut Graphics) {
        match self.motion.mode {
            MotionMode::Opacity => {}
            MotionMode::Position => {
                gfx.texture_move(self.motion.texture, position[0], position[1], false);
            }
            MotionMode::SourceOffset => {
                gfx.move_texture_source(self.motion.texture, position);
            }
        }
        gfx.texture_opacity(self.motion.texture, opacity);
    }

    fn tick(&self, now: u32, gfx: &mut Graphics) -> bool {
        if gfx.texture_state(self.motion.texture).is_none() {
            return false;
        }
        if self.paused_at.is_some() {
            return true;
        }
        let Some(elapsed) = now
            .wrapping_sub(self.started)
            .checked_sub(self.motion.delay)
        else {
            return true;
        };
        let intervals = (self.motion.path.len() - 1) as u64;
        let duration = u64::from(self.motion.interval) * intervals;
        if !self.motion.repeat && u64::from(elapsed) >= duration {
            self.finish(gfx);
            return false;
        }
        if self.motion.hide_after {
            gfx.show_texture(self.motion.texture, true);
        }
        let progress = (u64::from(elapsed) % duration) as f64 / duration as f64;
        let progress = if self.motion.ease > 0.0 {
            progress.powf(self.motion.ease)
        } else if self.motion.ease < 0.0 {
            1.0 - (1.0 - progress).powf(-self.motion.ease)
        } else {
            progress
        } * intervals as f64;
        let segment = (progress as usize).min(self.motion.path.len() - 2);
        let fraction = progress - segment as f64;
        let a = &self.motion.path[segment];
        let b = &self.motion.path[segment + 1];
        let position = std::array::from_fn(|axis| {
            let mut value = f64::from(a.position[axis]) * (1.0 - fraction)
                + f64::from(b.position[axis]) * fraction;
            if self.motion.curve {
                let rest = 1.0 - fraction;
                value += self.curve[segment][axis] * (rest * rest * rest - rest)
                    + self.curve[segment + 1][axis] * (fraction * fraction * fraction - fraction);
            }
            value as i32
        });
        let opacity =
            (f64::from(a.opacity) * (1.0 - fraction) + f64::from(b.opacity) * fraction) as u8;
        self.apply(position, opacity, gfx);
        true
    }
}

fn spline(path: &[MotionPoint]) -> Vec<[f64; 2]> {
    let mut values = vec![[0.0; 2]; path.len()];
    let mut diagonal = vec![1.0; path.len()];
    for i in 1..path.len() - 1 {
        let prev = if i == 1 { 0.0 } else { 1.0 / diagonal[i - 1] };
        diagonal[i] = 4.0 - prev;
        for axis in 0..2 {
            values[i][axis] = f64::from(path[i - 1].position[axis])
                - 2.0 * f64::from(path[i].position[axis])
                + f64::from(path[i + 1].position[axis])
                - prev * values[i - 1][axis];
        }
    }
    for i in (1..path.len() - 1).rev() {
        for axis in 0..2 {
            values[i][axis] = (values[i][axis] - values[i + 1][axis]) / diagonal[i];
        }
    }
    values
}

#[derive(Default)]
pub(crate) struct Animations {
    clips: BTreeMap<i32, Lag>,
    players: BTreeMap<i32, Player>,
    moving: BTreeMap<i32, MovingSprite>,
}

impl Animations {
    pub(crate) fn insert(&mut self, id: i32, data: Vec<u8>) -> io::Result<()> {
        self.clips.insert(id, Lag::parse(data)?);
        Ok(())
    }

    pub(crate) fn remove(&mut self, id: i32, gfx: &mut Graphics) -> bool {
        self.players.retain(|&texture, player| {
            if player.clip == id {
                gfx.free_texture(texture);
                false
            } else {
                true
            }
        });
        self.clips.remove(&id).is_some()
    }

    pub(crate) fn clear(&mut self, gfx: &mut Graphics) {
        for &texture in self.players.keys() {
            gfx.free_texture(texture);
        }
        self.players.clear();
        self.clips.clear();
    }

    pub(crate) fn start(
        &mut self,
        request: Playback,
        now: u32,
        gfx: &mut Graphics,
    ) -> io::Result<i32> {
        let clip = self.clips.get(&request.clip).ok_or_else(invalid)?;
        let frame = clip
            .sequence(request.sequence)
            .and_then(|sequence| sequence.first())
            .ok_or_else(invalid)?;
        let bytes = clip.image(frame.image).ok_or_else(invalid)?;
        let bitmap = gfx.load(bytes)?;
        let texture = gfx.texture_from_surface(bitmap, None, [frame.x.into(), frame.y.into()])?;
        gfx.free(bitmap);
        let status = gfx.attach_layer(request.target, texture, request.priority, 0, false, false);
        if status != 0 {
            gfx.free_texture(texture);
            return Ok(0);
        }
        self.players.insert(
            texture,
            Player {
                clip: request.clip,
                sequence: request.sequence,
                frame: 0,
                started: now,
                repeats: request.repeats,
                catch_up: request.catch_up,
                detach_on_end: request.detach_on_end,
                finished: false,
                paused_at: None,
            },
        );
        Ok(texture)
    }

    pub(crate) fn remove_player(&mut self, texture: i32, gfx: &mut Graphics) -> bool {
        if self.players.remove(&texture).is_some() {
            gfx.free_texture(texture);
            true
        } else if let Some(motion) = self.moving.remove(&texture) {
            motion.finish(gfx);
            true
        } else {
            false
        }
    }

    pub(crate) fn stop_tasks(&mut self, task_type: i32, gfx: &mut Graphics) {
        if task_type == 13000 {
            for &texture in self.players.keys() {
                gfx.free_texture(texture);
            }
            self.players.clear();
        }
        self.moving.retain(|_, moving| {
            if moving.task_type != task_type {
                return true;
            }
            moving.finish(gfx);
            false
        });
    }

    pub(crate) fn pause_player(&mut self, texture: i32, now: u32, pause: bool) -> bool {
        if let Some(motion) = self.moving.get_mut(&texture) {
            if pause {
                motion.paused_at.get_or_insert(now);
            } else if let Some(then) = motion.paused_at.take() {
                motion.started = motion.started.wrapping_add(now.wrapping_sub(then));
            }
            return true;
        }
        let Some(player) = self.players.get_mut(&texture) else {
            return false;
        };
        if pause {
            player.paused_at.get_or_insert(now);
        } else if let Some(then) = player.paused_at.take() {
            player.started = player.started.wrapping_add(now.wrapping_sub(then));
        }
        true
    }

    pub(crate) fn status(&self, texture: i32) -> i32 {
        if self.moving.contains_key(&texture) {
            return 1;
        }
        self.players
            .get(&texture)
            .map_or(0, |player| if player.finished { 2 } else { 1 })
    }

    pub(crate) fn pause_tasks(&mut self, kind: i32, now: u32, paused: bool) {
        let mut handles: Vec<_> = self
            .moving
            .iter()
            .filter_map(|(&id, player)| (kind < 0 || player.task_type == kind).then_some(id))
            .collect();
        if kind < 0 || kind == 13000 {
            handles.extend(self.players.keys().copied());
        }
        for id in handles {
            self.pause_player(id, now, paused);
        }
    }

    pub(crate) fn tick(&mut self, now: u32, gfx: &mut Graphics) -> io::Result<()> {
        self.moving.retain(|_, moving| moving.tick(now, gfx));
        for (&texture, player) in &mut self.players {
            if player.finished || player.paused_at.is_some() {
                continue;
            }
            let clip = self.clips.get(&player.clip).ok_or_else(invalid)?;
            let sequence = clip
                .sequence(player.sequence)
                .filter(|sequence| !sequence.is_empty())
                .ok_or_else(invalid)?;
            let previous = player.frame;
            for _ in 0..sequence.len().saturating_add(1) {
                let duration = sequence[player.frame].milliseconds.max(1);
                if now.wrapping_sub(player.started) < duration {
                    break;
                }
                if player.frame + 1 < sequence.len() {
                    player.frame += 1;
                } else if player.repeats < 0 || player.repeats > 1 {
                    player.frame = 0;
                    if player.repeats > 1 {
                        player.repeats -= 1;
                    }
                } else {
                    player.finished = true;
                    if player.detach_on_end || player.repeats == 1 {
                        gfx.free_texture(texture);
                    }
                    break;
                }
                player.started = if player.catch_up {
                    player.started.wrapping_add(duration)
                } else {
                    now
                };
                if !player.catch_up {
                    break;
                }
            }
            if !player.finished && previous != player.frame {
                let frame = sequence[player.frame];
                let image = clip.image(frame.image).ok_or_else(invalid)?;
                gfx.replace_texture(texture, image, [frame.x.into(), frame.y.into()])?;
            }
        }
        self.players.retain(|_, player| {
            !(player.finished && (player.detach_on_end || player.repeats == 1))
        });
        Ok(())
    }

    pub(crate) fn start_motion(
        &mut self,
        mut motion: Motion,
        now: u32,
        gfx: &mut Graphics,
    ) -> io::Result<i32> {
        let Some((position, source_offset, opacity)) = gfx.texture_state(motion.texture) else {
            return Ok(0);
        };
        if motion.path.is_empty() || motion.curve && motion.path.len() < 2 {
            return Ok(0);
        }
        let position = if matches!(motion.mode, MotionMode::SourceOffset) {
            source_offset
        } else {
            position
        };
        motion.path.insert(0, MotionPoint { position, opacity });
        let curve = if motion.curve {
            spline(&motion.path)
        } else {
            Vec::new()
        };
        let moving = MovingSprite {
            task_type: 12003 + i32::from(gfx.is_overlay(motion.texture)),
            motion,
            started: now,
            paused_at: None,
            curve,
        };
        if moving.motion.interval == 0 {
            moving.finish(gfx);
            return Ok(0);
        }
        let task = gfx.next_surface()?;
        self.moving.insert(task, moving);
        Ok(task)
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid Lucifen animation handle or sequence",
    )
}
