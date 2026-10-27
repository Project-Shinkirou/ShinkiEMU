//! Native 4712F0 mixed-width state records and 4774D0 CSV2 envelope.
use super::lzss::{compress, decompress};
use super::slot_globals::{NativeBgmSources, NativeGlobals};
use super::slot_io::{Reader, invalid};
use super::slot_projection::SavedProjection;
use super::{
    slot_channel::{SavedChannel, SavedText},
    slot_cmv::SavedCmv,
    slot_image::SavedImage,
};
use crate::vm::native_scene_worker::SavedNativeSceneWorker;
use shinki_emu_sdk::CoreError;
use std::collections::{BTreeMap, BTreeSet};

const HEADER_BYTES: usize = 600;
const MAX_STATE_BYTES: usize = 3 * 1024 * 1024;
const MAX_PROGRAM_BYTES: usize = 64 * 1024 * 1024;
const KEY: &[u8] = b"DAME_SAVE_IMAGE";
const SCALAR_TAGS: [u16; 14] = [
    274, 275, 276, 277, 262, 263, 264, 266, 512, 513, 514, 515, 576, 577,
];

/// Explicit interpreter regions written by 4712F0. No runtime pointer is stored.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeInterpreter {
    pub strings: [Vec<u8>; 6],
    pub scalars: BTreeMap<u16, u32>,
    pub suspend_words: [u32; 10],
    pub record_prefix: u32,
    pub records: Vec<[u32; 7]>,
    pub frame_bases: Vec<u32>,
    pub value_stack: Vec<u8>,
    pub text_state: [u32; 11],
    pub indexed_words: Vec<u32>,
    pub float_words: Vec<u32>,
    pub flags: Vec<u8>,
    pub global_strings: Vec<Vec<u8>>,
    pub script_data: Vec<u8>,
    pub globals: NativeGlobals,
    pub projection: SavedProjection,
    pub message_history: Option<crate::vm::message_history::MessageHistory>,
}

impl Default for NativeInterpreter {
    fn default() -> Self {
        Self {
            strings: std::array::from_fn(|_| Vec::new()),
            scalars: SCALAR_TAGS.into_iter().map(|tag| (tag, 0)).collect(),
            suspend_words: [0; 10],
            record_prefix: 0,
            records: vec![[0, 0, u32::MAX, 0, 0, 0, 0]; 64],
            frame_bases: vec![0; 64],
            value_stack: vec![0; 65536],
            text_state: [0; 11],
            indexed_words: vec![0; 2048],
            float_words: vec![0; 1024],
            flags: vec![0; 256],
            global_strings: vec![Vec::new(); 64],
            script_data: Vec::new(),
            globals: NativeGlobals::default(),
            projection: SavedProjection::default(),
            message_history: None,
        }
    }
}

