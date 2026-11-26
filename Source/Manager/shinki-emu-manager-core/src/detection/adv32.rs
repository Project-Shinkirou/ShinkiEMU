use super::*;

const MAX_SCRIPT_BYTES: u64 = 1024 * 1024;
const ENTRY_FLAG: u32 = 1 << 31;

impl<T: DetectionTree> Scan<'_, T> {
    pub(super) fn adv32(&mut self, data: &Path) -> DetectionResult<()> {
        let Some(path) = self.relative_path(data, "MES/start.mes")? else {
            return Ok(());
        };
        let Some(mut source) = self.open(&data.join(path), 0)? else {
            return Ok(());
        };
        if self.files == MAX_FILES {
            return Err(DetectionError::ReadLimit);
        }
        self.files += 1;
        if !(14..=MAX_SCRIPT_BYTES).contains(&source.length) {
            return Ok(());
        }

        // One u32 count, then a u32 offset and two auxiliary bytes per entry.
        // Offsets are relative to the dialect marker, not to the file start.
        let mut header = [0; 8];
        self.read_at(&mut source, 0, &mut header)?;
        let count = u64::from(u32::from_le_bytes(header[..4].try_into().unwrap()));
        let first = u32::from_le_bytes(header[4..].try_into().unwrap()) & !ENTRY_FLAG;
        let code_at = 4 + count * 6;
        if count == 0 || first != 3 || code_at + 4 > source.length {
            return Ok(());
        }
        let mut marker = [0; 3];
        self.read_at(&mut source, code_at, &mut marker)?;
        let version = u16::from_le_bytes([marker[0], marker[1]]);
        if marker[2] != 0 || !matches!(version, 0x9e97 | 0x9f9a | 0xa6b9 | 0xaab6 | 0xabb6) {
            return Ok(());
        }

        let code_size = source.length - code_at;
        let mut offsets = [0; 4096];
        let mut at = 8;
        let end = 4 + count * 4;
        while at < end {
            let chunk = (end - at).min(offsets.len() as u64) as usize;
            self.read_at(&mut source, at, &mut offsets[..chunk])?;
            if offsets[..chunk].as_chunks::<4>().0.iter().any(|bytes| {
                let offset = u64::from(u32::from_le_bytes(*bytes) & !ENTRY_FLAG);
                !(3..code_size).contains(&offset)
            }) {
                return Ok(());
            }
            at += chunk as u64;
        }
        // Instruction framing and entrypoint boundaries are verified by the core.
        self.matched("adv32", "adv32.mes")
    }
}
