use std::{collections::BTreeMap, io, sync::Arc};

use crate::{Graphics, Surface, graphics::BitmapTarget, random::Random};

pub(crate) struct Strip {
    pub(crate) bitmap: Arc<Surface>,
    pub(crate) destination: BitmapTarget,
    pub(crate) position: [i32; 2],
    pub(crate) frames: u32,
    pub(crate) frame_ms: u32,
    pub(crate) sequence: u8,
    pub(crate) on_stop: u8,
    pub(crate) transparent: bool,
    pub(crate) redraw: Vec<i32>,
}

pub(crate) struct StripPlayback {
    pub(crate) repeats: u8,
    pub(crate) interval: u32,
    pub(crate) jitter: u32,
    pub(crate) delayed: bool,
}

impl Strip {
    fn width(&self) -> i64 {
        i64::from(self.bitmap.width / self.frames)
    }

    fn restore(&self, previous: &Surface, graphics: &mut Graphics) -> bool {
        let [x, y] = self.position.map(i64::from);
        graphics.stamp_bitmap(
            self.destination,
            previous,
            [x, y, x + self.width(), y + i64::from(self.bitmap.height)],
            self.position,
            false,
        )
    }

    fn stamp(&self, frame: u32, previous: &Surface, graphics: &mut Graphics) -> bool {
        if self.transparent && !self.restore(previous, graphics) {
            return false;
        }
        let left = i64::from(frame) * self.width();
        let alive = graphics.stamp_bitmap(
            self.destination,
            &self.bitmap,
            [left, 0, left + self.width(), i64::from(self.bitmap.height)],
            self.position,
            self.transparent,
        );
        for &id in &self.redraw {
            graphics.invalidate_texture(id);
        }
        alive
    }
}

struct Player {
    clip: i32,
    previous: Arc<Surface>,
    started: u32,
    frame: Option<u32>,
    random_tick: Option<u32>,
    delay: u32,
    interval: u32,
    jitter: u32,
    repeats: u8,
    paused_at: Option<u32>,
}

impl Player {
    fn stop(&self, clip: &Strip, graphics: &mut Graphics) {
        match clip.on_stop {
            0 => {
                clip.stamp(0, &self.previous, graphics);
            }
            1 => {}
            _ => {
                clip.restore(&self.previous, graphics);
                for &id in &clip.redraw {
                    graphics.invalidate_texture(id);
                }
            }
        }
    }

    fn tick(
        &mut self,
        clip: &Strip,
        now: u32,
        graphics: &mut Graphics,
        random: &mut Random,
    ) -> bool {
        if graphics.bitmap(clip.destination).is_none() {
            return false;
        }
        if self.paused_at.is_some() {
            return true;
        }
        let Some(elapsed) = now.wrapping_sub(self.started).checked_sub(self.delay) else {
            return true;
        };
        let step = elapsed / clip.frame_ms;
        let length = match clip.sequence {
            0 => clip.frames,
            1 => clip.frames.saturating_mul(2) - 1,
            _ => 0,
        };
        let frame = if length == 0 {
            if self.random_tick == Some(step) {
                return true;
            }
            self.random_tick = Some(step);
            let mut next = random.below(clip.frames) as u32;
            if self.frame == Some(next) {
                next = (next + 1) % clip.frames;
            }
            next
        } else {
            let step = if step >= length {
                if self.repeats == 1 {
                    self.stop(clip, graphics);
                    return false;
                }
                self.repeats = self.repeats.saturating_sub(1);
                self.delay = self
                    .interval
                    .saturating_add(random.below(self.jitter) as u32);
                self.started = now;
                length - 1
            } else {
                step
            };
            if step < clip.frames {
                step
            } else {
                2 * clip.frames - step - 2
            }
        };
        if self.frame == Some(frame) {
            return true;
        }
        self.frame = Some(frame);
        clip.stamp(frame, &self.previous, graphics)
    }
}

#[derive(Default)]
pub(crate) struct Strips {
    clips: BTreeMap<i32, Strip>,
    players: BTreeMap<i32, Player>,
}

impl Strips {
    pub(crate) fn insert(&mut self, id: i32, clip: Strip) {
        self.clips.insert(id, clip);
    }

    pub(crate) fn insert_redraw(&mut self, clip: i32, target: i32, add: bool) {
        if let Some(clip) = self.clips.get_mut(&clip) {
            if add {
                if target != 0 && !clip.redraw.contains(&target) {
                    clip.redraw.push(target);
                }
            } else {
                clip.redraw.retain(|&id| id != target);
            }
        }
    }

