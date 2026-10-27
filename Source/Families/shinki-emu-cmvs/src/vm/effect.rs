//! Recovered CMVS effect-channel object model.
//!
//! The original engine stores one 0x9e4-byte channel with a separate
//! 0x3088-byte image surface. Quad records use a 76-byte (38-word) stride;
//! recovered handlers address it with `_WORD *` arithmetic. Modelling the
//! object as a sparse word-addressed image keeps every recovered read and
//! write at the original byte/word offset instead of inventing a new layout.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `462C10` copies the two CP932 kinsoku lists into text-object buffers
/// 324/388 and enables extended boundary handling (DWORD79).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsEffectTextSurface {
    /// Dword 320: the surface's stored value.
    pub value: u32,
    /// Owned native buffers; `None` is the constructor's disabled state.
    pub boundaries: Option<[Vec<u8>; 2]>,
}

/// One entry of the recovered effect playback table (`game[750]`, twelve
/// entries registered by case 349). Cases 276/277/278 write the fields the
/// original stores at record dwords 41/44/45.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsEffectPlaybackRecord {
    /// Record dword 41 (`sub_464AA0`, case 276).
    pub flag: u32,
    /// Record dword 44 (`sub_464A90`, case 277).
    pub value: u32,
    /// Record dword 45 (`sub_464A80`, case 278).
    pub value2: u32,
    /// The record's text surface (`sub_462C10`, case 286).
    pub text_surface: CmvsEffectTextSurface,
}

/// One child element of an effect channel's element container
/// (`channel[630]`, dereferenced through `sub_444EF0(container, id)`).
///
/// Cases 378/379/380 address one element by `(channel, child)`:
/// `sub_445EA0` writes the visibility dword, `sub_466AE0` answers the
/// existence query, and `sub_445E80` stores the element size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsEffectElement {
    /// Element dword 1035 (`sub_445EA0`, cases 402/403 and 378).
    pub visible: u32,
    /// Element width stored by `sub_445E80` (case 380).
    pub width: i32,
    /// Element height stored by `sub_445E80` (case 380).
    pub height: i32,
    /// `42E950` source XYWH; `None` means no allocated drawable basic.
    pub source: Option<[i32; 4]>,
    /// `sub_42E9B0`: local destination x/y.
    pub position: Option<[i32; 2]>,
    /// Header DWORD1036; `445010` follows the parent while bit1 is set.
    pub inheritance_flags: u32,
    /// Basic DWORD6/7: pivot, independent of atlas source coordinates.
    pub pivot: [i32; 2],
    /// Basic DWORD10, sorted descending within the channel's child list.
    pub order: i32,
    /// Basic DWORD17, signed upper-clamped alpha.
    pub alpha: i32,
    /// Basic DWORD1; native default is alpha-over (1).
    pub blend_mode: u8,
    /// Basic DWORD18/19: original IEEE754 scale words.
    pub scale_words: [u32; 2],
    pub basic_kind: u8,
    pub float_position_words: [u32; 3],
    pub projection_depth_word: u32,
    pub projection_height_word: u32,
    /// Native CBasic DWORD20, signed hundredths of a degree.
    pub rotation_hundredths: i32,
    pub image_resource: Option<String>,
    pub image_info: [u32; 3],
    pub(crate) image_reload: Option<Box<super::texture_reload::TextureReload>>,
    pub basic_effects: super::CmvsBasicEffects,
    pub image_effect_words: [u32; 11],
    pub collision_geometry: Option<super::CmvsNativeCollisionGeometry>,
}

impl Default for CmvsEffectElement {
    fn default() -> Self {
        Self {
            visible: 0,
            width: 0,
            height: 0,
            source: None,
            position: None,
            inheritance_flags: 0x7fff_ffff,
            pivot: [0; 2],
            order: 0,
            alpha: 255,
            blend_mode: 1,
            scale_words: [1.0_f32.to_bits(); 2],
            basic_kind: 0,
            float_position_words: [0; 3],
            projection_depth_word: 0,
            projection_height_word: 0,
            rotation_hundredths: 0,
            image_resource: None,
            image_info: [0; 3],
            image_reload: None,
            basic_effects: Default::default(),
            image_effect_words: [0; 11],
            collision_geometry: None,
        }
    }
}

impl CmvsEffectElement {
    /// `446900`/`4469A0` replace/free the basic without touching the
    /// element's visibility, reference dimensions, or inheritance header.
    pub(crate) fn reset_basic(&mut self, allocated: bool) {
        self.source = allocated.then_some([0; 4]);
        self.position = allocated.then_some([0; 2]);
        self.pivot = [0; 2];
        self.order = 0;
        self.alpha = 255;
        self.blend_mode = 1;
        self.scale_words = [1.0_f32.to_bits(); 2];
        self.basic_kind = 0;
        self.float_position_words = [0; 3];
        self.projection_depth_word = 0;
        self.projection_height_word = 0;
        self.rotation_hundredths = 0;
        self.basic_effects = Default::default();
    }
}

/// Native 0x9e4-byte channel, with a separate 0x3088-byte image surface.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsEffectChannel {
    pub words: BTreeMap<u32, u16>,
    pub image_resource: Option<String>,
    /// Source provider's bpp-derived alpha capability, not a pixel census.
    pub image_has_alpha: bool,
}

impl CmvsEffectChannel {
    /// Words per quad record: the original uses `this + 38 * quad` on a
    /// `_WORD *` (76 bytes per record).
    pub const QUAD_STRIDE_WORDS: u32 = 38;

    /// Reads the little-endian dword at a word index, like the original's
    /// `*(_DWORD *)(word_base + n)`.
    pub fn read_dword(&self, word: u32) -> u32 {
        let low = u32::from(self.words.get(&word).copied().unwrap_or(0));
        let high = u32::from(self.words.get(&(word + 1)).copied().unwrap_or(0));
        low | (high << 16)
    }