impl NativeInterpreter {
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if self.scalars.len() != SCALAR_TAGS.len()
            || SCALAR_TAGS
                .iter()
                .any(|tag| !self.scalars.contains_key(tag))
            || self.records.len() != 64
            || self.frame_bases.len() != 64
            || self.value_stack.len() != 65536
            || self.indexed_words.len() != 2048
            || self.float_words.len() != 1024
            || self.flags.len() != 256
            || self.global_strings.len() != 64
            || self.script_data.len() > MAX_PROGRAM_BYTES
        {
            return Err(invalid("native interpreter regions have invalid sizes"));
        }
        for (i, bytes) in self.strings.iter().enumerate() {
            validate_string(bytes, if i < 2 { 256 } else { 32 })?;
        }
        for bytes in &self.global_strings {
            validate_string(bytes, 1024)?;
        }
        self.globals.validate()?;
        self.projection.validate()?;
        let cursor = self.scalars[&576];
        let depth = self.scalars[&577] as usize;
        if cursor > 65536
            || depth >= 64
            || self.frame_bases[..=depth].windows(2).any(|v| v[0] > v[1])
            || self.frame_bases[depth] > cursor
            || self.scalars[&262] > 3
            || !self.scalars[&263].is_multiple_of(2)
        {
            return Err(invalid(
                "native interpreter cursor, depth or frame is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeSlot {
    pub header: Vec<u8>,
    pub interpreter: NativeInterpreter,
    pub program_image: Vec<u8>,
    pub preview_bmp: Vec<u8>,
    pub images: BTreeMap<u16, SavedImage>,
    pub channels: BTreeMap<u16, SavedChannel>,
    pub text: BTreeMap<u16, SavedText>,
    pub cmv: BTreeMap<u16, SavedCmv>,
    pub scene_workers: BTreeMap<u16, SavedNativeSceneWorker>,
}

impl NativeSlot {
    pub(crate) fn new(
        vm: &crate::CmvsPs2aVmState,
        interpreter: NativeInterpreter,
        program_image: Vec<u8>,
    ) -> Result<Self, CoreError> {
        crate::vm::native_slot::native_program_header(&program_image)?;
        let header = crate::vm::native_slot::capture_native_slot_header(vm)?;
        Ok(Self {
            header,
            interpreter,
            program_image,
            preview_bmp: Vec::new(),
            images: BTreeMap::new(),
            channels: BTreeMap::new(),
            text: BTreeMap::new(),
            cmv: BTreeMap::new(),
            scene_workers: BTreeMap::new(),
        })
    }
}

fn validate_string(bytes: &[u8], capacity: usize) -> Result<(), CoreError> {
    if bytes.len() >= capacity || bytes.contains(&0) {
        return Err(invalid("native string exceeds its field or contains NUL"));
    }
    let (_, malformed) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes);
    if malformed {
        return Err(invalid("native string is invalid Shift-JIS"));
    }
    Ok(())
}

fn short_payload<'a>(r: &mut Reader<'a>, expected: usize) -> Result<&'a [u8], CoreError> {
    if r.u16()? as usize != expected {
        return Err(invalid("native short record has an invalid length"));
    }
    r.bytes(expected)
}
fn long_payload<'a>(r: &mut Reader<'a>, maximum: usize) -> Result<&'a [u8], CoreError> {
    let length = r.u32()? as usize;
    if length > maximum {
        return Err(invalid("native record exceeds its byte limit"));
    }
    r.bytes(length)
}
fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect()
}
fn read_string(r: &mut Reader<'_>, capacity: usize) -> Result<Vec<u8>, CoreError> {
    let length = r.u16()? as usize;
    if length == 0 || length > capacity {
        return Err(invalid("native counted string has an invalid length"));
    }
    let bytes = r.bytes(length)?;
    if bytes.last() != Some(&0) {
        return Err(invalid("native counted string is unterminated"));
    }
    validate_string(&bytes[..length - 1], capacity)?;
    Ok(bytes[..length - 1].to_vec())
}
fn write_short(out: &mut Vec<u8>, tag: u16, bytes: &[u8]) {
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(bytes);
}
fn write_long(out: &mut Vec<u8>, tag: u16, bytes: &[u8]) {
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}
fn word_bytes(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}
fn write_string(out: &mut Vec<u8>, tag: u16, bytes: &[u8]) {
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&((bytes.len() + 1) as u16).to_le_bytes());
    out.extend_from_slice(bytes);
    out.push(0);
}

