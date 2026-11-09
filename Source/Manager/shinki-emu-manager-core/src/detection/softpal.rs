use super::*;
use super::formats::Rejected;

impl Scan<'_> {
    pub(super) fn softpal_pac(&mut self, source: &mut Source) -> Result<(), Rejected> {
        // pal-asset/pac.rs: 255 buckets at 0x0c, 40-byte records at 0x804.
        // PAC itself has no unique magic: require its actual Script.src Sv20 header.
        if source.length < 0x804 { return Err(Rejected::NoMatch); }
        let header = self.bytes(source, 0, 0x804)?;
        let word = |bytes: &[u8], at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as u64;
        // Reject unrelated large files before any record-table allocation.
        for bucket in header[12..].chunks_exact(8) {
            self.checkpoint()?;
            let count = word(bucket, 4);
            if count != 0 && 0x804 + (word(bucket, 0) + count) * 40 > source.length { return Err(Rejected::NoMatch); }
        }
        let mut script = None;
        for bucket in header[12..].chunks_exact(8) {
            self.checkpoint()?;
            let first = word(bucket, 0);
            let count = word(bucket, 4);
            if count == 0 { continue; }
            let records = self.bytes(source, 0x804 + first * 40, (count * 40) as usize)?;
            for record in records.chunks_exact(40) {
                self.checkpoint()?;
                let size = word(record, 32);
                let offset = word(record, 36);
                if offset + size > source.length { return Err(Rejected::NoMatch); }
                let end = record[..32].iter().position(|b| *b == 0).unwrap_or(32);
                if record[..end].eq_ignore_ascii_case(b"Script.src") { script = Some((offset, size)); }
            }
        }
        let Some((offset, size)) = script else { return Err(Rejected::NoMatch); };
        if size < 13 { return Err(Rejected::NoMatch); }
        let mut header = [0; 12];
        self.read_at(source, offset, &mut header)?;
        let entry = word(&header, 8);
        if &header[..4] != b"Sv20" || entry < 12 || entry >= size { return Err(Rejected::NoMatch); }
        self.matched("softpal", "softpal.sv20")?;
        Ok(())
    }
}
