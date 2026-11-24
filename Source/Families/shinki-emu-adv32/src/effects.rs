use crate::{
    crx::Crx,
    graphics::{self, Clip, Transform},
    render::{Canvas, RasterAxis},
};
use image::{Pixel, Rgba, RgbaImage};
use roukaku_media_core::OwnedPixelBuffer;
use shinki_emu_sdk::StageCanvas;
use std::{sync::Arc, time::Duration};

const DENSITY: [usize; 7] = [0, 1, 3, 6, 9, 12, 20];
const FRAMES: [u32; 6] = [7, 7, 6, 8, 5, 8];

pub(crate) struct SceneLayer {
    pub(crate) texture: Option<Arc<Crx>>,
    pub(crate) position: (i32, i32),
    pub(crate) pivot: (i32, i32),
    pub(crate) scale: u32,
    pub(crate) visible: bool,
    pub(crate) order: u8,
    motion: Option<LayerTransition>,
}

struct LayerTransition {
    elapsed: Duration,
    duration: Duration,
    x_offset: i32,
    hide: bool,
    mask: Option<Arc<Crx>>,
}

impl Default for SceneLayer {
    fn default() -> Self {
        Self {
            texture: None,
            position: (0, 0),
            pivot: (0, 0),
            scale: 100,
            visible: false,
            order: 50,
            motion: None,
        }
    }
}

impl SceneLayer {
    pub(crate) fn restored(
        texture: Option<Arc<Crx>>,
        position: (i32, i32),
        pivot: (i32, i32),
        scale: u32,
        visible: bool,
        order: u8,
    ) -> Self {
        let visible = visible && texture.is_some();
        Self {
            texture,
            position,
            pivot,
            scale,
            visible,
            order,
            motion: None,
        }
    }

    pub(crate) fn transition(
        &mut self,
        duration: Duration,
        x_offset: i32,
        hide: bool,
        mask: Option<Arc<Crx>>,
    ) {
        self.visible = !duration.is_zero() || !hide;
        self.motion = (!duration.is_zero()).then_some(LayerTransition {
            elapsed: Duration::ZERO,
            duration,
            x_offset,
            hide,
            mask,
        });
    }

    pub(crate) fn advance(&mut self, elapsed: Duration) -> bool {
        let Some(motion) = &mut self.motion else {
            return false;
        };
        motion.elapsed = motion.elapsed.saturating_add(elapsed);
        if motion.elapsed >= motion.duration {
            self.finish();
        }
        true
    }

    pub(crate) fn active(&self) -> bool {
        self.motion.is_some()
    }

    pub(crate) fn finish(&mut self) {
        if let Some(motion) = self.motion.take() {
            self.visible = !motion.hide;
        }
    }

    pub(crate) fn has_mask(&self) -> bool {
        self.visible
            && self
                .motion
                .as_ref()
                .is_some_and(|motion| motion.mask.is_some())
    }

    pub(crate) fn draw(&self, frame: &mut Canvas, offset: (i32, i32)) {
        let Some(texture) = &self.texture else { return };
        self.draw_texture(frame, texture, offset);
    }

