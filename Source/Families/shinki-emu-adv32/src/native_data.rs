//! Read-only data recovery from 32-bit ADV32 distributions.
use crate::{
    Error, Result,
    binary::{c_string, decode_japanese, dword},
    game::read_bounded,
};
use object::{Architecture, BinaryFormat, Object, ObjectSection, SectionKind};
use std::{collections::BTreeMap, fs, path::Path};

const MAX_EXECUTABLE_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MUSIC_TRACK_LIMIT: usize = 400;

pub(crate) fn system_template(root: &Path, header_bytes: usize) -> Result<Option<Vec<u8>>> {
    find_engine_value(root, |image| {
        initialized_header(image, header_bytes).map(<[u8]>::to_vec)
    })
}

pub(crate) fn save_namespace(root: &Path) -> Result<Option<String>> {
    find_engine_value(root, |image| {
        namespace(image).map(|name| String::from_utf8(name.to_vec()).expect("ASCII namespace"))
    })
}

pub(crate) fn choice_captions(
    root: &Path,
    labels: &[&[u8]],
    encoding: &'static encoding_rs::Encoding,
) -> Result<Option<BTreeMap<Vec<u8>, String>>> {
    find_engine_value(root, |image| {
        let mut captions = BTreeMap::new();
        for section in image
            .sections()
            .filter(|section| section.kind() == SectionKind::Text)
        {
            let Ok(code) = section.data() else { continue };
            for instruction in code.windows(10) {
                // The native choice builder receives consecutive literal caption and label arguments.
                if instruction[0] != 0x68 || instruction[5] != 0x68 {
                    continue;
                }
                let Some(label) = data_at(image, u64::from(dword(instruction, 6).ok()?))
                    .and_then(|data| c_string(&data[..data.len().min(32)]).ok())
                    .filter(|label| labels.contains(label))
                else {
                    continue;
                };
                let Some(caption) = data_at(image, u64::from(dword(instruction, 1).ok()?))
                    .and_then(|data| c_string(&data[..data.len().min(256)]).ok())
                    .and_then(|caption| {
                        encoding.decode_without_bom_handling_and_without_replacement(caption)
                    })
                else {
                    continue;
                };
                captions.insert(label.to_vec(), caption.into_owned());
            }
        }
        (!captions.is_empty()).then_some(captions)
    })
}

pub(crate) fn credit_slides(root: &Path, route: u16) -> Result<Option<String>> {
    find_engine_value(root, |image| {
        let mut archives = std::collections::BTreeSet::new();
        for section in image
            .sections()
            .filter(|section| section.kind() == SectionKind::Text)
        {
            let Ok(code) = section.data() else { continue };
            for bytes in code.windows(12) {
                // The switch tests the route score, then pushes the archive path
                // as the first argument to the numbered-pack loader.
                let index = match bytes[..7] {
                    [0x83, 0xf8, value, 0x75, _, 0x53, 0x53] => u16::from(value),
                    [_, 0x85, 0xc0, 0x75, _, 0x53, 0x53] => 0,
                    _ => continue,
                };
                if index != route || bytes[7] != 0x68 {
                    continue;
                }
                let Some(path) = data_at(image, u64::from(dword(bytes, 8).ok()?))
                    .and_then(|data| c_string(&data[..data.len().min(64)]).ok())
                    .and_then(|path| decode_japanese(path).ok())
                else {
                    continue;
                };
                let path = path.to_ascii_lowercase().replace('\\', "/");
                if path.starts_with("ed/") && path.ends_with(".pck") && !path.ends_with("list.pck")
                {
                    archives.insert(path);
                }
            }
        }
        if archives.len() == 1 {
            archives.pop_first()
        } else {
            None
        }
    })
}

#[derive(PartialEq, Eq)]
pub(crate) struct GalleryProfile {
    pub(crate) prefixes: Vec<String>,
    pub(crate) limits: Vec<[u8; 5]>,
}

