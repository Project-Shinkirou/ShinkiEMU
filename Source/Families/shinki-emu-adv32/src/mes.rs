use crate::{
    Error, Result,
    binary::{c_string, dword, range, word},
};

pub(crate) const CLASSIC_SCENE_TRANSLATION: u8 = 0x98;

const ENTRY_FLAG: u32 = 1 << 31;
pub(crate) const MAX_SCRIPT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Dialect {
    pub(crate) version: u16,
    small_end: u8,
    prefixed_end: u8,
    string_end: u8,
}

impl Dialect {
    fn from_header(header: &[u8]) -> Result<Self> {
        let version = word(header, 0)?;
        if header.get(2) != Some(&0) {
            return Err(Error::Invalid("MES version marker"));
        }
        let (small_end, prefixed_end, string_end) = match version {
            0xaab6 | 0xabb6 => (0x3b, 0x48, 0x6e),
            0xa6b9 => (0x3e, 0x4b, 0x71),
            0x9e97 => (0x3c, 0x44, 0x66),
            0x9f9a => (0x39, 0x41, 0x63),
            _ => return Err(Error::UnsupportedMesVersion(version)),
        };
        Ok(Self {
            version,
            small_end,
            prefixed_end,
            string_end,
        })
    }

    pub(crate) fn opcode(self, raw: u8) -> Option<u8> {
        if self.classic() {
            let legacy = if self.version == 0x9f9a {
                match raw {
                    0x4a => return Some(0x52),
                    0x50 => return Some(0x58),
                    0x59 => return Some(0x61),
                    0x68 => return Some(0x75),
                    0x69 => return Some(0x76),
                    0x6b => return Some(0x78),
                    0x73 => return Some(0x80),
                    0x8d => return Some(CLASSIC_SCENE_TRANSLATION),
                    0x95..=0x97 => return Some(raw),
                    0x39..=0x67 => raw + 3,
                    0x6a.. => raw.checked_add(2)?,
                    _ => raw,
                }
            } else {
                raw
            };
            return Some(match legacy {
                0x00..=0x07 => legacy,
                0x08 => 0x09,
                0x09 => 0x0a,
                0x0a | 0x0b => legacy + 1,
                0x0e => 0x0f,
                0x0f => 0x00,
                0x10 => 0x11,
                0x12 => 0x13,
                0x13..=0x16 => legacy + 2,
                0x18 | 0x1a | 0x1b | 0x1c | 0x1e | 0x20 | 0x22 => legacy + 2,
                0x23 => 0x25,
                0x26 => 0x27,
                0x27 => 0x28,
                0x31..=0x34 => legacy + 1,
                0x35 => 0x36,
                0x36 => 0x37,
                0x37 => 0x38,
                0x3d..=0x3f => legacy - 1,
                0x40 => 0x3f,
                0x41 => 0x41,
                0x43 => 0x43,
                0x45..=0x48 => legacy + 4 + u8::from(legacy >= 0x46),
                0x49 => 0x4e,
                0x4a..=0x4d | 0x4f..=0x51 | 0x54..=0x55 => legacy + 5,
                0x56 => 0x5c,
                0x5e..=0x60 => legacy + 5,
                0x63 => 0x6a,
                0x64 => 0x6b,
                0x65 => 0x6c,
                0x67..=0x6a => legacy + 9,
                0x6b => 0x76,
                0x6c => 0x77,
                0x6d => 0x78,
                0x6e..=0x6f => legacy + 11,
                0x70 => 0x7b,
                0x72..=0x74 => legacy + 11,
                0x76 => 0x81,
                0x77..=0x7e | 0x80 => legacy + 11,
                0x81 => 0x8d,
                0x82 => 0x8e,
                0x83 => 0x90,
                0x84 => 0x91,
                0x87 => 0x94,
                0x8c => 0x99,
                0x8f => CLASSIC_SCENE_TRANSLATION,
                0x90 => 0x9d,
                0x91 => 0x9e,
                0x92 => 0x9f,
                0x93 => 0xa0,
                0x95 => 0xa2,
                _ => return None,
            });
        }
        Some(match (self.version, raw) {
            (0xa6b9, 0x3e..) => raw - 3,
            _ => raw,
        })
    }

