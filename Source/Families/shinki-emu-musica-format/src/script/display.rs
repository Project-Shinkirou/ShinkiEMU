use super::ScCommand;

/// Original DLL's 131-rolling identity. Embedded NUL bytes in table records are skipped.
pub fn musica_display_hash(bytes: &[u8]) -> u32 {
    bytes.iter().filter(|byte| **byte != 0).fold(0u32, |hash, byte| {
        hash.wrapping_mul(131).wrapping_add(u32::from(*byte))
    }) & 0x7fff_ffff
}
/// The display endpoint receives a C string, so runtime input ends at its first NUL.
pub fn musica_display_cstr_hash(bytes: &[u8]) -> u32 {
    musica_display_hash(bytes.split(|byte| *byte == 0).next().unwrap_or_default())
}
impl ScCommand {
    /// Identity comes from original operand byte tokens before encoding decode.
    pub fn message_display_hashes(&self) -> Option<(u32, Option<u32>)> {
        if self.opcode != "message" { return None; }
        let mut tokens = self.raw_operands.split(|byte| matches!(byte, b' ' | b'\t'));
        tokens.next()?;
        tokens.next()?;
        let speaker = tokens.next()?;
        let speaker = (!speaker.is_empty()).then(|| musica_display_cstr_hash(speaker));
        let first = tokens.next()?;
        let mut hash = 0u32;
        let mut ended = false;
        for (index, token) in std::iter::once(first).chain(tokens).enumerate() {
            if ended { break; }
            if index != 0 { hash = hash.wrapping_mul(131).wrapping_add(u32::from(b' ')); }
            for byte in token {
                if *byte == 0 { ended = true; break; }
                hash = hash.wrapping_mul(131).wrapping_add(u32::from(*byte));
            }
        }
        Some((hash & 0x7fff_ffff, speaker))
    }
}
