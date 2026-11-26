//! Format facts and source pins are documented in docs/*/core-abi.md.
//! File extensions only route magic-less/ambiguous inputs; never establish a match.
use super::*;
use shinki_emu_lucifen_formats::{FORMAT_ID, SCRIPT_ARCHIVE, has_script_index};

pub(super) enum Rejected {
    NoMatch,
    Abort(DetectionError),
}
impl From<DetectionError> for Rejected {
    fn from(error: DetectionError) -> Self {
        Self::Abort(error)
    }
}
type ScanResult<T> = Result<T, Rejected>;

fn u32le(bytes: &[u8], offset: usize) -> u64 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as u64
}
fn u64le(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
fn require(condition: bool) -> ScanResult<()> {
    if condition {
        Ok(())
    } else {
        Err(Rejected::NoMatch)
    }
}
fn range(offset: u64, size: u64, length: u64) -> ScanResult<()> {
    require(offset.checked_add(size).is_some_and(|end| end <= length))
}
fn nul_name(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .position(|b| *b == 0)
        .is_some_and(|end| end > 0 && bytes[..end].iter().all(|b| *b >= 32))
}
fn extension(path: &Path, value: &str) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(value))
}

impl<T: DetectionTree> Scan<'_, T> {
    pub(super) fn formats(
        &mut self,
        path: &Path,
        source: &mut Source,
        head: &[u8],
    ) -> ScanResult<()> {
        self.checkpoint()?;
        if head.starts_with(b"CPZ") {
            return self.cpz(source, head);
        }
        if head.starts_with(b"PS2A") {
            require(head.len() >= 48)?;
            require(u32le(head, 4) == 48)?;
            let decoded = u32le(head, 16) * 4 + u32le(head, 20) + u32le(head, 24) + u32le(head, 28);
            require(decoded > 0 && u32le(head, 32) < u32le(head, 20))?;
            let packed = u32le(head, 36);
            range(
                48,
                if packed == 0 { decoded } else { packed },
                source.length,
            )?;
            self.matched("cmvs", "cmvs.ps2a")?;
            return Ok(());
        }
        if head.starts_with(b"pf2") || head.starts_with(b"pf6") || head.starts_with(b"pf8") {
            return self.pfs(source, head);
        }
        if head.starts_with(b"PackFile") || head.starts_with(b"BURIKO ARC20") {
            // A BGI archive in a look-ahead directory belongs to that
            // candidate, not to the parent being scanned.
            require(path.parent() == Some(Path::new("")))?;
            return self.bgi(source, head);
        }
        if head.starts_with(b"Sv20") {
            require(head.len() >= 12)?;
            require(u32le(head, 8) >= 12 && u32le(head, 8) < source.length)?;
            self.matched("softpal", "softpal.sv20")?;
            return Ok(());
        }
        const XP3: &[u8] = b"XP3\r\n \n\x1a\x8b\x67\x01";
        if head.starts_with(XP3) {
            require(head.len() >= 19)?;
            let offset = u64le(head, 11);
            require(offset >= 19)?;
            let mut index = [0; 17];
            range(offset, 9, source.length)?;
            self.read_at(source, offset, &mut index[..9])?;
            require(index[0] & 0x78 == 0 && index[0] & 7 <= 1)?;
            let size = u64le(&index, 1);
            let prefix = if index[0] & 7 == 1 { 17 } else { 9 };
            range(offset, prefix, source.length)?;
            if prefix == 17 {
                self.read_at(source, offset + 9, &mut index[9..])?;
            }
            range(offset + prefix, size, source.length)?;
            self.matched("krkr", "xp3")?;
            return Ok(());
        }
        if head.starts_with(b"MPK\0") {
            return self.mpk(source, head);
        }
        if head.starts_with(b"RPA-") {
            return self.rpa(source, head);
        }
        if head.starts_with(b"RENPY RPC2") {
            let mut cursor = 10;
            let mut slots = 0;
            loop {
                self.checkpoint()?;
                let mut entry = [0; 12];
                range(cursor, 12, source.length)?;
                self.read_at(source, cursor, &mut entry)?;
                let slot = u32le(&entry, 0);
                if slot == 0 {
                    break;
                }
                require(slot <= 2 && slots < 2)?;
                require(u32le(&entry, 4) >= cursor + 12 && u32le(&entry, 8) > 0)?;
                range(u32le(&entry, 4), u32le(&entry, 8), source.length)?;
                slots += 1;
                cursor += 12;
            }
            require(slots > 0)?;
            self.matched("renpy", "rpyc")?;
            return Ok(());
        }
        if head.starts_with(b"LPK1") {
            require(path.parent() == Some(Path::new("")))?;
            require(
                path.file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case(SCRIPT_ARCHIVE)),
            )?;
            require(has_script_index(head, source.length))?;
            self.matched("lucifen", FORMAT_ID)?;
            return Ok(());
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if head.starts_with(b"pack_scn")
            || name.eq_ignore_ascii_case("scene.pck")
            || name.eq_ignore_ascii_case("scene.chs")
        {
            if !self.siglus.allows(path) {
                return Err(Rejected::NoMatch);
            }
            return self.siglus(source, head);
        }
        if extension(path, "hcb") || extension(path, "bch") {
            return self.hcb(source, head);
        }
        if extension(path, "rpyc")
            && head.len() >= 2
            && head[0] & 15 == 8
            && head[0] >> 4 <= 7
            && u16::from_be_bytes([head[0], head[1]]) % 31 == 0
        {
            return self.legacy_rpyc(source);
        }
        if source.length >= 0x804 {
            return self.softpal_pac(source);
        }
        Ok(())
    }
    fn cpz(&mut self, source: &mut Source, head: &[u8]) -> ScanResult<()> {
        require(head.len() >= 4)?;
        let (minimum, format) = match head[3] {
            b'1' | b'2' => return Ok(()),
            b'3' => (20, "cmvs.cpz3"),
            b'5' => (64, "cmvs.cpz5"),
            b'6' => (64, "cmvs.cpz6"),
            b'7' => (72, "cmvs.cpz7"),
            _ => return Ok(()),
        };
        require(head.len() >= minimum)?;
        if head[3] == b'3' {
            let count = u32le(head, 4) ^ 0x5e9c4f37;
            let size = u32le(head, 8) ^ 0xf32aed17;
            require(count > 0 && count * 25 <= size)?;
            range(20, size, source.length)?;
        } else {
            let old = head[3] == b'5';
            let count = u32le(head, 4) ^ if old { 0xfe3a53d9 } else { 0xfe3a53da };
            let dirs = u32le(head, 8) ^ if old { 0x37f298e7 } else { 0x37f298e8 };
            let files = u32le(head, 12) ^ if old { 0x7a6f3a2c } else { 0x7a6f3a2d };
            let key = if head[3] == b'7' {
                u32le(head, 64) ^ 0x65ef99f3
            } else {
                0
            };
            require(count > 0 && count * 17 <= dirs && files > 0)?;
            if head[3] == b'7' {
                require(key >= 24)?;
            }
            range(minimum as u64, dirs + files + key, source.length)?;
        }
        // The encrypted index and checksums remain selected-core validation work.
        self.matched("cmvs", format)?;
        Ok(())
    }
    fn pfs(&mut self, source: &mut Source, head: &[u8]) -> ScanResult<()> {
        require(head.len() >= 11)?;
        let size = u32le(head, 3);
        let count = u32le(head, 7);
        require(count > 0 && size >= 4 && count <= (size - 4) / 17)?;
        range(7, size, source.length)?;
        let bytes = self.bytes(source, 7, size as usize)?;
        let mut cursor = 4;
        let mut project = false;
        for _ in 0..count {
            self.checkpoint()?;
            require(cursor + 4 <= bytes.len())?;
            let name_len = u32le(&bytes, cursor) as usize;
            cursor += 4;
            require(name_len > 0 && name_len <= 4096)?;
            require(cursor + name_len + 12 <= bytes.len())?;
            let name = &bytes[cursor..cursor + name_len];
            let name = name.strip_suffix(&[0]).unwrap_or(name);
            project |= name.eq_ignore_ascii_case(b"system.ini");
            cursor += name_len + 4;
            let offset = u32le(&bytes, cursor);
            require(offset >= 7 + size)?;
            range(offset, u32le(&bytes, cursor + 4), source.length)?;
            cursor += 8;
        }
        if project {
            self.matched("artemis", "artemis.pfs")?;
        }
        Ok(())
    }
    fn bgi(&mut self, source: &mut Source, head: &[u8]) -> ScanResult<()> {
        require(head.len() >= 16)?;
        let modern = head.starts_with(b"BURIKO ARC20");
        let (record, name_size) = if modern { (128, 96) } else { (32, 16) };
        let count = u32le(head, 12);
        require(count > 0)?;
        let table_size = count * record;
        let base = 16 + table_size;
        range(16, table_size, source.length)?;
        let table = self.bytes(source, 16, table_size as usize)?;
        let mut bootstrap = false;
        for entry in table.chunks_exact(record as usize) {
            self.checkpoint()?;
            require(nul_name(&entry[..name_size]))?;
            let name = &entry[..entry[..name_size].iter().position(|b| *b == 0).unwrap()];
            bootstrap |= name.eq_ignore_ascii_case(b"ipl._bp");
            range(
                base + u32le(entry, name_size),
                u32le(entry, name_size + 4),
                source.length,
            )?;
        }
        // A bootstrap entry disambiguates generic PackFile archives.
        if bootstrap || modern {
            self.matched("ethornell", "bgi-bp-arc")?;
        }
        Ok(())
    }
    fn mpk(&mut self, source: &mut Source, head: &[u8]) -> ScanResult<()> {
        require(head.len() >= 12)?;
        require(&head[4..8] == b"\0\0\x02\0")?;
        let count = u32le(head, 8);
        require(count > 0)?;
        range(64, count * 256, source.length)?;
        let table = self.bytes(source, 64, (count * 256) as usize)?;
        let base = 64 + count * 256;
        for entry in table.chunks_exact(256) {
            self.checkpoint()?;
            require(u32le(entry, 0) <= 1 && nul_name(&entry[32..]))?;
            require(u64le(entry, 8) >= base)?;
            range(u64le(entry, 8), u64le(entry, 16), source.length)?;
        }
        self.matched("mages", "mpk")?;
        Ok(())
    }
    fn rpa(&mut self, source: &mut Source, head: &[u8]) -> ScanResult<()> {
        require(head.len() >= 8)?;
        require(head.starts_with(b"RPA-2.0 ") || head.starts_with(b"RPA-3.0 "))?;
        let minimum = if head[4] == b'3' { 34 } else { 25 };
        require(head.len() >= minimum)?;
        let number = |value: &[u8]| {
            std::str::from_utf8(value)
                .ok()
                .and_then(|s| u64::from_str_radix(s, 16).ok())
        };
        let offset = number(&head[8..24]).ok_or(Rejected::NoMatch)?;
        if minimum == 34 {
            require(head[24] == b' ' && number(&head[25..33]).is_some())?;
        }
        require(offset >= minimum as u64)?;
        let mut zlib = [0; 2];
        range(offset, 2, source.length)?;
        self.read_at(source, offset, &mut zlib)?;
        require(zlib[0] & 15 == 8 && zlib[0] >> 4 <= 7 && (u16::from_be_bytes(zlib) % 31 == 0))?;
        self.matched("renpy", "rpa")?;
        Ok(())
    }
    fn siglus(&mut self, source: &mut Source, head: &[u8]) -> ScanResult<()> {
        let prefix = if head.starts_with(b"pack_scn") { 8 } else { 0 };
        require(head.len() >= prefix + 92)?;
        let header = &head[prefix..];
        let size = u32le(header, 0);
        require(size >= 92 && size <= 4096 && size % 4 == 0)?;
        range(prefix as u64, size, source.length)?;
        let count = u32le(header, 56);
        require(
            count > 0
                && count == u32le(header, 64)
                && count == u32le(header, 72)
                && count == u32le(header, 80),
        )?;
        // Validate all fixed-width table ranges; UTF-16 names and payload remain encrypted.
        for (field, width) in [(4, 8), (12, 8), (28, 8), (36, 8), (52, 8), (68, 8)] {
            self.checkpoint()?;
            let offset = u32le(header, field);
            let entries = u32le(header, field + 4);
            if entries > 0 {
                require(offset >= size + prefix as u64)?;
                range(offset, entries * width, source.length)?;
            }
        }
        let data = u32le(header, 76);
        require(data >= prefix as u64 + size && data < source.length)?;
        let indexes = self.bytes(source, u32le(header, 68), (count * 8) as usize)?;
        for entry in indexes.chunks_exact(8) {
            self.checkpoint()?;
            require(u32le(entry, 4) > 0)?;
            range(data + u32le(entry, 0), u32le(entry, 4), source.length)?;
        }
        self.matched("siglus", "siglus.scene_pck")?;
        Ok(())
    }
    fn hcb(&mut self, source: &mut Source, head: &[u8]) -> ScanResult<()> {
        require(head.len() >= 4)?;
        let descriptor = u32le(head, 0);
        require(descriptor >= 5 && descriptor < source.length)?;
        let bytes = self.bytes(
            source,
            descriptor,
            (source.length - descriptor)
                .try_into()
                .map_err(|_| Rejected::NoMatch)?,
        )?;
        require(bytes.len() >= 15)?;
        require(u32le(&bytes, 0) >= 4 && u32le(&bytes, 0) < descriptor && bytes[8] <= 15)?;
        let title = bytes[10] as usize;
        require(title > 0 && 11 + title + 2 <= bytes.len())?;
        require(bytes[10 + title] == 0)?;
        let mut cursor = 11 + title;
        let count = u16::from_le_bytes(bytes[cursor..cursor + 2].try_into().unwrap());
        cursor += 2;
        require(count > 0)?;
        for _ in 0..count {
            self.checkpoint()?;
            require(cursor + 2 <= bytes.len())?;
            let len = bytes[cursor + 1] as usize;
            cursor += 2;
            require(len > 1 && cursor + len <= bytes.len())?;
            require(
                bytes[cursor + len - 1] == 0
                    && bytes[cursor..cursor + len - 1]
                        .iter()
                        .all(|b| b.is_ascii_alphanumeric() || *b == b'_'),
            )?;
            cursor += len;
        }
        // The native parser stops after custom_syscall_count; converters may
        // append metadata. Trailing bytes do not invalidate this descriptor.
        require(cursor + 2 <= bytes.len())?;
        self.matched("fvp", "fvp.hcb")?;
        Ok(())
    }
}