fn decode_state(bytes: &[u8], slot: &mut NativeSlot) -> Result<(), CoreError> {
    let mut r = Reader::new(bytes);
    let mut seen = BTreeSet::new();
    while r.remaining() != 0 {
        let tag = r.u16()?;
        seen.insert(tag);
        let state = &mut slot.interpreter;
        match tag {
            256..=261 => {
                state.strings[(tag - 256) as usize] =
                    read_string(&mut r, if tag < 258 { 256 } else { 32 })?
            }
            262..=264 | 266 | 274..=277 | 512..=515 | 576..=577 => {
                let value = u32::from_le_bytes(short_payload(&mut r, 4)?.try_into().unwrap());
                state.scalars.insert(tag, value);
            }
            265 => state
                .suspend_words
                .copy_from_slice(&words(short_payload(&mut r, 40)?)),
            267 => {
                let values = words(short_payload(&mut r, 1796)?);
                state.record_prefix = values[0];
                state.records = values[1..].as_chunks::<7>().0.to_vec();
            }
            578 => {
                let payload = long_payload(&mut r, 256)?;
                if payload.len() != 256 {
                    return Err(invalid("native frame-base table must be 256 bytes"));
                }
                state.frame_bases = words(payload);
            }
            579 => {
                let payload = long_payload(&mut r, 65536)?;
                if payload.len() != 65536 {
                    return Err(invalid("native value stack must be 65536 bytes"));
                }
                state.value_stack = payload.to_vec();
            }
            584 => state
                .text_state
                .copy_from_slice(&words(short_payload(&mut r, 44)?)),
            640..=642 => {
                let expected = match tag {
                    640 => 8192,
                    641 => 4096,
                    _ => 256,
                };
                let payload = long_payload(&mut r, expected)?;
                if payload.len() != expected {
                    return Err(invalid("native process-global table has an invalid length"));
                }
                match tag {
                    640 => state.indexed_words = words(payload),
                    641 => state.float_words = words(payload),
                    _ => state.flags = payload.to_vec(),
                }
            }
            643 => {
                let payload = long_payload(&mut r, 65536)?;
                let mut strings = Reader::new(payload);
                state.global_strings.clear();
                for _ in 0..64 {
                    let bytes = strings.c_string(1024)?;
                    validate_string(bytes, 1024)?;
                    state.global_strings.push(bytes.to_vec());
                }
                strings.finish()?;
            }
            2304 => state.script_data = long_payload(&mut r, MAX_PROGRAM_BYTES)?.to_vec(),
            896 | 1024 | 1792 | 2048 | 2280 => {
                let index = r.u16()?;
                let limit = match tag {
                    896 => 12,
                    1024 => 8,
                    1792 => 256,
                    2048 => 4,
                    _ => 3,
                };
                if usize::from(index) >= limit {
                    return Err(invalid("native object index exceeds its table"));
                }
                let payload = long_payload(&mut r, MAX_STATE_BYTES)?;
                let duplicate = match tag {
                    896 => slot
                        .text
                        .insert(index, SavedText::decode(payload)?)
                        .is_some(),
                    1024 => slot
                        .channels
                        .insert(index, SavedChannel::decode(payload)?)
                        .is_some(),
                    1792 => slot
                        .images
                        .insert(index, SavedImage::decode(payload)?)
                        .is_some(),
                    2048 => slot
                        .scene_workers
                        .insert(index, SavedNativeSceneWorker::decode(payload)?)
                        .is_some(),
                    _ => slot.cmv.insert(index, SavedCmv::decode(payload)?).is_some(),
                };
                if duplicate {
                    return Err(invalid("duplicate native object index"));
                }
            }
            1344 => {
                state.message_history = Some(crate::vm::message_history::MessageHistory::decode(
                    long_payload(&mut r, MAX_STATE_BYTES)?,
                )?)
            }
            2240 | 2248 | 1536 | 1552 | 1568 | 1584 | 1696 => {
                return Err(invalid(
                    "native slot contains an unsupported auxiliary object family",
                ));
            }
            1760 | 784 | 792 => state
                .globals
                .decode_record(tag, long_payload(&mut r, MAX_STATE_BYTES)?)?,
            772 => state.globals.bgm = NativeBgmSources::decode_fields(&mut r)?,
            2176..=2179 => state
                .projection
                .decode_record(tag, long_payload(&mut r, MAX_STATE_BYTES)?)?,
            768 => {
                return Err(invalid(
                    "native slot contains an unsupported legacy BGM record",
                ));
            }
            _ => return Err(invalid("native slot contains an unknown record tag")),
        }
    }
    for tag in [
        256, 257, 258, 259, 260, 261, 262, 263, 264, 265, 266, 267, 274, 275, 276, 277, 512, 513,
        514, 515, 576, 577, 578, 579, 584, 640, 641, 642, 643, 1760, 2176, 2177, 2178, 2179, 2304,
        772, 784, 792,
    ] {
        if !seen.contains(&tag) {
            return Err(invalid("native interpreter is missing a required record"));
        }
    }
    slot.interpreter.validate()?;
    let program = crate::vm::native_slot::native_program_header(&slot.program_image)?;
    if program.metadata_size as usize != slot.interpreter.script_data.len()
        || slot.interpreter.scalars[&274] as usize != slot.program_image.len()
        || (slot.interpreter.scalars[&262] == 0
            && slot.interpreter.scalars[&263] >= program.bytecode_size)
    {
        return Err(invalid(
            "native interpreter does not match its saved program",
        ));
    }
    Ok(())
}

