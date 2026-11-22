use crate::{
    Error, Result,
    archive::Archive,
    binary::{c_string, decode_japanese, dword, range},
    crx::Crx,
    game::{Game, read_bounded},
    render::Canvas,
};
use image::{Rgba, RgbaImage};
use lru::LruCache;
use roukaku_media_core::{RectI, SceneCommand, Transform2D};
use std::{num::NonZeroUsize, path::PathBuf, sync::Arc};

const MAX_GRAPHIC_BYTES: usize = 64 * 1024 * 1024;
const CACHE_BYTES: usize = 96 * 1024 * 1024;

pub(crate) struct GraphicsCache {
    images: LruCache<String, Arc<Crx>>,
    archives: LruCache<PathBuf, Archive>,
    bytes: usize,
}

impl Default for GraphicsCache {
    fn default() -> Self {
        Self {
            images: LruCache::new(NonZeroUsize::new(128).expect("image cache capacity")),
            archives: LruCache::new(NonZeroUsize::new(16).expect("archive cache capacity")),
            bytes: 0,
        }
    }
}

impl GraphicsCache {
    pub(crate) fn load(&mut self, game: &Game, name: &str) -> Result<Arc<Crx>> {
        let key = name.replace('\\', "/").to_ascii_lowercase();
        if let Some(image) = self.images.get(&key) {
            return Ok(image.clone());
        }
        let image = load(game, name, &mut self.archives)?;
        let size = image.image.as_raw().len();
        if size <= CACHE_BYTES {
            while self.bytes > CACHE_BYTES - size {
                let (_, image) = self.images.pop_lru().expect("image cache accounting");
                self.bytes -= image.image.as_raw().len();
            }
            if let Some((_, image)) = self.images.push(key, image.clone()) {
                self.bytes -= image.image.as_raw().len();
            }
            self.bytes += size;
        }
        Ok(image)
    }
}

pub(crate) fn variant_name(name: &str, variant: u16, base: Option<&str>) -> String {
    let (member, archive) = name
        .split_once(',')
        .map_or((name, None), |(member, archive)| (member, Some(archive)));
    let marker = |member: &str| member.chars().find(|c| matches!(c, '*' | '@'));
    let requested = match variant {
        1 => Some('*'),
        2 => Some('@'),
        _ => None,
    };
    let base_member = base.map(|name| name.split_once(',').map_or(name, |(member, _)| member));
    let inherited = base_member.and_then(marker);
    let suffix = inherited.or(requested).or_else(|| marker(member));
    let mut member = if suffix == marker(member) {
        member.to_owned()
    } else {
        member.replace(['*', '@'], "")
    };
    if let Some(archive) = archive {
        let body = |member: &str| {
            member
                .trim_end_matches(['*', '@'])
                .to_ascii_uppercase()
                .ends_with("_B")
        };
        if base_member.is_some_and(body) && !body(&member) {
            member.truncate(member.trim_end_matches(['*', '@']).len());
            member.push_str("_B");
        }
        if let Some(suffix) = suffix
            && marker(&member).is_none()
        {
            member.push(suffix);
        }
        member.push(',');
        member.push_str(archive);
    } else if let Some(original) = marker(name)
        && let Some(suffix) = suffix
        && original != suffix
    {
        return name.replace(original, &suffix.to_string());
    }
    member
}

