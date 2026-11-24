use roukaku_core::{DiagnosticSeverity, Hash256};
use roukaku_media_core::{GlyphBitmap, GlyphBitmapFormat, MediaError, SceneCommand};
use roukaku_text::{
    CosmicTextLayoutProvider, GlyphResource, TextLayoutRequest, TextLayoutResult,
    TextRenderFrameCommands, TextRenderLayoutUpdate, TextRenderResourceOwner,
};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Weak},
};

#[cfg(test)]
mod tests;

/// Circular stroke around a glyph's coverage mask. Radius is in logical pixels.
#[derive(Clone, Copy)]
pub struct TextOutline {
    pub radius: u8,
    pub rgba: [u8; 4],
}

/// One core-owned text region. Layout policy and fonts remain explicit inputs.
#[derive(Clone)]
pub struct TextSceneLayout {
    pub request: TextLayoutRequest,
    pub translation: (i32, i32),
    pub rgba: [u8; 4],
    pub outline: Option<TextOutline>,
}

const MAX_LOGICAL_OUTLINE_RADIUS: u8 = 8;
const MAX_RASTER_OUTLINE_RADIUS: u8 = 64;
const MAX_OUTLINE_MASK_BYTES: usize = 1024 * 1024;
const MAX_OUTLINE_BYTES: usize = 16 * 1024 * 1024;
const MAX_OUTLINE_MASKS: usize = 4096;

#[derive(Clone, Copy, Default)]
struct OutlineBudget {
    bytes: usize,
    masks: usize,
}

impl OutlineBudget {
    fn fits(self, bytes: usize, masks: usize) -> bool {
        bytes <= MAX_OUTLINE_BYTES - self.bytes && masks <= MAX_OUTLINE_MASKS - self.masks
    }

    fn charge(&mut self, bytes: usize, masks: usize) -> Result<(), MediaError> {
        if !self.fits(bytes, masks) {
            return Err(MediaError::message("SHINKI_EMU_SDK_TEXT_OUTLINE_BUDGET"));
        }
        self.bytes += bytes;
        self.masks += masks;
        Ok(())
    }
}

#[derive(Default)]
struct OutlineMasks {
    // Glyph IDs carry raster size and phase; include the face hash in case an
    // asset is replaced under the same name. Cached pixels are shared owners.
    bitmaps: BTreeMap<String, GlyphBitmap>,
    budget: OutlineBudget,
}

struct CachedOutline {
    source: Weak<TextLayoutResult>,
    radius: u8,
    layout: Arc<TextLayoutResult>,
    cost: OutlineBudget,
}

struct PreparedRegion<'a> {
    id: &'a str,
    layout: Arc<TextLayoutResult>,
    translation: (i32, i32),
    rgba: [u8; 4],
    outline: Option<(String, [u8; 4], CachedOutline)>,
}

/// Shaping and glyph residency shared by emulator families.
///
/// A frame supplies all visible layouts. Missing layouts release their resources;
/// shared glyphs are updated atomically across regions before any draw commands.
/// Returned commands must be submitted in order to the same renderer. On a render
/// failure the caller must close the session, rather than discard the lifecycle.
pub struct TextScene {
    provider: CosmicTextLayoutProvider,
    resources: TextRenderResourceOwner,
    visible: BTreeSet<String>,
    outlines: BTreeMap<String, CachedOutline>,
    outline_masks: OutlineMasks,
}

impl TextScene {
    pub fn new(provider: CosmicTextLayoutProvider) -> Self {
        Self {
            provider,
            resources: TextRenderResourceOwner::default(),
            visible: BTreeSet::new(),
            outlines: BTreeMap::new(),
            outline_masks: OutlineMasks::default(),
        }
    }

    pub fn frame(&mut self, regions: &[TextSceneLayout]) -> Result<Vec<SceneCommand>, MediaError> {
        self.frame_scaled(regions, 1.0)
    }