fn encode_state(slot: &NativeSlot) -> Result<Vec<u8>, CoreError> {
    let state = &slot.interpreter;
    state.validate()?;
    let mut out = Vec::with_capacity(84000);
    for (index, bytes) in state.strings.iter().enumerate() {
        write_string(&mut out, 256 + index as u16, bytes);
    }
    for tag in [274, 275, 276, 277, 262, 263, 264] {
        write_short(&mut out, tag, &state.scalars[&tag].to_le_bytes());
    }
    write_short(&mut out, 265, &word_bytes(&state.suspend_words));
    write_short(&mut out, 266, &state.scalars[&266].to_le_bytes());
    let mut records = Vec::with_capacity(1796);
    records.extend_from_slice(&state.record_prefix.to_le_bytes());
    for row in &state.records {
        for word in row {
            records.extend_from_slice(&word.to_le_bytes());
        }
    }
    write_short(&mut out, 267, &records);
    for tag in [512, 513, 514, 515, 576, 577] {
        write_short(&mut out, tag, &state.scalars[&tag].to_le_bytes());
    }
    write_long(&mut out, 578, &word_bytes(&state.frame_bases));
    write_long(&mut out, 579, &state.value_stack);
    write_short(&mut out, 584, &word_bytes(&state.text_state));
    write_long(&mut out, 640, &word_bytes(&state.indexed_words));
    write_long(&mut out, 641, &word_bytes(&state.float_words));
    write_long(&mut out, 642, &state.flags);
    out.extend_from_slice(&643u16.to_le_bytes());
    let count: usize = state.global_strings.iter().map(|s| s.len() + 1).sum();
    out.extend_from_slice(&(count as u32).to_le_bytes());
    for bytes in &state.global_strings {
        out.extend_from_slice(bytes);
        out.push(0);
    }
    macro_rules! family {
        ($tag:expr,$map:expr,$limit:expr) => {
            for (&index, value) in $map {
                if index >= $limit {
                    return Err(invalid("native object index exceeds its table"));
                }
                out.extend_from_slice(&$tag.to_le_bytes());
                out.extend_from_slice(&index.to_le_bytes());
                let payload = value.encode()?;
                out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                out.extend_from_slice(&payload);
            }
        };
    }
    family!(896u16, &slot.text, 12);
    family!(1024u16, &slot.channels, 8);
    family!(1792u16, &slot.images, 256);
    family!(2048u16, &slot.scene_workers, 4);
    family!(2280u16, &slot.cmv, 3);
    if let Some(history) = &state.message_history {
        write_long(&mut out, 1344, &history.encode()?);
    }
    write_long(&mut out, 1760, &state.globals.encode_record(1760)?);
    for tag in 2176..=2179 {
        write_long(&mut out, tag, &state.projection.encode_record(tag)?);
    }
    write_long(&mut out, 2304, &state.script_data);
    out.extend_from_slice(&772u16.to_le_bytes());
    out.extend_from_slice(&state.globals.bgm.encode_fields()?);
    write_long(&mut out, 784, &state.globals.encode_record(784)?);
    write_long(&mut out, 792, &state.globals.encode_record(792)?);
    if out.len() > MAX_STATE_BYTES {
        return Err(invalid("native state exceeds its allocation limit"));
    }
    Ok(out)
}