pub(crate) fn gallery_profile(root: &Path) -> Result<Option<GalleryProfile>> {
    find_engine_value(root, |image| {
        let mut prefixes = None;
        for section in image
            .sections()
            .filter(|section| section.kind() == SectionKind::ReadOnlyData)
        {
            let Ok(data) = section.data() else { continue };
            for record in data.windows(9 * 4 + 5) {
                if &record[36..] != b"bst\\\0" {
                    continue;
                }
                let names = record[..36].rchunks_exact(4).collect::<Vec<_>>();
                if names.iter().all(|name| {
                    name[..2].iter().all(u8::is_ascii_lowercase) && name[2..] == [b'_', 0]
                }) {
                    // The base category for supporting characters follows the
                    // three selectable variants in the executable's string pool.
                    prefixes = Some(
                        [0, 1, 2, 3, 4, 8, 5, 6, 7]
                            .into_iter()
                            .map(|index| {
                                std::str::from_utf8(&names[index][..2])
                                    .expect("ASCII prefix")
                                    .to_owned()
                            })
                            .collect(),
                    );
                }
            }
        }

        let mut limits = None;
        for section in image
            .sections()
            .filter(|section| section.kind() == SectionKind::Text)
        {
            let Ok(code) = section.data() else { continue };
            let comparisons = code
                .windows(7)
                .enumerate()
                .filter_map(|(at, bytes)| {
                    // CMP [index * 4 + address], EAX.
                    (bytes[0] == 0x39 && bytes[1] == 0x04 && bytes[2] & 0xc7 == 0x85)
                        .then(|| (at, u64::from(dword(bytes, 3).expect("comparison address"))))
                })
                .collect::<Vec<_>>();
            for (at, base) in comparisons.iter().copied() {
                if ![4, 12, 16].into_iter().all(|column| {
                    comparisons.iter().any(|&(position, address)| {
                        position.abs_diff(at) <= 0x240 && address == base + column
                    })
                }) {
                    continue;
                }
                let Some(table) = data_at(image, base).and_then(|data| data.get(..9 * 5 * 4))
                else {
                    continue;
                };
                let rows = table
                    .chunks_exact(20)
                    .map(|row| {
                        Some([
                            u8::try_from(dword(row, 0).ok()?).ok()?,
                            u8::try_from(dword(row, 4).ok()?).ok()?,
                            u8::try_from(dword(row, 8).ok()?).ok()?,
                            u8::try_from(dword(row, 12).ok()?).ok()?,
                            u8::try_from(dword(row, 16).ok()?).ok()?,
                        ])
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(rows) = rows.filter(|rows| {
                    rows.iter()
                        .all(|row| row[0] <= 8 && row[1] <= 8 && row.iter().all(|&n| n <= 99))
                }) {
                    if limits.as_ref().is_some_and(|prior| prior != &rows) {
                        return None;
                    }
                    limits = Some(rows);
                }
            }
        }
        Some(GalleryProfile {
            prefixes: prefixes?,
            limits: limits?,
        })
    })
}

#[derive(PartialEq, Eq)]
pub(crate) struct MusicTrack {
    pub(crate) id: u16,
    pub(crate) title: String,
}

pub(crate) fn music_playlist(
    root: &Path,
    encoding: &'static encoding_rs::Encoding,
) -> Result<Option<Vec<MusicTrack>>> {
    find_engine_value(root, |image| {
        let mut addresses = std::collections::BTreeSet::new();
        for section in image
            .sections()
            .filter(|section| section.kind() == SectionKind::Text)
        {
            let Ok(code) = section.data() else { continue };
            for instruction in code.windows(7) {
                // CMP dword ptr [table], -1 tests the playlist's sentinel.
                if instruction[..2] == [0x83, 0x3d] && instruction[6] == 0xff {
                    addresses.insert(u64::from(dword(instruction, 2).ok()?));
                }
            }
        }
        let mut playlists = addresses
            .into_iter()
            .filter_map(|address| playlist_at(image, address, encoding));
        let playlist = playlists.next()?;
        playlists.all(|other| other == playlist).then_some(playlist)
    })
}

fn playlist_at(
    image: &object::File<'_>,
    address: u64,
    encoding: &'static encoding_rs::Encoding,
) -> Option<Vec<MusicTrack>> {
    let records = data_at(image, address)?;
    let mut playlist = Vec::new();
    for record in records.chunks_exact(8).take(MUSIC_TRACK_LIMIT + 1) {
        let track = dword(record, 0).ok()?;
        if track == u32::MAX {
            return (!playlist.is_empty()).then_some(playlist);
        }
        if track as usize >= MUSIC_TRACK_LIMIT {
            return None;
        }
        let title = data_at(image, u64::from(dword(record, 4).ok()?))?;
        let title = encoding
            .decode_without_bom_handling_and_without_replacement(
                c_string(&title[..title.len().min(256)]).ok()?,
            )?
            .into_owned();
        if title.is_empty() {
            return None;
        }
        playlist.push(MusicTrack {
            id: track as u16,
            title,
        });
    }
    None
}

fn data_at<'data>(image: &object::File<'data>, address: u64) -> Option<&'data [u8]> {
    image.sections().find_map(|section| {
        if !matches!(
            section.kind(),
            SectionKind::Data | SectionKind::ReadOnlyData
        ) {
            return None;
        }
        let offset = usize::try_from(address.checked_sub(section.address())?).ok()?;
        section.data().ok()?.get(offset..)
    })
}

