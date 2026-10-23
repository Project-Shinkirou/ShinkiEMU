//! CMVS system save (`system.dat`) container support.
//!
//! The container layout, checksum, cipher and decompression are recovered
//! from the CMVS 3.90 loader `sub_427B30` and its helpers.  The cipher key
//! string is a fixed engine constant, not case-private material.  Parsed
//! tables and CP932 string bytes are decoded into a temporary load candidate.

use shinki_emu_sdk::CoreError;

/// The fixed engine key string proven by `sub_427B30`/`sub_427E80`.
const SYSTEM_SAVE_KEY: &[u8] = b"YUKO_KAWAI_PURPLE_SYSTEM_SAVE_CHECK";
/// The fixed header region length; the encrypted payload starts right after.
const SYSTEM_SAVE_HEADER_BYTES: usize = 360;
/// Decompressed byte offset of the CG table (2048 dwords).
const CG_TABLE_OFFSET: usize = 32512;
/// Decompressed byte offset of the BGM table (1024 dwords).
const BGM_TABLE_OFFSET: usize = 40704;
/// Decompressed byte offset of the pooled title strings.
const TITLE_STRINGS_OFFSET: usize = 44800;
/// The recovered flag region byte count copied by `sub_48AB00`.
pub const SYSTEM_SAVE_FLAG_BYTES: usize = 0x7F00;
/// The recovered CG table word count copied by `sub_48AB70`.
pub const SYSTEM_SAVE_CG_WORDS: usize = 2048;
/// The recovered BGM table word count copied by `sub_48AB40`.
pub const SYSTEM_SAVE_BGM_WORDS: usize = 1024;
/// The fixed number of pooled title strings copied by the loader.
pub const SYSTEM_SAVE_TITLE_SLOTS: usize = 64;
/// The bounded size accepted for a system save container.
pub const MAX_SYSTEM_SAVE_BYTES: u64 = 16 * 1024 * 1024;

/// Player-progress tables recovered from one validated system save.
///
/// String bytes remain private native state and never enter diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmvsSystemSave {
    pub flags: Vec<u8>,
    pub cg_words: Vec<u32>,
    pub bgm_words: Vec<u32>,
    pub title_slots: Vec<Vec<u8>>,
    pub lists: [Vec<Vec<u8>>; 2],
}

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16, CoreError> {
    let chunk: [u8; 2] = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_SYSTEM_SAVE_BOUNDS",
                "system save header is truncated",
            )
        })?
        .try_into()
        .unwrap();
    Ok(u16::from_le_bytes(chunk))
}

fn le_u32(bytes: &[u8], offset: usize) -> Result<u32, CoreError> {
    let chunk: [u8; 4] = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_SYSTEM_SAVE_BOUNDS",
                "system save header is truncated",
            )
        })?
        .try_into()
        .unwrap();
    Ok(u32::from_le_bytes(chunk))
}