pub(crate) fn decode_native_slot(bytes: &[u8]) -> Result<NativeSlot, CoreError> {
    if bytes.len() < HEADER_BYTES + 2 || &bytes[..4] != b"CSV2" {
        return Err(invalid(
            "native slot header is truncated or has invalid magic",
        ));
    }
    let word = |offset| u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
    let dword = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    if dword(4) != HEADER_BYTES || word(544) != 4 || word(546) != 1 {
        return Err(invalid("unsupported native slot header version"));
    }
    let lengths = [dword(560), dword(564), dword(568)];
    let raw = [dword(580), dword(584), dword(588)];
    if raw[0] == 0
        || raw[0] > MAX_STATE_BYTES
        || raw[1] > MAX_PROGRAM_BYTES
        || raw[2] == 0
        || raw[2] > MAX_PROGRAM_BYTES
        || lengths[0] == 0
        || lengths[2] == 0
        || (lengths[1] == 0 && raw[1] != 0)
    {
        return Err(invalid("native slot section lengths are invalid"));
    }
    if lengths
        .iter()
        .zip(raw)
        .any(|(&packed, plain)| packed > plain + plain.div_ceil(8))
    {
        return Err(invalid(
            "native compressed section exceeds its encoding bound",
        ));
    }
    let end = lengths
        .iter()
        .try_fold(HEADER_BYTES, |n, v| n.checked_add(*v))
        .ok_or_else(|| invalid("native slot length overflowed"))?;
    if end.checked_add(2) != Some(bytes.len()) || dword(540) != end {
        return Err(invalid("native slot total length is invalid"));
    }
    let a_end = HEADER_BYTES + lengths[0];
    let b_end = a_end + lengths[1];
    let encrypted = &bytes[HEADER_BYTES..a_end];
    let checksum = encrypted
        .iter()
        .fold(0u16, |sum, byte| sum.wrapping_add(u16::from(*byte)));
    if checksum != word(end) {
        return Err(invalid("native slot encrypted-A checksum failed"));
    }
    let packed: Vec<u8> = encrypted
        .iter()
        .enumerate()
        .map(|(i, b)| (b ^ KEY[i % KEY.len()]).wrapping_sub(128))
        .collect();
    let state = decompress(&packed, raw[0])?;
    let preview_bmp = if lengths[1] == 0 {
        Vec::new()
    } else {
        decompress(&bytes[a_end..b_end], raw[1])?
    };
    if !preview_bmp.is_empty() {
        validate_preview(&preview_bmp)?;
    }
    let program_image = decompress(&bytes[b_end..end], raw[2])?;
    let mut slot = NativeSlot {
        header: bytes[..HEADER_BYTES].to_vec(),
        interpreter: NativeInterpreter::default(),
        program_image,
        preview_bmp,
        images: BTreeMap::new(),
        channels: BTreeMap::new(),
        text: BTreeMap::new(),
        cmv: BTreeMap::new(),
        scene_workers: BTreeMap::new(),
    };
    decode_state(&state, &mut slot)?;
    Ok(slot)
}

#[cfg(test)]
pub(crate) fn encode_native_slot(slot: &NativeSlot) -> Result<Vec<u8>, CoreError> {
    encode_native_slot_with_preview(slot, &slot.preview_bmp)
}

