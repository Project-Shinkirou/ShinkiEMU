//! Lucifen LPK1. Format references: GARbro (morkt, MIT) and Lucifen-Tools (UBTL, MIT).

use crate::sob::word;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
};

pub const CHUNK: usize = 1024 * 1024;
pub const SCRIPT_ARCHIVE: &str = "script.lpk";
const MAX_INDEX: u64 = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 1_048_576;
const MAX_MEMBER: usize = 512 * 1024 * 1024;
const INDEX_ALIGNED: u8 = 1;
const INDEX_TRIE: u8 = 2;
const HEADER_CRYPT: u8 = 4;
const PACKED_ENTRIES: u8 = 8;
const CONTENT_CRYPT: u8 = 16;

fn header_code(bytes: &[u8]) -> Option<u32> {
    word(bytes.strip_prefix(b"LPK1")?, 0)
}

fn index_layout(code: u32, size: u64) -> Option<(u8, usize)> {
    let flags = (code >> 24) as u8;
    if flags & INDEX_TRIE == 0 || flags & !0x1f != 0 {
        return None;
    }
    let length = u64::from(code & 0x00ff_ffff);
    let length = if flags & INDEX_ALIGNED != 0 {
        (length << 11).saturating_sub(8)
    } else {
        length
    };
    ((10..=MAX_INDEX).contains(&length) && length + 8 <= size).then_some((flags, length as usize))
}

/// Recognize the bootstrap archive's LPK1 header and in-file index span.
/// The caller checks the root archive name; content validation belongs to `Archive::open`.
pub fn has_script_index(header: &[u8], size: u64) -> bool {
    header_code(header).is_some_and(|code| {
        SCHEMES.iter().any(|scheme| {
            index_layout(code ^ scheme.for_stem(b"SCRIPT", (0, 0)).index_key, size).is_some()
        })
    })
}

#[derive(Clone, Copy)]
struct Scheme {
    index_key: u32,
    entry_key: u32,
    rotate: u32,
    content_xor: u8,
}

const SCHEMES: [Scheme; 2] = [
    Scheme {
        entry_key: 0xa5b9_ac6b,
        index_key: 0x9a63_9de5,
        rotate: 0x3174_6285,
        content_xor: 0x5d,
    },
    Scheme {
        entry_key: 0x39a4_e1fd,
        index_key: 0xd63b_1e29,
        rotate: 0x3614_7352,
        content_xor: 0xa6,
    },
];

impl Scheme {
    fn for_stem(mut self, stem: &[u8], auxiliary: (u32, u32)) -> Self {
        for (&forward, &backward) in stem.iter().zip(stem.iter().rev()) {
            self.index_key = (self.index_key ^ u32::from(forward)).rotate_left(7);
            self.entry_key = (self.entry_key ^ u32::from(backward)).rotate_right(7);
        }
        self.entry_key ^= auxiliary.0;
        self.index_key ^= auxiliary.1;
        self
    }

    fn index(self, bytes: &mut [u8]) {
        let (mut key, mut pattern) = (self.index_key, self.rotate);
        for word in bytes.chunks_exact_mut(4) {
            xor_word(word, key);
            pattern = pattern.rotate_left(4);
            key = key.rotate_right(pattern & 31);
        }
    }

    fn entry_header(self, bytes: &mut [u8]) {
        let (mut key, mut pattern) = (self.entry_key, self.rotate);
        let length = bytes.len().min(256);
        for word in bytes[..length].chunks_exact_mut(4) {
            xor_word(word, key);
            pattern = pattern.rotate_right(4);
            key = key.rotate_left(pattern & 31);
        }
    }
}

fn xor_word(bytes: &mut [u8], key: u32) {
    for (byte, key) in bytes.iter_mut().zip(key.to_le_bytes()) {
        *byte ^= key;
    }
}

pub fn member_key(name: &str) -> io::Result<String> {
    let name = name.replace('\\', "/");
    if name
        .split('/')
        .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || name.chars().any(|ch| ch.is_control() || ch == ':')
    {
        return Err(invalid("invalid LPK member name"));
    }
    Ok(name.to_ascii_lowercase())
}