/// Builds the decompressed system-save payload from the live interpreter
/// state: flag bytes carry process flags 2048..262144, the CG/BGM tables
/// mirror the indexed/float word windows and the pooled title strings
/// follow, each NUL-terminated. The two native lists serialize after the
/// title slots in list-then-list order, mirroring the loader's cursor.
pub fn build_system_payload(state: &crate::CmvsPs2aVmState) -> Vec<u8> {
    let mut payload = vec![0_u8; TITLE_STRINGS_OFFSET];
    for &bit in &state.process_flag_bits {
        if let Some(index) = bit.checked_sub(2048) {
            let byte = (index / 8) as usize;
            if byte < SYSTEM_SAVE_FLAG_BYTES {
                payload[byte] |= 1 << (index % 8);
            }
        }
    }
    for index in 0..SYSTEM_SAVE_CG_WORDS {
        let word = state
            .process_indexed_words
            .get(&(2048 + index as u32))
            .copied()
            .unwrap_or(0);
        payload[CG_TABLE_OFFSET + index * 4..CG_TABLE_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    for index in 0..SYSTEM_SAVE_BGM_WORDS {
        let word = state
            .process_float_words
            .get(&(1024 + index as u32))
            .copied()
            .unwrap_or(0);
        payload[BGM_TABLE_OFFSET + index * 4..BGM_TABLE_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    for index in 0..SYSTEM_SAVE_TITLE_SLOTS {
        let value = state
            .process_string_slots
            .get(&(64 + index as u32))
            .cloned()
            .unwrap_or_default();
        payload.extend_from_slice(&value);
        payload.push(0);
    }
    for list in &state.native_system_lists {
        for value in list {
            payload.extend_from_slice(value);
            payload.push(0);
        }
    }
    payload
}

/// Encodes the decompressed payload into the native container: literal
/// LZSS, the fixed cipher (key/addend/xor/rotation derived exactly like
/// the loader's inverse), the identity/version header and the byte-sum
/// checksum. `sub_427E80` writes this shape to `save/system.dat`.
pub fn encode_system_container(payload: &[u8], identity: u32, lists: &[usize; 2]) -> Vec<u8> {
    let mut plain = Vec::with_capacity(payload.len() + payload.len() / 8 + 1);
    for chunk in payload.chunks(8) {
        plain.push(0xff_u8);
        plain.extend_from_slice(chunk);
    }
    let rot = 3_u32;
    let addend = 5_u8;
    let xor_base = 9_u8;
    let encoded: Vec<u8> = plain
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            let key = xor_base.wrapping_add(SYSTEM_SAVE_KEY[index % SYSTEM_SAVE_KEY.len()]);
            let t = byte.wrapping_add(64) ^ key;
            let t = t.wrapping_add(addend);
            t.rotate_left(rot)
        })
        .collect();
    let total = SYSTEM_SAVE_HEADER_BYTES + encoded.len() + 2;
    let mut source = vec![0_u8; total];
    source[68..72].copy_from_slice(&identity.to_le_bytes());
    source[76..78].copy_from_slice(&1_u16.to_le_bytes());
    source[78] = addend;
    source[79] = 0;
    source[80] = xor_base;
    let shift = u16::from((addend >> 1) & 7);
    let word82 = (((rot - 2) & 3) as u16) << shift;
    source[82..84].copy_from_slice(&word82.to_le_bytes());
    // The loader reads list[1]'s count at 84 and list[0]'s at 88.
    source[84..88].copy_from_slice(&(lists[1] as u32).to_le_bytes());
    source[88..92].copy_from_slice(&(lists[0] as u32).to_le_bytes());
    source[92..96].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    source[96..100].copy_from_slice(&(encoded.len() as u32).to_le_bytes());
    source[100..104].copy_from_slice(&((total - 2) as u32).to_le_bytes());
    source[SYSTEM_SAVE_HEADER_BYTES..SYSTEM_SAVE_HEADER_BYTES + encoded.len()]
        .copy_from_slice(&encoded);
    let mut sum = 0_u16;
    for byte in &source[SYSTEM_SAVE_HEADER_BYTES..total - 2] {
        sum = sum.wrapping_add(u16::from(*byte));
    }
    source[total - 2..].copy_from_slice(&sum.to_le_bytes());
    source
}
pub fn parse_cmvs_system_save(
    source: &[u8],
    current_identity: u32,
) -> Result<CmvsSystemSave, CoreError> {
    if source.len() < SYSTEM_SAVE_HEADER_BYTES + 2 || source.len() > MAX_SYSTEM_SAVE_BYTES as usize
    {
        return Err(invalid(
            "SHINKI_EMU_CMVS_SYSTEM_SAVE_BOUNDS",
            "system save container length is outside the supported bounds",
        ));
    }
    let declared = le_u32(source, 100)?;
    if declared
        .checked_add(2)
        .is_none_or(|total| total as usize != source.len())
    {
        return Err(invalid(
            "SHINKI_EMU_CMVS_SYSTEM_SAVE_BOUNDS",
            "system save length does not match its declared checksum span",
        ));
    }
    let checksum = u16::from_le_bytes(
        source[source.len() - 2..]
            .try_into()
            .expect("bounds checked above"),
    );
    let mut sum = 0_u16;
    for byte in &source[SYSTEM_SAVE_HEADER_BYTES..source.len() - 2] {
        sum = sum.wrapping_add(u16::from(*byte));
    }
    if checksum != sum {
        return Err(invalid(
            "SHINKI_EMU_CMVS_SYSTEM_SAVE_CHECKSUM",
            "system save checksum does not match its payload",
        ));
    }
    let identity = le_u32(source, 68)?;
    if identity != 0 && identity != current_identity {
        return Err(invalid(
            "SHINKI_EMU_CMVS_SYSTEM_SAVE_IDENTITY",
            "system save identity does not match the running interpreter",
        ));
    }
    if le_u16(source, 76)? != 1 {
        return Err(invalid(
            "SHINKI_EMU_CMVS_SYSTEM_SAVE_VERSION",
            "system save uses an unrecovered container version",
        ));
    }
    let decompressed_size = le_u32(source, 92)? as usize;
    let payload_size = le_u32(source, 96)? as usize;
    let payload_end = SYSTEM_SAVE_HEADER_BYTES
        .checked_add(payload_size)
        .ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_SYSTEM_SAVE_BOUNDS",
                "system save payload range overflowed",
            )
        })?;
    if payload_end > source.len() - 2 || decompressed_size > MAX_SYSTEM_SAVE_BYTES as usize {
        return Err(invalid(
            "SHINKI_EMU_CMVS_SYSTEM_SAVE_BOUNDS",
            "system save payload bounds are invalid",
        ));
    }
    let rot = rotation_amount(source)?;
    let addend = source[78];
    let xor_base = source[80];
    let mut payload = source[SYSTEM_SAVE_HEADER_BYTES..payload_end].to_vec();
    for (index, byte) in payload.iter_mut().enumerate() {
        let rotated = byte.rotate_right(rot);
        let key = xor_base.wrapping_add(SYSTEM_SAVE_KEY[index % SYSTEM_SAVE_KEY.len()]);
        *byte = rotated.wrapping_sub(addend) ^ key;
        *byte = byte.wrapping_sub(64);
    }
    let decompressed = decompress_system_save(&payload, decompressed_size)?;
    if decompressed.len() < TITLE_STRINGS_OFFSET {
        return Err(invalid(
            "SHINKI_EMU_CMVS_SYSTEM_SAVE_LAYOUT",
            "system save payload is smaller than the recovered layout",
        ));
    }
    let flags = decompressed[..SYSTEM_SAVE_FLAG_BYTES].to_vec();
    let cg_words = read_word_table(&decompressed, CG_TABLE_OFFSET, SYSTEM_SAVE_CG_WORDS)?;
    let bgm_words = read_word_table(&decompressed, BGM_TABLE_OFFSET, SYSTEM_SAVE_BGM_WORDS)?;
    let mut cursor = TITLE_STRINGS_OFFSET;
    let mut title_slots = Vec::with_capacity(SYSTEM_SAVE_TITLE_SLOTS);
    for _ in 0..SYSTEM_SAVE_TITLE_SLOTS {
        title_slots.push(read_string(&decompressed, &mut cursor, 1023)?);
    }
    let counts = [le_u32(source, 88)?, le_u32(source, 84)?];
    let total = counts[0].checked_add(counts[1]).ok_or_else(layout_error)?;
    if u64::from(total) * 72 > MAX_SYSTEM_SAVE_BYTES {
        return Err(layout_error());
    }
    let mut lists = [Vec::new(), Vec::new()];
    for (list, count) in lists.iter_mut().zip(counts) {
        for _ in 0..count {
            list.push(read_string(&decompressed, &mut cursor, 63)?);
        }
    }
    let save = CmvsSystemSave {
        flags,
        cg_words,
        bgm_words,
        title_slots,
        lists,
    };
    save.validate()?;
    Ok(save)
}