    pub(crate) fn remove(&mut self, id: i32, graphics: &mut Graphics) -> bool {
        let Some(clip) = self.clips.remove(&id) else {
            return false;
        };
        self.players.retain(|_, player| {
            if player.clip != id {
                return true;
            }
            player.stop(&clip, graphics);
            false
        });
        true
    }

    pub(crate) fn start(
        &mut self,
        clip: i32,
        playback: StripPlayback,
        now: u32,
        graphics: &mut Graphics,
        random: &mut Random,
    ) -> io::Result<i32> {
        let Some(previous) = self
            .clips
            .get(&clip)
            .and_then(|clip| graphics.bitmap(clip.destination))
            .cloned()
        else {
            return Ok(0);
        };
        let id = graphics.next_surface()?;
        let delay = if playback.delayed {
            playback
                .interval
                .saturating_add(random.below(playback.jitter) as u32)
        } else {
            0
        };
        self.players.insert(
            id,
            Player {
                clip,
                previous,
                started: now,
                frame: None,
                random_tick: None,
                delay,
                interval: playback.interval,
                jitter: playback.jitter,
                repeats: playback.repeats,
                paused_at: None,
            },
        );
        Ok(id)
    }

    pub(crate) fn playing(&self, handle: i32) -> bool {
        self.players.contains_key(&handle)
    }

    pub(crate) fn pause(&mut self, handle: i32, now: u32, paused: bool) -> bool {
        let Some(player) = self.players.get_mut(&handle) else {
            return false;
        };
        if paused {
            player.paused_at.get_or_insert(now);
        } else if let Some(since) = player.paused_at.take() {
            player.started = player.started.wrapping_add(now.wrapping_sub(since));
        }
        true
    }

    pub(crate) fn stop(&mut self, handle: i32, graphics: &mut Graphics) -> bool {
        let Some(player) = self.players.remove(&handle) else {
            return false;
        };
        if let Some(clip) = self.clips.get(&player.clip) {
            player.stop(clip, graphics);
        }
        true
    }

    pub(crate) fn pause_all(&mut self, now: u32, paused: bool) {
        let handles: Vec<_> = self.players.keys().copied().collect();
        for handle in handles {
            self.pause(handle, now, paused);
        }
    }

    pub(crate) fn stop_all(&mut self, graphics: &mut Graphics) {
        for (_, player) in std::mem::take(&mut self.players) {
            if let Some(clip) = self.clips.get(&player.clip) {
                player.stop(clip, graphics);
            }
        }
    }

    pub(crate) fn clear(&mut self, graphics: &mut Graphics) {
        self.stop_all(graphics);
        self.clips.clear();
    }

    pub(crate) fn tick(&mut self, now: u32, graphics: &mut Graphics, random: &mut Random) {
        self.players.retain(|_, player| {
            self.clips
                .get(&player.clip)
                .is_some_and(|clip| player.tick(clip, now, graphics, random))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_stamps_each_frame_and_restores_its_destination() {
        let mut graphics = Graphics::open(2, 1).unwrap();
        let target = graphics.display();
        let background = [5, 10, 15, 255];
        graphics.fill(target, None, background);
        let atlas = graphics.create(2, 1).unwrap();
        graphics.fill(atlas, Some([0, 0, 1, 1]), [100, 0, 0, 255]);
        graphics.fill(atlas, Some([1, 0, 2, 1]), [0, 100, 0, 255]);
        let mut strips = Strips::default();
        let clip = graphics.next_surface().unwrap();
        strips.insert(
            clip,
            Strip {
                bitmap: graphics.surface(atlas).unwrap().clone(),
                destination: BitmapTarget::Surface(target),
                position: [1, 0],
                frames: 2,
                frame_ms: 10,
                sequence: 0,
                on_stop: 2,
                transparent: false,
                redraw: Vec::new(),
            },
        );
        let mut random = Random::seeded(1);
        let task = strips
            .start(
                clip,
                StripPlayback {
                    repeats: 1,
                    interval: 0,
                    jitter: 0,
                    delayed: false,
                },
                0,
                &mut graphics,
                &mut random,
            )
            .unwrap();
        for (time, color) in [
            (0, [100, 0, 0, 255]),
            (10, [0, 100, 0, 255]),
            (20, background),
        ] {
            strips.tick(time, &mut graphics, &mut random);
            let output = graphics.surface(target).unwrap().pixels();
            assert_eq!(&output[..4], &background);
            assert_eq!(&output[4..], &color);
        }
        assert!(!strips.playing(task));
    }
}
