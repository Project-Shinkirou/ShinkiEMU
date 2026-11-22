//! Indexed ADV32 resource bundles. Payloads are read on demand.
use crate::{
    Error, Result,
    binary::{c_string, decode_japanese, dword},
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const MAX_ENTRIES: usize = 65536;

#[derive(Clone, Copy)]
pub(crate) struct Span {
    offset: u64,
    size: u64,
}

pub(crate) struct Archive {
    file: PathBuf,
    names: BTreeMap<String, Span>,
    by_offset: BTreeMap<u64, Span>,
    numbered: Vec<Span>,
}

impl Archive {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        let mut header = [0; 16];
        file.read_exact(&mut header)?;
        let mut archive = Self {
            file: path.to_path_buf(),
            names: BTreeMap::new(),
            by_offset: BTreeMap::new(),
            numbered: Vec::new(),
        };
        if header.starts_with(b"CRXB") {
            let count = get_count(&header, 8)?;
            let index_end = 16 + count * 32;
            let index = read_range(&mut file, 16, count * 32, length)?;
            let mut offsets = Vec::with_capacity(count);
            for record in index.chunks_exact(32) {
                let offset = u64::from(dword(record, 0)?);
                if offset < index_end as u64 || offset >= length {
                    return Err(Error::Invalid("CRXB entry offset"));
                }
                offsets.push(offset);
            }
            offsets.sort_unstable();
            offsets.dedup();
            for (i, &offset) in offsets.iter().enumerate() {
                archive.by_offset.insert(
                    offset,
                    Span {
                        offset,
                        size: offsets.get(i + 1).copied().unwrap_or(length) - offset,
                    },
                );
            }
            for record in index.chunks_exact(32) {
                let offset = u64::from(dword(record, 0)?);
                let span = archive.by_offset[&offset];
                archive.insert_name(&record[8..], span)?;
            }
        } else {
            let count = get_count(&header, 0)?;
            let table_end = 4 + count * 8;
            let table = read_range(&mut file, 4, count * 8, length)?;
            let first = u64::from(dword(&table, 0)?);
            let has_names = if first >= (table_end + count * 64) as u64 {
                let first_record = read_range(&mut file, table_end as u64, 64, length)?;
                first == u64::from(dword(&first_record, 56)?)
                    && u64::from(dword(&table, 4)?) == u64::from(dword(&first_record, 60)?)
                    && first_record[0] != 0
            } else {
                false
            };
            for record in table.chunks_exact(8) {
                let span = checked_span(record, table_end as u64, length)?;
                archive.by_offset.insert(span.offset, span);
                archive.numbered.push(span);
            }
            if has_names {
                let names_end = table_end + count * 64;
                let names = read_range(&mut file, table_end as u64, count * 64, length)?;
                for record in names.chunks_exact(64) {
                    let span = checked_span(&record[56..], names_end as u64, length)?;
                    archive.insert_name(&record[..56], span)?;
                }
            }
        }
        Ok(archive)
    }

    fn insert_name(&mut self, name: &[u8], span: Span) -> Result<()> {
        let name = decode_japanese(c_string(name).unwrap_or(name))?.to_ascii_lowercase();
        if name.is_empty() || self.names.insert(name, span).is_some() {
            return Err(Error::Invalid("ADV32 archive name"));
        }
        Ok(())
    }

    pub(crate) fn find_name(&self, name: &str) -> Option<Span> {
        self.names.get(&name.to_ascii_lowercase()).copied()
    }

    pub(crate) fn by_offset(&self, offset: u32) -> Result<Span> {
        self.by_offset
            .get(&u64::from(offset))
            .copied()
            .ok_or(Error::Invalid("ADV32 archive member offset"))
    }

    pub(crate) fn frame_count(&self) -> usize {
        self.numbered.len()
    }

    pub(crate) fn frame(&self, index: usize, max: usize) -> Result<Vec<u8>> {
        let span = self
            .numbered
            .get(index)
            .copied()
            .ok_or(Error::Invalid("ADV32 animation frame number"))?;
        self.read(span, max)
    }

    pub(crate) fn read(&self, span: Span, max: usize) -> Result<Vec<u8>> {
        if span.size > max as u64 {
            return Err(Error::Invalid("ADV32 archive member length"));
        }
        let mut file = File::open(&self.file)?;
        let length = file.metadata()?.len();
        read_range(&mut file, span.offset, span.size as usize, length)
    }
}

fn get_count(data: &[u8], offset: usize) -> Result<usize> {
    let count = dword(data, offset)? as usize;
    if (1..=MAX_ENTRIES).contains(&count) {
        Ok(count)
    } else {
        Err(Error::Invalid("ADV32 archive directory size"))
    }
}

fn checked_span(record: &[u8], first: u64, length: u64) -> Result<Span> {
    let offset = u64::from(dword(record, 0)?);
    let size = u64::from(dword(record, 4)?);
    if offset < first || offset + size > length {
        return Err(Error::Invalid("ADV32 archive member range"));
    }
    Ok(Span { offset, size })
}

fn read_range(file: &mut File, offset: u64, size: usize, length: u64) -> Result<Vec<u8>> {
    if !offset
        .checked_add(size as u64)
        .is_some_and(|end| end <= length)
    {
        return Err(Error::Invalid("ADV32 archive range"));
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer = vec![0; size];
    file.read_exact(&mut buffer)?;
    Ok(buffer)
}
