use super::slot_io::{Reader, invalid};
use crate::vm::CmvsNativeCollisionGeometry;
use shinki_emu_sdk::CoreError;
use std::collections::BTreeMap;

/// CImage fields written by 445160, in native DWORD order (not an affine transform).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SavedImage {
    /// DWORD1034, 1035, 1036, 1038 and 1039.
    pub(crate) header: [u32; 5],
    pub(crate) primary: Option<SavedProvider>,
    pub(crate) secondary: Option<SavedProvider>,
    /// Native 1861 is an empty serializer stub, with an unsupported live owner.
    pub(crate) unsupported_object_1861: bool,
    /// CImage DWORD1031, serialized by 447D10 as native record1865.
    pub(crate) collision_geometry: Option<CmvsNativeCollisionGeometry>,
    pub(crate) basic: Option<SavedBasic>,
    /// DWORD1040..1050; the first word enables the native image effect.
    pub(crate) effect_words: [u32; 11],
    pub(crate) children: BTreeMap<u16, SavedImage>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SavedBasic {
    pub(crate) words: [u32; 28],
    pub(crate) effects: BTreeMap<u16, Vec<u8>>,
}

impl Default for SavedBasic {
    fn default() -> Self {
        let mut words = [0; 28];
        words[1] = 1;
        words[17] = 255;
        words[18] = 1.0_f32.to_bits();
        words[19] = 1.0_f32.to_bits();
        words[21] = 256;
        Self {
            words,
            effects: BTreeMap::new(),
        }
    }
}

