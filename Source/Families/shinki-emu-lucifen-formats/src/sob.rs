use std::{collections::BTreeMap, io};

pub type Bootstrap = BTreeMap<String, (u32, u32)>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleKind {
    Script,
    Library,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Relocation {
    EngineVariable(u32),
    NativeFunction(u32),
    UserVariable(u32),
    LibraryOffset(u32),
    ImageOffset(u32),
}

pub struct Sob<'a> {
    pub kind: ModuleKind,
    relocations: &'a [u8],
    pub code: &'a [u8],
    pub code_and_data: &'a [u8],
}

impl<'a> Sob<'a> {
    pub fn parse(bytes: &'a [u8]) -> io::Result<Self> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid SOB0 sections");
        let kind = match bytes.get(..4) {
            Some(b"SOB0") => ModuleKind::Script,
            Some(b"SOB2") => ModuleKind::Library,
            _ => return Err(invalid()),
        };
        let reloc_size = word(bytes, 4).ok_or_else(invalid)? as usize;
        let after_reloc = 8usize.checked_add(reloc_size).ok_or_else(invalid)?;
        let code_size = word(bytes, after_reloc).ok_or_else(invalid)? as usize;
        let image_size = word(bytes, after_reloc + 4).ok_or_else(invalid)? as usize;
        let relocations = bytes.get(8..after_reloc).ok_or_else(invalid)?;
        let image = bytes.get(after_reloc + 8..).ok_or_else(invalid)?;
        let code_and_data = image.get(..image_size).ok_or_else(invalid)?;
        let code = code_and_data.get(..code_size).ok_or_else(invalid)?;
        Ok(Self {
            kind,
            relocations,
            code,
            code_and_data,
        })
    }

    pub fn relocations(&self) -> io::Result<BTreeMap<u32, Relocation>> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid SOB relocation table");
        let mut bytes = self.relocations;
        let mut result = BTreeMap::new();
        let kinds = [
            Relocation::EngineVariable,
            Relocation::NativeFunction,
            Relocation::UserVariable,
            Relocation::LibraryOffset,
            Relocation::ImageOffset,
        ];
        for kind in kinds {
            let count = word(bytes, 0).ok_or_else(invalid)? as usize;
            let size = count.checked_mul(8).ok_or_else(invalid)?;
            let (records, rest) = bytes[4..].split_at_checked(size).ok_or_else(invalid)?;
            bytes = rest;
            for record in records.chunks_exact(8) {
                let offset = word(record, 0).unwrap();
                let operand = word(record, 4).unwrap();
                self.code_and_data
                    .get(offset as usize..)
                    .and_then(|body| body.get(..4))
                    .ok_or_else(invalid)?;
                let relocation = kind(operand);
                if matches!(relocation, Relocation::ImageOffset(_)) {
                    self.code_and_data
                        .get(operand as usize..)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(invalid)?;
                }
                if result.insert(offset, relocation).is_some() {
                    return Err(invalid());
                }
            }
        }
        if bytes != [0; 4] {
            return Err(invalid());
        }
        Ok(result)
    }

    // Archive setup compiles to paired set-key and mount calls. Read the
    // literal operands and the relative archive-name reference in that pair.
    // Format reference: GARbro ArcLPK.cs (MIT, morkt) and Lucifen-Tools (MIT, UBTL).
    pub fn archive_keys(&self) -> io::Result<Bootstrap> {
        let body = self.code_and_data;
        let mut keys = BTreeMap::new();
        const OPERANDS: &[(usize, u32)] = &[
            (0, 0x28),
            (1, 0),
            (3, 8),
            (4, 1),
            (5, 8),
            (6, 1),
            (7, 8),
            (9, 8),
            (11, 5),
            (17, 0x28),
            (18, 0),
            (20, 8),
            (22, 8),
            (23, u32::MAX),
            (24, 8),
            (25, 1),
            (26, 8),
            (27, 1),
            (28, 5),
        ];
        for (index, window) in self.code.chunks_exact(4).enumerate() {
            if window != 0x28u32.to_le_bytes() {
                continue;
            }
            let offset = index * 4;
            let Some(pair) = body.get(offset..).and_then(|rest| rest.get(..29 * 4)) else {
                break;
            };
            let operand = |i| word(pair, i * 4).expect("complete operand");
            if !OPERANDS.iter().all(|&(i, value)| operand(i) == value) {
                continue;
            }
            let name_offset = (offset as i64) + 0x34 + i64::from(operand(21) as i32)
                - i64::from(operand(2) as i32);
            let Some(name) = usize::try_from(name_offset)
                .ok()
                .and_then(|offset| body.get(offset..))
                .and_then(|rest| rest.split(|&byte| byte == 0).next())
            else {
                continue;
            };
            let Some(name) =
                encoding_rs::SHIFT_JIS.decode_without_bom_handling_and_without_replacement(name)
            else {
                continue;
            };
            let Ok(name) = crate::member_key(&name) else {
                continue;
            };
            if !name.ends_with(".lpk") || name.contains('/') {
                continue;
            }
            let key = (operand(8), operand(10));
            if keys.insert(name, key).is_some_and(|old| old != key) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "conflicting LPK key definitions",
                ));
            }
        }
        Ok(keys)
    }
}

pub(crate) fn word(bytes: &[u8], offset: usize) -> Option<u32> {
    let word = bytes.get(offset..)?.first_chunk::<4>()?;
    Some(u32::from_le_bytes(*word))
}