pub(crate) fn layout_error() -> CoreError {
    invalid(
        "SHINKI_EMU_CMVS_SYSTEM_SAVE_LAYOUT",
        "system save native layout or string bounds are invalid",
    )
}

pub(crate) fn validate_string(bytes: &[u8], maximum: usize) -> Result<(), CoreError> {
    if bytes.len() > maximum
        || bytes.contains(&0)
        || encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(bytes)
            .is_none()
    {
        return Err(layout_error());
    }
    Ok(())
}

fn read_string(bytes: &[u8], cursor: &mut usize, maximum: usize) -> Result<Vec<u8>, CoreError> {
    let length = nul_length(bytes, *cursor)?;
    let value = &bytes[*cursor..*cursor + length];
    validate_string(value, maximum)?;
    *cursor += length + 1;
    Ok(value.to_vec())
}

impl CmvsSystemSave {
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if self.flags.len() != SYSTEM_SAVE_FLAG_BYTES
            || self.cg_words.len() != SYSTEM_SAVE_CG_WORDS
            || self.bgm_words.len() != SYSTEM_SAVE_BGM_WORDS
            || self.title_slots.len() != SYSTEM_SAVE_TITLE_SLOTS
        {
            return Err(layout_error());
        }
        for value in &self.title_slots {
            validate_string(value, 1023)?;
        }
        validate_lists(&self.lists)
    }
}