#[derive(Debug)]
pub struct Entry {
    name: String,
    record: usize,
    offset: u64,
    size: u32,
    unpacked: u32,
    stored: u32,
}

impl Entry {
    pub fn name(&self) -> &str {
        &self.name
    }
}

pub struct Archive {
    entries: BTreeMap<String, Entry>,
    index: Vec<u8>,
    prefix: Vec<u8>,
    code: u32,
    scheme: Scheme,
    content_crypt: bool,
}

impl Archive {
    pub fn open(
        name: &str,
        size: u64,
        auxiliary: Option<(u32, u32)>,
        mut read: impl FnMut(u64, &mut [u8]) -> io::Result<()>,
    ) -> io::Result<Self> {
        let name = member_key(name)?;
        let stem = name
            .strip_suffix(".lpk")
            .filter(|s| !s.contains('/'))
            .ok_or_else(|| invalid("expected an LPK archive name"))?;
        let stem = stem.to_uppercase();
        let (encoded, _, errors) = encoding_rs::SHIFT_JIS.encode(&stem);
        if errors {
            return Err(invalid("LPK archive name cannot be represented in CP932"));
        }
        let mut header = [0; 8];
        read(0, &mut header)?;
        let header = header_code(&header).ok_or_else(|| invalid("invalid LPK1 signature"))?;
        for scheme in SCHEMES {
            let scheme = scheme.for_stem(&encoded, auxiliary.unwrap_or_default());
            let code = header ^ scheme.index_key;
            let Some((flags, length)) = index_layout(code, size) else {
                continue;
            };
            let mut index = vec![0; length];
            for (chunk, bytes) in index.chunks_mut(CHUNK).enumerate() {
                read(8 + (chunk * CHUNK) as u64, bytes)?;
            }
            scheme.index(&mut index);
            if let Ok((entries, prefix)) = parse_index(&index, flags, size) {
                return Ok(Self {
                    entries,
                    index,
                    prefix,
                    code,
                    scheme,
                    // The Lucifen patch-archive format reuses this bit.
                    content_crypt: flags & CONTENT_CRYPT != 0 && name != "patch.lpk",
                });
            }
        }
        Err(invalid("unsupported or damaged LPK index"))
    }