    pub(crate) fn draw_texture(&self, frame: &mut Canvas, texture: &Crx, offset: (i32, i32)) {
        if !self.visible {
            return;
        }
        let scale_origin = |position: i32, pivot: i32, texture_origin: i32, offset: i32| {
            let scaled = (i64::from(pivot) - i64::from(texture_origin))
                * (100 - i64::from(self.scale))
                / 100;
            (i64::from(position) + scaled + i64::from(offset)) as i32
        };
        let mut x = scale_origin(self.position.0, self.pivot.0, texture.x, offset.0);
        let y = scale_origin(self.position.1, self.pivot.1, texture.y, offset.1);
        let Some(motion) = &self.motion else {
            graphics::blit_alpha(frame, texture, x, y, None, 255, self.scale);
            return;
        };
        let fraction = motion.elapsed.div_duration_f64(motion.duration).min(1.0);
        let shown = if motion.hide {
            1.0 - fraction
        } else {
            fraction
        };
        x += (f64::from(motion.x_offset) * (1.0 - shown)).round() as i32;
        if let Some(mask) = &motion.mask {
            let (width, height) = frame.dimensions();
            let mut layer = Canvas::from_image(RgbaImage::new(width, height), None);
            graphics::blit_alpha(&mut layer, texture, x, y, None, 255, self.scale);
            let frame = frame.raster_mut();
            for (x, y, pixel) in layer.raster_mut().enumerate_pixels_mut() {
                if pixel[3] == 0 {
                    continue;
                }
                let intensity = f64::from(
                    mask.image.get_pixel(
                        x * mask.image.width() / width,
                        y * mask.image.height() / height,
                    )[0],
                );
                let opacity = (510.0 * shown - intensity).clamp(0.0, 255.0) as u16;
                pixel[3] = (u16::from(pixel[3]) * opacity / 255) as u8;
                frame.get_pixel_mut(x, y).blend(pixel);
            }
        } else {
            graphics::blit_alpha(
                frame,
                texture,
                x,
                y,
                None,
                (255.0 * shown) as u8,
                self.scale,
            );
        }
    }
}

pub(crate) struct SceneTransition {
    texture: Arc<Crx>,
    mode: TransitionMode,
    total: Duration,
    elapsed: Duration,
}

pub(crate) struct ScreenBreak {
    frame: OwnedPixelBuffer,
    presentation: Option<StageCanvas>,
    mask: Arc<Crx>,
    fragments: Vec<Fragment>,
    tick_fraction: u128,
    drift: f32,
    phase_step: f32,
}

struct Fragment {
    origin: (f32, f32),
    position: (f32, f32),
    velocity: (f32, f32),
    age: i32,
    lifetime: i32,
    phase: f32,
    drift: f32,
}

impl ScreenBreak {
    const CELL: i32 = 32;
    const LAST_AGE: i32 = 24;

    pub(crate) fn new(
        frame: OwnedPixelBuffer,
        extent: (u32, u32),
        presentation: Option<StageCanvas>,
        mask: Arc<Crx>,
        pattern: u8,
        random: &mut u32,
    ) -> Self {
        let mut next = || rand15(random) as i32;
        let (columns, rows) = (
            extent.0 as i32 / Self::CELL + 1,
            extent.1 as i32 / Self::CELL + 2,
        );
        let mut fragments = Vec::with_capacity((columns * rows) as usize);
        for row in 0..rows {
            for column in 0..columns {
                let origin = ((Self::CELL * column) as f32, (Self::CELL * row) as f32);
                let mut vx = 32 - (next() & 31);
                let vy = 32 + (next() & 31);
                if next() & 1 == 0 {
                    vx = (next() & 31) - 32;
                }
                fragments.push(Fragment {
                    origin,
                    position: origin,
                    velocity: (vx as f32, vy as f32),
                    age: 0,
                    lifetime: next() % Self::CELL + 16,
                    phase: 0.0,
                    drift: 0.0,
                });
            }
        }
        for (index, fragment) in fragments.iter_mut().enumerate() {
            let row = index as i32 / columns;
            let column = index as i32 % columns;
            let (age, x, y) = match pattern {
                0 => (0, 48 - (next() & 31), 48 - (next() & 31)),
                1 => (
                    next() % 2 - row / 5 + column - columns,
                    32 - (next() & 31),
                    32 - (next() & 48),
                ),
                2 => (
                    next() % 2 + row - rows,
                    4 - (next() & 8),
                    32 + (next() & 31),
                ),
                3 => (
                    next() % 2 - row / 5 - column,
                    (next() & 31) - 32,
                    32 - (next() & 48),
                ),
                4 => (0, 32 - (next() & 63), 32 - (next() & 48)),
                5 => (-2 * row - column, 2 - (next() & 8), (next() & 8) - 2),
                6 => (
                    -((rows / 2 - row).abs() + (columns / 2 - column).abs()),
                    4 - (next() & 8),
                    4 - (next() & 8),
                ),
                9 => (0, next() % 16 - 8, -32 - next() % 32),
                10 => {
                    fragment.age = -(next() % 20);
                    continue;
                }
                _ => continue,
            };
            fragment.age = age;
            fragment.velocity = (x as f32, y as f32);
        }
        Self {
            frame,
            presentation: presentation.filter(|mapping| mapping.logical() != mapping.raster()),
            mask,
            fragments,
            tick_fraction: 0,
            drift: match pattern {
                3 => -1.0,
                0 | 2 | 4..=6 | 9 => 0.0,
                _ => 1.0,
            },
            phase_step: if pattern == 9 { 0.32 } else { 0.08 },
        }
    }

