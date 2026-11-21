use super::*;
use shinki_emu_musica_format::ScriptEncoding;

enum Token<'a> { Word(&'a [u8]), String(&'a [u8]), Punct(u8) }
fn token<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<Token<'a>> {
    while *cursor < bytes.len() {
        let start = *cursor;
        let byte = bytes[start];
        *cursor += 1;
        if byte.is_ascii_whitespace() { continue; }
        if byte == b'/' && bytes.get(*cursor) == Some(&b'/') {
            while *cursor < bytes.len() && bytes[*cursor] != b'\n' { *cursor += 1; }
            continue;
        }
        if byte == b'/' && bytes.get(*cursor) == Some(&b'*') {
            *cursor += 1;
            let end = bytes.get(*cursor..)?.windows(2).position(|b| b == b"*/")?;
            *cursor += end + 2;
            continue;
        }
        // Do not reinterpret regexp data as builtin calls. Ambiguous division
        // syntax is conservatively skipped too; this is not a TJS interpreter.
        if byte == b'/' {
            while *cursor < bytes.len() {
                if bytes[*cursor] == b'\\' { *cursor += 2; continue; }
                if bytes[*cursor] == b'/' { *cursor += 1; return Some(Token::Punct(b'/')); }
                *cursor += 1;
            }
            return None;
        }
        if matches!(byte, b'\'' | b'"') {
            let content = *cursor;
            while *cursor < bytes.len() {
                if bytes[*cursor] == b'\\' { *cursor += 2; continue; }
                if bytes[*cursor] == byte {
                    let end = *cursor;
                    *cursor += 1;
                    return Some(Token::String(&bytes[content..end]));
                }
                *cursor += 1;
            }
            return None;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            while *cursor < bytes.len() && (bytes[*cursor].is_ascii_alphanumeric() || bytes[*cursor] == b'_') { *cursor += 1; }
            return Some(Token::Word(&bytes[start..*cursor]));
        }
        return Some(Token::Punct(byte));
    }
    None
}
fn call<'a>(bytes: &'a [u8], cursor: &mut usize, namespace: &[u8], method: &[u8]) -> bool {
    matches!(token(bytes, cursor), Some(Token::Word(word)) if word == namespace)
        && matches!(token(bytes, cursor), Some(Token::Punct(b'.')))
        && matches!(token(bytes, cursor), Some(Token::Word(word)) if word == method)
        && matches!(token(bytes, cursor), Some(Token::Punct(b'(')))
}
fn text(bytes: &[u8]) -> Option<String> {
    if let Some(bytes) = bytes.strip_prefix(b"\xff\xfe") {
        if bytes.len() % 2 != 0 { return None; }
        return char::decode_utf16(bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]]))).collect::<Result<String, _>>().ok();
    }
    std::str::from_utf8(bytes).ok().map(str::to_owned)
        .or_else(|| ScriptEncoding::ShiftJis.decode(bytes).map(|s| s.into_owned()))
}
impl<T: DetectionTree> Scan<'_, T> {
    pub(super) fn loose_krkr(&mut self, root: &Path, files: &[PathBuf]) -> DetectionResult<()> {
        let Some(path) = files.iter().find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("startup.tjs"))) else { return Ok(()); };
        let bytes = self.resource_bytes(root, path)?;
        let Some(source) = text(&bytes) else { return Ok(()); };
        let mut cursor = 0;
        while cursor < source.len() {
            self.checkpoint()?;
            let start = cursor;
            if call(source.as_bytes(), &mut cursor, b"Scripts", b"execStorage") {
                if let Some(Token::String(raw)) = token(source.as_bytes(), &mut cursor) {
                    if !matches!(token(source.as_bytes(), &mut cursor), Some(Token::Punct(b')'))) { continue; }
                    if raw.len() > 4096 { continue; }
                    let Ok(name) = std::str::from_utf8(raw) else { continue; };
                    let name = name.replace("\\\\", "/").replace("\\/", "/");
                    if name.contains('\\') { continue; }
                    let Some(target) = self.relative_path(root, &name)? else { continue; };
                    let bytes = self.resource_bytes(root, &target)?;
                    let Some(target) = text(&bytes) else { continue; };
                    // A mere literal/path is not enough: the actual target also
                    // needs a KiriKiri builtin call outside comments/string data.
                    let mut target_cursor = 0;
                    while target_cursor < target.len() {
                        self.checkpoint()?;
                        let saved = target_cursor;
                        for (namespace, method) in [(b"Scripts".as_slice(), b"execStorage".as_slice()), (b"System", b"inform"), (b"Storages", b"addAutoPath"), (b"Plugins", b"link")] {
                            target_cursor = saved;
                            if call(target.as_bytes(), &mut target_cursor, namespace, method) { return self.matched("krkr", "tjs"); }
                        }
                        target_cursor = saved;
                        if token(target.as_bytes(), &mut target_cursor).is_none() { break; }
                    }
                }
            }
            cursor = start;
            if token(source.as_bytes(), &mut cursor).is_none() { break; }
        }
        Ok(())
    }
}
