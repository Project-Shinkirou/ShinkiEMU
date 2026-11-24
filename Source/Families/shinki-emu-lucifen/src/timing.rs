use std::time::{Duration, Instant};

#[derive(Clone)]
struct Timer {
    started: u32,
    paused_at: u32,
    pauses: u32,
}

impl Timer {
    fn new(now: u32) -> Self {
        Self {
            started: now,
            paused_at: 0,
            pauses: 0,
        }
    }

    fn instant(&self, now: u32) -> u32 {
        if self.pauses == 0 {
            now
        } else {
            self.paused_at
        }
    }

    fn pause(&mut self, now: u32) {
        if self.pauses == 0 {
            self.paused_at = now;
        }
        self.pauses = self.pauses.saturating_add(1);
    }

    fn resume(&mut self, now: u32) {
        if self.pauses == 0 {
            return;
        }
        self.pauses -= 1;
        if self.pauses == 0 {
            self.started = self.started.wrapping_add(now.wrapping_sub(self.paused_at));
        }
    }
}

pub(crate) struct Timers {
    start: Instant,
    frozen_at: Option<Instant>,
    frozen_time: Duration,
    slots: Vec<Timer>,
}

impl Default for Timers {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            frozen_at: None,
            frozen_time: Duration::ZERO,
            slots: Vec::new(),
        }
    }
}

impl Timers {
    pub(crate) fn set_suspended(&mut self, paused: bool) {
        if paused {
            self.frozen_at.get_or_insert_with(Instant::now);
        } else if let Some(since) = self.frozen_at.take() {
            self.frozen_time += since.elapsed();
        }
    }

    pub(crate) fn now(&self) -> u32 {
        (self
            .frozen_at
            .unwrap_or_else(Instant::now)
            .duration_since(self.start)
            - self.frozen_time)
            .as_millis() as u32
    }

    pub(crate) fn control(&mut self, function: u32, slot: u16, value: u32) -> i32 {
        let now = self.now();
        match function {
            536 => {
                self.slots = vec![Timer::new(now); usize::from(slot)];
                i32::from(slot == 0)
            }
            537 => {
                if let Some(timer) = self.slots.get_mut(usize::from(slot)) {
                    *timer = Timer::new(now);
                    1
                } else {
                    0
                }
            }
            538 => self.slots.get(usize::from(slot)).map_or(0, |timer| {
                timer.instant(now).wrapping_sub(timer.started) as i32
            }),
            539 => {
                if let Some(timer) = self.slots.get_mut(usize::from(slot)) {
                    timer.started = timer.instant(now).wrapping_sub(value);
                }
                0
            }
            540 | 541 => {
                if let Some(timer) = self.slots.get_mut(usize::from(slot)) {
                    if function == 540 {
                        timer.pause(now);
                    } else {
                        timer.resume(now);
                    }
                }
                0
            }
            542 | 543 => {
                for timer in &mut self.slots {
                    if function == 542 {
                        timer.pause(now);
                    } else {
                        timer.resume(now);
                    }
                }
                0
            }
            _ => unreachable!("Lucifen timer opcode"),
        }
    }
}