    pub(crate) fn finished(&mut self, elapsed: Duration, extent: (u32, u32)) -> bool {
        const TICK: u128 = Duration::from_secs(1).as_nanos();
        self.tick_fraction += elapsed.as_nanos() * 60;
        let steps = self.tick_fraction / TICK;
        self.tick_fraction %= TICK;
        for _ in 0..steps {
            if self.fragments.is_empty() {
                return true;
            }
            self.fragments.retain_mut(|fragment| {
                fragment.age += 1;
                if fragment.age <= 0 {
                    return true;
                }
                let t = fragment.phase;
                let (vx, vy) = fragment.velocity;
                let (x, y) = fragment.origin;
                fragment.position = (x + 4.0 * (vx * t + fragment.drift), y - (vy + 3.92 * t) * t);
                fragment.drift += self.drift;
                fragment.phase += self.phase_step;
                fragment.lifetime -= 1;
                fragment.age < Self::LAST_AGE
                    && fragment.lifetime > 0
                    && fragment.position.0 < extent.0 as f32
                    && fragment.position.1 < extent.1 as f32
            });
        }
        self.fragments.is_empty()
    }

    pub(crate) fn draw(&self, frame: &mut Canvas) {
        let frame = frame.raster_mut();
        let source = self.frame.as_slice();
        let (width, height) = (frame.width() as i32, frame.height() as i32);
        let mapping = self.presentation.map(|canvas| {
            (RasterAxis::x(canvas), RasterAxis::y(canvas), canvas.raster().width)
        });
        for fragment in &self.fragments {
            let (x, y) = (fragment.position.0 as i32, fragment.position.1 as i32);
            let mask_x = fragment.age.max(0) * Self::CELL;
            for dy in y.max(0)..(y + Self::CELL).min(height) {
                let source_row = mapping.map_or(dy as u32 * width as u32, |(_, ys, stride)| {
                    ys.center(dy as u32) * stride
                });
                for dx in x.max(0)..(x + Self::CELL).min(width) {
                    // Fragments move the coverage mask. The captured RGB stays at its
                    // original screen coordinates, including where fragments overlap.
                    let Some(mask) = self
                        .mask
                        .image
                        .get_pixel_checked((mask_x + dx - x) as u32, (dy - y) as u32)
                    else {
                        continue;
                    };
                    if mask[0] == 0 {
                        let sx = mapping.map_or(dx as u32, |(xs, _, _)| xs.center(dx as u32));
                        let i = (source_row + sx) as usize * 4;
                        frame.put_pixel(
                            dx as u32,
                            dy as u32,
                            Rgba([source[i], source[i + 1], source[i + 2], 255]),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_break_moves_the_mask_without_moving_captured_color() {
        let extent = (64, 32);
        let original = RgbaImage::from_fn(extent.0, extent.1, |x, _| Rgba([x as u8, 0, 0, 255]));
        let mut atlas = RgbaImage::from_pixel(800, 32, Rgba([255; 4]));
        atlas.put_pixel(32 + 10, 0, Rgba([0, 0, 0, 255]));
        let mut random = 1;
        let mut effect = ScreenBreak::new(
            original.into_raw().into(),
            extent,
            None,
            Arc::new(Crx::from_image(0, 0, atlas)),
            4,
            &mut random,
        );
        effect.fragments.truncate(1);
        assert!(!effect.finished(Duration::from_millis(17), extent));
        effect.fragments[0].position = (17.0, 0.0);

        let mut image = Canvas::new(extent.0, extent.1);
        effect.draw(&mut image);
        assert_eq!(image.raster_mut().get_pixel(10, 0).0, [0, 0, 0, 255]);
        assert_eq!(image.raster_mut().get_pixel(27, 0).0, [27, 0, 0, 255]);
        assert!(effect.finished(Duration::from_millis(400), extent));
    }
}

enum TransitionMode {
    Sweep(Vec<SweepSprite>),
    Zoom,
}

struct SweepSprite {
    from: (i32, i32),
    to_x: i32,
    clip: Clip,
    pivot: (f64, f64),
    scale: f64,
    opacity: f64,
    duration: Duration,
}

impl SceneTransition {
    const ZOOM_HOLD: Duration = Duration::from_millis(500);
    const LARGE: Clip = Clip {
        x: 0,
        y: 0,
        width: 732,
        height: 720,
    };
    const SMALL: Clip = Clip {
        x: 720,
        y: 0,
        width: 278,
        height: 256,
    };

    pub(crate) fn new(texture: Arc<Crx>, sweep: bool, steps: u8, random: &mut u32) -> Self {
        let duration = Duration::from_millis(200 * u64::from(steps.max(1)));
        let (mode, total) = if sweep {
            let mut sprites = Vec::with_capacity(54);
            for (from, to_x) in [(-732, 1288), (1288, -732)] {
                sprites.push(SweepSprite {
                    from: (from, 0),
                    to_x,
                    clip: Self::LARGE,
                    pivot: (360.0, 355.0),
                    scale: 1.0,
                    opacity: 1.0,
                    duration,
                });
            }
            let mut next = |n| rand15(random) % n;
            let mut total = duration;
            for index in 0..52 {
                let x = next(1280) as i32;
                let y = next(848) as i32 - 128;
                let (from_x, to_x) = if index < 27 {
                    (-x - 800, 1280)
                } else {
                    (x + 2080, -800)
                };
                let scale = f64::from(next(80) + 20) / 100.0;
                let time = duration + Duration::from_millis(200 * u64::from(next(3)));
                total = total.max(time);
                sprites.push(SweepSprite {
                    from: (from_x, y),
                    to_x,
                    clip: Self::SMALL,
                    pivot: (139.0, 128.0),
                    scale,
                    opacity: 0.5 + f64::from(next(5)) / 10.0,
                    duration: time,
                });
            }
            (TransitionMode::Sweep(sprites), total)
        } else {
            (TransitionMode::Zoom, 2 * duration + Self::ZOOM_HOLD)
        };
        Self {
            texture,
            mode,
            total,
            elapsed: Duration::ZERO,
        }
    }

    pub(crate) fn classic_sweep(texture: Arc<Crx>, steps: u8) -> Self {
        let duration = Duration::from_millis(200 * u64::from(steps.max(1)));
        let sprites = vec![
            SweepSprite {
                from: (-732, 0),
                to_x: 1288,
                clip: Self::LARGE,
                pivot: (360.0, 355.0),
                scale: 1.0,
                opacity: 1.0,
                duration,
            },
            SweepSprite {
                from: (-278, 180),
                to_x: 1280,
                clip: Self::SMALL,
                pivot: (139.0, 128.0),
                scale: 1.0,
                opacity: 1.0,
                duration: duration / 2,
            },
        ];
        Self {
            texture,
            mode: TransitionMode::Sweep(sprites),
            total: duration,
            elapsed: Duration::ZERO,
        }
    }

    pub(crate) fn finished(&mut self, elapsed: Duration) -> bool {
        self.elapsed = self.elapsed.saturating_add(elapsed);
        self.elapsed >= self.total
    }

    pub(crate) fn draw(&self, frame: &mut Canvas) {
        match &self.mode {
            TransitionMode::Sweep(sprites) => {
                for (index, sprite) in sprites.iter().enumerate() {
                    let t = self.elapsed.div_duration_f64(sprite.duration);
                    if t >= 1.0 {
                        continue;
                    }
                    let x = f64::from(sprite.from.0) + f64::from(sprite.to_x - sprite.from.0) * t;
                    graphics::blit_transformed(
                        frame,
                        &self.texture,
                        sprite.clip,
                        Transform {
                            position: (x, f64::from(sprite.from.1)),
                            pivot: sprite.pivot,
                            scale: sprite.scale,
                            angle: 540.0 * t,
                            opacity: sprite.opacity * if index < 2 { 1.0 } else { 1.0 - t },
                        },
                    );
                }
            }
            TransitionMode::Zoom => {
                let duration = (self.total - Self::ZOOM_HOLD) / 2;
                let grow = self.elapsed.div_duration_f64(duration);
                let hold = self
                    .elapsed
                    .saturating_sub(duration)
                    .div_duration_f64(Self::ZOOM_HOLD);
                let shrink = self
                    .total
                    .saturating_sub(self.elapsed)
                    .div_duration_f64(duration);
                let scale = if grow < 1.0 {
                    0.01 + 4.99 * grow
                } else if hold < 1.0 {
                    5.0 + 0.5 * hold
                } else {
                    5.5 * shrink
                };
                graphics::blit_transformed(
                    frame,
                    &self.texture,
                    Self::LARGE,
                    Transform {
                        position: (280.0, 0.0),
                        pivot: (360.0, 355.0),
                        scale,
                        angle: 0.0,
                        opacity: 1.0,
                    },
                );
            }
        }
    }
}

pub(crate) fn rand15(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(214013).wrapping_add(2531011);
    (*state >> 16) & 0x7fff
}

pub(crate) struct FallingSprites {
    texture: Arc<Crx>,
    particles: Vec<Particle>,
    stage: (u32, u32),
    wind: i32,
    kind: ParticleKind,
    region: ParticleRegion,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ParticleRegion {
    pub(crate) bounds: [i32; 4],
    pub(crate) weight: u16,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParticleKind {
    Falling,
    Drifting,
    Rain,
    FastFalling,
}

#[derive(Clone, Default)]
pub(crate) struct LensPosition {
    pub(crate) center: (i32, i32),
    pub(crate) angles: (i32, i32),
    pub(crate) duration: Duration,
}

pub(crate) struct LensFlare {
    pub(crate) strength: u8,
    texture: Arc<Crx>,
    glints: Vec<Glint>,
    elapsed: Duration,
}

struct Glint {
    distance: (f64, f64),
    scale: u32,
    opacity: u8,
    clip: Clip,
}

impl LensFlare {
    pub(crate) fn new(texture: Arc<Crx>, strength: u8, random: &mut u32) -> Self {
        let mut glints = Vec::new();
        for index in 0..DENSITY[usize::from((strength % 10).min(6))] {
            let distance =
                |random: &mut u32| f64::from(rand15(random) % 20 + 64 * index as u32 + 8);
            let distance = (distance(random), distance(random));
            let scale = rand15(random) % 400 + 300;
            let clip = match rand15(random) % 3 {
                0 => Clip {
                    x: 140,
                    y: 0,
                    width: 40,
                    height: 40,
                },
                1 => Clip {
                    x: 180,
                    y: 0,
                    width: 20,
                    height: 20,
                },
                _ => Clip {
                    x: 200,
                    y: 0,
                    width: 8,
                    height: 8,
                },
            };
            let opacity = (rand15(random) % 10 * 255 / 100) as u8;
            glints.push(Glint {
                distance,
                scale,
                opacity,
                clip,
            });
        }
        Self {
            strength,
            texture,
            glints,
            elapsed: Duration::ZERO,
        }
    }

    pub(crate) fn advance(&mut self, elapsed: Duration) -> bool {
        self.elapsed = self.elapsed.saturating_add(elapsed);
        !self.glints.is_empty()
    }

    pub(crate) fn draw(&self, frame: &mut Canvas, position: &LensPosition) {
        let fraction = if position.duration.is_zero() {
            0.0
        } else {
            self.elapsed.div_duration_f64(position.duration).min(1.0)
        };
        let angle = f64::from(position.angles.0)
            + f64::from(position.angles.1 - position.angles.0) * fraction;
        let (sin, cos) = angle.to_radians().sin_cos();
        for glint in &self.glints {
            let (w, h) = (
                glint.clip.width * glint.scale / 100,
                glint.clip.height * glint.scale / 100,
            );
            let x = position.center.0 + (sin * glint.distance.0) as i32 - w as i32 / 2;
            let y = position.center.1 + (cos * glint.distance.1) as i32 - h as i32 / 2;
            graphics::blit_alpha(
                frame,
                &self.texture,
                x,
                y,
                Some(glint.clip),
                glint.opacity,
                glint.scale,
            );
        }
    }
}

impl FallingSprites {
    pub(crate) fn new(
        texture: Arc<Crx>,
        stage: (u32, u32),
        strength: u8,
        kind: ParticleKind,
        region: ParticleRegion,
        random: &mut u32,
    ) -> Self {
        let density = usize::from((strength % 10).min(6));
        let mut particles = Vec::with_capacity(5 * DENSITY[density]);
        for layer in (0..5).rev() {
            for _ in 0..DENSITY[density] {
                let particle = Particle::spawn(kind, stage, layer, density == 6, region, random);
                particles.push(particle);
            }
        }
        Self {
            texture,
            particles,
            stage,
            wind: if strength < 100 {
                i32::from(strength / 10)
            } else {
                -i32::from((strength % 100) / 10)
            },
            kind,
            region,
        }
    }

    pub(crate) fn set_region(&mut self, region: ParticleRegion) {
        self.region = region;
    }

    pub(crate) fn update(&mut self, time: Duration, random: &mut u32) -> bool {
        let steps = time.as_secs_f32() * 60.0;
        for particle in &mut self.particles {
            let drifting = self.kind == ParticleKind::Drifting;
            particle.y += (if drifting { -1.0 } else { 1.0 }) * particle.speed * steps;
            particle.phase += (if drifting { 0.01 } else { 0.05 }) * steps;
            if matches!(
                self.kind,
                ParticleKind::Drifting | ParticleKind::FastFalling
            ) {
                particle.brightness =
                    (particle.brightness + particle.brightness_step * steps).clamp(0.0, 0.8);
                if particle.brightness == 0.0 || particle.brightness == 0.8 {
                    particle.brightness_step = -particle.brightness_step;
                }
            } else if self.kind == ParticleKind::Falling {
                particle.x += self.wind as f32 * steps;
            }
            particle.frame_time += time;
            let interval = Duration::from_millis(if self.kind == ParticleKind::Falling {
                300
            } else {
                500
            });
            while self.kind != ParticleKind::Rain && particle.frame_time >= interval {
                particle.frame_time -= interval;
                particle.frame = (particle.frame + 1) % FRAMES[particle.row as usize];
            }
            let x = particle.draw_x();
            let y = particle.y;
            let outside = if drifting {
                x < -28.0 || x > self.stage.0 as f32 || y < -28.0 || y > self.stage.1 as f32 + 28.0
            } else {
                y > self.stage.1 as f32
            };
            if outside {
                *particle = Particle::spawn(
                    self.kind,
                    self.stage,
                    particle.layer,
                    false,
                    self.region,
                    random,
                );
            }
        }
        !self.particles.is_empty()
    }

    pub(crate) fn draw(&self, image: &mut Canvas) {
        for particle in &self.particles {
            let x = particle.draw_x().round() as i32;
            let y = particle.y.round() as i32;
            let clip = if self.kind == ParticleKind::Rain {
                Clip {
                    x: (particle.row * 8) as i32,
                    y: 0,
                    width: 2,
                    height: 24 * (particle.row + 1),
                }
            } else {
                Clip {
                    x: (particle.frame * 32) as i32,
                    y: (particle.row * 28) as i32,
                    width: 28,
                    height: 28,
                }
            };
            graphics::blit_alpha(
                image,
                &self.texture,
                x,
                y,
                Some(clip),
                (particle.brightness * 255.0).round() as u8,
                if self.kind == ParticleKind::Falling {
                    100 - particle.layer * 10
                } else {
                    100
                },
            );
        }
    }
}

struct Particle {
    x: f32,
    y: f32,
    phase: f32,
    swing: f32,
    speed: f32,
    row: u32,
    frame: u32,
    brightness: f32,
    brightness_step: f32,
    x_offset: f32,
    layer: u32,
    frame_time: Duration,
}

impl Particle {
    fn spawn(
        kind: ParticleKind,
        stage: (u32, u32),
        layer: u32,
        slow: bool,
        region: ParticleRegion,
        random: &mut u32,
    ) -> Self {
        match kind {
            ParticleKind::Falling => Self::falling(stage, layer, slow, random),
            ParticleKind::Drifting => Self::mote(stage, layer, region, random),
            ParticleKind::FastFalling | ParticleKind::Rain => {
                let mut next = |divisor| rand15(random) % divisor;
                let x = next(stage.0) as f32;
                let y = -(next(stage.1) as f32);
                let (speed, swing, row, phase, frame, brightness, brightness_step) =
                    if kind == ParticleKind::Rain {
                        (
                            (next(4) + 24) as f32,
                            0.0,
                            next(3),
                            0.0,
                            0,
                            0.5 - next(4) as f32 * 0.1,
                            0.0,
                        )
                    } else {
                        let speed = 0.5 + next(3) as f32 * 2.5;
                        let swing = (next(16) + 4) as f32;
                        let row = layer + next(2);
                        let phase = next(360) as f32;
                        let frame = next(FRAMES[row as usize]);
                        let brightness = next(10) as f32 / 10.0;
                        let step = if next(2) == 0 { -0.002 } else { 0.002 };
                        (speed, swing, row, phase, frame, brightness, step)
                    };
                Self {
                    x,
                    y,
                    speed,
                    swing,
                    row,
                    phase,
                    frame,
                    brightness,
                    brightness_step,
                    x_offset: 0.0,
                    layer,
                    frame_time: Duration::ZERO,
                }
            }
        }
    }

    fn draw_x(&self) -> f32 {
        self.x + self.phase.sin() * self.swing + self.x_offset
    }

    fn mote(
        (width, height): (u32, u32),
        layer: u32,
        region: ParticleRegion,
        random: &mut u32,
    ) -> Self {
        let mut next = |divisor| rand15(random) % divisor;
        let mut x = next(width) as i32;
        let mut y = next(height) as i32;
        let [left, top, right, bottom] = region.bounds;
        if region.weight != 0
            && left < right
            && top < bottom
            && next(100) <= u32::from(region.weight)
        {
            let mut coordinate = |extent, low, high| {
                let mut value = 0;
                for _ in 0..100 {
                    value = next(extent) as i32;
                    if (low..=high).contains(&value) {
                        break;
                    }
                }
                value
            };
            x = coordinate(width, left, right);
            y = coordinate(height, top, bottom);
        }
        let speed = (next(4) + 1) as f32 * 0.2 * if next(2) == 0 { -1.0 } else { 1.0 };
        let swing = (next(32) + 28) as f32;
        let x_offset = next(3) as f32 * if next(2) == 0 { -0.1 } else { 0.1 };
        let row = layer + next(2);
        let phase = next(360) as f32;
        Self {
            x: x as f32,
            y: y as f32,
            speed,
            swing,
            x_offset,
            row,
            phase,
            frame: 0,
            brightness: 0.0,
            brightness_step: 0.005,
            layer,
            frame_time: Duration::ZERO,
        }
    }

    fn falling((width, height): (u32, u32), layer: u32, slow: bool, random: &mut u32) -> Self {
        let mut next = |divisor| rand15(random) % divisor;
        let x = next(width) as f32;
        let x = match next(3) {
            1 => x - width as f32,
            2 => x + width as f32,
            _ => x,
        };
        let y = -(next(height) as f32);
        let speed = (next(if slow { 4 } else { 10 }) + 2) as f32 / 1.5;
        let swing = (next(32) + 28) as f32;
        let row = layer + next(2);
        let phase = next(360) as f32;
        let frame = next(FRAMES[row as usize]);
        let brightness = 0.5 + 0.1 * next(3) as f32 - 0.02 * layer as f32;
        Self {
            x,
            y,
            phase,
            swing,
            speed,
            row,
            frame,
            brightness,
            brightness_step: 0.0,
            x_offset: 0.0,
            layer,
            frame_time: Duration::ZERO,
        }
    }
}
