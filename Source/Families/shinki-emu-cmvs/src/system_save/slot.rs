//! saveNNN.dat serialization: the ProgState TLV + container writer per
//! `ProgState_serialize` (0x4712F0) and `Save_writeFile` (0x4774D0).
//!
//! The container: `[600-byte header][XOR+LZSS stream A (TLV)][plain
//! stream B][plain stream C][u16 checksum = sum of bytes after the
//! header]`. The TLV tags mirror the native ProgState layout for the
//! state the interpreter models; unknown native tags stay absent and the
//! reader only restores tags it understands.

use shinki_emu_sdk::CoreError;

use crate::system_save::lzss::{lzss_compress, lzss_decompress, xor_cipher};
use crate::vm::CmvsPs2aVmState;

/// Header region copied verbatim before the streams.
const HEADER_BYTES: usize = 600;
/// The native header magic at offset 0.
const SAVE_MAGIC: &[u8; 4] = b"CUS2";
/// Fixed version word at 546.
const SAVE_VERSION: u32 = 1;
// TLV tags (re-report §4).
const TAG_STRINGS_BASE: u32 = 256;
const TAG_U32_BASE: u32 = 274;
const TAG_SUSPEND_TRIPLE: u32 = 262_406;
const TAG_RECORD_TABLE: u32 = 117_702_923;
const TAG_VALUE_CURSOR: u32 = 262_720;
const TAG_FRAME_DEPTH: u32 = 262_721;
const TAG_VALUE_STACK: u32 = 578;
const TAG_FRAME_STACK: u32 = 579;
const TAG_GLOBAL_B: u32 = 640;
const TAG_GLOBAL_A: u32 = 642;

fn save_error(message: &str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_SAVE_SERIALIZE", message)
}

/// The subset of interpreter state one save slot carries. Every field is
/// optional so a reader from a different model revision can skip tags.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct CmvsSlotState {
    pub suspend: Option<(u32, u32, u32)>,
    pub value_stack_cursor_hint: Option<u32>,
    pub frame_depth_hint: Option<u32>,
    pub records: Vec<[u32; 7]>,
    pub strings: Vec<Option<Vec<u8>>>,
    pub numbers: Vec<Option<u32>>,
    pub value_stack: Vec<u32>,
    pub frame_bases: Vec<u32>,
    pub globals: [Option<u32>; 3],
}

fn push_tlv(out: &mut Vec<u8>, tag: u32, payload: &[u8]) {
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
}