fn load(game: &Game, name: &str, archives: &mut LruCache<PathBuf, Archive>) -> Result<Arc<Crx>> {
    let (member, bundle) = name
        .split_once(',')
        .map_or((name, None), |(member, bundle)| (member, Some(bundle)));
    let start = member.rfind(['/', '\\']).map_or(0, |at| at + 1);
    let end = member[start..]
        .rfind('.')
        .map_or(member.len(), |at| start + at);
    let mut member = member.to_owned();
    let variant = match end.checked_sub(1).and_then(|at| member.as_bytes().get(at)) {
        Some(b'@' | b'*' | b'?') => Some(member.remove(end - 1)),
        _ => None,
    };
    let mut result = if let Some(bundle) = bundle {
        // ADV32 names the requested image first and its CRM container second.
        let path = game.resource("grp", bundle, "crm")?;
        let archive = archives.try_get_or_insert_ref(path, || Archive::open(path))?;
        let member = member.rsplit(['/', '\\']).next().unwrap_or(&member);
        let member = archive
            .find_name(member)
            .or_else(|| archive.find_name(&format!("{member}.crx")))
            .ok_or_else(|| Error::MissingAsset(name.to_owned()))?;
        let bytes = archive.read(member, MAX_GRAPHIC_BYTES)?;
        decode_archive_image(archive, &bytes, 0)?
    } else {
        let asset = &member;
        let path = game.resource("grp", asset, "crx")?;
        let bytes = read_bounded(path, MAX_GRAPHIC_BYTES)?;
        if bytes.starts_with(b"CRXD") {
            let base_name = decode_japanese(c_string(range(&bytes, 12, 20)?)?)?;
            let sibling = asset
                .rsplit_once(['/', '\\'])
                .map_or(base_name.clone(), |(parent, _)| {
                    format!("{parent}/{base_name}")
                });
            let base = read_bounded(game.resource("grp", &sibling, "crx")?, MAX_GRAPHIC_BYTES)?;
            apply_diff(
                Crx::decode(&base)?,
                Crx::decode(range(&bytes, 32, bytes.len() - 32)?)?,
            )?
        } else {
            Crx::decode(&bytes)?
        }
    };
    if !matches!(variant, Some('@' | '*')) {
        return Ok(Arc::new(result));
    }
    for pixel in result.image_mut().pixels_mut() {
        if pixel[3] == 0 {
            continue;
        }
        let [r, g, b, alpha] = pixel.0.map(u16::from);
        pixel.0 = match variant {
            Some('@') => [3 * r / 4, 3 * g / 4, 3 * b / 4 + 16, alpha],
            Some('*') => [7 * r / 8 + 32, 7 * g / 8 + 8, 7 * b / 8, alpha],
            _ => unreachable!("tint variant checked above"),
        }
        .map(|component| component as u8);
    }
    Ok(Arc::new(result))
}

pub(crate) fn decode_archive_image(archive: &Archive, data: &[u8], depth: u8) -> Result<Crx> {
    if depth >= 8 {
        return Err(Error::Invalid("CRX image reference depth"));
    }
    if data.starts_with(b"CRXG") {
        return Crx::decode(data);
    }
    if !data.starts_with(b"CRXD") {
        return Err(Error::Unsupported("CRM member format"));
    }
    let diff = data.get(32..).ok_or(Error::Invalid("CRXD data"))?;
    let base = archive.by_offset(dword(data, 8)?)?;
    let base = decode_archive_image(archive, &archive.read(base, MAX_GRAPHIC_BYTES)?, depth + 1)?;
    apply_diff(base, decode_delta(archive, diff, depth + 1)?)
}

fn decode_delta(archive: &Archive, data: &[u8], depth: u8) -> Result<Crx> {
    if depth >= 8 {
        return Err(Error::Invalid("CRX delta reference depth"));
    }
    if data.starts_with(b"CRXJ") {
        let target = archive.by_offset(dword(data, 8)?)?;
        let record = archive.read(target, MAX_GRAPHIC_BYTES)?;
        if !record.starts_with(b"CRXD") {
            return Err(Error::Invalid("CRX delta reference record"));
        }
        let delta = record.get(32..).ok_or(Error::Invalid("CRX delta record"))?;
        return decode_delta(archive, delta, depth + 1);
    }
    Crx::decode(data)
}

fn apply_diff(mut base: Crx, delta: Crx) -> Result<Crx> {
    let (base_x, base_y) = (i64::from(base.x), i64::from(base.y));
    let image = base.image_mut();
    for (x, y, pixel) in delta.image.enumerate_pixels() {
        let x = i64::from(x) + i64::from(delta.x) - base_x;
        let y = i64::from(y) + i64::from(delta.y) - base_y;
        if x < 0 || y < 0 {
            continue;
        }
        if let Some(original) = image.get_pixel_mut_checked(x as u32, y as u32) {
            for channel in 0..3 {
                original[channel] = original[channel].wrapping_add(pixel[channel]);
            }
            original[3] = original[3].wrapping_sub(pixel[3]);
        }
    }
    Ok(base)
}

