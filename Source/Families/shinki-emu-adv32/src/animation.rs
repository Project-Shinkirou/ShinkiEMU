use crate::render::Canvas;
use crate::{
    Error, Result,
    archive::Archive,
    crx::Crx,
    graphics::{blit_alpha, decode_archive_image},
};
use std::{sync::Arc, time::Duration};

const TICK: Duration = Duration::from_nanos(16_666_667);
const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// Streams numbered CRX frames from a native PCK. Only the displayed frame is
/// decoded after advancing the clock, even if presentation fell behind.
pub(crate) struct Animation {
    source: Arc<Archive>,
    frame: Crx,
    index: usize,
    interval: Duration,
    elapsed: Duration,
    repeat: bool,
}

impl Animation {
    pub(crate) fn new(source: Arc<Archive>, ticks: u16, repeat: bool) -> Result<Self> {
        Self::with_interval(source, TICK * u32::from(ticks.max(1)), repeat)
    }

    fn with_interval(source: Arc<Archive>, interval: Duration, repeat: bool) -> Result<Self> {
        if source.frame_count() == 0 {
            return Err(Error::Invalid("empty ADV32 animation"));
        }
        let frame = decode_archive_image(&source, &source.frame(0, MAX_FRAME_BYTES)?, 0)?;
        Ok(Self {
            source,
            frame,
            index: 0,
            interval,
            elapsed: Duration::ZERO,
            repeat,
        })
    }

    pub(crate) fn advance(&mut self, elapsed: Duration) -> Result<bool> {
        Ok(self.advance_frame(elapsed)?.is_some())
    }

    fn advance_frame(&mut self, elapsed: Duration) -> Result<Option<Crx>> {
        self.elapsed = self.elapsed.saturating_add(elapsed);
        let elapsed_frames = self.elapsed.as_nanos() / self.interval.as_nanos();
        let count = self.source.frame_count() as u128;
        let index = if self.repeat {
            elapsed_frames % count
        } else {
            elapsed_frames.min(count - 1)
        } as usize;
        if index == self.index {
            return Ok(None);
        }
        let frame =
            decode_archive_image(&self.source, &self.source.frame(index, MAX_FRAME_BYTES)?, 0)?;
        self.index = index;
        Ok(Some(std::mem::replace(&mut self.frame, frame)))
    }

    pub(crate) fn busy(&self) -> bool {
        !self.repeat
            && self.elapsed.as_nanos() / self.interval.as_nanos()
                < self.source.frame_count() as u128
    }

    pub(crate) fn complete(&mut self) -> Result<()> {
        if !self.repeat {
            self.advance(self.interval * self.source.frame_count() as u32)?;
        }
        Ok(())
    }

    pub(crate) fn frame(&self) -> &Crx {
        &self.frame
    }
}

pub(crate) struct StaffRoll {
    source: Archive,
    tiles: [Option<(usize, Crx)>; 2],
    elapsed: Duration,
    duration: Duration,
    hold_at: Duration,
    hold: Duration,
    height: u32,
    pixels: u32,
    opacity: u8,
    backdrop: Option<Arc<Crx>>,
    followup_backdrops: Option<[Arc<Crx>; 2]>,
    followup_at: Option<Duration>,
    slides: Option<Animation>,
    previous_slide: Option<(Crx, Duration)>,
}

impl StaffRoll {
    const HOLD: Duration = Duration::from_secs(5);
    const FADE: Duration = Duration::from_millis(3333);

    pub(crate) fn new(source: Archive, duration: Duration) -> Result<Self> {
        let count = source.frame_count();
        if count == 0 || duration.is_zero() {
            return Err(Error::Invalid("ADV32 staff roll"));
        }
        let first = decode_archive_image(&source, &source.frame(0, MAX_FRAME_BYTES)?, 0)?;
        let height = first.image.height();
        let hold_at = duration.mul_f64((count - 1) as f64 / count as f64);
        let mut roll = Self {
            source,
            tiles: [Some((0, first)), None],
            elapsed: Duration::ZERO,
            duration,
            hold_at,
            hold: Self::HOLD,
            height,
            pixels: 0,
            opacity: 255,
            backdrop: None,
            followup_backdrops: None,
            followup_at: None,
            slides: None,
            previous_slide: None,
        };
        roll.load_visible_tiles()?;
        Ok(roll)
    }

    pub(crate) fn classic(
        source: Archive,
        duration: Duration,
        backdrop: Arc<Crx>,
        slides: Option<Archive>,
    ) -> Result<Self> {
        let mut roll = Self::new(source, duration)?;
        roll.backdrop = Some(backdrop);
        if let Some(slides) = slides {
            let interval = duration / slides.frame_count().max(1) as u32;
            roll.slides = Some(Animation::with_interval(Arc::new(slides), interval, false)?);
            roll.hold_at = duration;
            roll.hold = Duration::ZERO;
        } else {
            roll.hold = Duration::from_secs(10);
        }
        Ok(roll)
    }