pub(crate) fn validate_lists(lists: &[Vec<Vec<u8>>; 2]) -> Result<(), CoreError> {
    if lists.iter().map(Vec::len).sum::<usize>() > MAX_SYSTEM_SAVE_BYTES as usize / 72 {
        return Err(layout_error());
    }
    for value in lists.iter().flatten() {
        validate_string(value, 63)?;
    }
    Ok(())
}

fn rotation_amount(source: &[u8]) -> Result<u32, CoreError> {
    let word78 = le_u16(source, 78)?;
    let word82 = le_u16(source, 82)?;
    let shift = u32::from((word78 >> 1) & 7);
    Ok(u32::from((word82 >> shift) & 3) + 2)
}

fn read_word_table(
    decompressed: &[u8],
    offset: usize,
    words: usize,
) -> Result<Vec<u32>, CoreError> {
    let end = offset
        .checked_add(words * 4)
        .ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_SYSTEM_SAVE_LAYOUT",
                "system save table range overflowed",
            )
        })?
        .min(decompressed.len());
    if end < offset + words * 4 {
        return Err(invalid(
            "SHINKI_EMU_CMVS_SYSTEM_SAVE_LAYOUT",
            "system save table lies outside the payload",
        ));
    }
    let mut table = Vec::with_capacity(words);
    for index in 0..words {
        table.push(le_u32(decompressed, offset + index * 4)?);
    }
    Ok(table)
}

fn nul_length(bytes: &[u8], start: usize) -> Result<usize, CoreError> {
    bytes
        .get(start..)
        .and_then(|slice| slice.iter().position(|byte| *byte == 0))
        .ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_SYSTEM_SAVE_LAYOUT",
                "system save string pool is unterminated",
            )
        })
}

/// The recovered `sub_491600` decompressor: an LZSS variant with a
/// 1024-byte window seeded at position 976, LSB-first flag bytes, literal
/// bits and two-byte matches whose offset is assembled from the low byte and
/// the top two bits of the high byte, and whose count is `(hi & 0x3f) + 2`.
/// Decoding stops when the source is consumed, matching the original.
pub fn decompress_system_save(source: &[u8], expected_size: usize) -> Result<Vec<u8>, CoreError> {
    if expected_size > MAX_SYSTEM_SAVE_BYTES as usize {
        return Err(invalid(
            "SHINKI_EMU_CMVS_SYSTEM_SAVE_BOUNDS",
            "system save decompression size is outside the bounded policy",
        ));
    }
    let mut window = [0_u8; 1024];
    let mut window_pos = 976_usize;
    let mut output = Vec::with_capacity(expected_size);
    let mut input_pos = 0_usize;
    let mut flags = 0_u16;
    macro_rules! emit {
        ($byte:expr) => {{
            if output.len() >= expected_size {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_SYSTEM_SAVE_LZSS",
                    "system save stream expands beyond its declared size",
                ));
            }
            output.push($byte);
            window[window_pos] = $byte;
            window_pos = (window_pos + 1) & 0x3ff;
        }};
    }
    while input_pos < source.len() {
        flags >>= 1;
        if flags & 0x100 == 0 {
            let byte = *source.get(input_pos).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_SYSTEM_SAVE_LZSS",
                    "system save stream ended in a flag byte",
                )
            })?;
            input_pos += 1;
            flags = u16::from(byte) | 0xff00;
        }
        if flags & 1 != 0 {
            let byte = *source.get(input_pos).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_SYSTEM_SAVE_LZSS",
                    "system save stream ended in a literal",
                )
            })?;
            input_pos += 1;
            emit!(byte);
        } else {
            let lo = *source.get(input_pos).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_SYSTEM_SAVE_LZSS",
                    "system save stream ended in a match offset",
                )
            })?;
            let hi = *source.get(input_pos + 1).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_SYSTEM_SAVE_LZSS",
                    "system save stream ended in a match length",
                )
            })?;
            input_pos += 2;
            let offset = usize::from(lo) | (usize::from(hi & 0xc0) << 2);
            let count = usize::from(hi & 0x3f) + 2;
            for step in 0..count {
                let byte = window[(offset + step) & 0x3ff];
                emit!(byte);
            }
        }
    }
    Ok(output)
}

pub mod lzss;
pub mod slot;
#[cfg(test)]
pub(crate) mod tests;