#[derive(Clone, Copy)]
pub(crate) struct Clip {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) struct Transform {
    pub(crate) position: (f64, f64),
    pub(crate) pivot: (f64, f64),
    pub(crate) scale: f64,
    pub(crate) angle: f64,
    pub(crate) opacity: f64,
}

pub(crate) fn blit(frame: &mut Canvas, source: &Crx, x: i32, y: i32, clip: Option<Clip>) {
    blit_alpha(frame, source, x, y, clip, 255, 100);
}

pub(crate) fn blit_alpha(
    frame: &mut Canvas,
    source: &Crx,
    x: i32,
    y: i32,
    clip: Option<Clip>,
    opacity: u8,
    scale: u32,
) {
    let (left, top, width, height, x, y) = match clip {
        Some(clip) => (
            clip.x.max(0) as u32,
            clip.y.max(0) as u32,
            clip.width,
            clip.height,
            i64::from(x),
            i64::from(y),
        ),
        None => (
            0,
            0,
            source.image.width(),
            source.image.height(),
            i64::from(x) + i64::from(source.x),
            i64::from(y) + i64::from(source.y),
        ),
    };
    let width = width.min(source.image.width().saturating_sub(left));
    let height = height.min(source.image.height().saturating_sub(top));
    let scale = i64::from(scale);
    if scale == 0 || opacity == 0 {
        return;
    }
    if frame.queue_sprite(
        source,
        RectI::new(left as i32, top as i32, width, height),
        RectI::new(
            x as i32,
            y as i32,
            (i64::from(width) * scale / 100) as u32,
            (i64::from(height) * scale / 100) as u32,
        ),
        f32::from(opacity) / 255.0,
    ) {
        return;
    }
    let frame = frame.raster_mut();
    let stop_x = (x + i64::from(width) * scale / 100).min(i64::from(frame.width()));
    let stop_y = (y + i64::from(height) * scale / 100).min(i64::from(frame.height()));
    if stop_x <= x.max(0) || stop_y <= y.max(0) {
        return;
    }
    let dst_stride = frame.width() as usize * 4;
    let src_stride = source.image.width() as usize * 4;
    let dst_x = x.max(0);
    let span = (stop_x - dst_x) as usize * 4;
    let opaque = opacity == 255 && source.is_opaque();
    if scale == 100 {
        let src_x = (i64::from(left) + dst_x - x) as usize;
        for dst_y in y.max(0)..stop_y {
            let src_y = (i64::from(top) + dst_y - y) as usize;
            let dst_start = dst_y as usize * dst_stride + dst_x as usize * 4;
            let src_start = src_y * src_stride + src_x * 4;
            let dst = &mut frame.as_mut()[dst_start..dst_start + span];
            let src = &source.image.as_raw()[src_start..src_start + span];
            if opaque {
                dst.copy_from_slice(src);
                continue;
            }
            for (dst, src) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<4>().0)
            {
                blend_sample(dst, *src, opacity);
            }
        }
        return;
    }
    let samples_x: Vec<usize> = (dst_x..stop_x)
        .map(|dest_x| left as usize + ((dest_x - x) * 100 / scale) as usize)
        .collect();
    for dst_y in y.max(0)..stop_y {
        let src_y = top as usize + ((dst_y - y) * 100 / scale) as usize;
        let dst_start = dst_y as usize * dst_stride + dst_x as usize * 4;
        let src_start = src_y * src_stride;
        let src = source.image.as_raw()[src_start..src_start + src_stride]
            .as_chunks::<4>()
            .0;
        let dst = frame.as_mut()[dst_start..dst_start + span]
            .as_chunks_mut::<4>()
            .0;
        if opaque {
            for (dst, &sample_x) in dst.iter_mut().zip(&samples_x) {
                *dst = src[sample_x];
            }
        } else {
            for (dst, &sample_x) in dst.iter_mut().zip(&samples_x) {
                blend_sample(dst, src[sample_x], opacity);
            }
        }
    }
}