pub(crate) fn encode_native_slot_with_preview(
    slot: &NativeSlot,
    preview_bmp: &[u8],
) -> Result<Vec<u8>, CoreError> {
    if slot.header.len() != HEADER_BYTES || &slot.header[..4] != b"CSV2" {
        return Err(invalid("native slot header must contain 600 CSV2 bytes"));
    }
    if slot.header[544..548] != [4, 0, 1, 0] {
        return Err(invalid("unsupported native slot header version"));
    }
    let program = crate::vm::native_slot::native_program_header(&slot.program_image)?;
    if program.metadata_size as usize != slot.interpreter.script_data.len()
        || slot.interpreter.scalars.get(&274).copied() != Some(slot.program_image.len() as u32)
        || (slot.interpreter.scalars.get(&262) == Some(&0)
            && slot
                .interpreter
                .scalars
                .get(&263)
                .copied()
                .unwrap_or(u32::MAX)
                >= program.bytecode_size)
    {
        return Err(invalid(
            "native interpreter does not match its saved program",
        ));
    }
    let preview_bmp = if u16::from_le_bytes(slot.header[548..550].try_into().unwrap()) != 0 {
        preview_bmp
    } else {
        &[]
    };
    if !preview_bmp.is_empty() {
        validate_preview(preview_bmp)?;
    }
    let state = encode_state(slot)?;
    let mut a = compress(&state);
    for (i, byte) in a.iter_mut().enumerate() {
        *byte = byte.wrapping_add(128) ^ KEY[i % KEY.len()];
    }
    let b = compress(preview_bmp);
    let c = compress(&slot.program_image);
    let end = HEADER_BYTES + a.len() + b.len() + c.len();
    let mut out = Vec::with_capacity(end + 2);
    out.extend_from_slice(&slot.header);
    crate::vm::native_slot::refresh_native_slot_header_time(
        &mut out[..HEADER_BYTES],
        crate::vm::native_slot::native_save_local_timestamp()?,
    )?;
    out[4..8].copy_from_slice(&600u32.to_le_bytes());
    out[540..544].copy_from_slice(&(end as u32).to_le_bytes());
    out[544..546].copy_from_slice(&4u16.to_le_bytes());
    out[546..548].copy_from_slice(&1u16.to_le_bytes());
    out[548..550].copy_from_slice(&slot.header[548..550]);
    for (offset, length) in [
        (560, a.len()),
        (564, b.len()),
        (568, c.len()),
        (580, state.len()),
        (584, preview_bmp.len()),
        (588, slot.program_image.len()),
    ] {
        out[offset..offset + 4].copy_from_slice(&(length as u32).to_le_bytes());
    }
    let checksum = a
        .iter()
        .fold(0u16, |sum, byte| sum.wrapping_add(u16::from(*byte)));
    out.extend_from_slice(&a);
    out.extend_from_slice(&b);
    out.extend_from_slice(&c);
    out.extend_from_slice(&checksum.to_le_bytes());
    Ok(out)
}

fn validate_preview(bytes: &[u8]) -> Result<(), CoreError> {
    if bytes.len() < 54 || &bytes[..2] != b"BM" {
        return Err(invalid("native preview is not a BMP"));
    }
    let dword = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    let width = dword(18);
    let height = dword(22);
    let stride = width
        .checked_mul(3)
        .and_then(|v| v.checked_add(3))
        .map(|v| v & !3)
        .ok_or_else(|| invalid("native BMP dimensions overflowed"))?;
    let size = stride
        .checked_mul(height)
        .and_then(|v| v.checked_add(dword(10)))
        .ok_or_else(|| invalid("native BMP dimensions overflowed"))?;
    if dword(2) != bytes.len()
        || dword(10) < 54
        || dword(14) != 40
        || width == 0
        || height == 0
        || bytes[26..30] != [1, 0, 24, 0]
        || dword(30) != 0
        || size != bytes.len()
    {
        return Err(invalid("native BMP preview layout is invalid"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "slot_tests.rs"]
mod tests;

pub(crate) fn slot_file_name(slot: u32) -> String {
    format!("save{:03}.dat", slot % 1000)
}