    /// Shape through the same provider and cache used by `frame`. Family-native
    /// layout policies can read glyph advances without guessing from bitmaps or
    /// owning a second font system. The result owns no renderer resources.
    pub fn layout_shared(
        &self,
        request: &TextLayoutRequest,
    ) -> Result<Arc<TextLayoutResult>, MediaError> {
        let layout = self.provider.layout_shared(request)?;
        if layout.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.severity,
                DiagnosticSeverity::Error | DiagnosticSeverity::Blocking
            )
        }) {
            return Err(MediaError::Diagnostics(layout.diagnostics.clone()));
        }
        Ok(layout)
    }

    /// Build a frame at a physical raster density while keeping the caller's
    /// layout in logical stage units. The provider receives scaled constraints
    /// and therefore rasterizes glyphs at the requested density; the caller can
    /// map the resulting commands back through its raster-to-logical transform.
    pub fn frame_scaled(
        &mut self,
        regions: &[TextSceneLayout],
        raster_scale: f32,
    ) -> Result<Vec<SceneCommand>, MediaError> {
        let frame = self.frame_parts(regions, raster_scale)?;
        let mut commands = frame.lifecycle;
        for layout in frame.layouts {
            commands.extend(layout.commands);
        }
        Ok(commands)
    }

    /// Keep text draw batches separate so callers can interleave them with sprites.
    /// The lifecycle commands precede all draws.
    pub fn frame_parts(
        &mut self,
        regions: &[TextSceneLayout],
        raster_scale: f32,
    ) -> Result<TextRenderFrameCommands, MediaError> {
        self.frame_parts_with_layout(regions, raster_scale, |_, layout| layout)
    }

    /// Apply engine positioning to shaped glyphs before updating the glyph atlas.
    /// The callback receives layouts in physical pixels, in region order.
    /// Each outlined region draws its stroke once per glyph before the fill.
    pub fn frame_parts_with_layout(
        &mut self,
        regions: &[TextSceneLayout],
        raster_scale: f32,
        mut position: impl FnMut(usize, Arc<TextLayoutResult>) -> Arc<TextLayoutResult>,
    ) -> Result<TextRenderFrameCommands, MediaError> {
        if !raster_scale.is_finite() || raster_scale <= 0.0 {
            return Err(MediaError::message("SHINKI_EMU_SDK_TEXT_SCALE"));
        }
        let mut visible = BTreeSet::new();
        let mut prepared = Vec::with_capacity(regions.len());
        let mut outline_budget = OutlineBudget::default();
        for (index, logical) in regions.iter().enumerate() {
            let id = logical.request.key.as_str();
            if id.trim().is_empty() || !visible.insert(id.to_owned()) {
                return Err(MediaError::message("SHINKI_EMU_SDK_TEXT_LAYOUT_ID"));
            }
            let region = scale_layout(logical, raster_scale)?;
            let layout = position(index, self.layout_shared(&region.request)?);
            let outline = if let Some(outline) = region.outline {
                let outline_id = format!("{id}.outline");
                if !visible.insert(outline_id.clone()) {
                    return Err(MediaError::message("SHINKI_EMU_SDK_TEXT_LAYOUT_ID"));
                }
                let cached = match self.outlines.remove(&outline_id) {
                    Some(cached)
                        if cached.radius == outline.radius
                            && std::ptr::eq(cached.source.as_ptr(), Arc::as_ptr(&layout)) =>
                    {
                        outline_budget.charge(cached.cost.bytes, cached.cost.masks)?;
                        cached
                    }
                    _ => self
                        .outline_masks
                        .layout(&layout, outline.radius, &mut outline_budget)?,
                };
                Some((outline_id, outline.rgba, cached))
            } else {
                None
            };
            prepared.push(PreparedRegion {
                id,
                layout,
                translation: region.translation,
                rgba: region.rgba,
                outline,
            });
        }
        let mut updates = Vec::with_capacity(visible.len());
        for region in &prepared {
            if let Some((id, rgba, outline)) = &region.outline {
                updates.push(TextRenderLayoutUpdate {
                    layout_id: id,
                    layout: &outline.layout,
                    shared_layout: Some(&outline.layout),
                    rgba: *rgba,
                    translation: region.translation,
                });
            }
            updates.push(TextRenderLayoutUpdate {
                layout_id: region.id,
                layout: &region.layout,
                shared_layout: Some(&region.layout),
                rgba: region.rgba,
                translation: region.translation,
            });
        }
        let removals = self
            .visible
            .difference(&visible)
            .map(String::as_str)
            .collect::<Vec<_>>();
        let frame = self.resources.update_frame(&updates, &removals)?;
        self.outlines = prepared
            .into_iter()
            .filter_map(|region| {
                let (id, _, outline) = region.outline?;
                Some((id, outline))
            })
            .collect();
        self.visible = visible;
        Ok(frame)
    }

    /// Release all glyphs while the consumer's renderer is still alive.
    pub fn clear(&mut self) -> Vec<SceneCommand> {
        self.visible.clear();
        self.outlines.clear();
        self.outline_masks = OutlineMasks::default();
        self.resources.shutdown()
    }
}

