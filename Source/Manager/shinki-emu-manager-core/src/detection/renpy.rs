use super::formats::Rejected;
use super::*;

impl<T: DetectionTree> Scan<'_, T> {
    pub(super) fn legacy_rpyc(&mut self, source: &mut Source) -> Result<(), Rejected> {
        let compressed = self.bytes(
            source,
            0,
            source.length.try_into().map_err(|_| Rejected::NoMatch)?,
        )?;
        let mut decoder = flate2::read::ZlibDecoder::new(compressed.as_slice());
        let mut decoded = Vec::new();
        let mut chunk = [0; 8192];
        loop {
            self.checkpoint()?;
            let count = match decoder.read(&mut chunk) {
                Ok(count) => count,
                Err(_) => return Err(Rejected::NoMatch),
            };
            if count == 0 {
                break;
            }
            if decoded.len() + count > MAX_FILE_READ_BYTES as usize {
                return Err(DetectionError::ReadLimit.into());
            }
            decoded.extend_from_slice(&chunk[..count]);
        }
        if !renpy_ast_pickle(&decoded, self.cancellation) {
            self.checkpoint()?;
            return Err(Rejected::NoMatch);
        }
        self.matched("renpy", "rpyc")?;
        Ok(())
    }
}

/// Walk pickle opcode boundaries as DATA. GLOBAL/REDUCE/BUILD are never called.
/// A complete pickle declaring a renpy.ast class establishes the serialized AST
/// format. Generic zlib streams and generic pickled dictionaries do not.
fn renpy_ast_pickle(bytes: &[u8], cancellation: &NativeCancellation) -> bool {
    fn line<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
        let end = bytes.get(*cursor..)?.iter().position(|b| *b == b'\n')? + *cursor;
        let value = &bytes[*cursor..end];
        *cursor = end + 1;
        Some(value)
    }
    fn length(bytes: &[u8], cursor: &mut usize, count: usize) -> Option<usize> {
        let field = bytes.get(*cursor..cursor.checked_add(count)?)?;
        *cursor += count;
        let mut value = [0; 8];
        value[..count].copy_from_slice(field);
        usize::try_from(u64::from_le_bytes(value)).ok()
    }
    fn identity(module: &[u8], class: &[u8]) -> bool {
        module == b"renpy.ast"
            && !class.is_empty()
            && class
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
    }
    let mut cursor = 0;
    let mut ast = false;
    let mut strings: Vec<&[u8]> = Vec::new();
    let mut pending_ast = false;
    while cursor < bytes.len() {
        if cancellation.is_cancelled() {
            return false;
        }
        let opcode = bytes[cursor];
        cursor += 1;
        match opcode {
            b'.' => return ast && cursor == bytes.len(),
            b'c' | b'i' => {
                let Some(module) = line(bytes, &mut cursor) else {
                    return false;
                };
                let Some(class) = line(bytes, &mut cursor) else {
                    return false;
                };
                pending_ast = identity(module, class);
                if opcode == b'i' {
                    ast |= pending_ast;
                    pending_ast = false;
                }
                strings.clear();
            }
            0x93 => {
                pending_ast = strings.len() == 2 && identity(strings[0], strings[1]);
                strings.clear();
            }
            b'U' | b'T' | b'X' | b'B' | b'C' | 0x8c | 0x8d | 0x8e | 0x96 => {
                let width = match opcode {
                    b'U' | b'C' | 0x8c => 1,
                    0x8d | 0x8e | 0x96 => 8,
                    _ => 4,
                };
                let Some(size) = length(bytes, &mut cursor, width) else {
                    return false;
                };
                let Some(end) = cursor.checked_add(size) else {
                    return false;
                };
                let Some(value) = bytes.get(cursor..end) else {
                    return false;
                };
                if matches!(opcode, b'U' | b'T' | b'X' | 0x8c | 0x8d) {
                    if strings.len() == 2 {
                        strings.remove(0);
                    }
                    strings.push(value);
                } else {
                    strings.clear();
                }
                cursor = end;
            }
            b'V' => {
                let Some(value) = line(bytes, &mut cursor) else {
                    return false;
                };
                if strings.len() == 2 {
                    strings.remove(0);
                }
                strings.push(value);
            }
            b'F' | b'I' | b'L' | b'P' | b'S' | b'g' => {
                if line(bytes, &mut cursor).is_none() {
                    return false;
                }
                strings.clear();
            }
            b'p' => {
                if line(bytes, &mut cursor).is_none() {
                    return false;
                }
            }
            b'q' | b'r' | 0x94 => {
                let width = if opcode == b'q' {
                    1
                } else if opcode == b'r' {
                    4
                } else {
                    0
                };
                if length(bytes, &mut cursor, width).is_none() {
                    return false;
                }
            }
            0x80 => {
                let Some(protocol) = bytes.get(cursor) else {
                    return false;
                };
                if *protocol > 5 {
                    return false;
                }
                cursor += 1;
            }
            0x95 => {
                let Some(size) = length(bytes, &mut cursor, 8) else {
                    return false;
                };
                if cursor.checked_add(size).is_none_or(|end| end > bytes.len()) {
                    return false;
                }
            }
            b'J' | b'K' | b'M' | b'G' | b'h' | b'j' | 0x82 | 0x83 | 0x84 => {
                let width = match opcode {
                    b'K' | b'h' | 0x82 => 1,
                    b'M' | 0x83 => 2,
                    b'G' => 8,
                    _ => 4,
                };
                if length(bytes, &mut cursor, width).is_none() {
                    return false;
                }
                strings.clear();
            }
            0x8a | 0x8b => {
                let Some(size) = length(bytes, &mut cursor, if opcode == 0x8a { 1 } else { 4 })
                else {
                    return false;
                };
                let Some(end) = cursor.checked_add(size).filter(|end| *end <= bytes.len()) else {
                    return false;
                };
                cursor = end;
                strings.clear();
            }
            b'R' | b'o' | 0x81 | 0x92 => {
                ast |= pending_ast;
                pending_ast = false;
                strings.clear();
            }
            b'('
            | b'0'
            | b'1'
            | b'2'
            | b'N'
            | b'Q'
            | b'a'
            | b'b'
            | b'd'
            | b'e'
            | b'l'
            | b's'
            | b't'
            | b'u'
            | b')'
            | b']'
            | b'}'
            | 0x85..=0x89
            | 0x8f..=0x91
            | 0x97
            | 0x98 => strings.clear(),
            _ => return false,
        }
    }
    false
}