    /// Writes the little-endian dword at a word index.
    pub fn write_dword(&mut self, word: u32, value: u32) {
        self.words.insert(word, value as u16);
        self.words.insert(word + 1, (value >> 16) as u16);
    }

    pub fn read_i16(&self, word: u32) -> i16 {
        self.words.get(&word).copied().unwrap_or(0) as i16
    }

    pub fn write_i16(&mut self, word: u32, value: i16) {
        self.words.insert(word, value as u16);
    }

    /// `sub_47F270` (case 333) reads the channel object's dword at word 4.
    pub fn activity(&self) -> bool {
        self.read_dword(4) != 0
    }

    /// The channel origin stored as dwords at bytes 12/16 (`sub_4678C0`
    /// writes `*(this+3)`/`*(this+4)` on the dword view), i.e. words 6-9
    /// on the u16 view; `sub_466870`'s hit test adds it to every quad
    /// rectangle.
    pub fn origin(&self) -> (i32, i32) {
        (self.read_dword(6) as i32, self.read_dword(8) as i32)
    }

    /// `sub_4678C0` (case 336) writes the origin at bytes 12/16 and
    /// mirrors it into the recovered duplicate dword pair at bytes
    /// 2480/2484 (words 1240-1243).
    pub fn set_origin(&mut self, x: i32, y: i32) {
        self.write_dword(6, x as u32);
        self.write_dword(8, y as u32);
        self.write_dword(1240, x as u32);
        self.write_dword(1242, y as u32);
    }

    /// `4679B0`: mode at byte 28, coordinate shorts at bytes 30/32.
    pub fn set_playback_mode(&mut self, mode: u16, first: i16, second: i16) {
        self.words.insert(14, mode);
        match mode {
            0 | 2 => {
                self.write_dword(15, 0);
            }
            1 | 3 => {
                self.write_i16(15, first);
                self.write_i16(16, second);
            }
            _ => {}
        }
    }

    /// Native indicator mode at byte 28 (word 14).
    pub fn playback_mode(&self) -> u16 {
        self.words.get(&14).copied().unwrap_or(0)
    }

    /// `467CF0` (case 346): alternate origin at DWORD618/619, words1236/1238.
    /// `467C60` selects this pair while the enabled view predicate is true.
    pub fn set_value_pair(&mut self, first: u32, second: u32) {
        self.write_dword(1236, first);
        self.write_dword(1238, second);
    }

    /// `467CE0` (case 347): origin-swap enable at DWORD622, word1244.
    pub fn set_flag(&mut self, value: bool) {
        self.write_dword(1244, u32::from(value));
    }

    /// `sub_466D50`/`sub_466D90`/`sub_47F470` (cases 402/403/405) address the
    /// quad record's visibility dword at byte `76*quad + 40`, i.e. word
    /// `38*quad + 20`.
    fn quad_visibility_word(quad: u32) -> u32 {
        Self::QUAD_STRIDE_WORDS * quad + 20
    }

    pub fn quad_visible(&self, quad: u32) -> bool {
        self.read_dword(Self::quad_visibility_word(quad)) != 0
    }

    pub fn set_quad_visible(&mut self, quad: u32, visible: bool) {
        self.write_dword(Self::quad_visibility_word(quad), u32::from(visible));
    }

    /// `sub_466CB0` (case 400) writes six `__int16` geometry words at
    /// `38*quad + 6*sub + 22`.
    pub fn write_quad_geometry(&mut self, quad: u32, sub: u32, values: [i16; 6]) {
        let base = Self::QUAD_STRIDE_WORDS * quad + 6 * sub + 22;
        for (index, value) in values.iter().enumerate() {
            self.write_i16(base + index as u32, *value);
        }
    }

    /// `sub_466D10` (case 401) writes the quad's four `__int16` hit-rectangle
    /// words at `38*quad + 52`.
    pub fn write_quad_rect(&mut self, quad: u32, rect: [i16; 4]) {
        let base = Self::QUAD_STRIDE_WORDS * quad + 52;
        for (index, value) in rect.iter().enumerate() {
            self.write_i16(base + index as u32, *value);
        }
    }

    /// The quad's hit rectangle in the channel's local coordinates.
    pub fn quad_rect(&self, quad: u32) -> [i32; 4] {
        let base = Self::QUAD_STRIDE_WORDS * quad + 52;
        [
            i32::from(self.read_i16(base)),
            i32::from(self.read_i16(base + 1)),
            i32::from(self.read_i16(base + 2)),
            i32::from(self.read_i16(base + 3)),
        ]
    }

    /// `sub_47F330` adds the channel's 36-byte header before reading
    /// byte `76*quad + 76`: the selected frame is at word `38*quad + 56`.
    pub fn quad_frame(&self, quad: u32) -> u16 {
        self.words
            .get(&(Self::QUAD_STRIDE_WORDS * quad + 56))
            .copied()
            .unwrap_or(0)
    }

    pub fn set_quad_frame(&mut self, quad: u32, frame: u16) {
        self.words
            .insert(Self::QUAD_STRIDE_WORDS * quad + 56, frame);
    }

    /// The source x/y/width/height at word `22 + 38*quad + 6*frame`
    /// (`sub_47F330`), matching the first four words written by case 400.
    pub fn quad_frame_geometry(&self, quad: u32, frame: u16) -> [i16; 4] {
        let base = 18 + Self::QUAD_STRIDE_WORDS * quad + 6 * u32::from(frame);
        [
            self.read_i16(base + 4),
            self.read_i16(base + 5),
            self.read_i16(base + 6),
            self.read_i16(base + 7),
        ]
    }
}
