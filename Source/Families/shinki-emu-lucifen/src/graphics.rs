use shinki_emu_lucifen_formats::{ElgImage, decode_elg};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
    io,
    sync::Arc,
};

const MAX_PIXELS: u64 = 16_777_216;
const MAX_SURFACE_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum BitmapBlend {
    Copy,
    Alpha(u8),
    Color,
}

#[derive(Clone)]
pub struct Surface {
    pub width: u32,
    pub height: u32,
    pub origin: (i16, i16),
    pixels: Vec<u8>,
    indices: Option<Arc<[u8]>>,
    group: i32,
}

impl Surface {
    fn empty(width: u32, height: u32) -> io::Result<Self> {
        let pixels = u64::from(width) * u64::from(height);
        if !(1..=MAX_PIXELS).contains(&pixels) {
            return Err(invalid("invalid Lucifen surface dimensions"));
        }
        Ok(Self {
            width,
            height,
            origin: (0, 0),
            pixels: vec![0; pixels as usize * 4],
            indices: None,
            group: 0,
        })
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    fn byte_len(&self) -> usize {
        self.pixels.len() + self.indices.as_ref().map_or(0, |indices| indices.len())
    }

    fn full_region(&self) -> [i32; 4] {
        [0, 0, self.width as i32, self.height as i32]
    }

    fn clamped_region(&self, [left, top, right, bottom]: [i32; 4]) -> io::Result<[i32; 4]> {
        let left = left.clamp(0, self.width as i32);
        let right = right.clamp(0, self.width as i32);
        let top = top.clamp(0, self.height as i32);
        let bottom = bottom.clamp(0, self.height as i32);
        if left >= right || top >= bottom {
            return Err(invalid("invalid Lucifen texture rectangle"));
        }
        Ok([left, top, right, bottom])
    }

    fn composite(&mut self, source: &Self, x: i32, y: i32, opacity: u8) {
        self.composite_region(
            source,
            source.full_region().map(i64::from),
            x,
            y,
            Some(opacity),
        );
    }

    fn composite_region(
        &mut self,
        source: &Self,
        region: [i64; 4],
        x: i32,
        y: i32,
        opacity: Option<u8>,
    ) {
        self.blit(
            source,
            region,
            x,
            y,
            opacity.map_or(BitmapBlend::Copy, BitmapBlend::Alpha),
        );
    }

    fn blit(
        &mut self,
        source: &Self,
        [src_left, src_top, src_right, src_bottom]: [i64; 4],
        x: i32,
        y: i32,
        blend: BitmapBlend,
    ) {
        let x = i64::from(x) - src_left;
        let y = i64::from(y) - src_top;
        let left = (x + src_left.max(0)).max(0);
        let top = (y + src_top.max(0)).max(0);
        let right = (x + src_right.min(i64::from(source.width))).min(i64::from(self.width));
        let bottom = (y + src_bottom.min(i64::from(source.height))).min(i64::from(self.height));
        if right <= left || bottom <= top || matches!(blend, BitmapBlend::Alpha(0)) {
            return;
        }
        let count = (right - left) as usize * 4;
        for row in top..bottom {
            let src = (source.width as usize * (row - y) as usize + (left - x) as usize) * 4;
            let dst = (self.width as usize * row as usize + left as usize) * 4;
            let source = &source.pixels[src..src + count];
            let target = &mut self.pixels[dst..dst + count];
            match blend {
                BitmapBlend::Copy => target.copy_from_slice(source),
                BitmapBlend::Alpha(opacity) => {
                    for (source, target) in source
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(target.as_chunks_mut::<4>().0)
                    {
                        mix(target, *source, opacity);
                    }
                }
                BitmapBlend::Color => {
                    for (source, target) in source
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(target.as_chunks_mut::<4>().0)
                    {
                        let alpha = i32::from(source[3]);
                        for (target, source) in target[..3].iter_mut().zip(source) {
                            let from = i32::from(*target);
                            *target = (from + (((i32::from(*source) - from) * alpha) >> 8)) as u8;
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn flip(&mut self, [left, top, right, bottom]: [i32; 4], horizontal: bool) {
        let [left, top, right, bottom] = [left, top, right, bottom].map(|value| value as usize);
        let stride = self.width as usize;
        let pixels = self.pixels.as_chunks_mut::<4>().0;
        let mut indices = self.indices.as_mut().map(Arc::make_mut);
        if horizontal {
            for y in top..bottom {
                let row = y * stride + left..y * stride + right;
                pixels[row.clone()].reverse();
                if let Some(indices) = &mut indices {
                    indices[row].reverse();
                }
            }
        } else {
            for y in 0..(bottom - top) / 2 {
                let first = (top + y) * stride;
                let last = (bottom - 1 - y) * stride;
                let (upper, lower) = pixels.split_at_mut(last);
                upper[first + left..first + right].swap_with_slice(&mut lower[left..right]);
                if let Some(indices) = &mut indices {
                    let (upper, lower) = indices.split_at_mut(last);
                    upper[first + left..first + right].swap_with_slice(&mut lower[left..right]);
                }
            }
        }
    }
}

#[derive(Clone)]
struct Texture {
    group: i32,
    source: Option<i32>,
    pixels: Arc<Surface>,
    region: [i32; 4],
    source_offset: [i32; 2],
    position: [i32; 2],
    blend: u8,
    opacity: u8,
    visible: bool,
    sampling: u8,
    stretch: Option<([i32; 2], [i32; 2])>,
}

impl Texture {
    fn source_region(&self) -> [i64; 4] {
        let [x, y] = self.source_offset.map(i64::from);
        let [left, top, right, bottom] = self.region.map(i64::from);
        [left + x, top + y, right + x, bottom + y]
    }

    fn draw(&self, image: &Surface, surface: &mut Surface, offset: [i32; 2]) {
        if let Some((position, size)) = self.stretch {
            let position = std::array::from_fn(|axis| position[axis].saturating_add(offset[axis]));
            draw_stretched_texture(
                image,
                surface,
                self.source_region(),
                position,
                size,
                self.opacity,
                self.sampling & 0x0f != 0,
            );
            return;
        }
        let [x, y] = self.position;
        surface.composite_region(
            image,
            self.source_region(),
            x.saturating_add(offset[0]),
            y.saturating_add(offset[1]),
            Some(self.opacity),
        );
    }

    fn placement(&self) -> ([i32; 2], [i32; 2]) {
        self.stretch.unwrap_or_else(|| {
            let [left, top, right, bottom] = self.region;
            (self.position, [right - left, bottom - top])
        })
    }
}

struct Background {
    source: i32,
    image: Arc<Surface>,
    position: [i32; 2],
    window: Option<[i32; 2]>,
    stretch: Option<BackgroundStretch>,
    transformed: Option<Arc<Surface>>,
    pivot: [i32; 2],
    rotation: BackgroundRotation,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct BackgroundStretch {
    destination: [i32; 4],
    source: [i32; 4],
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct BackgroundRotation {
    pub(crate) offset: [i32; 2],
    pub(crate) turns: i32,
    pub(crate) scale: f64,
}

impl Default for BackgroundRotation {
    fn default() -> Self {
        Self {
            offset: [0; 2],
            turns: 0,
            scale: 1.0,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct BackgroundTransform {
    pub(crate) center: [i32; 2],
    pub(crate) offset: [i32; 2],
    pub(crate) scale: f64,
    pub(crate) turns: f64,
    pub(crate) opacity: u8,
    pub(crate) sampling: u8,
    pub(crate) alpha: bool,
    pub(crate) in_place: bool,
}

struct Layer {
    target: i32,
    priority: u32,
    group: i32,
    overlay: bool,
    order: i64,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SceneOffset {
    pub(crate) primary: [i32; 2],
    pub(crate) overlay: [i32; 2],
    pub(crate) fill_edges: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct SpriteGeometry {
    pub(crate) position: [i32; 2],
    pub(crate) size: [i32; 2],
    pub(crate) origin: [i32; 2],
    pub(crate) viewport: [i32; 2],
}

#[derive(Default)]
struct HiddenPasses {
    background: bool,
    primary: bool,
    overlay: bool,
}

pub(crate) struct BackgroundWarp {
    pub(crate) table: Arc<[i32]>,
    pub(crate) phase: usize,
    pub(crate) gain: i32,
    pub(crate) fill_edges: bool,
}

impl BackgroundWarp {
    fn apply(&self, image: &mut Arc<Surface>, border: [u8; 4]) {
        if self.gain == 0 {
            return;
        }
        let source = image.clone();
        let image = Arc::make_mut(image);
        let width = i64::from(image.width);
        let height = i64::from(image.height);
        for (y, row) in image
            .pixels
            .chunks_exact_mut(width as usize * 4)
            .enumerate()
        {
            let offset = (i64::from(self.table[(self.phase + y) % self.table.len()])
                * i64::from(self.gain))
                >> 9;
            let source_y = y as i64 + offset;
            let (left, right) = if (0..height).contains(&source_y) {
                (
                    (-offset).clamp(0, width) as usize,
                    (width - offset).clamp(0, width) as usize,
                )
            } else {
                (0, 0)
            };
            let (left_edge, rest) = row.split_at_mut(left * 4);
            let (inside, right_edge) = rest.split_at_mut((right - left) * 4);
            if !inside.is_empty() {
                let src = (source_y * width + left as i64 + offset) as usize * 4;
                inside.copy_from_slice(&source.pixels[src..src + inside.len()]);
            }
            if self.fill_edges {
                for pixel in left_edge
                    .chunks_exact_mut(4)
                    .chain(right_edge.chunks_exact_mut(4))
                {
                    pixel.copy_from_slice(&border);
                }
            }
        }
    }
}

pub(crate) struct SceneEffect {
    pub(crate) duration: u32,
    pub(crate) rule: Option<(i32, i32)>,
    pub(crate) mode: u8,
    pub(crate) commit: bool,
    pub(crate) quake: Option<TransitionQuake>,
    pub(crate) transform: Option<TransitionTransform>,
}

#[derive(Clone, Copy)]
pub(crate) enum TransitionTransform {
    VerticalSplit {
        reverse: bool,
    },
    HorizontalBlur {
        cross: bool,
        radius: u8,
    },
    Scroll {
        direction: u8,
        style: u8,
    },
    RotateAxis {
        direction: u8,
    },
    RotateCenter {
        style: u8,
        sampling: u8,
    },
    AddSubtract {
        flags: u8,
    },
    Wave {
        mode: i32,
        amplitude: i32,
        frequency: i32,
        border: [[u8; 3]; 2],
    },
    Mosaic {
        alignment: u8,
        size: u32,
        fit_aspect: bool,
    },
    ZoomFlip {
        mode: u8,
        sampling: u8,
    },
}

pub(crate) struct TransitionQuake {
    pub(crate) amplitude: [i32; 2],
    pub(crate) end_gain: f64,
    pub(crate) fill_edges: bool,
}

pub(crate) struct BitmapRuleBlend {
    pub(crate) progress: u8,
    pub(crate) softness: i32,
    pub(crate) mode: u8,
    pub(crate) size: [i32; 2],
    pub(crate) positions: [[i32; 2]; 4],
}

#[derive(Clone, Copy)]
pub(crate) enum BitmapTarget {
    Surface(i32),
    Texture(i32),
}

struct Transition {
    start: u32,
    duration: u32,
    progress: u8,
    peer: i32,
    mode: u8,
    commit: bool,
    from: Option<Arc<Surface>>,
    mask: Option<(Arc<[u8]>, i32)>,
    quake: Option<TransitionQuake>,
    displacement: [i32; 2],
    reverse: bool,
    transform: Option<TransitionTransform>,
    transform_progress: u32,
    feedback: Option<(u32, Arc<Surface>)>,
}

impl Transition {
    fn blend(&self, source: &Surface, target: &mut Surface) {
        if let Some(TransitionTransform::ZoomFlip { mode, sampling }) = self.transform {
            self.blend_zoom_flip(source, target, mode, sampling);
            return;
        }
        if let Some(TransitionTransform::Mosaic {
            alignment,
            size,
            fit_aspect,
        }) = self.transform
        {
            let phase = self.transform_progress;
            let block = phase.min(2 * size - phase);
            if block >= 2 {
                let image = if phase < size { source } else { &*target };
                target.pixels = mosaic(image, block, alignment, fit_aspect);
            } else if phase < size {
                target.pixels.copy_from_slice(&source.pixels);
            }
            return;
        }
        if let Some(TransitionTransform::Wave {
            mode,
            amplitude,
            frequency,
            border,
        }) = self.transform
        {
            self.blend_wave(source, target, mode, amplitude, frequency, border);
            return;
        }
        if let Some(TransitionTransform::AddSubtract { flags }) = self.transform {
            if self.progress == 255 {
                return;
            }
            let amount = if flags & 1 == 0 {
                target.pixels.copy_from_slice(&source.pixels);
                self.progress
            } else {
                255 - self.progress
            };
            for pixel in target.pixels.as_chunks_mut::<4>().0 {
                for channel in &mut pixel[..3] {
                    *channel = if flags >= 2 {
                        channel.saturating_sub(amount)
                    } else {
                        channel.saturating_add(amount)
                    };
                }
            }
            return;
        }
        if let Some(TransitionTransform::RotateAxis { direction }) = self.transform {
            let phase = self.transform_progress;
            if phase == 0 {
                target.pixels.copy_from_slice(&source.pixels);
            } else if phase <= 128 {
                draw_axis_rotation(source, target, direction, phase);
            } else if phase < 256 {
                let incoming = target.clone();
                draw_axis_rotation(&incoming, target, direction, phase + 256);
            }
            return;
        }
        if let Some(TransitionTransform::RotateCenter { style, sampling }) = self.transform {
            let phase = self.transform_progress;
            let incoming;
            let (image, phase, opacity) = if style == 1 {
                (source, 512 - phase, 255)
            } else {
                incoming = target.clone();
                if let Some((previous, feedback)) = &self.feedback {
                    target.pixels.copy_from_slice(&feedback.pixels);
                    if *previous == phase {
                        return;
                    }
                } else {
                    target.pixels.copy_from_slice(&source.pixels);
                }
                let opacity = if style == 2 && self.feedback.is_some() {
                    16
                } else {
                    255
                };
                (&incoming, phase, opacity)
            };
            let center = [target.width as i32 / 2, target.height as i32 / 2];
            draw_background_transform(
                image,
                target,
                BackgroundTransform {
                    center,
                    offset: [0; 2],
                    scale: f64::from(phase) / 512.0,
                    turns: -f64::from(phase),
                    opacity,
                    sampling,
                    alpha: false,
                    in_place: false,
                },
            );
            return;
        }
        if let Some(TransitionTransform::Scroll { direction, style }) = self.transform {
            let axis = usize::from(!matches!(direction, 0 | 2));
            let direction = if matches!(direction, 1 | 2) { 1 } else { -1 };
            let extent = [target.width as i32, target.height as i32][axis];
            let amount = self.transform_progress as i32;
            let mut previous = [0; 2];
            if style != 0 {
                previous[axis] = direction * amount;
            }
            if style == 1 {
                target.composite_region(
                    source,
                    source.full_region().map(i64::from),
                    previous[0],
                    previous[1],
                    None,
                );
            } else {
                let incoming = target.clone();
                let mut next = [0; 2];
                next[axis] = direction * (amount - extent);
                target.composite_region(
                    source,
                    source.full_region().map(i64::from),
                    previous[0],
                    previous[1],
                    None,
                );
                target.composite_region(
                    &incoming,
                    incoming.full_region().map(i64::from),
                    next[0],
                    next[1],
                    None,
                );
            }
            return;
        }
        if let Some(TransitionTransform::HorizontalBlur { cross, radius }) = self.transform {
            let radius = u16::from(radius);
            if cross {
                let blur = u16::from(self.progress.min(255 - self.progress)) * radius / 127;
                if blur != 0 {
                    let incoming = target.clone();
                    target.pixels.copy_from_slice(&source.pixels);
                    blur_rows(&incoming, target, blur, Some(self.progress));
                    return;
                }
            } else {
                let step = self.transform_progress as u16;
                if step <= radius {
                    if step == 0 {
                        target.pixels.copy_from_slice(&source.pixels);
                    } else {
                        blur_rows(source, target, step, None);
                    }
                } else {
                    let blur = 2 * radius + 1 - step;
                    if blur != 0 {
                        let incoming = target.clone();
                        blur_rows(&incoming, target, blur, None);
                    }
                }
                return;
            }
        }
        if let Some(TransitionTransform::VerticalSplit { reverse }) = self.transform {
            let progress = if reverse {
                256 - self.transform_progress
            } else {
                self.transform_progress
            };
            if progress == 256 {
                return;
            }
            let incoming = target.clone();
            target.pixels.copy_from_slice(&source.pixels);
            let middle = i64::from(target.height / 2);
            let height = middle * i64::from(progress) / 256;
            if height != 0 {
                let width = i64::from(target.width);
                target.composite_region(&incoming, [0, middle - height, width, middle], 0, 0, None);
                target.composite_region(
                    &incoming,
                    [0, middle, width, middle + height],
                    0,
                    target.height as i32 - height as i32,
                    None,
                );
            }
            return;
        }
        if let Some(quake) = &self.quake {
            let [x, y] = self.displacement;
            if [x, y] != [0, 0] {
                let shifted = target.clone();
                if quake.fill_edges {
                    for pixel in target.pixels.chunks_exact_mut(4) {
                        pixel.copy_from_slice(&[0, 0, 0, 255]);
                    }
                } else {
                    target.pixels.copy_from_slice(&source.pixels);
                }
                target.composite_region(&shifted, shifted.full_region().map(i64::from), x, y, None);
            }
            return;
        }
        let progress = i32::from(self.progress);
        for (index, (old, new)) in source
            .pixels
            .chunks_exact(4)
            .zip(target.pixels.chunks_exact_mut(4))
            .enumerate()
        {
            let alpha = match &self.mask {
                Some((indices, softness)) if *softness > 0 => {
                    let threshold = progress * (256 + softness) / 255;
                    ((threshold - i32::from(indices[index])) * 255 / softness).clamp(0, 255)
                }
                Some((indices, _)) => 255 * i32::from(i32::from(indices[index]) < progress),
                None => progress,
            } as u32;
            for (before, after) in old.iter().zip(new) {
                *after = ((u32::from(*after) * alpha + u32::from(*before) * (255 - alpha) + 127)
                    / 255) as u8;
            }
        }
    }

    fn blend_zoom_flip(&self, previous: &Surface, target: &mut Surface, mode: u8, sampling: u8) {
        let phase = self.transform_progress as i32;
        let incoming = target.clone();
        let extent = [target.width as i32, target.height as i32];
        if mode <= 3 {
            let axis = usize::from(mode & 1);
            let mut old_size = extent;
            let mut new_size = extent;
            old_size[axis] -= phase;
            new_size[axis] = phase;
            let mut old_position = [0; 2];
            let mut new_position = [0; 2];
            if mode < 2 {
                old_position[axis] = phase;
            } else {
                new_position[axis] = old_size[axis];
            }
            for (source, position, size) in [
                (previous, old_position, old_size),
                (&incoming, new_position, new_size),
            ] {
                draw_stretched_texture(
                    source,
                    target,
                    source.full_region().map(i64::from),
                    position,
                    size,
                    255,
                    sampling != 0,
                );
            }
            return;
        }
        let (source, zoom, mut opacity) = if mode == 6 {
            target.pixels.copy_from_slice(&previous.pixels);
            (&incoming, phase, phase.min(255) as u8)
        } else if phase <= 256 {
            (previous, (256 - phase).max(1), 255)
        } else {
            (&incoming, phase - 256, 255)
        };
        if mode == 5
            && let Some((last_phase, frame)) = &self.feedback
        {
            target.pixels.copy_from_slice(&frame.pixels);
            if *last_phase == self.transform_progress {
                return;
            }
            if zoom < 256 {
                opacity = 8;
            }
        }
        let crop = extent.map(|length| {
            let length = i64::from(length);
            let size = (length * i64::from(zoom) / 256).max(1);
            let start = (length - size) / 2;
            [start, start + size]
        });
        draw_stretched_texture(
            source,
            target,
            [crop[0][0], crop[1][0], crop[0][1], crop[1][1]],
            [0; 2],
            extent,
            opacity,
            sampling != 0,
        );
    }

    fn blend_wave(
        &self,
        source: &Surface,
        target: &mut Surface,
        mode: i32,
        amplitude: i32,
        frequency: i32,
        border: [[u8; 3]; 2],
    ) {
        let elapsed = self.transform_progress;
        let remaining = self.duration - elapsed;
        let half = (self.duration / 2).max(1);
        let envelope = sine_index(i64::from(elapsed.min(remaining)) * 128 / i64::from(half));
        let frequency = i64::from(frequency);
        let fraction = |value: u32| frequency * i64::from(value) / i64::from(self.duration);
        let step = match mode {
            0 => (frequency as f64 * envelope) as i64,
            1 => fraction(remaining),
            2 => fraction(elapsed),
            _ => frequency,
        };
        let phase = if mode > 2 {
            i64::from(amplitude) * fraction(elapsed)
        } else {
            -step * i64::from(target.height / 2)
        };
        let amplitude = f64::from(amplitude) * envelope;
        let weight = u32::from(self.progress);
        let blend = |a: u8, b: u8| {
            ((u32::from(a) * (255 - weight) + u32::from(b) * weight + 127) / 255) as u8
        };
        let border = [
            blend(border[0][0], border[1][0]),
            blend(border[0][1], border[1][1]),
            blend(border[0][2], border[1][2]),
            255,
        ];
        let width = i64::from(target.width);
        let stride = width as usize * 4;
        let incoming = target.clone();
        for (y, row) in target.pixels.chunks_exact_mut(stride).enumerate() {
            let angle = phase.wrapping_add(step.wrapping_mul(y as i64)) >> 9;
            let delta = (amplitude * sine_index(angle)) as i64;
            for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x = x as i64 - delta;
                if !(0..width).contains(&x) {
                    *pixel = border;
                    continue;
                }
                let offset = y * stride + x as usize * 4;
                for (index, channel) in pixel.iter_mut().enumerate() {
                    *channel = blend(
                        source.pixels[offset + index],
                        incoming.pixels[offset + index],
                    );
                }
            }
        }
    }
}

/// CPU implementation of the Lucifen host surfaces. The VM uses numbered
/// surfaces and presentation leases an immutable image to the host.
#[derive(Default)]
pub struct Graphics {
    surfaces: BTreeMap<i32, Arc<Surface>>,
    resident_bytes: usize,
    next_surface: i32,
    group: i32,
    texture_group: i32,
    textures: BTreeMap<i32, Texture>,
    texture_bytes: usize,
    layers: BTreeMap<i32, Layer>,
    background_colors: BTreeMap<i32, [u8; 4]>,
    backgrounds: BTreeMap<i32, Background>,
    hidden_passes: BTreeMap<i32, HiddenPasses>,
    manual_refresh: BTreeSet<i32>,
    background_warps: BTreeMap<i32, BackgroundWarp>,
    scene_offsets: BTreeMap<i32, SceneOffset>,
    layer_order: i64,
    pub(crate) sprite_move_priorities: Option<[i32; 2]>,
    base_display: i32,
    display: i32,
    transition_sources: BTreeMap<i32, i32>,
    transition_buffers: BTreeMap<i32, i32>,
    transitions: BTreeMap<i32, Transition>,
    dirty: bool,
    presented_scene: Option<(i32, Arc<Surface>)>,
    presented: Option<Arc<Surface>>,
}

impl Graphics {
    pub(crate) fn blend_bitmap_rule(
        &mut self,
        handles: [i32; 4],
        settings: BitmapRuleBlend,
    ) -> i32 {
        let [destination, first, second, mask] = handles;
        let bitmap = |id| {
            self.bitmap(BitmapTarget::Surface(id))
                .cloned()
                .or_else(|| self.bitmap(BitmapTarget::Texture(id)).cloned())
        };
        let (Some(target), Some(first), Some(second), Some(mask)) = (
            bitmap(destination),
            bitmap(first),
            bitmap(second),
            bitmap(mask),
        ) else {
            return -1;
        };
        let dimensions = [&target, &first, &second, &mask]
            .map(|image| [i64::from(image.width), i64::from(image.height)]);
        let mut positions = settings.positions.map(|point| point.map(i64::from));
        let mut size = settings.size.map(i64::from);
        for axis in 0..2 {
            let shift = positions
                .iter()
                .map(|point| -point[axis])
                .max()
                .unwrap()
                .max(0);
            if size[axis] <= 0 {
                size[axis] = dimensions[0][axis];
            }
            size[axis] -= shift;
            for (position, dimension) in positions.iter_mut().zip(dimensions) {
                position[axis] += shift;
                size[axis] = size[axis].min(dimension[axis] - position[axis]);
            }
            if size[axis] <= 0 {
                return 3;
            }
        }
        let indices = mask.indices.as_deref();
        if indices.is_none() && !matches!(settings.progress, 0 | 255) {
            return 3;
        }
        if settings.mode > 2 && !matches!(settings.progress, 0 | 255) {
            return 0;
        }
        drop(target);
        let Some((source, target)) = self.bitmap_mut(destination) else {
            return -1;
        };
        let target = Arc::make_mut(target);
        let softness = i64::from(settings.softness.max(0));
        let threshold = i64::from(settings.progress) * (softness + 256) / 256;
        let offsets = positions.map(|point| point.map(|coordinate| coordinate as usize));
        let widths = dimensions.map(|dimension| dimension[0] as usize);
        for y in 0..size[1] as usize {
            let starts: [usize; 4] = std::array::from_fn(|index| {
                let [left, top] = offsets[index];
                (top + y) * widths[index] + left
            });
            for x in 0..size[0] as usize {
                let a = &first.pixels[(starts[1] + x) * 4..][..4];
                let b = &second.pixels[(starts[2] + x) * 4..][..4];
                let out = &mut target.pixels[(starts[0] + x) * 4..][..4];
                let index = indices.map_or(0, |indices| i64::from(indices[starts[3] + x]));
                if settings.progress == 0 || settings.progress != 255 && index >= threshold {
                    out.copy_from_slice(a);
                } else if settings.progress == 255 || index < threshold - softness {
                    out.copy_from_slice(b);
                } else {
                    let weight = (255 - (index - threshold + softness) * 255 / softness) as i32;
                    blend_rule_pixel(out, a, b, weight, settings.mode);
                }
            }
        }
        self.surface_changed(source);
        self.dirty |= self.layers.contains_key(&destination);
        0
    }

    pub fn open(width: u32, height: u32) -> io::Result<Self> {
        let mut graphics = Self::default();
        let id = graphics.create(width, height)?;
        graphics.base_display = id;
        graphics.display = id;
        Ok(graphics)
    }

    pub fn display(&self) -> i32 {
        self.display
    }

    pub fn select_display(&mut self, id: i32) -> bool {
        if self.surfaces.contains_key(&id) {
            self.display = id;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub(crate) fn set_transition_source(&mut self, target: i32, source: i32) -> i32 {
        let (Some(target_surface), Some(source_surface)) =
            (self.surfaces.get(&target), self.surfaces.get(&source))
        else {
            return -1;
        };
        if (target_surface.width, target_surface.height)
            != (source_surface.width, source_surface.height)
        {
            return 1;
        }
        self.transition_sources.insert(target, source);
        0
    }

    fn retain_targets(&mut self) {
        let transitioning = self.transitions.contains_key(&self.display);
        if !self.surfaces.contains_key(&self.display) {
            self.display = self.base_display;
            self.dirty = true;
        }
        self.layers
            .retain(|_, layer| self.surfaces.contains_key(&layer.target));
        self.transition_sources.retain(|target, source| {
            self.surfaces.contains_key(target) && self.surfaces.contains_key(source)
        });
        let mut released = Vec::new();
        self.transition_buffers.retain(|target, buffer| {
            if !self.surfaces.contains_key(target) {
                released.push(*buffer);
                return false;
            }
            self.surfaces.contains_key(buffer)
        });
        for buffer in released {
            self.release_surface(buffer);
        }
        self.transitions.retain(|target, effect| {
            self.surfaces.contains_key(target) && self.surfaces.contains_key(&effect.peer)
        });
        self.dirty |= transitioning && !self.transitions.contains_key(&self.display);
        self.backgrounds
            .retain(|target, _| self.surfaces.contains_key(target));
        self.hidden_passes
            .retain(|target, _| self.surfaces.contains_key(target));
        self.background_warps
            .retain(|target, _| self.surfaces.contains_key(target));
        self.scene_offsets
            .retain(|target, _| self.surfaces.contains_key(target));
    }

    pub fn present_display(&mut self) -> io::Result<()> {
        if self.dirty {
            self.present()?;
            self.dirty = false;
        }
        Ok(())
    }

    pub fn set_background_color(
        &mut self,
        target: i32,
        color: [u8; 4],
        mode: u8,
    ) -> io::Result<bool> {
        if !self.surfaces.contains_key(&target) {
            return Ok(false);
        }
        self.background_colors.insert(target, color);
        if mode < 2 {
            self.backgrounds.remove(&target);
            self.fill(target, None, color);
            if mode == 1 {
                self.present_display()?;
            }
        }
        Ok(true)
    }

    pub(crate) fn set_background_image(
        &mut self,
        target: i32,
        image: i32,
        position: [i32; 2],
        redraw: bool,
    ) -> io::Result<bool> {
        if !self.surfaces.contains_key(&target) {
            return Ok(false);
        }
        if image == 0 {
            self.backgrounds.remove(&target);
            if let Some(&color) = self.background_colors.get(&target) {
                self.fill(target, None, color);
            }
        } else {
            let Some(surface) = self.surfaces.get(&image) else {
                return Ok(false);
            };
            self.backgrounds.insert(
                target,
                Background {
                    source: image,
                    image: surface.clone(),
                    position,
                    window: None,
                    stretch: None,
                    transformed: None,
                    pivot: [0; 2],
                    rotation: BackgroundRotation::default(),
                },
            );
        }
        self.dirty = true;
        if redraw {
            self.present_display()?;
        }
        Ok(true)
    }

    pub(crate) fn move_background(&mut self, target: i32, position: [i32; 2]) -> io::Result<i32> {
        if !self.surfaces.contains_key(&target) {
            return Ok(-1);
        }
        if self.shift_background(target, position) {
            self.present_display()?;
        }
        Ok(0)
    }

    pub(crate) fn shift_background(&mut self, target: i32, position: [i32; 2]) -> bool {
        let Some(background) = self.backgrounds.get_mut(&target) else {
            return false;
        };
        self.dirty |= background.position != position;
        background.position = position;
        true
    }

    pub(crate) fn background_geometry(
        &self,
        target: i32,
    ) -> Option<([i32; 2], [i32; 2], [i32; 2])> {
        let background = self.backgrounds.get(&target)?;
        let image = self
            .surfaces
            .get(&background.source)
            .unwrap_or(&background.image);
        let viewport = self.surfaces.get(&target)?;
        Some((
            background.position,
            [image.width as i32, image.height as i32],
            [viewport.width as i32, viewport.height as i32],
        ))
    }

    pub(crate) fn stretch_background(
        &mut self,
        target: i32,
        mut destination: [i32; 4],
        source: Option<[i32; 4]>,
    ) -> i32 {
        let Some(canvas) = self.surfaces.get(&target) else {
            return -1;
        };
        let Some(background) = self.backgrounds.get_mut(&target) else {
            return 0;
        };
        let full_destination = destination == [0; 4];
        if destination[2] <= 0 {
            destination[2] = canvas.width as i32;
        }
        if destination[3] <= 0 {
            destination[3] = canvas.height as i32;
        }
        let stretch = (!full_destination).then(|| BackgroundStretch {
            destination,
            source: source.unwrap_or_else(|| background.image.full_region()),
        });
        if stretch.is_some() && background.stretch == stretch && background.position == [0; 2] {
            return 0;
        }
        background.stretch = stretch;
        background.position = [0; 2];
        background.window = None;
        background.rotation = BackgroundRotation::default();
        self.dirty = true;
        1
    }

    pub(crate) fn background_state(&self, target: i32) -> Option<(i32, [i32; 2], [u8; 3])> {
        self.surfaces.get(&target)?;
        let (source, position) = self
            .backgrounds
            .get(&target)
            .map_or((0, [0; 2]), |background| {
                (background.source, background.position)
            });
        let [red, green, blue, _] = self
            .background_colors
            .get(&target)
            .copied()
            .unwrap_or_default();
        Some((source, position, [red, green, blue]))
    }

    pub(crate) fn set_background_pivot(
        &mut self,
        target: i32,
        mode: i32,
        custom: [i32; 2],
    ) -> bool {
        let Some(background) = self.backgrounds.get_mut(&target) else {
            return false;
        };
        let [width, height] = [
            background.image.width as i32,
            background.image.height as i32,
        ];
        background.pivot = match mode {
            0 => [0, 0],
            1 => [width, 0],
            2 => [0, height],
            3 => [width, height],
            4 => [width / 2, height / 2],
            _ => custom,
        };
        self.dirty = true;
        true
    }

    pub(crate) fn background_rotation(&self, target: i32) -> Option<BackgroundRotation> {
        Some(self.backgrounds.get(&target)?.rotation)
    }

    pub(crate) fn rotate_background(&mut self, target: i32, rotation: BackgroundRotation) -> bool {
        let Some(background) = self.backgrounds.get_mut(&target) else {
            return false;
        };
        let stretched = background.stretch.take().is_some();
        self.dirty |= stretched || background.rotation != rotation;
        background.rotation = rotation;
        true
    }

    pub(crate) fn background_window(&self, target: i32) -> Option<[i32; 4]> {
        let viewport = self.surfaces.get(&target)?;
        let background = self.backgrounds.get(&target)?;
        let [x, y] = background.position;
        let [width, height] = background
            .window
            .unwrap_or([viewport.width as i32, viewport.height as i32]);
        Some([x, y, width, height])
    }

    pub(crate) fn set_background_window(
        &mut self,
        target: i32,
        [x, y, width, height]: [i32; 4],
    ) -> bool {
        let Some(background) = self.backgrounds.get_mut(&target) else {
            return false;
        };
        let stretched = background.stretch.take().is_some();
        self.dirty |= stretched
            || background.position != [x, y]
            || background.window != Some([width, height]);
        background.position = [x, y];
        background.window = Some([width, height]);
        true
    }

    pub(crate) fn set_background_warp(&mut self, target: i32, warp: BackgroundWarp) {
        let changed = self.background_warps.get(&target).is_none_or(|old| {
            old.phase != warp.phase
                || old.gain != warp.gain
                || old.fill_edges != warp.fill_edges
                || !Arc::ptr_eq(&old.table, &warp.table)
        });
        if changed {
            self.background_warps.insert(target, warp);
            self.dirty = true;
        }
    }

    pub(crate) fn clear_background_warp(&mut self, target: i32) {
        self.dirty |= self.background_warps.remove(&target).is_some();
    }

    pub(crate) fn background_image(&self, target: i32) -> Option<Arc<Surface>> {
        let background = self.backgrounds.get(&target)?;
        Some(
            self.surfaces
                .get(&background.source)
                .unwrap_or(&background.image)
                .clone(),
        )
    }

    pub(crate) fn transform_background(
        &mut self,
        target: i32,
        source: &Surface,
        transform: BackgroundTransform,
    ) -> bool {
        let Some(background) = self.backgrounds.get_mut(&target) else {
            return false;
        };
        if transform.in_place {
            if let Some(image) = self.surfaces.get_mut(&background.source) {
                draw_background_transform(source, Arc::make_mut(image), transform);
                let id = background.source;
                self.surface_changed(id);
            } else {
                draw_background_transform(source, Arc::make_mut(&mut background.image), transform);
            }
        } else {
            if background.transformed.is_none() {
                let Some(base) = self.surfaces.get(&target) else {
                    return false;
                };
                let mut frame = base.clone();
                let image = self
                    .surfaces
                    .get(&background.source)
                    .unwrap_or(&background.image);
                Arc::make_mut(&mut frame).composite(image, 0, 0, 255);
                background.transformed = Some(frame);
            }
            draw_background_transform(
                source,
                Arc::make_mut(background.transformed.as_mut().unwrap()),
                transform,
            );
        }
        self.dirty = true;
        true
    }

    pub(crate) fn clear_background_transform(&mut self, target: i32) {
        if let Some(background) = self.backgrounds.get_mut(&target) {
            self.dirty |= background.transformed.take().is_some();
        }
    }

    pub(crate) fn set_scene_offset(&mut self, target: i32, offset: SceneOffset) {
        let previous = if offset == SceneOffset::default() {
            self.scene_offsets.remove(&target)
        } else {
            self.scene_offsets.insert(target, offset)
        }
        .unwrap_or_default();
        if previous != offset {
            self.invalidate_scene(target);
        }
    }

    pub(crate) fn hide_layer_passes(&mut self, target: i32, primary: bool, overlay: bool) -> i32 {
        if !self.surfaces.contains_key(&target) {
            return -1;
        }
        let hidden = self.hidden_passes.entry(target).or_default();
        self.dirty |= hidden.primary != primary || hidden.overlay != overlay;
        hidden.primary = primary;
        hidden.overlay = overlay;
        0
    }

    pub(crate) fn set_manual_refresh(&mut self, target: i32, manual: bool) -> i32 {
        if !self.surfaces.contains_key(&target) {
            return 0;
        }
        if manual {
            i32::from(!self.manual_refresh.insert(target))
        } else {
            let previous = self.manual_refresh.remove(&target);
            if previous {
                self.surface_changed(target);
            }
            i32::from(previous)
        }
    }

    pub(crate) fn invalidate_scene(&mut self, target: i32) -> bool {
        if !self.surfaces.contains_key(&target) {
            return false;
        }
        if target == self.display || self.transitions.contains_key(&target) {
            self.dirty = true;
        }
        true
    }

    pub(crate) fn hide_background_pass(&mut self, target: i32, background: bool) -> i32 {
        if !self.surfaces.contains_key(&target) {
            return -1;
        }
        let hidden = self.hidden_passes.entry(target).or_default();
        self.dirty |= hidden.background != background;
        hidden.background = background;
        0
    }

    pub fn clear_layers(&mut self, target: i32) {
        let before = self.layers.len();
        self.layers.retain(|_, layer| layer.target != target);
        self.dirty |= self.layers.len() != before;
    }

    pub fn set_layers_visible(
        &mut self,
        target: i32,
        visible: bool,
        priority: i32,
        layer_group: i32,
        texture_group: i32,
        inverted: bool,
    ) {
        for (id, layer) in &self.layers {
            let Some(texture) = self.textures.get_mut(id) else {
                continue;
            };
            if layer.target == target
                && (priority == -1 || (layer.priority == priority as u32) != inverted)
                && (layer_group == -1 || (layer.group == layer_group) != inverted)
                && (texture_group == -1 || (texture.group == texture_group) != inverted)
            {
                self.dirty |= texture.visible != visible;
                texture.visible = visible;
            }
        }
    }

    pub fn attach_layer(
        &mut self,
        target: i32,
        id: i32,
        priority: u32,
        group: i32,
        overlay: bool,
        append: bool,
    ) -> i32 {
        if !self.surfaces.contains_key(&target) {
            return -1;
        }
        if !self.textures.contains_key(&id) {
            return 1;
        }
        if self.layers.contains_key(&id) {
            return 2;
        }
        self.layer_order = self.layer_order.saturating_add(1);
        let order = if append {
            self.layer_order
        } else {
            -self.layer_order
        };
        self.layers.insert(
            id,
            Layer {
                target,
                priority,
                group,
                overlay,
                order,
            },
        );
        self.dirty = true;
        0
    }

    pub fn contains_layer(&self, target: i32, id: i32) -> bool {
        self.layers
            .get(&id)
            .is_some_and(|layer| layer.target == target)
    }

    pub fn is_layer(&self, id: i32) -> bool {
        self.layers.contains_key(&id)
    }

    pub(crate) fn is_overlay(&self, id: i32) -> bool {
        self.layers.get(&id).is_some_and(|layer| layer.overlay)
    }

    pub fn remove_layer(&mut self, target: i32, id: i32) -> bool {
        if !self.contains_layer(target, id) {
            return false;
        }
        self.layers.remove(&id);
        self.dirty = true;
        true
    }

    pub fn remove_layers(&mut self, target: i32, priority: u32, others: bool) {
        let before = self.layers.len();
        self.layers
            .retain(|_, layer| layer.target != target || (layer.priority != priority) != others);
        self.dirty |= self.layers.len() != before;
    }

    pub fn remove_layer_groups(
        &mut self,
        target: i32,
        group: i32,
        texture_group: i32,
        others: bool,
    ) {
        let before = self.layers.len();
        self.layers.retain(|id, layer| {
            if layer.target != target {
                return true;
            }
            let match_layer = group == -1 || ((layer.group == group) != others);
            let match_texture = texture_group == -1
                || self
                    .textures
                    .get(id)
                    .is_some_and(|texture| (texture.group == texture_group) != others);
            !(match_layer && match_texture)
        });
        self.dirty |= self.layers.len() != before;
    }

    pub fn move_layer(
        &mut self,
        target: i32,
        id: i32,
        priority: u32,
        only_if_changed: bool,
        append: bool,
    ) -> bool {
        let Some(layer) = self.layers.get_mut(&id) else {
            return false;
        };
        if layer.target != target {
            return false;
        }
        if only_if_changed && layer.priority == priority {
            return true;
        }
        self.layer_order = self.layer_order.saturating_add(1);
        layer.priority = priority;
        layer.order = if append {
            self.layer_order
        } else {
            -self.layer_order
        };
        self.dirty = true;
        true
    }

    pub fn alpha_compositing(&self) -> bool {
        true
    }

    pub(crate) fn next_surface(&mut self) -> io::Result<i32> {
        self.next_surface = self
            .next_surface
            .checked_add(1)
            .ok_or_else(|| invalid("Lucifen surface identifiers exhausted"))?;
        Ok(self.next_surface)
    }

    pub fn create(&mut self, width: u32, height: u32) -> io::Result<i32> {
        let id = self.next_surface()?;
        self.insert(id, Surface::empty(width, height)?)?;
        Ok(id)
    }

    pub fn load(&mut self, bytes: &[u8]) -> io::Result<i32> {
        self.load_image(bytes, None)
    }

    pub(crate) fn load_image(
        &mut self,
        bytes: &[u8],
        color_key: Option<[u8; 3]>,
    ) -> io::Result<i32> {
        let ElgImage {
            width,
            height,
            origin,
            mut rgba,
            indices,
        } = decode_elg(bytes)?;
        if let Some(color_key) = color_key {
            for pixel in rgba.chunks_exact_mut(4) {
                pixel[3] = if pixel[..3] == color_key { 0 } else { 255 };
            }
        }
        let id = self.next_surface()?;
        self.insert(
            id,
            Surface {
                width,
                height,
                origin,
                pixels: rgba,
                indices: indices.map(Arc::from),
                group: 0,
            },
        )?;
        Ok(id)
    }

    pub fn fill(&mut self, id: i32, within: Option<[i32; 4]>, color: [u8; 4]) -> bool {
        let Some(surface) = self.surfaces.get_mut(&id) else {
            return false;
        };
        let surface = Arc::make_mut(surface);
        let width = surface.width as i32;
        let height = surface.height as i32;
        let [left, top, right, bottom] = within.unwrap_or([0, 0, width, height]);
        let [left, top, right, bottom] = [
            left.clamp(0, width),
            top.clamp(0, height),
            right.clamp(0, width),
            bottom.clamp(0, height),
        ];
        if right <= left || bottom <= top {
            return false;
        }
        for row in top..bottom {
            let start = (row as usize * width as usize + left as usize) * 4;
            let end = start + (right - left) as usize * 4;
            for pixel in surface.pixels[start..end].chunks_exact_mut(4) {
                pixel.copy_from_slice(&color);
            }
        }
        self.surface_changed(id);
        true
    }

    fn insert(&mut self, id: i32, mut surface: Surface) -> io::Result<()> {
        let old_bytes = self
            .surfaces
            .get(&id)
            .map_or(0, |surface| surface.byte_len());
        let resident = self.resident_bytes - old_bytes + surface.byte_len();
        if id < 0 || resident > MAX_SURFACE_BYTES {
            return Err(invalid("Lucifen surface identifier or memory limit"));
        }
        self.resident_bytes = resident;
        surface.group = self.group;
        self.surfaces.insert(id, Arc::new(surface));
        Ok(())
    }

    pub(crate) fn clone_bitmap(&mut self, source: i32) -> io::Result<i32> {
        let Some(bitmap) = self
            .surfaces
            .get(&source)
            .map(|surface| (**surface).clone())
        else {
            return Ok(0);
        };
        let bitmap_id = self.next_surface()?;
        self.insert(bitmap_id, bitmap)?;
        Ok(bitmap_id)
    }

    pub fn texture_from_surface(
        &mut self,
        source: i32,
        rect: Option<[i32; 4]>,
        position: [i32; 2],
    ) -> io::Result<i32> {
        let bitmap = self.surface(source)?;
        let region = rect
            .map(|rect| bitmap.clamped_region(rect))
            .transpose()?
            .unwrap_or_else(|| bitmap.full_region());
        let pixels = bitmap.clone();
        let blend = if pixels.pixels.chunks_exact(4).any(|pixel| pixel[3] != 255) {
            2
        } else {
            0
        };
        self.insert_texture(Texture {
            group: self.texture_group,
            source: Some(source),
            pixels,
            region,
            source_offset: [0, 0],
            sampling: 0,
            stretch: None,
            position,
            blend,
            opacity: 255,
            visible: true,
        })
    }

    pub(crate) fn clone_texture(&mut self, source: i32) -> io::Result<i32> {
        let Some(mut texture) = self.textures.get(&source).cloned() else {
            return Ok(0);
        };
        texture.group = self.texture_group;
        self.insert_texture(texture)
    }

    pub(crate) fn rebind_texture(
        &mut self,
        id: i32,
        source: i32,
        placement: Option<(Option<[i32; 4]>, [i32; 2])>,
    ) -> io::Result<i32> {
        let Some(bitmap) = self.surfaces.get(&source).cloned() else {
            return Ok(-2);
        };
        let Some(texture) = self.textures.get_mut(&id) else {
            return Ok(-1);
        };
        let total = self.texture_bytes - texture.pixels.byte_len() + bitmap.byte_len();
        if total > MAX_SURFACE_BYTES {
            return Err(invalid("Lucifen texture memory limit"));
        }
        if let Some((rect, position)) = placement {
            texture.region = rect
                .map(|rect| bitmap.clamped_region(rect))
                .transpose()?
                .unwrap_or_else(|| bitmap.full_region());
            texture.source_offset = [0, 0];
            texture.position = position;
        }
        texture.source = Some(source);
        texture.pixels = bitmap;
        self.texture_bytes = total;
        self.dirty |= self.layers.contains_key(&id);
        Ok(0)
    }

    fn insert_texture(&mut self, texture: Texture) -> io::Result<i32> {
        let total = self.texture_bytes + texture.pixels.byte_len();
        if total > MAX_SURFACE_BYTES {
            return Err(invalid("Lucifen texture memory limit"));
        }
        let id = self.next_surface()?;
        self.textures.insert(id, texture);
        self.texture_bytes = total;
        Ok(id)
    }

    pub(crate) fn set_texture_region(&mut self, texture: i32, rect: [i32; 4]) -> io::Result<()> {
        let Some(target) = self.textures.get_mut(&texture) else {
            return Ok(());
        };
        target.region = target.pixels.clamped_region(rect)?;
        target.source_offset = [0, 0];
        self.dirty |= self.layers.contains_key(&texture);
        Ok(())
    }

    pub fn select_texture_group(&mut self, group: i32) -> i32 {
        std::mem::replace(&mut self.texture_group, group)
    }

    pub fn release_texture_group(&mut self, group: i32, keep_group: bool) {
        self.textures.retain(|_, texture| {
            let keep = (texture.group != group) != keep_group;
            if !keep {
                self.texture_bytes -= texture.pixels.byte_len();
            }
            keep
        });
        let before = self.layers.len();
        self.layers.retain(|id, _| self.textures.contains_key(id));
        self.dirty |= self.layers.len() != before;
    }

    pub fn free_texture(&mut self, id: i32) -> bool {
        if let Some(texture) = self.textures.remove(&id) {
            self.texture_bytes -= texture.pixels.byte_len();
            self.dirty |= self.layers.remove(&id).is_some();
            true
        } else {
            false
        }
    }

    pub fn clear_textures(&mut self) {
        self.textures.clear();
        self.dirty |= !self.layers.is_empty();
        self.layers.clear();
        self.texture_bytes = 0;
    }

    pub fn show_texture(&mut self, id: i32, visible: bool) {
        if let Some(texture) = self.textures.get_mut(&id) {
            self.dirty |= texture.visible != visible && self.layers.contains_key(&id);
            texture.visible = visible;
        }
    }

    pub(crate) fn texture_visible(&self, id: i32) -> bool {
        self.textures
            .get(&id)
            .is_some_and(|texture| texture.visible)
    }

    pub fn invalidate_texture(&mut self, id: i32) -> bool {
        self.dirty |= self.layers.contains_key(&id);
        self.textures.contains_key(&id)
    }

    pub(crate) fn replace_texture(
        &mut self,
        id: i32,
        image: &[u8],
        position: [i32; 2],
    ) -> io::Result<()> {
        let Some(texture) = self.textures.get_mut(&id) else {
            return Ok(());
        };
        let ElgImage {
            width,
            height,
            origin,
            rgba,
            ..
        } = decode_elg(image)?;
        let resident = self.texture_bytes - texture.pixels.byte_len() + rgba.len();
        if resident > MAX_SURFACE_BYTES {
            return Err(invalid("Lucifen texture memory limit"));
        }
        texture.pixels = Arc::new(Surface {
            width,
            height,
            origin,
            pixels: rgba,
            indices: None,
            group: 0,
        });
        texture.source = None;
        texture.region = texture.pixels.full_region();
        texture.source_offset = [0, 0];
        texture.position = position;
        self.texture_bytes = resident;
        self.dirty |= self.layers.contains_key(&id);
        Ok(())
    }

    pub fn texture_blend(&mut self, id: i32, blend: u8) -> i32 {
        let Some(texture) = self.textures.get_mut(&id) else {
            return -2;
        };
        if blend > 31 {
            return 1;
        }
        texture.blend = blend;
        self.dirty |= self.layers.contains_key(&id);
        0
    }

    pub fn texture_opacity(&mut self, id: i32, opacity: u8) -> i32 {
        let Some(texture) = self.textures.get_mut(&id) else {
            return -2;
        };
        texture.opacity = opacity;
        self.dirty |= self.layers.contains_key(&id);
        0
    }

    pub fn texture_move(&mut self, id: i32, x: i32, y: i32, relative: bool) -> bool {
        let Some(texture) = self.textures.get_mut(&id) else {
            return false;
        };
        let [old_x, old_y] = texture.position;
        let pos = if relative {
            [old_x.saturating_add(x), old_y.saturating_add(y)]
        } else {
            [x, y]
        };
        let changed = texture.position != pos;
        texture.position = pos;
        self.dirty |= changed && self.layers.contains_key(&id);
        changed
    }

    pub fn texture_bounds(&self, id: i32) -> Option<[i32; 4]> {
        let texture = self.textures.get(&id)?;
        let ([x, y], [width, height]) = texture.placement();
        Some([x, y, x.saturating_add(width), y.saturating_add(height)])
    }

    pub(crate) fn stretch_texture(
        &mut self,
        id: i32,
        stretch: Option<([i32; 2], [i32; 2])>,
    ) -> bool {
        let Some(texture) = self.textures.get_mut(&id) else {
            return false;
        };
        let changed = std::mem::replace(&mut texture.stretch, stretch) != stretch;
        self.dirty |= changed && self.layers.contains_key(&id);
        true
    }

    pub(crate) fn sprite_geometry(&self, id: i32) -> Option<SpriteGeometry> {
        let texture = self.textures.get(&id)?;
        let surface = self.surfaces.get(&self.layers.get(&id)?.target)?;
        let (position, size) = texture.placement();
        let (x, y) = self.texture_image(texture).origin;
        Some(SpriteGeometry {
            position,
            size,
            origin: [i32::from(x), i32::from(y)],
            viewport: [surface.width as i32, surface.height as i32],
        })
    }

    pub(crate) fn set_sprite_plane(&mut self, id: i32, plane: i32) {
        let Some([front, back]) = self.sprite_move_priorities else {
            return;
        };
        let priority = match plane {
            1 => front,
            2 => back,
            _ => return,
        };
        if let Some(target) = self.layers.get(&id).map(|layer| layer.target) {
            self.move_layer(target, id, priority as u32, false, plane == 2);
        }
    }

    pub(crate) fn move_texture_source(&mut self, id: i32, source_offset: [i32; 2]) -> bool {
        let Some(texture) = self.textures.get_mut(&id) else {
            return false;
        };
        let changed = std::mem::replace(&mut texture.source_offset, source_offset) != source_offset;
        self.dirty |= changed && self.layers.contains_key(&id);
        true
    }

    pub(crate) fn set_texture_sampling(&mut self, id: i32, filter: u8) -> bool {
        let Some(texture) = self.textures.get_mut(&id) else {
            return false;
        };
        if filter & 0x0f > 3 || filter & 0xf0 > 0x10 {
            return false;
        }
        let changed = std::mem::replace(&mut texture.sampling, filter) != filter;
        self.dirty |= changed && texture.stretch.is_some() && self.layers.contains_key(&id);
        changed
    }

    pub(crate) fn texture_state(&self, id: i32) -> Option<([i32; 2], [i32; 2], u8)> {
        self.textures
            .get(&id)
            .map(|texture| (texture.position, texture.source_offset, texture.opacity))
    }

    pub fn hit_texture(&self, id: i32, x: i32, y: i32, alpha: bool) -> bool {
        let Some(texture) = self.textures.get(&id) else {
            return false;
        };
        let image = self.texture_image(texture);
        let ([left, top], [width, height]) = texture.placement();
        let [src_left, src_top, src_right, src_bottom] = texture.source_region();
        let x = i64::from(x) - i64::from(left);
        let y = i64::from(y) - i64::from(top);
        if !(0..i64::from(width)).contains(&x) || !(0..i64::from(height)).contains(&y) {
            return false;
        }
        let x = x * (src_right - src_left) / i64::from(width) + src_left;
        let y = y * (src_bottom - src_top) / i64::from(height) + src_top;
        if !(src_left.max(0)..src_right.min(i64::from(image.width))).contains(&x)
            || !(src_top.max(0)..src_bottom.min(i64::from(image.height))).contains(&y)
        {
            return false;
        }
        let offset = (y as usize * image.width as usize + x as usize) * 4;
        !alpha || image.pixels[offset + 3] != 0
    }

    pub fn draw_texture(&mut self, id: i32, target: i32) -> io::Result<i32> {
        let Some(texture) = self.textures.get(&id) else {
            return Ok(-2);
        };
        let image = self.texture_image(texture).clone();
        let Some(surface) = self.surfaces.get_mut(&target) else {
            return Ok(-1);
        };
        if !matches!(texture.blend, 0 | 2) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "unsupported Lucifen texture blending mode",
            ));
        }
        texture.draw(&image, Arc::make_mut(surface), [0, 0]);
        self.surface_changed(target);
        Ok(0)
    }

    pub fn select_group(&mut self, group: i32) -> i32 {
        std::mem::replace(&mut self.group, group)
    }

    pub fn release_group(&mut self, group: i32, keep_group: bool) {
        let released: Vec<_> = self
            .surfaces
            .iter()
            .filter_map(|(&id, surface)| {
                (id != self.base_display
                    && (surface.group == group) != keep_group
                    && !self.transition_buffers.values().any(|&buffer| buffer == id))
                .then_some(id)
            })
            .collect();
        for id in released {
            self.release_surface(id);
        }
        self.retain_targets();
    }

    pub fn clear(&mut self) {
        self.surfaces.retain(|&id, _| id == self.base_display);
        self.clear_textures();
        self.resident_bytes = self
            .surfaces
            .values()
            .map(|surface| surface.byte_len())
            .sum();
        self.display = self.base_display;
        self.transition_sources.clear();
        self.transition_buffers.clear();
        self.transitions.clear();
        self.backgrounds.clear();
        self.hidden_passes.clear();
        self.background_warps.clear();
        self.manual_refresh.clear();
        self.scene_offsets.clear();
        self.background_colors.clear();
        self.presented_scene = None;
        self.dirty = false;
    }

    pub fn free(&mut self, id: i32) {
        if id == self.base_display {
            return;
        }
        if self.release_surface(id) {
            self.retain_targets();
        }
    }

    fn release_surface(&mut self, id: i32) -> bool {
        let Some(surface) = self.surfaces.remove(&id) else {
            return false;
        };
        self.manual_refresh.remove(&id);
        self.resident_bytes -= surface.byte_len();
        for texture in self
            .textures
            .values_mut()
            .filter(|texture| texture.source == Some(id))
        {
            texture.source = None;
            texture.pixels = surface.clone();
        }
        for background in self
            .backgrounds
            .values_mut()
            .filter(|background| background.source == id)
        {
            background.image = surface.clone();
        }
        true
    }

    fn texture_image<'a>(&'a self, texture: &'a Texture) -> &'a Arc<Surface> {
        texture
            .source
            .and_then(|source| self.surfaces.get(&source))
            .unwrap_or(&texture.pixels)
    }

    fn surface_changed(&mut self, id: i32) {
        if self.manual_refresh.contains(&id) {
            return;
        }
        self.dirty |= id == self.display
            || self.layers.iter().any(|(handle, layer)| {
                layer.target == self.display
                    && self
                        .textures
                        .get(handle)
                        .is_some_and(|texture| texture.source == Some(id))
            })
            || self
                .backgrounds
                .get(&self.display)
                .is_some_and(|background| background.source == id)
            || !self.transitions.is_empty();
    }

    pub fn surface(&self, id: i32) -> io::Result<&Arc<Surface>> {
        self.surfaces
            .get(&id)
            .ok_or_else(|| invalid("unknown Lucifen surface"))
    }

    pub(crate) fn bitmap(&self, target: BitmapTarget) -> Option<&Arc<Surface>> {
        match target {
            BitmapTarget::Surface(id) => self.surfaces.get(&id),
            BitmapTarget::Texture(id) => self
                .textures
                .get(&id)
                .map(|texture| self.texture_image(texture)),
        }
    }

    pub(crate) fn text_window_skin(
        base: Option<Arc<Surface>>,
        mask: Option<&Surface>,
        color: [u8; 4],
    ) -> io::Result<Option<Arc<Surface>>> {
        let Some(mask) = mask else {
            return Ok(base);
        };
        let mut skin = Surface::empty(mask.width, mask.height)?;
        for (index, pixel) in skin.pixels.chunks_exact_mut(4).enumerate() {
            let alpha = mask
                .indices
                .as_ref()
                .map_or(mask.pixels[index * 4], |indices| indices[index]);
            pixel[..3].copy_from_slice(&color[..3]);
            pixel[3] = ((u16::from(alpha) * u16::from(color[3]) + 127) / 255) as u8;
        }
        if let Some(base) = base {
            skin.composite(&base, 0, 0, 255);
        }
        Ok(Some(Arc::new(skin)))
    }

    pub(crate) fn button_color_key(&mut self, id: i32, [x, y]: [i32; 2]) {
        let Some(surface) = self.surfaces.get_mut(&id) else {
            return;
        };
        let key: [u8; 3] = if y >= 0 && (x as u32) < surface.width && (y as u32) < surface.height {
            let offset = (y as usize * surface.width as usize + x as usize) * 4;
            surface.pixels[offset..offset + 3].try_into().unwrap()
        } else if y < 0 && x != -1 {
            let [blue, green, red, _] = x.to_le_bytes();
            [red, green, blue]
        } else {
            return;
        };
        for pixel in Arc::make_mut(surface).pixels.chunks_exact_mut(4) {
            if pixel[..3] == key {
                pixel[3] = 0;
            }
        }
        self.surface_changed(id);
    }

    pub(crate) fn flip_bitmap(
        &mut self,
        id: i32,
        region: Option<[i32; 4]>,
        horizontal: bool,
    ) -> i32 {
        let Some((source, bitmap)) = self.bitmap_mut(id) else {
            return -1;
        };
        let [left, top, right, bottom] = region.unwrap_or_else(|| bitmap.full_region());
        if left < 0
            || top < 0
            || right < left
            || bottom < top
            || right as u32 > bitmap.width
            || bottom as u32 > bitmap.height
        {
            return 2;
        }
        Arc::make_mut(bitmap).flip([left, top, right, bottom], horizontal);
        self.surface_changed(source);
        self.dirty |= self.layers.contains_key(&id);
        0
    }

    pub(crate) fn blur_bitmap(
        &mut self,
        id: i32,
        radius: [u32; 2],
        region: Option<[i32; 4]>,
        alpha: bool,
    ) -> i32 {
        let Some((source_id, bitmap)) = self.bitmap_mut(id) else {
            return -1;
        };
        let rect = if let Some(rect) = region {
            let Ok(rect) = bitmap.clamped_region(rect) else {
                return 3;
            };
            rect
        } else {
            bitmap.full_region()
        };
        if radius != [0, 0] {
            let original = bitmap.clone();
            box_blur(&original, Arc::make_mut(bitmap), rect, radius, alpha);
            self.surface_changed(source_id);
            self.dirty |= self.layers.contains_key(&id);
        }
        0
    }

    pub(crate) fn grayscale_bitmap(&mut self, id: i32, region: Option<[i32; 4]>) -> i32 {
        let Some((source, bitmap)) = self.bitmap_mut(id) else {
            return -1;
        };
        let rect = if let Some(rect) = region {
            let Ok(rect) = bitmap.clamped_region(rect) else {
                return 0;
            };
            rect
        } else {
            bitmap.full_region()
        };
        let [left, top, right, bottom] = rect.map(|coord| coord as usize);
        let pixels = Arc::make_mut(bitmap);
        let stride = pixels.width as usize;
        let mut indices = pixels.indices.as_mut().map(Arc::make_mut);
        for y in top..bottom {
            let start = y * stride + left;
            let end = y * stride + right;
            for (index, rgba) in pixels.pixels[4 * start..4 * end]
                .chunks_exact_mut(4)
                .enumerate()
            {
                let gray =
                    ((77 * u16::from(rgba[0]) + 150 * u16::from(rgba[1]) + 29 * u16::from(rgba[2]))
                        >> 8) as u8;
                rgba[..3].fill(gray);
                if let Some(indices) = indices.as_deref_mut() {
                    indices[start + index] = gray;
                }
            }
        }
        self.surface_changed(source);
        self.dirty |= self.layers.contains_key(&id);
        0
    }

    pub(crate) fn set_bitmap_brightness(
        &mut self,
        id: i32,
        level: u16,
        region: Option<[i32; 4]>,
    ) -> i32 {
        if level > 510 {
            return 0;
        }
        let white = level.saturating_sub(255);
        let gain = level.min(510 - level);
        i32::from(
            self.transform_colors(id, region, |pixel| {
                for channel in &mut pixel[..3] {
                    *channel = (white + (u16::from(*channel) * gain + 127) / 255) as u8;
                }
            })
            .unwrap_or(false),
        )
    }

    pub(crate) fn adjust_bitmap_colors(
        &mut self,
        id: i32,
        region: Option<[i32; 4]>,
        gain: [i32; 3],
        offset: [i32; 3],
    ) -> i32 {
        if gain.iter().any(|value| !(0..=256).contains(value))
            || offset.iter().any(|value| !(-255..=255).contains(value))
        {
            return 3;
        }
        self.transform_colors(id, region, |pixel| {
            for channel in 0..3 {
                let value = ((i32::from(pixel[channel]) * gain[channel]) >> 7) + offset[channel];
                pixel[channel] = value.clamp(0, 255) as u8;
            }
        })
        .map_or(-1, |_| 0)
    }

    pub(crate) fn invert_bitmap_colors(
        &mut self,
        id: i32,
        region: Option<[i32; 4]>,
        mode: u8,
    ) -> i32 {
        let range = match mode {
            0 => 0..3,
            1 => 0..4,
            _ => 3..4,
        };
        self.transform_colors(id, region, |pixel| {
            for value in &mut pixel[range.clone()] {
                *value = !*value;
            }
        })
        .map_or(-1, |_| 0)
    }

    fn transform_colors(
        &mut self,
        id: i32,
        region: Option<[i32; 4]>,
        edit: impl Fn(&mut [u8]),
    ) -> Option<bool> {
        let (source, bitmap) = self.bitmap_mut(id)?;
        let rect = if let Some(rect) = region {
            let Ok(rect) = bitmap.clamped_region(rect) else {
                return Some(false);
            };
            rect
        } else {
            bitmap.full_region()
        };
        let [left, top, right, bottom] = rect.map(|coord| coord as usize);
        let bitmap = Arc::make_mut(bitmap);
        let stride = bitmap.width as usize * 4;
        for row in bitmap
            .pixels
            .chunks_exact_mut(stride)
            .take(bottom)
            .skip(top)
        {
            for pixel in row[left * 4..right * 4].chunks_exact_mut(4) {
                edit(pixel);
            }
        }
        self.surface_changed(source);
        self.dirty |= self.layers.contains_key(&id);
        Some(true)
    }

    fn bitmap_mut(&mut self, id: i32) -> Option<(i32, &mut Arc<Surface>)> {
        let source = self
            .textures
            .get(&id)
            .and_then(|texture| texture.source)
            .unwrap_or(id);
        if let Some(surface) = self.surfaces.get_mut(&source) {
            Some((source, surface))
        } else {
            self.textures
                .get_mut(&id)
                .map(|texture| (id, &mut texture.pixels))
        }
    }

    pub(crate) fn repeat_bitmap_pixels(
        &mut self,
        id: i32,
        source: &Surface,
        offset: usize,
    ) -> bool {
        let Some(target) = self.surfaces.get_mut(&id) else {
            return false;
        };
        let pixels = &mut Arc::make_mut(target).pixels;
        let start = offset * 4;
        let tail = &source.pixels[start..];
        let initial = tail.len().min(pixels.len());
        pixels[..initial].copy_from_slice(&tail[..initial]);
        for chunk in pixels[initial..].chunks_mut(source.pixels.len()) {
            chunk.copy_from_slice(&source.pixels[..chunk.len()]);
        }
        self.surface_changed(id);
        true
    }

    pub(crate) fn stamp_bitmap(
        &mut self,
        target: BitmapTarget,
        bitmap: &Surface,
        region: [i64; 4],
        position: [i32; 2],
        blend: bool,
    ) -> bool {
        let output = match target {
            BitmapTarget::Surface(id) => id,
            BitmapTarget::Texture(id) => {
                let Some(texture) = self.textures.get_mut(&id) else {
                    return false;
                };
                if let Some(source) = texture.source {
                    source
                } else {
                    Arc::make_mut(&mut texture.pixels).composite_region(
                        bitmap,
                        region,
                        position[0],
                        position[1],
                        blend.then_some(255),
                    );
                    self.dirty |= self.layers.contains_key(&id);
                    return true;
                }
            }
        };
        let Some(surface) = self.surfaces.get_mut(&output) else {
            return false;
        };
        Arc::make_mut(surface).composite_region(
            bitmap,
            region,
            position[0],
            position[1],
            blend.then_some(255),
        );
        self.surface_changed(output);
        true
    }

    pub(crate) fn write_bgr_bitmap(
        &mut self,
        target: BitmapTarget,
        width: u32,
        height: u32,
        bgr: &[u8],
    ) -> io::Result<bool> {
        if self
            .bitmap(target)
            .is_none_or(|bitmap| width > bitmap.width || height > bitmap.height)
        {
            return Ok(false);
        }
        let mut image = Surface::empty(width, height)?;
        if bgr.len() != image.pixels.len() / 4 * 3 {
            return Err(invalid("invalid Lucifen bitmap byte count"));
        }
        for (source, target) in bgr.chunks_exact(3).zip(image.pixels.chunks_exact_mut(4)) {
            target.copy_from_slice(&[source[2], source[1], source[0], 255]);
        }
        Ok(self.stamp_bitmap(
            target,
            &image,
            image.full_region().map(i64::from),
            [0, 0],
            false,
        ))
    }

    pub fn composite(
        &mut self,
        destination: i32,
        source: i32,
        x: i32,
        y: i32,
        opacity: u8,
    ) -> io::Result<()> {
        if self.blend_bitmap(
            destination,
            source,
            [x, y],
            None,
            BitmapBlend::Alpha(opacity),
        ) == 0
        {
            Ok(())
        } else {
            Err(invalid("unknown Lucifen surface"))
        }
    }

    pub(crate) fn blend_bitmap(
        &mut self,
        destination: i32,
        source: i32,
        [x, y]: [i32; 2],
        region: Option<[i32; 4]>,
        blend: BitmapBlend,
    ) -> i32 {
        let source = self
            .bitmap(BitmapTarget::Surface(source))
            .or_else(|| self.bitmap(BitmapTarget::Texture(source)))
            .cloned();
        let Some(source) = source else {
            return -1;
        };
        let Some((bitmap, target)) = self.bitmap_mut(destination) else {
            return -1;
        };
        let region = region
            .unwrap_or_else(|| source.full_region())
            .map(i64::from);
        Arc::make_mut(target).blit(&source, region, x, y, blend);
        self.surface_changed(bitmap);
        self.dirty |= self.layers.contains_key(&destination);
        0
    }

    pub(crate) fn draw_glyphs(
        &mut self,
        target: i32,
        x: i32,
        y: i32,
        font: &crate::text::Font,
        layout: &roukaku_text::TextLayoutResult,
    ) -> io::Result<()> {
        use roukaku_media_core::GlyphBitmapFormat;
        let surface = self
            .surfaces
            .get_mut(&target)
            .ok_or_else(|| invalid("unknown Lucifen surface"))?;
        let surface = Arc::make_mut(surface);
        let glyphs: BTreeMap<_, _> = layout
            .glyph_resources
            .iter()
            .map(|resource| (resource.resource_id.as_str(), &resource.bitmap))
            .collect();
        for glyph in layout.shaped_runs.iter().flat_map(|run| &run.glyphs) {
            let (Some(id), Some(left), Some(top)) =
                (&glyph.resource_id, glyph.render_x, glyph.render_y)
            else {
                continue;
            };
            let bitmap = glyphs
                .get(id.as_str())
                .ok_or_else(|| invalid("missing shaped Lucifen glyph"))?;
            let base = [
                i64::from(x) + i64::from(left),
                i64::from(y) + i64::from(top),
            ];
            let mut layers = Vec::with_capacity(3);
            if font.effect.0 != 0 {
                layers.push((
                    i64::from(font.effect.1),
                    i64::from(font.effect.2),
                    font.shadow,
                    None,
                ));
            }
            if font.bold {
                layers.push((1, 0, font.foreground, font.gradient));
            }
            layers.push((0, 0, font.foreground, font.gradient));
            for (dx, dy, color, gradient) in layers {
                for row in 0..bitmap.height {
                    let y = base[1] + i64::from(row) + dy;
                    if !(0..i64::from(surface.height)).contains(&y) {
                        continue;
                    }
                    let shear = if font.italic {
                        i64::from((bitmap.height - row) / 4)
                    } else {
                        0
                    };
                    let color = gradient.map_or(color, |[first, last]| {
                        let top = bitmap.height - row;
                        std::array::from_fn(|channel| {
                            ((u32::from(first[channel]) * top + u32::from(last[channel]) * row)
                                / bitmap.height) as u8
                        })
                    });
                    for column in 0..bitmap.width {
                        let x = base[0] + i64::from(column) + dx + shear;
                        if !(0..i64::from(surface.width)).contains(&x) {
                            continue;
                        }
                        let offset = (row * bitmap.width + column) as usize;
                        let source = match bitmap.format {
                            GlyphBitmapFormat::Alpha8 => [
                                color[0],
                                color[1],
                                color[2],
                                bitmap.pixels.as_slice()[offset],
                            ],
                            GlyphBitmapFormat::Rgba8 => bitmap.pixels.as_slice()[offset * 4..][..4]
                                .try_into()
                                .unwrap(),
                        };
                        let offset = (y as usize * surface.width as usize + x as usize) * 4;
                        mix(&mut surface.pixels[offset..][..4], source, 255);
                    }
                }
            }
        }
        self.surface_changed(target);
        Ok(())
    }

    fn present(&mut self) -> io::Result<()> {
        let target = self.display;
        let mut frame = if let Some(effect) = self.transitions.get(&target) {
            let (source, mut frame) = match effect.mode {
                0 => (
                    effect.from.as_ref().unwrap().clone(),
                    self.surface(effect.peer)?.clone(),
                ),
                1 => (
                    self.render_scene(target, 0, 0, false)?,
                    self.render_scene(effect.peer, 0, 0, false)?,
                ),
                _ => (
                    self.render_background(target)?,
                    self.render_background(effect.peer)?,
                ),
            };
            effect.blend(&source, Arc::make_mut(&mut frame));
            let mode = effect.mode;
            if matches!(
                effect.transform,
                Some(
                    TransitionTransform::RotateCenter { style: 2, .. }
                        | TransitionTransform::ZoomFlip { mode: 5, .. }
                )
            ) {
                let effect = self.transitions.get_mut(&target).unwrap();
                effect.feedback = Some((effect.transform_progress, frame.clone()));
            }
            if mode == 2 {
                self.draw_layers(&mut frame, target, 0, 0, false)?;
            }
            frame
        } else {
            self.render_scene(target, 0, 0, false)?
        };
        self.presented_scene = Some((target, frame.clone()));
        self.draw_layers(&mut frame, target, 0, 0, true)?;
        if self.transitions.contains_key(&target)
            && let Some(buffer) = self.transition_buffers.get(&target)
            && let Some(surface) = self.surfaces.get_mut(buffer)
        {
            Arc::make_mut(surface)
                .pixels
                .copy_from_slice(frame.pixels());
        }
        self.presented = Some(frame);
        Ok(())
    }

    pub(crate) fn begin_transition(
        &mut self,
        target: i32,
        effect: SceneEffect,
        now: u32,
    ) -> io::Result<i32> {
        if effect.duration == 0 {
            return Ok(-2);
        }
        let Some(base) = self.surfaces.get(&target) else {
            return Ok(-1);
        };
        let Some(&peer) = self.transition_sources.get(&target) else {
            return Ok(2);
        };
        if effect.mode == 2
            && !(self.backgrounds.contains_key(&target) && self.backgrounds.contains_key(&peer))
        {
            return Ok(4);
        }
        let size = (base.width, base.height);
        let mask = if let Some((id, softness)) = effect.rule {
            let Some(surface) = self.surfaces.get(&id) else {
                return Ok(-2);
            };
            let Some(indices) = &surface.indices else {
                return Ok(-2);
            };
            if (surface.width, surface.height) != (base.width, base.height) {
                return Ok(-2);
            }
            Some((indices.clone(), softness.clamp(0, 255)))
        } else {
            None
        };
        let from = if effect.mode == 0 {
            Some(match &self.presented_scene {
                Some((id, frame)) if *id == target => frame.clone(),
                _ => self.surface(target)?.clone(),
            })
        } else {
            None
        };
        self.finish_transition(target);
        if !self.transition_buffers.contains_key(&target) {
            let buffer = self.create(size.0, size.1)?;
            self.transition_buffers.insert(target, buffer);
        }
        self.transitions.insert(
            target,
            Transition {
                start: now,
                duration: effect.duration,
                progress: 0,
                peer,
                mode: effect.mode,
                commit: effect.commit,
                from,
                mask,
                quake: effect.quake,
                displacement: [0, 0],
                reverse: false,
                transform: effect.transform,
                transform_progress: 0,
                feedback: None,
            },
        );
        self.dirty = true;
        Ok(0)
    }

    pub(crate) fn transition_active(&self, target: i32) -> bool {
        self.transitions.contains_key(&target)
    }

    pub(crate) fn transition_buffer(&self, target: i32) -> i32 {
        self.transition_buffers.get(&target).copied().unwrap_or(0)
    }

    pub(crate) fn finish_transition(&mut self, target: i32) -> bool {
        let Some(effect) = self.transitions.remove(&target) else {
            return false;
        };
        if effect.commit && effect.mode != 0 {
            self.exchange_scene(target, effect.peer, effect.mode == 1);
        }
        self.dirty = true;
        true
    }

    pub(crate) fn exchange_transition_scene(&mut self, target: i32, mode: u8) -> bool {
        if !self.surfaces.contains_key(&target) {
            return false;
        }
        if let Some(&peer) = self.transition_sources.get(&target) {
            if mode != 1 {
                self.exchange_scene(target, peer, true);
            }
            if matches!(mode, 1 | 2) {
                self.exchange_scene(target, peer, false);
            }
        }
        true
    }

    fn exchange_scene(&mut self, target: i32, peer: i32, layers: bool) {
        swap_scene_entries(&mut self.backgrounds, target, peer);
        swap_scene_entries(&mut self.background_colors, target, peer);
        if layers {
            for layer in self.layers.values_mut().filter(|layer| !layer.overlay) {
                if layer.target == target {
                    layer.target = peer;
                } else if layer.target == peer {
                    layer.target = target;
                }
            }
        }
        self.dirty = true;
    }

    pub(crate) fn update_transitions(&mut self, now: u32, random: &mut crate::random::Random) {
        let mut finished = Vec::new();
        for (&target, transition) in &mut self.transitions {
            let elapsed = now.wrapping_sub(transition.start);
            let progress =
                (u64::from(elapsed) * 255 / u64::from(transition.duration)).min(255) as u8;
            self.dirty |= progress != transition.progress;
            transition.progress = progress;
            if let Some(transform) = transition.transform {
                let end = match transform {
                    TransitionTransform::VerticalSplit { .. } => 256,
                    TransitionTransform::RotateAxis { .. } => 256,
                    TransitionTransform::RotateCenter { .. } => 512,
                    TransitionTransform::AddSubtract { .. } => 255,
                    TransitionTransform::Wave { .. } => u64::from(transition.duration),
                    TransitionTransform::Mosaic { size, .. } => 2 * u64::from(size),
                    TransitionTransform::ZoomFlip { mode, .. } => match mode {
                        0 | 2 => u64::from(self.surfaces[&target].width),
                        1 | 3 => u64::from(self.surfaces[&target].height),
                        6 => 256,
                        _ => 512,
                    },
                    TransitionTransform::HorizontalBlur {
                        cross: false,
                        radius,
                    } => 2 * u64::from(radius) + 1,
                    TransitionTransform::HorizontalBlur { cross: true, .. } => 255,
                    TransitionTransform::Scroll { direction, .. } => {
                        let canvas = &self.surfaces[&target];
                        u64::from(if matches!(direction, 0 | 2) {
                            canvas.width
                        } else {
                            canvas.height
                        })
                    }
                };
                let step =
                    (u64::from(elapsed) * end / u64::from(transition.duration)).min(end) as u32;
                self.dirty |= transition.transform_progress != step;
                transition.transform_progress = step;
            }
            if let Some(quake) = &transition.quake {
                let fraction = if transition.duration == u32::MAX {
                    0.0
                } else {
                    f64::from(elapsed.min(transition.duration)) / f64::from(transition.duration)
                };
                let gain = 1.0 - (1.0 - quake.end_gain) * fraction;
                let offset = crate::effects::random_quake_offset(
                    quake.amplitude,
                    gain,
                    transition.reverse,
                    random,
                );
                transition.reverse = !transition.reverse;
                self.dirty |= transition.displacement != offset;
                transition.displacement = offset;
            }
            if elapsed >= transition.duration {
                finished.push(target);
            }
        }
        for target in finished {
            self.finish_transition(target);
        }
    }

    pub(crate) fn capture_scene(
        &mut self,
        target: i32,
        destination: i32,
        filter: u8,
        group: i32,
        overlays: bool,
    ) -> io::Result<i32> {
        let Some(destination_surface) = self.surfaces.get(&destination) else {
            return Ok(if destination == 0 { -2 } else { -1 });
        };
        let Some(source_surface) = self.surfaces.get(&target) else {
            return Ok(-2);
        };
        if (source_surface.width, source_surface.height)
            != (destination_surface.width, destination_surface.height)
        {
            return Ok(2);
        }
        let frame = self.render_scene(target, filter, group, overlays)?;
        let destination_surface = self.surfaces.get_mut(&destination).unwrap();
        Arc::make_mut(destination_surface)
            .pixels
            .copy_from_slice(frame.pixels());
        self.surface_changed(destination);
        Ok(0)
    }

    fn render_scene(
        &self,
        target: i32,
        filter: u8,
        group: i32,
        overlays: bool,
    ) -> io::Result<Arc<Surface>> {
        let mut frame = self.render_background(target)?;
        self.draw_layers(&mut frame, target, filter, group, false)?;
        if overlays {
            self.draw_layers(&mut frame, target, filter, group, true)?;
        }
        Ok(frame)
    }

    fn render_background(&self, target: i32) -> io::Result<Arc<Surface>> {
        let mut frame = self.surface(target)?.clone();
        if self
            .hidden_passes
            .get(&target)
            .is_some_and(|hidden| hidden.background)
        {
            return Ok(frame);
        }
        if let Some(background) = self.backgrounds.get(&target) {
            if let Some(transformed) = &background.transformed {
                frame = transformed.clone();
            } else {
                let image = self
                    .surfaces
                    .get(&background.source)
                    .unwrap_or(&background.image);
                let [x, y] = background.position;
                let destination = Arc::make_mut(&mut frame);
                if background.rotation != BackgroundRotation::default()
                    && background.rotation.scale != 0.0
                {
                    let color = self
                        .background_colors
                        .get(&target)
                        .copied()
                        .unwrap_or([0, 0, 0, 255]);
                    for pixel in destination.pixels.as_chunks_mut::<4>().0 {
                        *pixel = color;
                    }
                    draw_background_transform(
                        image,
                        destination,
                        BackgroundTransform {
                            center: background.pivot,
                            offset: background.rotation.offset,
                            scale: background.rotation.scale.abs(),
                            turns: f64::from(background.rotation.turns)
                                + if background.rotation.scale < 0.0 {
                                    256.0
                                } else {
                                    0.0
                                },
                            opacity: 255,
                            sampling: 0,
                            alpha: false,
                            in_place: false,
                        },
                    );
                } else if let Some(stretch) = background.stretch {
                    let [left, top, width, height] = stretch.destination;
                    let color = self
                        .background_colors
                        .get(&target)
                        .copied()
                        .unwrap_or([0, 0, 0, 255]);
                    for pixel in destination.pixels.as_chunks_mut::<4>().0 {
                        *pixel = color;
                    }
                    draw_stretched_texture(
                        image,
                        destination,
                        stretch.source.map(i64::from),
                        [left.saturating_sub(x), top.saturating_sub(y)],
                        [width, height],
                        255,
                        true,
                    );
                } else if let Some([width, height]) = background.window {
                    scale_background(image, destination, [x, y, width, height]);
                } else {
                    destination.composite(image, x.saturating_neg(), y.saturating_neg(), 255);
                }
            }
        }
        if let Some(warp) = self.background_warps.get(&target) {
            warp.apply(
                &mut frame,
                self.background_colors
                    .get(&target)
                    .copied()
                    .unwrap_or([0, 0, 0, 255]),
            );
        }
        if let Some(offset) = self.scene_offsets.get(&target)
            && offset.primary != [0, 0]
        {
            let mut moved = self.surface(target)?.clone();
            let destination = Arc::make_mut(&mut moved);
            if offset.fill_edges {
                let edge = self
                    .background_colors
                    .get(&target)
                    .copied()
                    .unwrap_or([0, 0, 0, 255]);
                for pixel in destination.pixels.chunks_exact_mut(4) {
                    pixel.copy_from_slice(&edge);
                }
            }
            let [x, y] = offset.primary;
            destination.composite_region(&frame, frame.full_region().map(i64::from), x, y, None);
            frame = moved;
        }
        Ok(frame)
    }

    fn draw_layers(
        &self,
        frame: &mut Arc<Surface>,
        target: i32,
        filter: u8,
        group: i32,
        overlay: bool,
    ) -> io::Result<()> {
        if self.hidden_passes.get(&target).is_some_and(|hidden| {
            if overlay {
                hidden.overlay
            } else {
                hidden.primary
            }
        }) {
            return Ok(());
        }
        let mut visible: Vec<_> = self
            .layers
            .iter()
            .filter(|(_, layer)| layer.target == target && layer.overlay == overlay)
            .collect();
        // Lucifen keeps each sprite list in ascending priority order and draws from its tail.
        visible.sort_unstable_by_key(|(_, layer)| (Reverse(layer.priority), Reverse(layer.order)));
        let offset = self.scene_offsets.get(&target).map_or([0, 0], |offset| {
            if overlay {
                offset.overlay
            } else {
                offset.primary
            }
        });
        for (id, _) in visible {
            let Some(texture) = self.textures.get(id).filter(|texture| {
                texture.visible
                    && match filter {
                        0 => true,
                        1 => texture.group == group,
                        2 => texture.group != group,
                        _ => false,
                    }
            }) else {
                continue;
            };
            if !matches!(texture.blend, 0 | 2) {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "unsupported Lucifen scene blending mode",
                ));
            }
            texture.draw(self.texture_image(texture), Arc::make_mut(frame), offset);
        }
        Ok(())
    }

    pub fn presented(&self) -> Option<&Arc<Surface>> {
        self.presented.as_ref()
    }
}

fn swap_scene_entries<T>(entries: &mut BTreeMap<i32, T>, first: i32, second: i32) {
    let a = entries.remove(&first);
    let b = entries.remove(&second);
    if let Some(a) = a {
        entries.insert(second, a);
    }
    if let Some(b) = b {
        entries.insert(first, b);
    }
}

fn draw_background_transform(
    source: &Surface,
    target: &mut Surface,
    transform: BackgroundTransform,
) {
    if transform.scale <= 0.0 || transform.opacity == 0 {
        return;
    }
    let angle = -transform.turns.trunc().rem_euclid(512.0) * std::f64::consts::TAU / 512.0;
    let (sin, cos) = angle.sin_cos();
    let [mut dx, mut dy] = transform.center.map(f64::from);
    let scale = transform.scale;
    let mut scale_y = scale;
    let mut scale_x = scale;
    if sin == 0.0 && cos == 1.0 {
        dx = (dx - dx * scale + 0.5).trunc();
        dy = (dy - dy * scale + 0.5).trunc();
        scale_x = (f64::from(source.width) * scale).round() / f64::from(source.width);
        scale_y = (f64::from(source.height) * scale).round() / f64::from(source.height);
        if scale_x == 0.0 || scale_y == 0.0 {
            return;
        }
    } else {
        [dx, dy] = [
            dx - (dx * cos - dy * sin) * scale,
            dy - (dx * sin + dy * cos) * scale,
        ];
    }
    dx += f64::from(transform.offset[0]);
    dy += f64::from(transform.offset[1]);
    let [xx, xy, yx, yy] = [cos / scale_x, sin / scale_x, -sin / scale_y, cos / scale_y];
    let width = f64::from(source.width);
    let height = f64::from(source.height);
    for (y, row) in target
        .pixels
        .chunks_exact_mut(target.width as usize * 4)
        .enumerate()
    {
        let py = y as f64 + 0.5 - dy;
        for (x, target) in row.chunks_exact_mut(4).enumerate() {
            let px = x as f64 + 0.5 - dx;
            let sx = px * xx + py * xy;
            let sy = px * yx + py * yy;
            if !(0.0..width).contains(&sx) || !(0.0..height).contains(&sy) {
                continue;
            }
            let pixel = |x: usize, y: usize| {
                let start = (y * source.width as usize + x) * 4;
                <[u8; 4]>::try_from(&source.pixels[start..start + 4]).unwrap()
            };
            let mut color = if transform.sampling & 15 == 0 {
                pixel(sx as usize, sy as usize)
            } else {
                let [x, y] = [(sx - 0.5).max(0.0), (sy - 0.5).max(0.0)];
                let [left, top] = [x as usize, y as usize];
                let right = (left + 1).min(source.width as usize - 1);
                let bottom = (top + 1).min(source.height as usize - 1);
                let [tl, tr, bl, br] = [
                    pixel(left, top),
                    pixel(right, top),
                    pixel(left, bottom),
                    pixel(right, bottom),
                ];
                let wx = (x.fract() * 256.0) as u32;
                let wy = (y.fract() * 256.0) as u32;
                std::array::from_fn(|i| {
                    let upper = u32::from(tl[i]) * (256 - wx) + u32::from(tr[i]) * wx;
                    let lower = u32::from(bl[i]) * (256 - wx) + u32::from(br[i]) * wx;
                    ((upper * (256 - wy) + lower * wy + 32768) >> 16) as u8
                })
            };
            if !transform.alpha {
                color[3] = 255;
            }
            mix(target, color, transform.opacity);
        }
    }
}

fn draw_axis_rotation(source: &Surface, target: &mut Surface, direction: u8, phase: u32) {
    let (sin, cos) = (f64::from(phase) * std::f64::consts::TAU / 512.0).sin_cos();
    let axis = usize::from(direction > 1);
    let depth_slope = if matches!(direction, 1 | 2) {
        sin
    } else {
        -sin
    };
    let target_size = [f64::from(target.width), f64::from(target.height)];
    let src_width = source.width as usize;
    let src_height = source.height as usize;
    for (row, destination) in target
        .pixels
        .chunks_exact_mut(target.width as usize * 4)
        .enumerate()
    {
        let y = 1.0 - (2.0 * row as f64 + 1.0) / target_size[1];
        for (col, target) in destination.chunks_exact_mut(4).enumerate() {
            let x = (2.0 * col as f64 + 1.0) / target_size[0] - 1.0;
            let mut point = [x, y];
            let projected = point[axis];
            let denominator = cos * 2.0 - depth_slope * projected;
            if cos.abs() > 1e-9 && denominator.abs() > 1e-9 {
                point[axis] = projected * 2.0 / denominator;
                point[1 - axis] *= (2.0 + depth_slope * point[axis]) * 0.5;
                let u = (point[0] + 1.0) * 0.5;
                let v = (1.0 - point[1]) * 0.5;
                if (0.0..1.0).contains(&u) && (0.0..1.0).contains(&v) {
                    let row = ((v * src_height as f64) as usize).min(src_height - 1);
                    let col = ((u * src_width as f64) as usize).min(src_width - 1);
                    let offset = (row * src_width + col) * 4;
                    target.copy_from_slice(&source.pixels[offset..offset + 4]);
                    continue;
                }
            }
            target.copy_from_slice(&[0, 0, 0, 255]);
        }
    }
}

fn sine_index(step: i64) -> f64 {
    ((step & 511) as f64 * std::f64::consts::TAU / 512.0).sin()
}

fn mosaic(source: &Surface, size: u32, alignment: u8, fit_aspect: bool) -> Vec<u8> {
    let [width, height] = [source.width, source.height];
    let [mut block_width, mut block_height] = [size; 2];
    if fit_aspect {
        let scale = |extent, longest| {
            (u64::from(size) * u64::from(extent) / u64::from(longest)).max(2) as u32
        };
        if width < height {
            block_width = scale(width, height);
        } else {
            block_height = scale(height, width);
        }
    }
    let first = |extent: u32, block, end_aligned| {
        let partial = match alignment {
            0 => (extent.saturating_sub(block) / 2) % block,
            _ if end_aligned => extent % block,
            _ => 0,
        };
        if partial == 0 { block } else { partial }
    };
    let first_x = first(width, block_width, matches!(alignment, 2 | 4));
    let first_y = first(height, block_height, matches!(alignment, 3 | 4));
    let block_start = |coordinate, first, step| {
        if coordinate < first {
            0
        } else {
            first + (coordinate - first) / step * step
        }
    };
    let stride = width as usize * 4;
    let mut result = vec![0; source.pixels.len()];
    for (y, row) in result.chunks_exact_mut(stride).enumerate() {
        let row_offset = block_start(y as u32, first_y, block_height) as usize * stride;
        for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let offset = row_offset + block_start(x as u32, first_x, block_width) as usize * 4;
            pixel.copy_from_slice(&source.pixels[offset..offset + 4]);
        }
    }
    result
}

fn scale_background(source: &Surface, target: &mut Surface, window: [i32; 4]) {
    let [x, y, width, height] = window;
    let columns = scale_samples(x, width, source.width, target.width);
    let rows = scale_samples(y, height, source.height, target.height);
    for (vertical, row) in rows
        .into_iter()
        .zip(target.pixels.chunks_exact_mut(target.width as usize * 4))
    {
        for (&horizontal, target) in columns.iter().zip(row.chunks_exact_mut(4)) {
            let pixel = interpolate_pixel(source, horizontal, vertical);
            if pixel[3] == 255 {
                target.copy_from_slice(&pixel);
            } else {
                mix(target, pixel, 255);
            }
        }
    }
}

fn interpolate_pixel(
    source: &Surface,
    (left, right, wx): (usize, usize, u32),
    (top, bottom, wy): (usize, usize, u32),
) -> [u8; 4] {
    let stride = source.width as usize * 4;
    let start = top * stride + left * 4;
    if wx == 0 && wy == 0 {
        return source.pixels[start..start + 4].try_into().unwrap();
    }
    let offsets = [
        start,
        top * stride + right * 4,
        bottom * stride + left * 4,
        bottom * stride + right * 4,
    ];
    std::array::from_fn(|channel| {
        let [tl, tr, bl, br] = offsets.map(|offset| u32::from(source.pixels[offset + channel]));
        let upper = tl * (256 - wx) + tr * wx;
        let lower = bl * (256 - wx) + br * wx;
        ((upper * (256 - wy) + lower * wy + 32768) >> 16) as u8
    })
}

fn scale_samples(start: i32, length: i32, extent: u32, output: u32) -> Vec<(usize, usize, u32)> {
    let start = start.clamp(0, extent as i32 - 1) as u32;
    let length = length.clamp(1, (extent - start) as i32) as u32;
    (0..output)
        .map(|index| {
            let point = (((2 * u64::from(index) + 1) * u64::from(length) << 7) / u64::from(output))
                .saturating_sub(128)
                .min(u64::from(length - 1) << 8);
            let first = (point >> 8) as u32;
            let weight = (point & 255) as u32;
            (
                (start + first) as usize,
                (start + (first + 1).min(length - 1)) as usize,
                weight,
            )
        })
        .collect()
}

fn draw_stretched_texture(
    source: &Surface,
    target: &mut Surface,
    region: [i64; 4],
    position: [i32; 2],
    size: [i32; 2],
    opacity: u8,
    filtered: bool,
) {
    if size[0] <= 0 || size[1] <= 0 || opacity == 0 {
        return;
    }
    let target_extent = [i64::from(target.width), i64::from(target.height)];
    let source_extent = [i64::from(source.width), i64::from(source.height)];
    let position = position.map(i64::from);
    let size = size.map(i64::from);
    let end = std::array::from_fn::<_, 2, _>(|axis| {
        (position[axis] + size[axis]).min(target_extent[axis])
    });
    let starts = position.map(|coordinate| coordinate.max(0));
    let sample = |index: i64, axis: usize| {
        let start = region[axis];
        let length = region[axis + 2] - start;
        if length <= 0 {
            return None;
        }
        let index = index - position[axis];
        let extent = source_extent[axis];
        let point = i128::from(2 * index + 1) * i128::from(length) * 128 / i128::from(size[axis]);
        if !filtered {
            let coordinate = start + (point >> 8) as i64;
            return (0..extent).contains(&coordinate).then_some((
                coordinate as usize,
                coordinate as usize,
                0,
            ));
        }
        let point = (point - 128).clamp(0, i128::from(length - 1) * 256) as i64;
        let first = start + (point >> 8);
        let last = ((first + 1).min(region[axis + 2] - 1)).min(extent - 1);
        (0..extent).contains(&first).then_some((
            first as usize,
            last as usize,
            (point & 255) as u32,
        ))
    };
    let columns: Vec<_> = (starts[0]..end[0]).map(|x| sample(x, 0)).collect();
    for y in starts[1]..end[1] {
        let Some(vertical) = sample(y, 1) else {
            continue;
        };
        let start = (y * target_extent[0] + starts[0]) as usize * 4;
        for (x, column) in columns.iter().enumerate() {
            let Some(horizontal) = *column else {
                continue;
            };
            let rgba = interpolate_pixel(source, horizontal, vertical);
            let offset = start + x * 4;
            mix(&mut target.pixels[offset..offset + 4], rgba, opacity);
        }
    }
}

fn blur_rows(source: &Surface, target: &mut Surface, radius: u16, opacity: Option<u8>) {
    let radius = usize::from(radius);
    let width = source.width as usize;
    let factor = 65536 / (2 * radius as u32 + 1);
    for (source, target) in source
        .pixels
        .chunks_exact(width * 4)
        .zip(target.pixels.chunks_exact_mut(width * 4))
    {
        let source = source.as_chunks::<4>().0;
        let mut sum = source[0].map(|value| u32::from(value) * (radius as u32 + 1));
        for x in 1..=radius {
            for (sum, value) in sum.iter_mut().zip(source[x.min(width - 1)]) {
                *sum += u32::from(value);
            }
        }
        for (x, target) in target.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let blurred = sum.map(|value| ((value * factor + 32768) >> 16) as u8);
            if let Some(opacity) = opacity {
                let weight = i32::from(opacity) + i32::from(opacity != 0);
                for (target, value) in target[..3].iter_mut().zip(blurred) {
                    let from = i32::from(*target);
                    *target = (from + ((i32::from(value) - from) * weight >> 8)) as u8;
                }
            } else {
                *target = blurred;
            }
            let leaving = source[x.saturating_sub(radius)];
            let entering = source[(x + radius + 1).min(width - 1)];
            for ((sum, leaving), entering) in sum.iter_mut().zip(leaving).zip(entering) {
                *sum = *sum - u32::from(leaving) + u32::from(entering);
            }
        }
    }
}

fn blend_rule_pixel(out: &mut [u8], a: &[u8], b: &[u8], mut weight: i32, mode: u8) {
    let channels = if mode == 1 {
        let alpha_a = i32::from(a[3]) * (256 - weight) >> 8;
        let alpha_b = i32::from(b[3]) * weight >> 8;
        let alpha = 255 - (255 - alpha_a) * (255 - alpha_b) / 255;
        weight = if alpha == 0 { 0 } else { alpha_b * 255 / alpha };
        out[3] = alpha as u8;
        3
    } else {
        4
    };
    for index in 0..channels {
        let from = i32::from(a[index]);
        out[index] = (from + ((i32::from(b[index]) - from) * weight >> 8)) as u8;
    }
}

fn box_blur(source: &Surface, target: &mut Surface, rect: [i32; 4], radius: [u32; 2], alpha: bool) {
    let size = [source.width as usize, source.height as usize];
    let stride = [1, size[0]];
    // Keep only a rolling strip of sums along the shorter dimension.
    let inner = usize::from(size[0] > size[1]);
    let outer = 1 - inner;
    let radius = radius.map(|value| value as usize);
    let rect = rect.map(|value| value as usize);
    let index = |row, column| (row * stride[outer] + column * stride[inner]) * 4;
    let mut columns = vec![[0_u64; 4]; size[inner]];
    let start = rect[outer].saturating_sub(radius[outer]);
    let mut previous_rows = [start, start];
    for row in rect[outer]..rect[outer + 2] {
        let rows = blur_window(row, radius[outer], size[outer]);
        for (start, end, add) in [
            (previous_rows[0], rows[0], false),
            (previous_rows[1], rows[1], true),
        ] {
            for sampled_row in start..end {
                for (column, total) in columns.iter_mut().enumerate() {
                    let pixel = &source.pixels[index(sampled_row, column)..][..4];
                    let weight = if alpha { u64::from(pixel[3]) } else { 1 };
                    let values = [
                        u64::from(pixel[0]) * weight,
                        u64::from(pixel[1]) * weight,
                        u64::from(pixel[2]) * weight,
                        u64::from(pixel[3]),
                    ];
                    accumulate(total, values, add);
                }
            }
        }
        previous_rows = rows;

        let mut total = [0_u64; 4];
        let start = rect[inner].saturating_sub(radius[inner]);
        let mut previous_columns = [start, start];
        for column in rect[inner]..rect[inner + 2] {
            let window = blur_window(column, radius[inner], size[inner]);
            for &sum in &columns[previous_columns[0]..window[0]] {
                accumulate(&mut total, sum, false);
            }
            for &sum in &columns[previous_columns[1]..window[1]] {
                accumulate(&mut total, sum, true);
            }
            previous_columns = window;
            let count = ((rows[1] - rows[0]) * (window[1] - window[0])) as u64;
            let color_count = if alpha { total[3].max(1) } else { count };
            let pixel = &mut target.pixels[index(row, column)..][..4];
            for channel in 0..3 {
                pixel[channel] = ((total[channel] + color_count / 2) / color_count) as u8;
            }
            pixel[3] = ((total[3] + count / 2) / count) as u8;
        }
    }
}

fn blur_window(center: usize, radius: usize, extent: usize) -> [usize; 2] {
    [
        center.saturating_sub(radius),
        center.saturating_add(radius).saturating_add(1).min(extent),
    ]
}

fn accumulate(total: &mut [u64; 4], values: [u64; 4], add: bool) {
    for (channel, value) in total.iter_mut().zip(values) {
        if add {
            *channel += value;
        } else {
            *channel -= value;
        }
    }
}

fn mix(target: &mut [u8], source: [u8; 4], opacity: u8) {
    let alpha = (u32::from(source[3]) * u32::from(opacity) + 127) / 255;
    if alpha == 255 {
        target.copy_from_slice(&source);
    } else if alpha != 0 {
        let destination_alpha = u32::from(target[3]) * (255 - alpha);
        let total = alpha * 255 + destination_alpha;
        for channel in 0..3 {
            let mixed = u32::from(source[channel]) * alpha * 255
                + u32::from(target[channel]) * destination_alpha;
            target[channel] = ((mixed + total / 2) / total) as u8;
        }
        target[3] = ((total + 127) / 255) as u8;
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_refresh_holds_the_presented_frame_until_updates_resume() {
        let mut graphics = Graphics::open(1, 1).unwrap();
        let display = graphics.display();
        let prior = [240, 20, 80, 255];
        let next = [20, 80, 240, 255];
        graphics.fill(display, None, prior);
        graphics.present_display().unwrap();

        assert_eq!(graphics.set_manual_refresh(display, true), 0);
        graphics.fill(display, None, next);
        graphics.present_display().unwrap();
        assert_eq!(graphics.presented().unwrap().pixels(), prior);

        assert_eq!(graphics.set_manual_refresh(display, false), 1);
        graphics.present_display().unwrap();
        assert_eq!(graphics.presented().unwrap().pixels(), next);
    }

    #[test]
    fn alpha_aware_box_blur_does_not_mix_transparent_rgb_into_edges() {
        let mut graphics = Graphics::open(3, 1).unwrap();
        let display = graphics.display();
        graphics.fill(display, None, [0, 0, 255, 0]);
        graphics.fill(display, Some([1, 0, 2, 1]), [255, 0, 0, 255]);
        assert_eq!(graphics.blur_bitmap(display, [1, 0], None, true), 0);
        assert_eq!(
            graphics.surface(display).unwrap().pixels(),
            [255, 0, 0, 128, 255, 0, 0, 85, 255, 0, 0, 128]
        );
    }

    #[test]
    fn brightness_scales_toward_black_or_white_without_changing_alpha() {
        let mut graphics = Graphics::open(2, 1).unwrap();
        let display = graphics.display();
        graphics.fill(display, None, [100, 200, 0, 84]);
        assert_eq!(
            graphics.set_bitmap_brightness(display, 128, Some([0, 0, 1, 1])),
            1
        );
        assert_eq!(
            graphics.set_bitmap_brightness(display, 383, Some([1, 0, 2, 1])),
            1
        );
        assert_eq!(
            graphics.surface(display).unwrap().pixels(),
            [50, 100, 0, 84, 178, 228, 128, 84]
        );
    }

    #[test]
    fn horizontal_transition_blur_repeats_edges_and_preserves_crossfade_alpha() {
        let mut source = Surface::empty(3, 1).unwrap();
        source
            .pixels
            .copy_from_slice(&[255, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
        let mut result = Surface::empty(3, 1).unwrap();
        blur_rows(&source, &mut result, 1, None);
        assert_eq!(
            result.pixels(),
            [170, 0, 0, 255, 85, 0, 0, 255, 0, 0, 0, 255]
        );
        let mut cross = Surface::empty(3, 1).unwrap();
        blur_rows(&source, &mut cross, 1, Some(127));
        assert_eq!(cross.pixels(), [85, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn rule_blend_clips_shared_origins_and_reads_an_aliased_input() {
        let first_color = [255, 0, 0, 255];
        let second_color = [0, 0, 255, 255];
        let mut graphics = Graphics::open(3, 1).unwrap();
        let first = graphics.display();
        let second = graphics.create(3, 1).unwrap();
        let mask = graphics.create(3, 1).unwrap();
        Arc::make_mut(graphics.surfaces.get_mut(&mask).unwrap()).indices =
            Some(Arc::from([0, 255, 128]));
        graphics.fill(first, None, first_color);
        graphics.fill(second, None, second_color);
        let settings = BitmapRuleBlend {
            progress: 128,
            softness: 0,
            mode: 0,
            size: [3, 1],
            positions: [[-1, 0], [0, 0], [0, 0], [-1, 0]],
        };
        assert_eq!(
            graphics.blend_bitmap_rule([first, first, second, mask], settings),
            0
        );
        assert_eq!(
            graphics.surface(first).unwrap().pixels(),
            [second_color, first_color, first_color].concat()
        );
    }

    #[test]
    fn scroll_pushes_the_two_scenes_without_an_overlap_or_gap() {
        let old = [
            [10, 0, 0, 255],
            [20, 0, 0, 255],
            [30, 0, 0, 255],
            [40, 0, 0, 255],
        ];
        let new = [
            [0, 0, 10, 255],
            [0, 0, 20, 255],
            [0, 0, 30, 255],
            [0, 0, 40, 255],
        ];
        let mut graphics = Graphics::open(4, 1).unwrap();
        let display = graphics.display();
        let peer = graphics.create(4, 1).unwrap();
        for (id, colors) in [(display, old), (peer, new)] {
            for (x, color) in colors.into_iter().enumerate() {
                graphics.fill(id, Some([x as i32, 0, x as i32 + 1, 1]), color);
            }
        }
        graphics.present_display().unwrap();
        graphics.set_transition_source(display, peer);
        assert_eq!(
            graphics
                .begin_transition(
                    display,
                    SceneEffect {
                        duration: 100,
                        rule: None,
                        mode: 1,
                        commit: false,
                        quake: None,
                        transform: Some(TransitionTransform::Scroll {
                            direction: 0,
                            style: 2
                        }),
                    },
                    0
                )
                .unwrap(),
            0
        );
        graphics.update_transitions(50, &mut crate::random::Random::seeded(1));
        graphics.present_display().unwrap();
        let crop = [old[2], old[3], new[0], new[1]].concat();
        assert_eq!(graphics.presented().unwrap().pixels(), crop);
    }

    #[test]
    fn stretched_background_clips_to_the_canvas_and_fills_uncovered_pixels() {
        let black = [0, 0, 0, 255];
        let red = [255, 0, 0, 255];
        let blue = [0, 0, 255, 255];
        let green = [0, 255, 0, 255];
        let mut graphics = Graphics::open(3, 1).unwrap();
        let display = graphics.display();
        let image = graphics.create(3, 1).unwrap();
        for (x, color) in [red, blue, green].into_iter().enumerate() {
            graphics.fill(image, Some([x as i32, 0, x as i32 + 1, 1]), color);
        }
        graphics
            .set_background_image(display, image, [0, 0], false)
            .unwrap();
        assert_eq!(
            graphics.stretch_background(display, [-1, 0, 3, 1], Some([1, 0, 2, 1])),
            1
        );
        graphics.present_display().unwrap();
        assert_eq!(
            graphics.presented().unwrap().pixels(),
            [blue, blue, black].concat()
        );

        assert_eq!(graphics.stretch_background(display, [0; 4], None), 1);
        graphics.present_display().unwrap();
        assert_eq!(
            graphics.presented().unwrap().pixels(),
            [red, blue, green].concat()
        );
    }

    #[test]
    fn vertical_split_copies_the_incoming_center_rows_to_the_edges() {
        let old = [0, 0, 0, 255];
        let colors = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 255, 255],
        ];
        let mut graphics = Graphics::open(1, 4).unwrap();
        let target = graphics.display();
        graphics.fill(target, None, old);
        let incoming = graphics.create(1, 4).unwrap();
        for (y, color) in colors.into_iter().enumerate() {
            graphics.fill(incoming, Some([0, y as i32, 1, y as i32 + 1]), color);
        }
        assert_eq!(graphics.set_transition_source(target, incoming), 0);
        assert_eq!(
            graphics
                .begin_transition(
                    target,
                    SceneEffect {
                        duration: 100,
                        rule: None,
                        mode: 0,
                        commit: false,
                        quake: None,
                        transform: Some(TransitionTransform::VerticalSplit { reverse: false }),
                    },
                    0
                )
                .unwrap(),
            0
        );

        graphics.update_transitions(50, &mut crate::random::Random::seeded(0));
        graphics.present_display().unwrap();
        assert_eq!(
            graphics.presented().unwrap().pixels(),
            [colors[1], old, old, colors[2]].concat()
        );
    }

    #[test]
    fn transition_source_does_not_leak_into_the_normal_scene_or_capture() {
        let mut graphics = Graphics::open(4, 4).unwrap();
        let window = graphics.display();
        let back = graphics.create(4, 4).unwrap();
        assert!(graphics.fill(back, None, [240, 20, 80, 255]));
        assert_eq!(graphics.set_transition_source(window, back), 0);
        let bitmap = graphics.create(2, 2).unwrap();
        assert!(graphics.fill(bitmap, None, [20, 80, 240, 255]));
        let texture = graphics
            .texture_from_surface(bitmap, Some([0, 0, 1, 1]), [2, 1])
            .unwrap();
        assert_eq!(
            graphics.attach_layer(window, texture, 0, 0, false, false),
            0
        );
        graphics.present_display().unwrap();
        let offset = (4 + 2) * 4;
        assert_eq!(
            &graphics.presented().unwrap().pixels()[offset..offset + 4],
            &[20, 80, 240, 255]
        );
        assert!(graphics.remove_layer(window, texture));
        graphics.present_display().unwrap();
        assert_eq!(
            &graphics.presented().unwrap().pixels()[offset..offset + 4],
            &[0, 0, 0, 0]
        );
        assert_eq!(graphics.capture_scene(window, back, 0, 0, true).unwrap(), 0);
        assert_eq!(
            &graphics.surface(back).unwrap().pixels()[..4],
            &[0, 0, 0, 0]
        );
    }

    #[test]
    fn layers_draw_from_the_tail_of_each_priority_list() {
        let mut graphics = Graphics::open(1, 1).unwrap();
        let target = graphics.display();
        let mut texture = |color| {
            let image = graphics.create(1, 1).unwrap();
            assert!(graphics.fill(image, None, color));
            graphics.texture_from_surface(image, None, [0, 0]).unwrap()
        };
        let existing = texture([100, 0, 0, 255]);
        let prepended = texture([0, 100, 0, 255]);
        let appended = texture([0, 0, 100, 255]);
        let background = texture([200, 200, 0, 255]);
        let overlay = texture([0, 200, 200, 255]);
        for (id, priority, overlay, append) in [
            (existing, 10, false, false),
            (prepended, 10, false, false),
            (appended, 10, false, true),
            (background, 100, false, true),
            (overlay, 1000, true, false),
        ] {
            assert_eq!(
                graphics.attach_layer(target, id, priority, 0, overlay, append),
                0
            );
        }
        assert!(graphics.remove_layer(target, overlay));
        assert!(graphics.move_layer(target, prepended, 10, true, true));
        graphics.present_display().unwrap();
        assert_eq!(graphics.presented().unwrap().pixels(), &[0, 100, 0, 255]);
        assert!(graphics.move_layer(target, prepended, 10, false, true));
        graphics.present_display().unwrap();
        assert_eq!(graphics.presented().unwrap().pixels(), &[100, 0, 0, 255]);
        assert!(graphics.move_layer(target, prepended, 10, false, false));
        assert_eq!(
            graphics.attach_layer(target, overlay, 1000, 0, true, false),
            0
        );
        for (id, color) in [
            (overlay, [0, 200, 200, 255]),
            (prepended, [0, 100, 0, 255]),
            (existing, [100, 0, 0, 255]),
            (appended, [0, 0, 100, 255]),
            (background, [200, 200, 0, 255]),
        ] {
            graphics.present_display().unwrap();
            assert_eq!(graphics.presented().unwrap().pixels(), &color);
            assert!(graphics.remove_layer(target, id));
        }
    }

    #[test]
    fn drawing_into_a_sprite_bitmap_updates_its_attached_layer() {
        let mut graphics = Graphics::open(1, 1).unwrap();
        let target = graphics.display();
        let bitmap = graphics.create(1, 1).unwrap();
        let texture = graphics.texture_from_surface(bitmap, None, [0, 0]).unwrap();
        assert_eq!(
            graphics.attach_layer(target, texture, 0, 0, false, false),
            0
        );
        graphics.present_display().unwrap();
        assert!(graphics.fill(bitmap, None, [64, 128, 255, 255]));
        graphics.present_display().unwrap();
        assert_eq!(graphics.presented().unwrap().pixels(), &[64, 128, 255, 255]);
        graphics.free(bitmap);
        graphics.present().unwrap();
        assert_eq!(graphics.presented().unwrap().pixels(), &[64, 128, 255, 255]);
    }
}