#[inline]
fn blend_sample(dst: &mut [u8; 4], mut src: [u8; 4], opacity: u8) {
    use image::Pixel;
    src[3] = (u16::from(src[3]) * u16::from(opacity) / 255) as u8;
    match src[3] {
        0 => {}
        255 => *dst = src,
        _ => {
            let mut pixel = Rgba(*dst);
            pixel.blend(&Rgba(src));
            *dst = pixel.0;
        }
    }
}

pub(crate) fn mosaic(frame: &mut RgbaImage, divisions: u32) {
    let (width, height) = frame.dimensions();
    let rows = divisions.min(height);
    let columns = divisions.min(width);
    for cell_y in 0..rows {
        let top = (cell_y * height).div_ceil(rows);
        let bottom = ((cell_y + 1) * height).div_ceil(rows);
        for cell_x in 0..columns {
            let left = (cell_x * width).div_ceil(columns);
            let right = ((cell_x + 1) * width).div_ceil(columns);
            let pixel = *frame.get_pixel(left, top);
            for y in top..bottom {
                for x in left..right {
                    frame.put_pixel(x, y, pixel);
                }
            }
        }
    }
}

pub(crate) fn blit_transformed(frame: &mut Canvas, source: &Crx, clip: Clip, transform: Transform) {
    use image::Pixel;
    let (width, height) = source.image.dimensions();
    let left = clip.x.max(0) as u32;
    let top = clip.y.max(0) as u32;
    let width = f64::from(clip.width.min(width.saturating_sub(left)));
    let height = f64::from(clip.height.min(height.saturating_sub(top)));
    let (px, py) = transform.pivot;
    let (x, y) = (transform.position.0 + px, transform.position.1 + py);
    let factor = transform.scale;
    if factor <= 0.0 || transform.opacity <= 0.0 {
        return;
    }
    let (sin, cos) = transform.angle.to_radians().sin_cos();
    let matrix = Transform2D {
        m11: (factor * cos) as f32,
        m12: (factor * sin) as f32,
        m21: (-factor * sin) as f32,
        m22: (factor * cos) as f32,
        tx: (x - factor * (px * cos - py * sin)) as f32,
        ty: (y - factor * (px * sin + py * cos)) as f32,
    };
    if frame.queue(SceneCommand::PushTransform { transform: matrix }) {
        frame.queue_sprite(
            source,
            RectI::new(left as i32, top as i32, width as u32, height as u32),
            RectI::new(0, 0, width as u32, height as u32),
            transform.opacity as f32,
        );
        frame.queue(SceneCommand::PopTransform);
        return;
    }
    let frame = frame.raster_mut();
    let mut bounds = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for (sx, sy) in [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)] {
        let dx = (sx - px) * factor;
        let dy = (sy - py) * factor;
        let dest_x = x + dx * cos - dy * sin;
        let dest_y = y + dx * sin + dy * cos;
        bounds = (
            bounds.0.min(dest_x),
            bounds.1.min(dest_y),
            bounds.2.max(dest_x),
            bounds.3.max(dest_y),
        );
    }
    let start_x = bounds.0.floor().max(0.0) as u32;
    let start_y = bounds.1.floor().max(0.0) as u32;
    let end_x = bounds.2.ceil().min(f64::from(frame.width())) as u32;
    let end_y = bounds.3.ceil().min(f64::from(frame.height())) as u32;
    for dy in start_y..end_y {
        let cy = f64::from(dy) - y;
        for dx in start_x..end_x {
            let cx = f64::from(dx) - x;
            let sx = (cx * cos + cy * sin) / factor + px;
            let sy = (cy * cos - cx * sin) / factor + py;
            if (0.0..width).contains(&sx) && (0.0..height).contains(&sy) {
                let mut color = *source.image.get_pixel(left + sx as u32, top + sy as u32);
                color[3] = (f64::from(color[3]) * transform.opacity).round() as u8;
                frame.get_pixel_mut(dx, dy).blend(&color);
            }
        }
    }
}