impl OutlineMasks {
    fn layout(
        &mut self,
        source: &Arc<TextLayoutResult>,
        radius: u8,
        budget: &mut OutlineBudget,
    ) -> Result<CachedOutline, MediaError> {
        let before = *budget;
        let used: BTreeSet<_> = source
            .shaped_runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .filter(|glyph| glyph.render_x.is_some() && glyph.render_y.is_some())
            .filter_map(|glyph| glyph.resource_id.as_deref())
            .collect();
        let mut ids = BTreeMap::new();
        let mut glyph_resources = Vec::with_capacity(used.len().min(MAX_OUTLINE_MASKS));
        for glyph in &source.glyph_resources {
            if !used.contains(glyph.resource_id.as_str()) {
                continue;
            }
            let (width, height) = outline_dimensions(&glyph.bitmap, radius)?;
            let bytes = width * height;
            budget.charge(bytes, 1)?;
            let id = format!(
                "sdk:outline:{radius}:{}:{}",
                glyph.font_hash,
                Hash256::from_sha256(glyph.resource_id.as_bytes()),
            );
            let bitmap = if let Some(cached) = self.bitmaps.get(&id) {
                cached.clone()
            } else {
                let bitmap = dilate_outline(&glyph.bitmap, radius, width, height)?;
                if !self.budget.fits(bytes, 1) {
                    self.bitmaps.clear();
                    self.budget = OutlineBudget::default();
                }
                self.budget.charge(bytes, 1)?;
                self.bitmaps.insert(id.clone(), bitmap.clone());
                bitmap
            };
            ids.insert(glyph.resource_id.as_str(), id.clone());
            glyph_resources.push(GlyphResource {
                resource_id: id,
                font_asset_id: glyph.font_asset_id.clone(),
                font_hash: glyph.font_hash,
                glyph_id: glyph.glyph_id,
                bitmap,
            });
        }
        let mut shaped_runs = source.shaped_runs.clone();
        for glyph in shaped_runs.iter_mut().flat_map(|run| &mut run.glyphs) {
            if let (Some(id), Some(x), Some(y)) =
                (glyph.resource_id.as_deref(), glyph.render_x, glyph.render_y)
            {
                let id = ids
                    .get(id)
                    .ok_or_else(|| MediaError::message("SHINKI_EMU_SDK_TEXT_OUTLINE_RESOURCE"))?;
                let offset = i32::from(radius);
                let (x, y) = x
                    .checked_sub(offset)
                    .zip(y.checked_sub(offset))
                    .ok_or_else(|| MediaError::message("SHINKI_EMU_SDK_TEXT_OUTLINE_BOUNDS"))?;
                glyph.resource_id = Some(id.clone());
                glyph.render_x = Some(x);
                glyph.render_y = Some(y);
            }
        }
        Ok(CachedOutline {
            source: Arc::downgrade(source),
            radius,
            layout: Arc::new(TextLayoutResult {
                schema: source.schema.clone(),
                key: source.key.clone(),
                width: source.width,
                height: source.height,
                lines: Vec::new(),
                shaped_runs,
                glyph_resources,
                ruby_boxes: Vec::new(),
                voice_refs: Vec::new(),
                clip: source.clip,
                clipped: source.clipped,
                ellipsized: source.ellipsized,
                diagnostics: Vec::new(),
                revision: source.revision,
            }),
            cost: OutlineBudget {
                bytes: budget.bytes - before.bytes,
                masks: budget.masks - before.masks,
            },
        })
    }
}

fn outline_dimensions(bitmap: &GlyphBitmap, radius: u8) -> Result<(usize, usize), MediaError> {
    bitmap.validate_integrity()?;
    let padding = usize::from(radius) * 2;
    let width = bitmap.width as usize + padding;
    let height = bitmap.height as usize + padding;
    if width > 4096 || height > 4096 || width * height > MAX_OUTLINE_MASK_BYTES {
        return Err(MediaError::message("SHINKI_EMU_SDK_TEXT_OUTLINE_BUDGET"));
    }
    Ok((width, height))
}