impl Default for SavedImage {
    fn default() -> Self {
        Self {
            header: [0, 1, 0x7fff_ffff, 0, 0],
            primary: None,
            secondary: None,
            unsupported_object_1861: false,
            collision_geometry: None,
            basic: None,
            effect_words: [0; 11],
            children: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SavedProvider {
    pub(crate) load_flags: u32,
    pub(crate) frame: u32,
    pub(crate) kind: u32,
    pub(crate) filter: [u32; 4],
    pub(crate) dynamic_parameter: u32,
    pub(crate) source: SavedProviderSource,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SavedProviderSource {
    File {
        resource: Vec<u8>,
        mask: Vec<u8>,
        archive: u32,
        width: u32,
        height: u32,
    },
    /// CTexture's composed provider (435A40), distinct from a CMV dynamic surface.
    Layers(BTreeMap<u16, [u8; 132]>),
}

fn words<const N: usize>(reader: &mut Reader<'_>) -> Result<[u32; N], CoreError> {
    let mut result = [0; N];
    for word in &mut result {
        *word = reader.u32()?;
    }
    Ok(result)
}
fn put_words(output: &mut Vec<u8>, words: &[u32]) {
    for word in words {
        output.extend_from_slice(&word.to_le_bytes());
    }
}
fn next_tag(reader: Reader<'_>) -> Result<u16, CoreError> {
    let mut r = reader;
    r.u16()
}
fn record<'a>(reader: &mut Reader<'a>, tag: u16) -> Result<Option<&'a [u8]>, CoreError> {
    if next_tag(*reader)? != tag {
        return Ok(None);
    }
    reader.u16()?;
    let length = reader.u32()? as usize;
    // 445EB0 consumes the low WORD of these DWORD lengths.
    if length > u16::MAX as usize {
        return Err(invalid(
            "native image subrecord exceeds its WORD reader limit",
        ));
    }
    Ok(Some(reader.bytes(length)?))
}
fn put_record(output: &mut Vec<u8>, tag: u16, payload: &[u8]) -> Result<(), CoreError> {
    if payload.len() > u16::MAX as usize {
        return Err(invalid(
            "native image subrecord exceeds its WORD reader limit",
        ));
    }
    output.extend_from_slice(&tag.to_le_bytes());
    output.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    output.extend_from_slice(payload);
    Ok(())
}

impl SavedProvider {
    pub(crate) fn decode(payload: &[u8]) -> Result<Self, CoreError> {
        let mut r = Reader::new(payload);
        let version = r.u32()?;
        if !(1..=3).contains(&version) {
            return Err(invalid("unsupported native texture-provider version"));
        }
        let mut load_flags = r.u32()?;
        let (frame, kind, composed, filter) = if version >= 2 {
            (r.u32()?, r.u32()?, r.u32()?, words(&mut r)?)
        } else {
            load_flags = u32::from(load_flags == 0);
            (0, 0, 0, [0; 4])
        };
        let dynamic_parameter = if version >= 3 { r.u32()? } else { 0 };
        let source = if composed != 0 {
            if r.u16()? != 1 {
                return Err(invalid("unsupported native composed-provider version"));
            }
            let count = r.u16()?;
            if count > 16 {
                return Err(invalid("native composed-provider layer count exceeds 16"));
            }
            let mut layers = BTreeMap::new();
            for _ in 0..count {
                let id = r.u16()?;
                if id >= 16
                    || layers
                        .insert(id, r.bytes(132)?.try_into().unwrap())
                        .is_some()
                {
                    return Err(invalid(
                        "native composed-provider layer id is invalid or duplicated",
                    ));
                }
            }
            SavedProviderSource::Layers(layers)
        } else {
            let resource = r.c_string(64)?.to_vec();
            let mask = r.c_string(32)?.to_vec();
            SavedProviderSource::File {
                resource,
                mask,
                archive: r.u32()?,
                width: r.u32()?,
                height: r.u32()?,
            }
        };
        r.finish()?;
        Ok(Self {
            load_flags,
            frame,
            kind,
            filter,
            dynamic_parameter,
            source,
        })
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        let mut out = Vec::new();
        put_words(
            &mut out,
            &[
                3,
                self.load_flags,
                self.frame,
                self.kind,
                u32::from(matches!(self.source, SavedProviderSource::Layers(_))),
            ],
        );
        put_words(&mut out, &self.filter);
        put_words(&mut out, &[self.dynamic_parameter]);
        match &self.source {
            SavedProviderSource::File {
                resource,
                mask,
                archive,
                width,
                height,
            } => {
                if resource.len() >= 64
                    || mask.len() >= 32
                    || resource.contains(&0)
                    || mask.contains(&0)
                {
                    return Err(invalid(
                        "native provider resource or mask exceeds its native buffer",
                    ));
                }
                out.extend_from_slice(resource);
                out.push(0);
                out.extend_from_slice(mask);
                out.push(0);
                put_words(&mut out, &[*archive, *width, *height]);
            }
            SavedProviderSource::Layers(layers) => {
                if layers.len() > 16 || layers.keys().any(|id| *id >= 16) {
                    return Err(invalid("native provider layer id exceeds 15"));
                }
                out.extend_from_slice(&1_u16.to_le_bytes());
                out.extend_from_slice(&(layers.len() as u16).to_le_bytes());
                for (id, layer) in layers {
                    out.extend_from_slice(&id.to_le_bytes());
                    out.extend_from_slice(layer);
                }
            }
        }
        Ok(out)
    }
}

impl SavedBasic {
    /// 42E260 restores these owners by memcpy, including the running clock.
    pub(crate) fn restore_effects(&self) -> Result<crate::vm::CmvsBasicEffects, CoreError> {
        use crate::vm::image_effects::{CmvsFloatMotion, CmvsFloatSegment};
        use crate::vm::{
            CmvsAlphaEffect, CmvsAlphaSegment, CmvsBasicEffects, CmvsSpriteEffect, CmvsSpriteFrame,
        };
        let mut effects = CmvsBasicEffects::default();
        for (&tag, bytes) in &self.effects {
            let mut r = Reader::new(bytes);
            match tag {
                1938 => {
                    if r.u16()? != 1 {
                        return Err(invalid("unsupported native alpha-effect version"));
                    }
                    let mut effect = CmvsAlphaEffect {
                        active: r.u32()?,
                        origin: r.u32()?,
                        start: r.u32()?,
                        current: r.u32()?,
                        elapsed_ms: r.u32()?,
                        ..Default::default()
                    };
                    for segment in &mut effect.segments {
                        *segment = CmvsAlphaSegment {
                            next: r.u32()? as i32,
                            target: r.u32()?,
                            step: r.u32()?,
                            duration_ms: r.u32()?,
                        };
                    }
                    effects.alpha = Some(effect);
                }
                1939 => {
                    let version = r.u16()?;
                    if !(1..=2).contains(&version) {
                        return Err(invalid("unsupported native sprite-effect version"));
                    }
                    let mut effect = CmvsSpriteEffect {
                        active: r.u32()?,
                        start: r.u32()?,
                        current: r.u32()?,
                        elapsed_ms: r.u32()?,
                        ..Default::default()
                    };
                    let count = if version == 1 { 32 } else { 64 };
                    for index in 0..count {
                        let next = r.u32()? as i32;
                        let source = [
                            r.u16()? as i16,
                            r.u16()? as i16,
                            r.u16()? as i16,
                            r.u16()? as i16,
                        ];
                        let pivot = [r.u16()? as i16, r.u16()? as i16];
                        effect.frames[index / 32][index % 32] = CmvsSpriteFrame {
                            next,
                            source,
                            pivot,
                            duration_ms: r.u32()?,
                        };
                    }
                    effects.sprite = Some(effect);
                }
                1941 => {
                    if r.u16()? != 1 {
                        return Err(invalid("unsupported native float-motion version"));
                    }
                    let mut effect = CmvsFloatMotion {
                        active: r.u32()?,
                        origin: [r.u32()?, r.u32()?, r.u32()?],
                        start: r.u32()?,
                        current: r.u32()?,
                        elapsed_ms: r.u32()?,
                        ..Default::default()
                    };
                    for segment in &mut effect.segments {
                        *segment = CmvsFloatSegment {
                            next: r.u32()? as i32,
                            target: [r.u32()?, r.u32()?, r.u32()?],
                            curve: r.u32()?,
                            duration_ms: r.u32()?,
                        };
                    }
                    effects.motion = Some(effect);
                }
                _ => return Err(invalid("native basic effect family is not modeled")),
            }
            r.finish()?;
        }
        effects.validate()?;
        Ok(effects)
    }

    pub(crate) fn capture_effects(
        &mut self,
        effects: &crate::vm::CmvsBasicEffects,
    ) -> Result<(), CoreError> {
        effects.validate()?;
        self.effects.clear();
        if let Some(effect) = &effects.alpha {
            let mut bytes = Vec::with_capacity(150);
            bytes.extend_from_slice(&1_u16.to_le_bytes());
            put_words(
                &mut bytes,
                &[
                    effect.active,
                    effect.origin,
                    effect.start,
                    effect.current,
                    effect.elapsed_ms,
                ],
            );
            for segment in &effect.segments {
                put_words(
                    &mut bytes,
                    &[
                        segment.next as u32,
                        segment.target,
                        segment.step,
                        segment.duration_ms,
                    ],
                );
            }
            self.effects.insert(1938, bytes);
        }
        if let Some(effect) = &effects.sprite {
            let mut bytes = Vec::with_capacity(1298);
            bytes.extend_from_slice(&2_u16.to_le_bytes());
            put_words(
                &mut bytes,
                &[
                    effect.active,
                    effect.start,
                    effect.current,
                    effect.elapsed_ms,
                ],
            );
            for frame in effect.frames.iter().flatten() {
                put_words(&mut bytes, &[frame.next as u32]);
                for word in frame.source.iter().chain(&frame.pivot) {
                    bytes.extend_from_slice(&word.to_le_bytes());
                }
                put_words(&mut bytes, &[frame.duration_ms]);
            }
            self.effects.insert(1939, bytes);
        }
        if let Some(effect) = &effects.motion {
            let mut bytes = Vec::with_capacity(222);
            bytes.extend_from_slice(&1_u16.to_le_bytes());
            put_words(&mut bytes, &[effect.active]);
            put_words(&mut bytes, &effect.origin);
            put_words(
                &mut bytes,
                &[effect.start, effect.current, effect.elapsed_ms],
            );
            for segment in &effect.segments {
                put_words(&mut bytes, &[segment.next as u32]);
                put_words(&mut bytes, &segment.target);
                put_words(&mut bytes, &[segment.curve, segment.duration_ms]);
            }
            self.effects.insert(1941, bytes);
        }
        Ok(())
    }

    pub(crate) fn decode(payload: &[u8]) -> Result<Self, CoreError> {
        let mut r = Reader::new(payload);
        let version = r.u32()?;
        if !(1..=4).contains(&version) {
            return Err(invalid("unsupported native basic-surface version"));
        }
        let mut value = Self::default();
        for word in &mut value.words[..22] {
            *word = r.u32()?;
        }
        if version >= 2 {
            for word in &mut value.words[22..26] {
                *word = r.u32()?;
            }
        }
        if version >= 3 {
            value.words[26] = r.u32()?;
        }
        if version >= 4 {
            value.words[27] = r.u32()?;
        }
        for tag in 1936..=1941 {
            if let Some(bytes) = record(&mut r, tag)? {
                value.effects.insert(tag, bytes.to_vec());
            }
        }
        if r.u16()? != 30601 {
            return Err(invalid("native basic-surface sentinel is missing"));
        }
        r.finish()?;
        Ok(value)
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        let mut out = Vec::new();
        put_words(&mut out, &[4]);
        put_words(&mut out, &self.words);
        for (tag, payload) in &self.effects {
            if !(1936..=1941).contains(tag) {
                return Err(invalid("unknown native basic effect tag"));
            }
            put_record(&mut out, *tag, payload)?;
        }
        out.extend_from_slice(&30601_u16.to_le_bytes());
        Ok(out)
    }
}

impl SavedImage {
    pub(crate) fn decode(payload: &[u8]) -> Result<Self, CoreError> {
        let mut nodes = 0;
        Self::decode_tree(payload, 0, &mut nodes)
    }
    fn decode_tree(payload: &[u8], depth: usize, nodes: &mut usize) -> Result<Self, CoreError> {
        *nodes += 1;
        if depth >= 32 || *nodes > 4096 {
            return Err(invalid(
                "native image tree exceeds its recursion or object limit",
            ));
        }
        let mut r = Reader::new(payload);
        let version = if next_tag(r)? == 0xffff {
            r.u16()?;
            r.u16()?
        } else {
            0
        };
        if version > 4 {
            return Err(invalid("unsupported native image version"));
        }
        let mut value = Self::default();
        value.header[..3].copy_from_slice(&words::<3>(&mut r)?);
        if version > 0 && version < 4 {
            r.bytes(12)?;
        }
        if version >= 3 {
            value.header[3..].copy_from_slice(&words::<2>(&mut r)?);
        }
        if let Some(bytes) = record(&mut r, 1856)? {
            value.primary = Some(SavedProvider::decode(bytes)?);
        }
        if let Some(bytes) = record(&mut r, 1857)? {
            value.secondary = Some(SavedProvider::decode(bytes)?);
        }
        if let Some(bytes) = record(&mut r, 1861)? {
            if !bytes.is_empty() {
                return Err(invalid("native image object1861 serializer must be empty"));
            }
            value.unsupported_object_1861 = true;
        }
        if let Some(bytes) = record(&mut r, 1865)? {
            let mut geometry_reader = Reader::new(bytes);
            let kind = geometry_reader.u32()?;
            let count = geometry_reader.u32()? as usize;
            if count > 128 || bytes.len() != 8 + count * 12 {
                return Err(invalid(
                    "native collision geometry count or length is invalid",
                ));
            }
            let mut vertices = Vec::with_capacity(count);
            for _ in 0..count {
                vertices.push(words(&mut geometry_reader)?);
            }
            geometry_reader.finish()?;
            value.collision_geometry = Some(CmvsNativeCollisionGeometry { kind, vertices });
        }
        if let Some(bytes) = record(&mut r, 1920)? {
            value.basic = Some(SavedBasic::decode(bytes)?);
        }
        if version >= 2 {
            value.effect_words = words(&mut r)?;
        }
        while next_tag(r)? == 1792 {
            if value.header[2] as i32 >= 0 {
                return Err(invalid(
                    "native image child records require the container flag",
                ));
            }
            r.u16()?;
            let id = r.u16()?;
            let length = r.u32()? as usize;
            if id >= 1024 || length > u16::MAX as usize || value.children.contains_key(&id) {
                return Err(invalid(
                    "native image child id or length is invalid or duplicated",
                ));
            }
            let child = Self::decode_tree(r.bytes(length)?, depth + 1, nodes)?;
            value.children.insert(id, child);
        }
        if r.u16()? != 0xf0f0 {
            return Err(invalid("native image sentinel is missing"));
        }
        r.finish()?;
        Ok(value)
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        let mut nodes = 0;
        self.encode_tree(0, &mut nodes)
    }
    fn encode_tree(&self, depth: usize, nodes: &mut usize) -> Result<Vec<u8>, CoreError> {
        *nodes += 1;
        if depth >= 32 || *nodes > 4096 {
            return Err(invalid(
                "native image tree exceeds its recursion or object limit",
            ));
        }
        let mut out = Vec::new();
        out.extend_from_slice(&0xffff_u16.to_le_bytes());
        out.extend_from_slice(&4_u16.to_le_bytes());
        put_words(&mut out, &self.header);
        if let Some(provider) = &self.primary {
            put_record(&mut out, 1856, &provider.encode()?)?;
        }
        if let Some(provider) = &self.secondary {
            put_record(&mut out, 1857, &provider.encode()?)?;
        }
        if self.unsupported_object_1861 {
            put_record(&mut out, 1861, &[])?;
        }
        if let Some(geometry) = &self.collision_geometry {
            geometry.validate()?;
            let mut payload = Vec::with_capacity(8 + geometry.vertices.len() * 12);
            put_words(
                &mut payload,
                &[geometry.kind, geometry.vertices.len() as u32],
            );
            for vertex in &geometry.vertices {
                put_words(&mut payload, vertex);
            }
            put_record(&mut out, 1865, &payload)?;
        }
        if let Some(basic) = &self.basic {
            put_record(&mut out, 1920, &basic.encode()?)?;
        }
        put_words(&mut out, &self.effect_words);
        if !self.children.is_empty() && self.header[2] as i32 >= 0 {
            return Err(invalid(
                "native image without container flag cannot serialize children",
            ));
        }
        for (id, child) in &self.children {
            if *id >= 1024 {
                return Err(invalid("native image child id exceeds 1023"));
            }
            let payload = child.encode_tree(depth + 1, nodes)?;
            out.extend_from_slice(&1792_u16.to_le_bytes());
            out.extend_from_slice(&id.to_le_bytes());
            if payload.len() > u16::MAX as usize {
                return Err(invalid("native child image exceeds its WORD reader limit"));
            }
            put_words(&mut out, &[payload.len() as u32]);
            out.extend_from_slice(&payload);
        }
        out.extend_from_slice(&0xf0f0_u16.to_le_bytes());
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_constructor_fixture_has_exact_header_and_sentinel() {
        // 445160: FFFF/version4, five raw header DWORDs, eleven effect DWORDs, F0F0.
        let mut native = vec![0xff, 0xff, 4, 0];
        for word in [0_u32, 1, 0x7fff_ffff, 0, 0].into_iter().chain([0; 11]) {
            native.extend_from_slice(&word.to_le_bytes());
        }
        native.extend_from_slice(&[0xf0, 0xf0]);
        assert_eq!(SavedImage::decode(&native).unwrap(), SavedImage::default());
        assert_eq!(SavedImage::default().encode().unwrap(), native);
        native.push(0);
        assert!(SavedImage::decode(&native).is_err());
    }
    #[test]
    fn native_provider_fixture_preserves_filter_and_dynamic_kind() {
        let provider = SavedProvider {
            load_flags: 1,
            frame: 0,
            kind: 3,
            filter: [1, 2, 0x123456, 0x10001],
            dynamic_parameter: 1,
            source: SavedProviderSource::File {
                resource: vec![],
                mask: vec![],
                archive: 0,
                width: 640,
                height: 480,
            },
        };
        let bytes = provider.encode().unwrap();
        assert_eq!(
            &bytes[..20],
            &[3, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(bytes.len(), 54);
        assert_eq!(SavedProvider::decode(&bytes).unwrap(), provider);
    }
    #[test]
    fn recursive_children_and_basic_words_are_not_affine_converted() {
        let mut root = SavedImage::default();
        root.header[2] |= 0x8000_0000;
        let mut child = SavedImage::default();
        let mut basic = SavedBasic::default();
        basic.words[2..14].copy_from_slice(&[
            3, 4, 91, 53, 7, 11, 101, 103, 19, 0x3f800001, 0x80000000, 0x7fc00001,
        ]);
        basic.words[17] = (-7_i32) as u32;
        child.basic = Some(basic);
        root.children.insert(1023, child);
        assert_eq!(SavedImage::decode(&root.encode().unwrap()).unwrap(), root);
        root.children.insert(1024, SavedImage::default());
        assert!(root.encode().is_err());
    }

    #[test]
    fn native_effect_owner_layouts_preserve_running_and_disabled_storage() {
        use crate::vm::{
            CmvsAlphaEffect, CmvsAlphaSegment, CmvsBasicEffects, CmvsSpriteEffect, CmvsSpriteFrame,
        };
        let mut alpha = CmvsAlphaEffect {
            active: 1,
            elapsed_ms: 625,
            ..Default::default()
        };
        alpha.segments[0] = CmvsAlphaSegment {
            next: 1,
            target: 255,
            step: 0,
            duration_ms: 750,
        };
        alpha.segments[1] = CmvsAlphaSegment {
            next: 2,
            target: 255,
            step: 0,
            duration_ms: 1000,
        };
        alpha.segments[2] = CmvsAlphaSegment {
            next: -2,
            target: 0,
            step: 0,
            duration_ms: 750,
        };
        let mut sprite = CmvsSpriteEffect {
            active: 1,
            current: 25,
            elapsed_ms: 81,
            ..Default::default()
        };
        for index in 0..32 {
            sprite.frames[0][index] = CmvsSpriteFrame {
                next: ((index + 1) % 32) as i32,
                source: [(index * 24) as i16, 690, 24, 24],
                pivot: [0, 0],
                duration_ms: 100,
            };
        }
        let effects = CmvsBasicEffects {
            alpha: Some(alpha),
            sprite: Some(sprite),
            ..Default::default()
        };
        let mut basic = SavedBasic::default();
        basic.capture_effects(&effects).unwrap();
        assert_eq!(basic.effects[&1938].len(), 150);
        assert_eq!(basic.effects[&1939].len(), 1298);
        assert_eq!(
            &basic.effects[&1938][..22],
            &[
                1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 113, 2, 0, 0
            ]
        );
        assert_eq!(
            SavedBasic::decode(&basic.encode().unwrap())
                .unwrap()
                .restore_effects()
                .unwrap(),
            effects
        );
        let mut disabled = effects;
        let sprite = disabled.sprite.as_mut().unwrap();
        sprite.active = 0;
        sprite.current = u32::MAX;
        sprite.frames[1][31].next = i32::MIN;
        basic.capture_effects(&disabled).unwrap();
        assert_eq!(basic.restore_effects().unwrap(), disabled);
        basic.effects.insert(1936, vec![0; 4]);
        assert!(basic.restore_effects().is_err());
    }

    #[test]
    fn native_sprite_v1_restores_first_32_frames_and_zeroes_new_capacity() {
        let mut bytes = vec![0; 658];
        bytes[..2].copy_from_slice(&1_u16.to_le_bytes());
        bytes[6..10].copy_from_slice(&u32::MAX.to_le_bytes());
        let mut basic = SavedBasic::default();
        basic.effects.insert(1939, bytes);
        let sprite = basic.restore_effects().unwrap().sprite.unwrap();
        assert_eq!(sprite.start, u32::MAX);
        assert_eq!(
            sprite.frames[1],
            [crate::vm::CmvsSpriteFrame::default(); 32]
        );
    }

    #[test]
    fn native1865_typed_wire_preserves_kind_xyz_bits_and_child_owner() {
        let geometry = CmvsNativeCollisionGeometry {
            kind: 0x87654321,
            vertices: vec![
                [1.25_f32.to_bits(), 0x80000000, 0x7fc00001],
                [0, 1, u32::MAX],
            ],
        };
        let mut image = SavedImage {
            collision_geometry: Some(geometry.clone()),
            ..Default::default()
        };
        image.header[2] |= 0x80000000;
        image.children.insert(
            1023,
            SavedImage {
                collision_geometry: Some(geometry),
                ..Default::default()
            },
        );
        let bytes = image.encode().unwrap();
        assert_eq!(&bytes[24..30], &[0x49, 0x07, 32, 0, 0, 0]);
        assert_eq!(&bytes[30..38], &[0x21, 0x43, 0x65, 0x87, 2, 0, 0, 0]);
        assert_eq!(SavedImage::decode(&bytes).unwrap(), image);
    }

    #[test]
    fn native1865_empty_owner_is_distinct_from_absent_and_caps_at128() {
        let absent = SavedImage::default();
        let mut present = absent.clone();
        present.collision_geometry = Some(Default::default());
        assert_ne!(absent.encode().unwrap(), present.encode().unwrap());
        assert_eq!(
            SavedImage::decode(&present.encode().unwrap()).unwrap(),
            present
        );
        present.collision_geometry.as_mut().unwrap().vertices = vec![[0; 3]; 128];
        assert_eq!(
            SavedImage::decode(&present.encode().unwrap()).unwrap(),
            present
        );
        present
            .collision_geometry
            .as_mut()
            .unwrap()
            .vertices
            .push([0; 3]);
        assert!(present.encode().is_err());
    }

    #[test]
    fn malformed1865_count_length_and_truncation_leave_source_owner_unchanged() {
        let image = SavedImage {
            collision_geometry: Some(CmvsNativeCollisionGeometry {
                kind: 0,
                vertices: vec![[1, 2, 3]],
            }),
            ..Default::default()
        };
        let before = image.clone();
        let wire = image.encode().unwrap();
        for count in [0_u32, 2, 129, u32::MAX] {
            let mut corrupt = wire.clone();
            corrupt[34..38].copy_from_slice(&count.to_le_bytes());
            assert!(SavedImage::decode(&corrupt).is_err());
            assert_eq!(image, before);
        }
        for length in [0_u32, 7, 8, 19, 21] {
            let mut corrupt = wire.clone();
            corrupt[26..30].copy_from_slice(&length.to_le_bytes());
            assert!(SavedImage::decode(&corrupt).is_err());
        }
        for length in 24..50 {
            assert!(SavedImage::decode(&wire[..length]).is_err());
        }
        assert_eq!(SavedImage::decode(&wire).unwrap(), before);
    }

    #[test]
    fn native1861_is_explicit_empty_unsupported_owner_not_geometry() {
        let image = SavedImage {
            unsupported_object_1861: true,
            collision_geometry: Some(Default::default()),
            ..Default::default()
        };
        let wire = image.encode().unwrap();
        assert_eq!(&wire[24..30], &[0x45, 0x07, 0, 0, 0, 0]);
        assert_eq!(SavedImage::decode(&wire).unwrap(), image);
        let mut corrupt = wire;
        corrupt[26..30].copy_from_slice(&1_u32.to_le_bytes());
        corrupt.insert(30, 0);
        assert!(SavedImage::decode(&corrupt).is_err());
    }
}