    pub(crate) fn route_slides(&self) -> bool {
        self.slides.is_some()
    }

    pub(crate) fn set_followup_backdrops(&mut self, backdrops: [Arc<Crx>; 2]) {
        self.followup_backdrops = Some(backdrops);
    }

    pub(crate) fn needs_followup_music(&self) -> bool {
        self.followup_backdrops.is_some() && self.followup_at.is_none() && !self.holding()
    }

    pub(crate) fn start_followup(&mut self) {
        self.followup_at = Some(self.elapsed);
    }

    pub(crate) fn holding(&self) -> bool {
        self.elapsed >= self.hold_at
    }

    pub(crate) fn advance(&mut self, elapsed: Duration, skip: bool) -> Result<bool> {
        let fade_at = self.hold_at + self.hold;
        let was_scrolling = self.elapsed < fade_at;
        if skip {
            self.elapsed = self.elapsed.max(fade_at);
        }
        self.elapsed = self.elapsed.saturating_add(elapsed);
        let fading = self.elapsed.saturating_sub(fade_at);
        if fading >= Self::FADE {
            return Ok(true);
        }
        self.opacity = (255.0 * (1.0 - fading.div_duration_f64(Self::FADE))).round() as u8;
        if let Some((_, remaining)) = &mut self.previous_slide {
            *remaining = remaining.saturating_sub(elapsed);
            if remaining.is_zero() {
                self.previous_slide = None;
            }
        }
        if !skip
            && was_scrolling
            && let Some(slides) = &mut self.slides
            && let Some(previous) = slides.advance_frame(elapsed)?
        {
            self.previous_slide = Some((previous, Duration::from_secs(1)));
        }
        if was_scrolling && !skip {
            let count = self.source.frame_count() as u32;
            self.pixels = if self.holding() {
                (count - 1) * self.height
            } else {
                (self.elapsed.div_duration_f64(self.duration) * f64::from(count * self.height))
                    as u32
            }
            .min((count - 1) * self.height);
            self.load_visible_tiles()?;
        }
        Ok(false)
    }

    fn load_visible_tiles(&mut self) -> Result<()> {
        let first = (self.pixels / self.height) as usize;
        for index in first..(first + 2).min(self.source.frame_count()) {
            let tile = &mut self.tiles[index % self.tiles.len()];
            if !tile.as_ref().is_some_and(|(cached, _)| *cached == index) {
                let bytes = self.source.frame(index, MAX_FRAME_BYTES)?;
                *tile = Some((index, decode_archive_image(&self.source, &bytes, 0)?));
            }
        }
        Ok(())
    }

    pub(crate) fn draw(&self, frame: &mut Canvas) {
        if let Some(backdrop) = &self.backdrop {
            let fraction = self.elapsed.div_duration_f64(self.duration).min(1.0);
            let mut draw_backdrop = |image: &Crx, opacity| {
                let extra_height = image.image.height().saturating_sub(frame.dimensions().1);
                let y = -(f64::from(extra_height) * fraction).round() as i32;
                blit_alpha(frame, image, 0, y, None, opacity, 100);
            };
            draw_backdrop(backdrop, self.opacity);
            if let (Some(images), Some(at)) = (&self.followup_backdrops, self.followup_at) {
                let elapsed = self.elapsed.saturating_sub(at).as_secs_f64();
                let second = (f64::from(self.opacity) * (elapsed / 5.0).min(1.0)).round() as u8;
                let third = (f64::from(self.opacity) * ((elapsed - 5.0) / 6.0).clamp(0.0, 1.0))
                    .round() as u8;
                draw_backdrop(&images[0], second);
                draw_backdrop(&images[1], third);
            }
        }
        if let Some(slides) = &self.slides {
            let mut opacity = self.opacity;
            if let Some((previous, remaining)) = &self.previous_slide {
                blit_alpha(frame, previous, 0, 0, None, opacity, 100);
                opacity = (f64::from(opacity) * (1.0 - remaining.as_secs_f64())).round() as u8;
            }
            blit_alpha(frame, slides.frame(), 0, 0, None, opacity, 100);
        }
        let first = (self.pixels / self.height) as usize;
        for (index, tile) in self.tiles.iter().flatten() {
            if (first..first + 2).contains(index) {
                let y = (*index as u32 * self.height) as i32 - self.pixels as i32;
                let spare = frame.width().saturating_sub(tile.image.width()) as i32;
                let x = if self.route_slides() {
                    spare
                } else {
                    spare / 2
                };
                blit_alpha(frame, tile, x, y, None, self.opacity, 100);
            }
        }
    }
}