/// Serializes the VM's save-relevant state into the native TLV stream.
/// Serializes a pre-captured story snapshot with the same TLV layout
/// the live serializer emits.
pub fn serialize_snapshot(state: &CmvsSlotState) -> Result<Vec<u8>, CoreError> {
    let mut out = Vec::with_capacity(4096);
    if let Some((counter, pc, frame)) = state.suspend {
        let mut payload = Vec::with_capacity(12);
        for word in [counter, pc, frame] {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        push_tlv(&mut out, TAG_SUSPEND_TRIPLE, &payload);
    }
    let mut records = Vec::with_capacity(64 * 28);
    for row in state.records.iter() {
        for word in row.iter() {
            records.extend_from_slice(&word.to_le_bytes());
        }
    }
    push_tlv(&mut out, TAG_RECORD_TABLE, &records);
    if let Some(cursor) = state.value_stack_cursor_hint {
        push_tlv(&mut out, TAG_VALUE_CURSOR, &cursor.to_le_bytes());
    }
    if let Some(depth) = state.frame_depth_hint {
        push_tlv(&mut out, TAG_FRAME_DEPTH, &depth.to_le_bytes());
    }
    let mut stack = Vec::with_capacity(state.value_stack.len() * 4);
    for word in state.value_stack.iter() {
        stack.extend_from_slice(&word.to_le_bytes());
    }
    push_tlv(&mut out, TAG_VALUE_STACK, &stack);
    let mut bases = Vec::with_capacity(state.frame_bases.len() * 4);
    for base in state.frame_bases.iter() {
        bases.extend_from_slice(&base.to_le_bytes());
    }
    push_tlv(&mut out, TAG_FRAME_STACK, &bases);
    for (index, value) in state.globals.iter().enumerate() {
        if let Some(word) = value {
            push_tlv(&mut out, TAG_GLOBAL_B + index as u32, &word.to_le_bytes());
        }
    }
    Ok(out)
}

pub fn serialize_prog_state(vm: &CmvsPs2aVmState) -> Result<Vec<u8>, CoreError> {
    let mut out = Vec::with_capacity(4096);
    let triple = (
        vm.interpreter_words.get(&0x33A8).copied().unwrap_or(0),
        vm.program_counter,
        u32::from(vm.current_frame),
    );
    if triple != (0, 0, 0) {
        let mut payload = Vec::with_capacity(12);
        for word in [triple.0, triple.1, triple.2] {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        push_tlv(&mut out, TAG_SUSPEND_TRIPLE, &payload);
    }
    // Record flags, frame, PC, three arguments and timestamp are distinct
    // from the scheduler return triples already carried by value_stack.
    let mut records = Vec::with_capacity(64 * 28);
    for slot in 0..64_u32 {
        for field in 0..7_u32 {
            let default = if field == 2 { u32::MAX } else { 0 };
            let word = vm
                .interpreter_words
                .get(&(0x33D8 + slot * 28 + field * 4))
                .copied()
                .unwrap_or(default);
            records.extend_from_slice(&word.to_le_bytes());
        }
    }
    push_tlv(&mut out, TAG_RECORD_TABLE, &records);
    push_tlv(
        &mut out,
        TAG_VALUE_CURSOR,
        &vm.stack_cursor_bytes.to_le_bytes(),
    );
    push_tlv(
        &mut out,
        TAG_FRAME_DEPTH,
        &(vm.call_frame_bases.len() as u32).to_le_bytes(),
    );
    let mut stack = Vec::with_capacity(vm.stack_bytes.len());
    for chunk in vm.stack_bytes.as_chunks::<4>().0 {
        stack.extend_from_slice(chunk);
    }
    push_tlv(&mut out, TAG_VALUE_STACK, &stack);
    let mut frames = Vec::new();
    for base in vm.call_frame_bases.iter() {
        frames.extend_from_slice(&base.to_le_bytes());
    }
    push_tlv(&mut out, TAG_FRAME_STACK, &frames);
    let globals = [
        vm.interpreter_words.get(&640).copied(),
        vm.interpreter_words.get(&641).copied(),
        vm.interpreter_words.get(&642).copied(),
    ];
    for (index, value) in globals.into_iter().enumerate() {
        if let Some(word) = value {
            push_tlv(&mut out, TAG_GLOBAL_B + index as u32, &word.to_le_bytes());
        }
    }
    for (index, key) in (0x644_u32..0x64A_u32).enumerate() {
        if let Some(word) = vm.interpreter_words.get(&key).copied() {
            push_tlv(&mut out, TAG_U32_BASE + index as u32, &word.to_le_bytes());
        }
    }
    Ok(out)
}

/// Parses the TLV stream back into slot state; unknown tags are skipped
/// without failing (forward compatibility with native saves).
pub fn deserialize_prog_state(stream: &[u8]) -> Result<CmvsSlotState, CoreError> {
    let mut state = CmvsSlotState::default();
    let mut position = 0_usize;
    while position + 8 <= stream.len() {
        let tag = u32::from_le_bytes(
            stream[position..position + 4]
                .try_into()
                .map_err(|_| save_error("TLV tag is truncated"))?,
        );
        let length = u32::from_le_bytes(
            stream[position + 4..position + 8]
                .try_into()
                .map_err(|_| save_error("TLV length is truncated"))?,
        ) as usize;
        position += 8;
        let end = position
            .checked_add(length)
            .filter(|end| *end <= stream.len())
            .ok_or_else(|| save_error("TLV payload exceeds the stream"))?;
        let payload = &stream[position..end];
        position = end;
        match tag {
            TAG_SUSPEND_TRIPLE if payload.len() == 12 => {
                let words: [u32; 3] = [
                    u32::from_le_bytes(payload[0..4].try_into().unwrap()),
                    u32::from_le_bytes(payload[4..8].try_into().unwrap()),
                    u32::from_le_bytes(payload[8..12].try_into().unwrap()),
                ];
                state.suspend = Some((words[0], words[1], words[2]));
            }
            TAG_RECORD_TABLE => {
                if payload.len() != 64 * 28 {
                    return Err(save_error("coroutine record table has an invalid length"));
                }
                state.records = payload
                    .as_chunks::<28>().0.iter()
                    .map(|chunk| {
                        std::array::from_fn(|i| {
                            u32::from_le_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap())
                        })
                    })
                    .collect();
            }
            TAG_VALUE_CURSOR if payload.len() == 4 => {
                state.value_stack_cursor_hint =
                    Some(u32::from_le_bytes(payload.try_into().unwrap()));
            }
            TAG_FRAME_DEPTH if payload.len() == 4 => {
                state.frame_depth_hint = Some(u32::from_le_bytes(payload.try_into().unwrap()));
            }
            TAG_VALUE_STACK if payload.len().is_multiple_of(4) => {
                state.value_stack = payload
                    .as_chunks::<4>().0.iter()
                    .map(|chunk| u32::from_le_bytes(*chunk))
                    .collect();
            }
            TAG_FRAME_STACK if payload.len().is_multiple_of(4) => {
                state.frame_bases = payload
                    .as_chunks::<4>().0.iter()
                    .map(|chunk| u32::from_le_bytes(*chunk))
                    .collect();
            }
            TAG_GLOBAL_B..=TAG_GLOBAL_A if payload.len() == 4 => {
                let index = (tag - TAG_GLOBAL_B) as usize;
                if index < state.globals.len() {
                    state.globals[index] = Some(u32::from_le_bytes(payload.try_into().unwrap()));
                }
            }
            TAG_STRINGS_BASE..=261 => {
                let index = (tag - TAG_STRINGS_BASE) as usize;
                if state.strings.len() <= index {
                    state.strings.resize(index + 1, None);
                }
                state.strings[index] = Some(payload.to_vec());
            }
            TAG_U32_BASE..=277 => {
                let index = (tag - TAG_U32_BASE) as usize;
                if state.numbers.len() <= index {
                    state.numbers.resize(index + 1, None);
                }
                state.numbers[index] = Some(u32::from_le_bytes(payload.try_into().unwrap()));
            }
            _ => {}
        }
    }
    Ok(state)
}

/// Packs the TLV into the saveNNN.dat container bytes.
pub fn pack_save_container(tlv: &[u8], stream_b: &[u8], stream_c: &[u8]) -> Vec<u8> {
    let mut tlv_packed = lzss_compress(tlv);
    tlv_packed = xor_cipher(&tlv_packed);
    let total = HEADER_BYTES + tlv_packed.len() + stream_b.len() + stream_c.len() + 2;
    let mut out = vec![0_u8; HEADER_BYTES];
    out[0..4].copy_from_slice(SAVE_MAGIC);
    out[540..544].copy_from_slice(&(total as u32).to_le_bytes());
    out[546..550].copy_from_slice(&SAVE_VERSION.to_le_bytes());
    out[560..564].copy_from_slice(&(tlv_packed.len() as u32).to_le_bytes());
    out[564..568].copy_from_slice(&(stream_b.len() as u32).to_le_bytes());
    out[568..572].copy_from_slice(&(stream_c.len() as u32).to_le_bytes());
    out.extend_from_slice(&tlv_packed);
    out.extend_from_slice(stream_b);
    out.extend_from_slice(stream_c);
    let checksum: u16 = out[HEADER_BYTES..]
        .iter()
        .fold(0_u32, |sum, byte| sum.wrapping_add(u32::from(*byte))) as u16;
    out.extend_from_slice(&checksum.to_le_bytes());
    out
}

/// Unpacks and validates a save container, returning the TLV stream.
pub fn unpack_save_container(bytes: &[u8]) -> Result<Vec<u8>, CoreError> {
    if bytes.len() < HEADER_BYTES + 2 {
        return Err(save_error("save container is truncated"));
    }
    if &bytes[0..4] != SAVE_MAGIC {
        return Err(save_error("save magic mismatch"));
    }
    let checksum = u16::from_le_bytes(
        bytes[bytes.len() - 2..]
            .try_into()
            .map_err(|_| save_error("save checksum is truncated"))?,
    );
    let actual: u16 = bytes[HEADER_BYTES..bytes.len() - 2]
        .iter()
        .fold(0_u32, |sum, byte| sum.wrapping_add(u32::from(*byte))) as u16;
    if checksum != actual {
        return Err(save_error("save checksum mismatch"));
    }
    let stream_a_len = u32::from_le_bytes(bytes[560..564].try_into().unwrap()) as usize;
    if HEADER_BYTES + stream_a_len > bytes.len() - 2 {
        return Err(save_error("save stream A exceeds the container"));
    }
    let packed = xor_cipher(&bytes[HEADER_BYTES..HEADER_BYTES + stream_a_len]);
    // The TLV length is unknown before decompression; cap it at the
    // native ProgState buffer size.
    let tlv = lzss_decompress(&packed, 0x300_000)?;
    Ok(tlv)
}

/// Slot file name for a numeric slot.
pub fn slot_file_name(slot: u32) -> String {
    format!("save{slot:03}.dat")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_round_trips_tlv_state() {
        let mut tlv = Vec::new();
        push_tlv(
            &mut tlv,
            TAG_SUSPEND_TRIPLE,
            &[1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0],
        );
        push_tlv(&mut tlv, TAG_U32_BASE, &42_u32.to_le_bytes());
        let packed = pack_save_container(&tlv, b"", b"");
        let restored = unpack_save_container(&packed).unwrap();
        let state = deserialize_prog_state(&restored).unwrap();
        assert_eq!(state.suspend, Some((1, 2, 3)));
        assert_eq!(state.numbers.first().copied().flatten(), Some(42));
    }

    #[test]
    fn container_rejects_corruption() {
        let mut tlv = Vec::new();
        push_tlv(&mut tlv, TAG_U32_BASE, &7_u32.to_le_bytes());
        let mut packed = pack_save_container(&tlv, b"", b"");
        let last = packed.len() - 1;
        packed[last] ^= 0xFF;
        assert!(unpack_save_container(&packed).is_err());
    }
}