fn find_engine_value<T: Eq>(
    root: &Path,
    read: impl Fn(&object::File<'_>) -> Option<T>,
) -> Result<Option<T>> {
    let mut value: Option<T> = None;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_file()
            || !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
            || entry.metadata()?.len() > MAX_EXECUTABLE_BYTES as u64
        {
            continue;
        }
        let bytes = read_bounded(&path, MAX_EXECUTABLE_BYTES)?;
        // Both the launcher and any embedded engine remain read-only inputs.
        for (start, _) in bytes
            .windows(2)
            .enumerate()
            .filter(|(_, magic)| *magic == b"MZ")
        {
            let Ok(image) = object::File::parse(&bytes[start..]) else {
                continue;
            };
            if image.format() != BinaryFormat::Pe || image.architecture() != Architecture::I386 {
                continue;
            }
            let Some(candidate) = read(&image) else {
                continue;
            };
            if let Some(found) = &value {
                if *found != candidate {
                    return Err(Error::Invalid("ambiguous original ADV32 engine data"));
                }
            } else {
                value = Some(candidate);
            }
        }
    }
    Ok(value)
}

fn namespace<'data>(image: &object::File<'data>) -> Option<&'data [u8]> {
    const SLOT_FORMAT: &[u8] = b"%s\\%s%02d.dat\0";
    let formats = image
        .sections()
        .filter_map(|section| {
            if !matches!(
                section.kind(),
                SectionKind::Data | SectionKind::ReadOnlyData
            ) {
                return None;
            }
            let bytes = section.data().ok()?;
            let offset = bytes
                .windows(SLOT_FORMAT.len())
                .position(|bytes| bytes == SLOT_FORMAT)?;
            u32::try_from(section.address() + offset as u64).ok()
        })
        .collect::<Vec<_>>();
    if formats.is_empty() {
        return None;
    }
    let mut found = None;
    for section in image
        .sections()
        .filter(|section| section.kind() == SectionKind::Text)
    {
        let Ok(code) = section.data() else { continue };
        for instruction in code.windows(19) {
            // sprintf(buf, format, directory, save stem, decimal index).
            // The two literal string arguments are pushed before LEA buf.
            if instruction[0] != 0x68
                || instruction[5] != 0x68
                || instruction[10] != 0x8d
                || instruction[14] != 0x68
                || !formats.contains(&dword(instruction, 15).ok()?)
            {
                continue;
            }
            let stem = u64::from(dword(instruction, 1).ok()?);
            let Some(name) = data_at(image, stem).and_then(|data| {
                let name = c_string(data.get(..data.len().min(33))?).ok()?;
                (!name.is_empty()
                    && name
                        .iter()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')))
                .then_some(name)
            }) else {
                continue;
            };
            if found.is_some_and(|found| found != name) {
                return None;
            }
            found = Some(name);
        }
    }
    found
}

fn initialized_header<'data>(image: &object::File<'data>, length: usize) -> Option<&'data [u8]> {
    let size = (length as u32).to_le_bytes();
    let copy_size = [0x68, size[0], size[1], size[2], size[3], 0x68];
    let mut found = None;
    for section in image
        .sections()
        .filter(|section| section.kind() == SectionKind::Text)
    {
        let Ok(code) = section.data() else {
            continue;
        };
        for sequence in code.windows(19) {
            // push size; push source address; push destination register;
            // call the copier; restore the three cdecl arguments.
            if !sequence.starts_with(&copy_size)
                || !(0x50..=0x57).contains(&sequence[10])
                || sequence[11] != 0xe8
                || sequence[16..] != [0x83, 0xc4, 0x0c]
            {
                continue;
            }
            let address = u64::from(dword(sequence, 6).ok()?);
            let Some(template) = data_at(image, address).and_then(|data| data.get(..length)) else {
                continue;
            };
            if found.is_some_and(|found| found != template) {
                return None;
            }
            found = Some(template);
        }
    }
    found
}