fn dilate_outline(
    source: &GlyphBitmap,
    radius: u8,
    width: usize,
    height: usize,
) -> Result<GlyphBitmap, MediaError> {
    let radius = usize::from(radius);
    let source_width = source.width as usize;
    let channels = match source.format {
        GlyphBitmapFormat::Alpha8 => 1,
        GlyphBitmapFormat::Rgba8 => 4,
    };
    let mut pixels = vec![0u8; width * height];
    let mut spans = [0; MAX_RASTER_OUTLINE_RADIUS as usize + 1];
    let mut half = radius;
    for (dy, span) in spans[..=radius].iter_mut().enumerate() {
        while half * half + dy * dy > radius * radius {
            half -= 1;
        }
        *span = half;
    }

    // Each row of the disk is a horizontal max filter, evaluated in linear
    // time with a monotonic queue. Mirror each span to both output rows.
    const WINDOW: usize = MAX_RASTER_OUTLINE_RADIUS as usize * 2 + 1;
    let mut queue = [(0usize, 0u8); WINDOW];
    for (y, row) in source.pixels.chunks_exact(source_width * channels).enumerate() {
        for (dy, &span) in spans[..=radius].iter().enumerate() {
            let mut head = 0;
            let mut tail = 0;
            let start = (y + radius - dy) * width + radius - span;
            let mirror = start + 2 * dy * width;
            for x in 0..source_width + 2 * span {
                if head < tail && queue[head % WINDOW].0 < x.saturating_sub(2 * span) {
                    head += 1;
                }
                if x < source_width {
                    let coverage = row[(x + 1) * channels - 1];
                    if coverage != 0 {
                        while head < tail && queue[(tail - 1) % WINDOW].1 <= coverage {
                            tail -= 1;
                        }
                        queue[tail % WINDOW] = (x, coverage);
                        tail += 1;
                    }
                }
                if head < tail {
                    let coverage = queue[head % WINDOW].1;
                    pixels[start + x] = pixels[start + x].max(coverage);
                    if dy != 0 {
                        pixels[mirror + x] = pixels[mirror + x].max(coverage);
                    }
                }
            }
        }
    }
    // Leave already covered glyph interior to the foreground pass.
    for (y, row) in source.pixels.chunks_exact(source_width * channels).enumerate() {
        let start = (y + radius) * width + radius;
        for (dst, src) in pixels[start..start + source_width]
            .iter_mut()
            .zip(row.chunks_exact(channels))
        {
            *dst -= src[channels - 1];
        }
    }
    GlyphBitmap::from_vec(width as u32, height as u32, GlyphBitmapFormat::Alpha8, pixels)
}

fn scale_layout(
    region: &TextSceneLayout,
    scale: f32,
) -> Result<Cow<'_, TextSceneLayout>, MediaError> {
    if let Some(outline) = region.outline
        && (!(1..=MAX_LOGICAL_OUTLINE_RADIUS).contains(&outline.radius) || outline.rgba[3] == 0)
    {
        return Err(MediaError::message("SHINKI_EMU_SDK_TEXT_OUTLINE"));
    }
    let constraint = &region.request.constraint;
    let max_width = scale_f32(constraint.max_width, scale)?;
    let max_height = constraint
        .max_height
        .map(|value| scale_f32(value, scale))
        .transpose()?;
    let font_size = scale_f32(constraint.font_size, scale)?;
    let line_height = scale_f32(constraint.line_height, scale)?;
    if scale == 1.0 {
        return Ok(Cow::Borrowed(region));
    }
    let mut request = region.request.clone();
    request.constraint.max_width = max_width;
    request.constraint.max_height = max_height;
    request.constraint.font_size = font_size;
    request.constraint.line_height = line_height;
    let translation = (
        scale_i32(region.translation.0, scale)?,
        scale_i32(region.translation.1, scale)?,
    );
    let outline = region
        .outline
        .map(|outline| {
            let radius = (f32::from(outline.radius) * scale).round().max(1.0);
            if !(1.0..=f32::from(MAX_RASTER_OUTLINE_RADIUS)).contains(&radius) {
                return Err(MediaError::message("SHINKI_EMU_SDK_TEXT_OUTLINE"));
            }
            Ok(TextOutline {
                radius: radius as u8,
                rgba: outline.rgba,
            })
        })
        .transpose()?;
    Ok(Cow::Owned(TextSceneLayout {
        request,
        translation,
        rgba: region.rgba,
        outline,
    }))
}

fn scale_f32(value: f32, scale: f32) -> Result<f32, MediaError> {
    let scaled = value * scale;
    if !scaled.is_finite() || scaled <= 0.0 {
        return Err(MediaError::message("SHINKI_EMU_SDK_TEXT_SCALE"));
    }
    Ok(scaled)
}

fn scale_i32(value: i32, scale: f32) -> Result<i32, MediaError> {
    let scaled = f64::from(value) * f64::from(scale);
    if !scaled.is_finite() || scaled < f64::from(i32::MIN) || scaled > f64::from(i32::MAX) {
        return Err(MediaError::message("SHINKI_EMU_SDK_TEXT_SCALE"));
    }
    Ok(scaled.round() as i32)
}