    pub(crate) fn classic(self) -> bool {
        matches!(self.version, 0x9e97 | 0x9f9a)
    }
}

pub(crate) struct Script {
    data: Vec<u8>,
    code_at: usize,
    pub(crate) dialect: Dialect,
    entries: Vec<(usize, bool)>,
}

#[derive(Clone, Copy)]
pub(crate) struct Instruction<'a> {
    pub(crate) opcode: u8,
    pub(crate) offset: usize,
    pub(crate) next: usize,
    pub(crate) args: Operands<'a>,
}

#[derive(Clone, Copy)]
pub(crate) enum Operands<'a> {
    Pair([u8; 2]),
    Words([u16; 4]),
    Text { index: Option<u8>, bytes: &'a [u8] },
}

impl Script {
    pub(crate) fn parse(data: Vec<u8>) -> Result<Self> {
        if data.len() > MAX_SCRIPT_BYTES {
            return Err(Error::Invalid("MES length"));
        }
        let count = dword(&data, 0)? as usize;
        let code_at = count
            .checked_mul(6)
            .and_then(|size| size.checked_add(4))
            .ok_or(Error::Invalid("MES directory"))?;
        let dialect = Dialect::from_header(range(&data, code_at, 3)?)?;
        let mut entries = Vec::with_capacity(count);
        for index in 0..count {
            let raw = dword(&data, 4 + index * 4)?;
            let offset = (raw & !ENTRY_FLAG) as usize;
            if offset < 3 || offset >= data.len() - code_at {
                return Err(Error::Invalid("MES entry offset"));
            }
            entries.push((offset, raw & ENTRY_FLAG != 0));
        }
        if entries.first().map(|entry| entry.0) != Some(3) {
            return Err(Error::Invalid("MES first entry"));
        }
        let mut script = Self {
            data,
            code_at,
            dialect,
            entries,
        };
        let mut required = script
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.0, index))
            .collect::<Vec<_>>();
        required.sort_unstable();
        let mut required = required.into_iter().peekable();
        let mut valid = vec![false; count];
        let mut markers = vec![(None, false); count];
        let mut offset = 3;
        while offset < script.data.len() - code_at {
            while let Some(&(position, index)) = required.peek()
                && position <= offset
            {
                valid[index] = position == offset;
                required.next();
            }
            let instruction = script.instruction(offset)?;
            if script.dialect.version == 0x9f9a
                && matches!(instruction.opcode, 0x03 | 0x04)
                && let Operands::Pair(index) = instruction.args
                && let Some((marker, repeated)) =
                    markers.get_mut(usize::from(u16::from_le_bytes(index)))
            {
                *repeated |= marker.replace(instruction.next).is_some();
            }
            offset = instruction.next;
        }
        // A localized script can move its entry text while leaving the header
        // offsets unchanged. An unambiguous embedded entry marker relocates it.
        for ((entry, valid), (marker, repeated)) in
            script.entries.iter_mut().zip(valid).zip(markers)
        {
            if !valid {
                entry.0 = marker
                    .filter(|&offset| !repeated && offset < script.data.len() - code_at)
                    .ok_or(Error::Invalid("MES entry inside an instruction"))?;
            }
        }
        Ok(script)
    }

    pub(crate) fn entry(&self, index: usize) -> Result<usize> {
        self.entries
            .get(index)
            .map(|entry| entry.0)
            .ok_or(Error::Invalid("MES entry number"))
    }

    pub(crate) fn read_step(&self, entry: u16) -> Result<u16> {
        let entry = usize::from(entry);
        if entry >= self.entries.len() {
            return Err(Error::Invalid("MES read-mark entry number"));
        }
        word(&self.data, 4 + 4 * self.entries.len() + 2 * entry)
    }

    pub(crate) fn label(&self, name: &[u8], opcode: u8) -> Result<Option<usize>> {
        let mut offset = 3;
        while offset < self.data.len() - self.code_at {
            let instruction = self.instruction(offset)?;
            if self.dialect.opcode(instruction.opcode) == Some(opcode)
                && matches!(instruction.args, Operands::Text { index: None, bytes } if bytes == name)
            {
                return Ok(Some(offset));
            }
            if instruction.opcode == 0 {
                break;
            }
            offset = instruction.next;
        }
        Ok(None)
    }

    fn display_bytes(bytes: &[u8]) -> Vec<u8> {
        bytes
            .iter()
            .map(|byte| {
                let decoded = byte.wrapping_add(0x20);
                if decoded == b'$' { b' ' } else { decoded }
            })
            .collect()
    }

    pub(crate) fn decode_text(
        bytes: &[u8],
        encoding: &'static encoding_rs::Encoding,
    ) -> Result<String> {
        encoding
            .decode_without_bom_handling_and_without_replacement(&Self::display_bytes(bytes))
            .map(std::borrow::Cow::into_owned)
            .ok_or(Error::Invalid("ADV32 display text encoding"))
    }

    pub(crate) fn has_gbk_dialogue(&self) -> bool {
        let mut offset = 3;
        let mut matches = 0;
        while let Ok(instruction) = self.instruction(offset) {
            offset = instruction.next;
            if let Some(0x6a | 0x6b) = self.dialect.opcode(instruction.opcode)
                && let Operands::Text { bytes, .. } = instruction.args
            {
                let raw = Self::display_bytes(bytes);
                if encoding_rs::SHIFT_JIS
                    .decode_without_bom_handling_and_without_replacement(&raw)
                    .is_none()
                    && encoding_rs::GBK
                        .decode_without_bom_handling_and_without_replacement(&raw)
                        .is_some()
                {
                    matches += 1;
                    if matches == 3 {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub(crate) fn instruction(&self, offset: usize) -> Result<Instruction<'_>> {
        let code = &self.data[self.code_at..];
        let opcode = *code
            .get(offset)
            .ok_or(Error::Invalid("MES instruction offset"))?;
        let payload_at = offset + 1;
        let (args, len) = if opcode < self.dialect.small_end {
            let bytes = range(code, payload_at, 2)?;
            (Operands::Pair([bytes[0], bytes[1]]), 2)
        } else if opcode < self.dialect.string_end {
            let mut position = payload_at;
            let index = if opcode < self.dialect.prefixed_end {
                let index = *code
                    .get(position)
                    .ok_or(Error::Invalid("MES string index"))?;
                position += 1;
                Some(index)
            } else {
                None
            };
            let bytes = c_string(&code[position..])?;
            (
                Operands::Text { index, bytes },
                position - payload_at + bytes.len() + 1,
            )
        } else {
            (
                Operands::Words([
                    word(code, payload_at)?,
                    word(code, payload_at + 2)?,
                    word(code, payload_at + 4)?,
                    word(code, payload_at + 6)?,
                ]),
                8,
            )
        };
        Ok(Instruction {
            opcode,
            offset,
            next: payload_at + len,
            args,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jump_entries_and_rejects_a_jump_into_a_string() {
        let mut script = Vec::from(2u32.to_le_bytes());
        script.extend(3u32.to_le_bytes());
        script.extend((ENTRY_FLAG | 7).to_le_bytes());
        script.extend([0, 0, 0, 0, 0xb6, 0xab, 0, 0x63, b'A', b'B', 0, 4, 0, 0]);
        let parsed = Script::parse(script.clone()).unwrap();
        assert_eq!(parsed.entry(1).unwrap(), 7);
        let entry = parsed.instruction(3).unwrap();
        assert!(matches!(
            entry.args,
            Operands::Text {
                index: None,
                bytes: b"AB"
            }
        ));
        script[8..12].copy_from_slice(&4u32.to_le_bytes());
        assert!(Script::parse(script).is_err());
    }

    #[test]
    fn expanded_classic_relocates_a_stale_entry_to_its_unique_tick_marker() {
        let mut script = Vec::from(2u32.to_le_bytes());
        script.extend(3u32.to_le_bytes());
        script.extend(7u32.to_le_bytes());
        script.extend([
            0, 0, 0, 0, 0x9a, 0x9f, 0, 3, 0, 0, 3, 1, 0, 6, 0, 0, 0, 0, 0,
        ]);
        assert_eq!(Script::parse(script.clone()).unwrap().entry(1).unwrap(), 9);
        script[23] = 0;
        assert!(Script::parse(script).is_err());
    }
}