    pub fn entry(&self, name: &str) -> io::Result<Option<&Entry>> {
        Ok(self.entries.get(&member_key(name)?))
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    pub fn read_member(
        &self,
        entry: &Entry,
        limit: usize,
        mut read: impl FnMut(u64, &mut [u8]) -> io::Result<()>,
    ) -> io::Result<Vec<u8>> {
        if entry.size == 0 {
            return Ok(Vec::new());
        }
        let decoded = if entry.unpacked != 0 {
            entry.unpacked
        } else {
            entry.stored
        } as usize;
        let total = decoded
            .checked_add(self.prefix.len())
            .ok_or_else(|| invalid("LPK member length overflow"))?;
        if total > limit.min(MAX_MEMBER) || entry.stored as usize > MAX_MEMBER {
            return Err(invalid("LPK member exceeds the decode limit"));
        }
        let mut data = self.prefix.clone();
        if entry.unpacked == 0 {
            data.resize(total, 0);
            read_member_bytes(entry.offset, &mut data[self.prefix.len()..], &mut read)?;
        } else {
            let mut stored = vec![0; entry.stored as usize];
            read_member_bytes(entry.offset, &mut stored, &mut read)?;
            lzss(&stored, decoded, &mut data)?;
        }
        let bytes = &mut data[self.prefix.len()..];
        if self.content_crypt {
            for byte in &mut *bytes {
                *byte = (*byte ^ self.scheme.content_xor).rotate_right(4);
            }
        }
        if self.flags() & HEADER_CRYPT != 0 {
            self.scheme.entry_header(bytes);
        }
        Ok(data)
    }

    pub fn rewrite(
        self,
        mut replacements: Replacements,
        mut read: impl FnMut(u64, &mut [u8]) -> io::Result<()>,
        mut read_replacement: impl FnMut(&str, u64, &mut [u8]) -> io::Result<()>,
        mut output: impl Write,
    ) -> io::Result<()> {
        let flags = self.flags();
        let mut entries: Vec<_> = self.entries.values().collect();
        entries.sort_unstable_by_key(|entry| entry.offset);
        let mut index = self.index.clone();
        let mut next = 8 + index.len() as u64;
        let mut layout = Vec::with_capacity(entries.len());
        for entry in entries {
            let replacement = replacements.names.remove(&entry.name);
            let (size_field, stored_size) = if let Some((_, size)) = &replacement {
                let size_field = u32::try_from(*size)
                    .map_err(|_| invalid("LPK replacement exceeds 32-bit size"))?;
                let stored_size = size_field
                    .checked_sub(self.prefix.len() as u32)
                    .ok_or_else(|| invalid("LPK replacement is shorter than the archive prefix"))?;
                (size_field, stored_size)
            } else {
                (entry.size, entry.stored)
            };
            let raw_offset = if flags & INDEX_ALIGNED != 0 {
                next = (next + 2047) & !2047;
                next >> 11
            } else {
                next
            };
            let offset =
                u32::try_from(raw_offset).map_err(|_| invalid("LPK member offset overflow"))?;
            let record = &mut index[entry.record..];
            record[..4].copy_from_slice(&offset.to_le_bytes());
            record[4..8].copy_from_slice(&size_field.to_le_bytes());
            if replacement.is_some() && flags & PACKED_ENTRIES != 0 {
                record[8..12].fill(0);
            }
            layout.push((entry, replacement, next, stored_size as usize));
            next += u64::from(stored_size);
        }
        if !replacements.names.is_empty() {
            return Err(invalid("replacement is absent from original LPK"));
        }

        output.write_all(b"LPK1")?;
        output.write_all(&(self.code ^ self.scheme.index_key).to_le_bytes())?;
        self.scheme.index(&mut index);
        output.write_all(&index)?;
        let mut written = 8 + index.len() as u64;
        let mut scratch = vec![0; CHUNK];
        for (entry, replacement, offset, size) in layout {
            let padding = (offset - written) as usize;
            scratch[..padding].fill(0);
            output.write_all(&scratch[..padding])?;
            if let Some((path, _)) = &replacement {
                read_replacement(path, 0, &mut [])?;
                let prefix = &mut scratch[..self.prefix.len()];
                if !prefix.is_empty() {
                    read_replacement(path, 0, prefix)?;
                    if *prefix != self.prefix {
                        return Err(invalid("LPK replacement has a different archive prefix"));
                    }
                }
            }
            for start in (0..size).step_by(CHUNK) {
                let bytes = &mut scratch[..CHUNK.min(size - start)];
                if let Some((path, _)) = &replacement {
                    read_replacement(path, (self.prefix.len() + start) as u64, bytes)?;
                    if start == 0 && flags & HEADER_CRYPT != 0 {
                        self.scheme.entry_header(bytes);
                    }
                    if self.content_crypt {
                        for byte in &mut *bytes {
                            *byte = byte.rotate_left(4) ^ self.scheme.content_xor;
                        }
                    }
                } else {
                    read(entry.offset + start as u64, bytes)?;
                }
                output.write_all(bytes)?;
            }
            written = offset + size as u64;
        }
        output.flush()
    }

    fn flags(&self) -> u8 {
        (self.code >> 24) as u8
    }
}

#[derive(Default)]
pub struct Replacements {
    names: BTreeMap<String, (String, u64)>,
}

impl Replacements {
    pub fn insert(&mut self, name: &str, path: String, size: u64) -> io::Result<()> {
        let key = member_key(name)?;
        if self.names.insert(key, (path, size)).is_some() {
            return Err(invalid("conflicting LPK replacements"));
        }
        Ok(())
    }
}

fn parse_index(
    index: &[u8],
    flags: u8,
    size: u64,
) -> io::Result<(BTreeMap<String, Entry>, Vec<u8>)> {
    let err = || invalid("invalid LPK index");
    let count = word(index, 0).ok_or_else(err)? as usize;
    if !(1..=MAX_ENTRIES).contains(&count) {
        return Err(err());
    }
    let width_offset = 5 + index[4] as usize;
    let prefix = index.get(5..width_offset).ok_or_else(err)?.to_vec();
    let width = if *index.get(width_offset).ok_or_else(err)? == 0 {
        2
    } else {
        4
    };
    let trie_start = width_offset + 5;
    let trie_end = trie_start
        .checked_add(word(index, width_offset + 1).ok_or_else(err)? as usize)
        .ok_or_else(err)?;
    let trie = index.get(trie_start..trie_end).ok_or_else(err)?;
    let records = index.get(trie_end..).ok_or_else(err)?;
    let base_stride = if flags & PACKED_ENTRIES != 0 { 12 } else { 8 };
    let stride = (base_stride + 1).min(records.len() / count);
    if stride < base_stride {
        return Err(err());
    }
    let mut entries = BTreeMap::new();
    let mut visited = BTreeSet::new();
    let mut members = BTreeSet::new();
    let mut name = Vec::new();
    let mut stack = Vec::new();
    let root_count = *trie.first().ok_or_else(err)?;
    visited.insert(0);
    stack.push((1usize, root_count));
    while let Some((pos, remaining)) = stack.last_mut() {
        if *remaining == 0 {
            stack.pop();
            name.pop();
            continue;
        }
        *remaining -= 1;
        let letter = *trie.get(*pos).ok_or_else(err)?;
        let bytes = trie.get(*pos + 1..*pos + 1 + width).ok_or_else(err)?;
        let relative = if width == 2 {
            u16::from_le_bytes(bytes.try_into().expect("fixed width")) as usize
        } else {
            u32::from_le_bytes(bytes.try_into().expect("fixed width")) as usize
        };
        *pos += 1 + width;
        if letter != 0 {
            let child = pos.checked_add(relative).ok_or_else(err)?;
            if name.len() >= 259 || !visited.insert(child) {
                return Err(err());
            }
            name.push(letter);
            let child_count = *trie.get(child).ok_or_else(err)?;
            stack.push((child + 1, child_count));
            continue;
        }
        if relative >= count || !members.insert(relative) {
            return Err(err());
        }
        let decoded = encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(&name)
            .ok_or_else(err)?;
        let name = member_key(&decoded)?;
        let record = trie_end + relative * stride + (stride - base_stride);
        let mut offset = u64::from(word(index, record).ok_or_else(err)?);
        if flags & INDEX_ALIGNED != 0 {
            offset <<= 11;
        }
        let member_size = word(index, record + 4).ok_or_else(err)?;
        let unpacked = if flags & PACKED_ENTRIES != 0 {
            word(index, record + 8).ok_or_else(err)?
        } else {
            0
        };
        let stored = if member_size == 0 || unpacked != 0 {
            member_size
        } else {
            member_size
                .checked_sub(prefix.len() as u32)
                .ok_or_else(err)?
        };
        if stored != 0 && (offset < (index.len() + 8) as u64 || offset + u64::from(stored) > size) {
            return Err(err());
        }
        let entry = Entry {
            name: name.clone(),
            record,
            offset,
            size: member_size,
            unpacked,
            stored,
        };
        if entries.insert(name, entry).is_some() {
            return Err(invalid("duplicate LPK member name"));
        }
    }
    if entries.len() != count {
        return Err(err());
    }
    Ok((entries, prefix))
}

pub(crate) fn lzss(input: &[u8], length: usize, out: &mut Vec<u8>) -> io::Result<()> {
    let err = || invalid("truncated LPK LZSS member");
    let end = out.len() + length;
    let mut input = input.iter().copied();
    let mut window = [0; 4096];
    let mut cursor = 0xfeeusize;
    while out.len() < end {
        let flags = input.next().ok_or_else(err)?;
        for bit in 0..8 {
            if out.len() == end {
                break;
            }
            let first = input.next().ok_or_else(err)?;
            if flags & (1 << bit) != 0 {
                window[cursor & 4095] = first;
                out.push(first);
                cursor += 1;
            } else {
                let second = input.next().ok_or_else(err)?;
                let start = first as usize | ((second as usize & 0xf0) << 4);
                let count = (second as usize & 15) + 3;
                for position in start..start + count.min(end - out.len()) {
                    let byte = window[position & 4095];
                    window[cursor & 4095] = byte;
                    out.push(byte);
                    cursor += 1;
                }
            }
        }
    }
    Ok(())
}

fn read_member_bytes(
    offset: u64,
    bytes: &mut [u8],
    mut read: impl FnMut(u64, &mut [u8]) -> io::Result<()>,
) -> io::Result<()> {
    for (chunk, bytes) in bytes.chunks_mut(CHUNK).enumerate() {
        read(offset + (chunk * CHUNK) as u64, bytes)?;
    }
    Ok(())
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(bytes: &[u8], offset: u64, destination: &mut [u8]) -> io::Result<()> {
        let source = bytes
            .get(offset as usize..)
            .and_then(|source| source.get(..destination.len()))
            .ok_or_else(|| invalid("test input exhausted"))?;
        destination.copy_from_slice(source);
        Ok(())
    }

    fn archive() -> Vec<u8> {
        let scheme = SCHEMES[0].for_stem(b"TEST", (0, 0));
        let flags = INDEX_TRIE | PACKED_ENTRIES | HEADER_CRYPT | CONTENT_CRYPT;
        // Root with two single-letter members, each ending at its own record.
        let trie = [2, b'a', 3, 0, b'b', 4, 0, 1, 0, 0, 0, 1, 0, 1, 0];
        let mut index = vec![2, 0, 0, 0, 2, b'E', b'L', 0];
        index.extend_from_slice(&(trie.len() as u32).to_le_bytes());
        index.extend_from_slice(&trie);
        let first_offset = (8 + index.len() + 26) as u32;
        // The unpacked member counts the shared prefix in its logical size.
        for (offset, size, unpacked) in [(first_offset, 7u32, 0u32), (first_offset + 5, 4, 3)] {
            index.push(0);
            index.extend_from_slice(&offset.to_le_bytes());
            index.extend_from_slice(&size.to_le_bytes());
            index.extend_from_slice(&unpacked.to_le_bytes());
        }
        let code = ((u32::from(flags) << 24) | index.len() as u32) ^ scheme.index_key;
        scheme.index(&mut index);
        let mut archive = b"LPK1".to_vec();
        archive.extend_from_slice(&code.to_le_bytes());
        archive.extend_from_slice(&index);
        let mut first = *b"alpha";
        scheme.entry_header(&mut first);
        archive.extend(first.map(|b| b.rotate_left(4) ^ scheme.content_xor));
        // LZSS group with three obfuscated literal bytes.
        archive.push(0b111);
        archive.extend(b"xyz".map(|b| b.rotate_left(4) ^ scheme.content_xor));
        archive
    }

    #[test]
    fn rewrite_preserves_prefix_and_packed_member_bytes() {
        let original = archive();
        let lpk = Archive::open("Test.LPK", original.len() as u64, None, |off, buf| {
            read(&original, off, buf)
        })
        .unwrap();
        let packed = lpk.entry("b").unwrap().unwrap();
        let packed_source = &original[packed.offset as usize..][..packed.stored as usize];
        let mut replacements = Replacements::default();
        replacements.insert("A", "a".into(), 8).unwrap();
        let mut patched = Vec::new();
        lpk.rewrite(
            replacements,
            |off, buf| read(&original, off, buf),
            |_, off, buf| read(b"ELlonger", off, buf),
            &mut patched,
        )
        .unwrap();
        let lpk = Archive::open("TEST.lpk", patched.len() as u64, None, |off, buf| {
            read(&patched, off, buf)
        })
        .unwrap();
        let member = |name| {
            lpk.read_member(lpk.entry(name).unwrap().unwrap(), 100, |off, buf| {
                read(&patched, off, buf)
            })
            .unwrap()
        };
        assert_eq!(member("a"), b"ELlonger");
        assert_eq!(member("b"), b"ELxyz");
        let packed = lpk.entry("b").unwrap().unwrap();
        assert_eq!(
            &patched[packed.offset as usize..][..packed.stored as usize],
            packed_source
        );
    }
}
