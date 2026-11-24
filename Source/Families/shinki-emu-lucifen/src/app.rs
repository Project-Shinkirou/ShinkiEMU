use object::{Object, ObjectSection};
use std::{collections::BTreeSet, fs, io, path::Path};

const MAX_APPLICATION_BYTES: u64 = 64 * 1024 * 1024;
const APP_CONFIG_MARKER: &[u8] = b"SCRIPT\\LL_APP.INF\0";

pub(crate) struct System {
    pub width: u32,
    pub height: u32,
    pub save_root: std::path::PathBuf,
}

impl System {
    pub fn read(root: &Path, data: &[u8]) -> io::Result<Self> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid Lucifen LL_APP.SYS");
        let length = u16::from_le_bytes(*data.first_chunk().ok_or_else(invalid)?) as usize;
        let config = data
            .get(2..)
            .and_then(|data| data.get(..length))
            .ok_or_else(invalid)?;
        let [w0, w1, h0, h1, ..] = config else {
            return Err(invalid());
        };
        let width = u32::from(u16::from_le_bytes([*w0, *w1]));
        let height = u32::from(u16::from_le_bytes([*h0, *h1]));
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16_777_216 {
            return Err(invalid());
        }
        let mode = *config.get(21).ok_or_else(invalid)?;
        let save_root = match mode {
            1 => root.join("SAVEDATA"),
            0 | 2..=4 => root.join(".shinki").join("lucifen").join("saves"),
            _ => root.to_owned(),
        };
        Ok(Self {
            width,
            height,
            save_root,
        })
    }
}

/// The native path query's selector zero returns the application binary's
/// build identifier. Read it from the executable's data section so the
/// script still performs its original check against independently supplied data.
pub(crate) fn build_id(root: &Path) -> io::Result<Vec<u8>> {
    let mut ids = BTreeSet::new();
    read_applications(root, |image| {
        if let Some(section) = image.section_by_name(".data") {
            for data in section.data().map_err(io::Error::other)?.windows(39) {
                if data[38] == 0 && guid(&data[..38]) {
                    ids.insert(data[..38].to_vec());
                }
            }
        }
        Ok(())
    })?;
    match ids.len() {
        1 => Ok(ids.pop_first().unwrap()),
        0 => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Lucifen application build identifier is missing",
        )),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Lucifen application build identifier is ambiguous",
        )),
    }
}

fn read_applications(
    root: &Path,
    mut visit: impl FnMut(&object::File<'_>) -> io::Result<()>,
) -> io::Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_file()
            || !path
                .extension()
                .is_some_and(|name| name.eq_ignore_ascii_case("exe"))
            || entry.metadata()?.len() > MAX_APPLICATION_BYTES
        {
            continue;
        }
        let executable = fs::read(path)?;
        let Ok(image) = object::File::parse(executable.as_slice()) else {
            continue;
        };
        if image.format() != object::BinaryFormat::Pe
            || !executable
                .windows(APP_CONFIG_MARKER.len())
                .any(|bytes| bytes.eq_ignore_ascii_case(APP_CONFIG_MARKER))
        {
            continue;
        }
        visit(&image)?;
    }
    Ok(())
}

/// The native slot writer emits two adjacent four-byte records: its game
/// signature and the zero revision. Find those paired writes in an x86
/// Lucifen executable instead of inferring a signature from the game name.
pub(crate) fn slot_signature(root: &Path) -> io::Result<[u8; 4]> {
    let mut signatures = BTreeSet::new();
    read_applications(root, |image| {
        let Some(code) = image.section_by_name(".text") else {
            return Ok(());
        };
        let instructions = code.data().map_err(io::Error::other)?;
        for (index, pair) in instructions.windows(36).enumerate() {
            if pair[..3] != [0x6a, 4, 0x68]
                || pair[18..21] != [0x6a, 4, 0x68]
                || pair[7..9] != [0x8d, 0x8d]
                || pair[7..13] != pair[25..31]
                || pair[13] != 0xe8
                || pair[31] != 0xe8
            {
                continue;
            }
            let displacement =
                |offset| i32::from_le_bytes(pair[offset..][..4].try_into().unwrap()) as i64;
            if displacement(14) - displacement(32) != 18 {
                continue;
            }
            let data_address = u64::from(u32::from_le_bytes(pair[3..7].try_into().unwrap()));
            let revision_address = u64::from(u32::from_le_bytes(pair[21..25].try_into().unwrap()));
            if revision_address != data_address + 4 {
                continue;
            }
            for section_name in [".rdata", ".data"] {
                let Some(section) = image.section_by_name(section_name) else {
                    continue;
                };
                let Some(offset) = data_address
                    .checked_sub(section.address())
                    .and_then(|offset| usize::try_from(offset).ok())
                else {
                    continue;
                };
                let data = section.data().map_err(io::Error::other)?;
                let Some(bytes) = data.get(offset..).and_then(|data| data.first_chunk::<8>())
                else {
                    continue;
                };
                if bytes[4..] == [0; 4] && bytes[..4].iter().all(u8::is_ascii_alphanumeric) {
                    // A nearby reference to the native slot filename marks
                    // this paired write as the save header rather than a
                    // different binary resource with the same short prefix.
                    let before = &instructions[index.saturating_sub(128)..index];
                    if before
                        .windows(5)
                        .filter(|instruction| instruction[0] == 0x68)
                        .any(|instruction| {
                            let pointer =
                                u64::from(u32::from_le_bytes(instruction[1..].try_into().unwrap()));
                            [".rdata", ".data"].into_iter().any(|name| {
                                let Some(names) = image.section_by_name(name) else {
                                    return false;
                                };
                                let Some(offset) = pointer
                                    .checked_sub(names.address())
                                    .and_then(|offset| usize::try_from(offset).ok())
                                else {
                                    return false;
                                };
                                names
                                    .data()
                                    .ok()
                                    .and_then(|data| data.get(offset..))
                                    .is_some_and(|bytes| bytes.starts_with(b"%sDATA%02d%02d.SAV\0"))
                            })
                        })
                    {
                        signatures.insert(bytes[..4].try_into().unwrap());
                    }
                }
            }
        }
        Ok(())
    })?;
    match signatures.len() {
        1 => Ok(signatures.pop_first().unwrap()),
        0 => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Lucifen native save signature is missing",
        )),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Lucifen native save signature is ambiguous",
        )),
    }
}

fn guid(bytes: &[u8]) -> bool {
    bytes.iter().enumerate().all(|(index, byte)| match index {
        0 => *byte == b'{',
        9 | 14 | 19 | 24 => *byte == b'-',
        37 => *byte == b'}',
        _ => byte.is_ascii_hexdigit(),
    })
}

pub(crate) fn read_options(root: &Path, application: &AppConfig) -> io::Result<([i32; 12], i32)> {
    let mut options = [0; 12];
    let mut display_mode = 1;
    if let Some(bytes) = read_configuration(&root.join("USER.CFG"), 64 * 1024)? {
        let mut remaining = bytes.as_slice();
        let mut block = || -> io::Result<&[u8]> {
            let length =
                u16::from_le_bytes(*remaining.first_chunk().ok_or_else(config_error)?) as usize;
            let (block, rest) = remaining[2..]
                .split_at_checked(length)
                .ok_or_else(config_error)?;
            remaining = rest;
            Ok(block)
        };
        let _ = block()?;
        let window = block()?;
        if let Some(&mode) = window.first() {
            display_mode = i32::from(mode) - 1;
        }
        let host = block()?;
        let byte = |bytes: &[u8], offset| i32::from(bytes.get(offset).copied().unwrap_or_default());
        let word =
            |bytes: &[u8], offset: usize| byte(bytes, offset) | (byte(bytes, offset + 1) << 8);
        options[..7].copy_from_slice(&[
            byte(window, 10),
            byte(host, 5),
            byte(window, 13),
            word(window, 11),
            byte(host, 7),
            byte(host, 9),
            word(host, 11),
        ]);
    }
    for (offset, byte) in options[7..].iter_mut().zip(&application.0[164..169]) {
        *offset = i32::from(*byte);
    }
    Ok((options, display_mode))
}

fn read_configuration(path: &Path, limit: u64) -> io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(config_error());
    }
    Ok(Some(bytes))
}

fn config_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid Lucifen application configuration",
    )
}

#[derive(Clone)]
pub(crate) struct AppConfig([u8; 176], Vec<u8>);

impl AppConfig {
    pub(crate) fn slot_ring(&self, group: i32) -> Option<(i32, i32)> {
        match group {
            10 => Some((i32::from(self.0[155]), 0)),
            11 => Some((i32::from(self.0[156]), 5)),
            _ => None,
        }
    }

    pub(crate) fn record_slot_write(&mut self, group: i32, slot: i32, next: Option<i32>) {
        self.0[158] = 1;
        match group {
            10 => self.0[159] = slot as u8,
            11 if slot >= 5 => self.0[160] = slot as u8,
            _ => {}
        }
        if let Some(next) = next {
            match group {
                10 => self.0[155] = next as u8,
                11 => self.0[156] = next as u8,
                _ => {}
            }
        }
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let length = self.0.len() + self.1.len();
        let mut bytes = Vec::with_capacity(4 + length);
        bytes.extend_from_slice(&(length as u32).to_le_bytes());
        bytes.extend_from_slice(&self.0);
        bytes.extend_from_slice(&self.1);
        bytes
    }

    pub(crate) fn open(saves: &Path) -> io::Result<Self> {
        let mut data = [0; 176];
        data[32..41].copy_from_slice(b"MS Gothic");
        // Default settings used when the original game has not yet created
        // APP_CFG.DAT. Offsets are relative to its serialized payload.
        for (offset, value) in [
            (0, 1),
            (4, 1),
            (5, 1),
            (7, 1),
            (9, 1),
            (13, 1),
            (20, 1),
            (22, 30),
            (23, 1),
            (24, 30),
            (26, 50),
            (27, 1),
            (64, 1),
            (66, 2),
            (69, 1),
            (70, 140),
            (77, 1),
            (78, 1),
            (79, 1),
            (80, 1),
            (100, 1),
            (105, 65),
            (106, 44),
            (107, 45),
            (108, 30),
            (109, 65),
            (110, 66),
            (111, 67),
            (112, 68),
            (113, 57),
            (114, 50),
            (115, 46),
            (116, 210),
            (117, 31),
            (118, 14),
            (119, 20),
            (120, 1),
            (137, 1),
            (141, 1),
            (142, 1),
            (143, 1),
            (145, 1),
            (146, 1),
            (152, 1),
            (154, 1),
            (156, 5),
            (162, 1),
            (164, 1),
            (168, 1),
        ] {
            data[offset] = value;
        }
        let mut extra = Vec::new();
        if let Some(file) = read_configuration(&saves.join("APP_CFG.DAT"), 64 * 1024)? {
            let length = u32::from_le_bytes(*file.first_chunk().ok_or_else(config_error)?) as usize;
            let configured = file
                .get(4..)
                .and_then(|tail| tail.get(..length))
                .ok_or_else(config_error)?;
            let count = configured.len().min(data.len());
            data[..count].copy_from_slice(&configured[..count]);
            extra.extend_from_slice(&configured[count..]);
        }
        Ok(Self(data, extra))
    }

    pub(crate) fn font_name(&self) -> &[u8] {
        let field = &self.0[32..64];
        &field[..field
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(field.len())]
    }

    pub(crate) fn set_font_name(&mut self, name: &[u8]) {
        let field = &mut self.0[32..64];
        field.fill(0);
        let len = name.len().min(field.len() - 1);
        field[..len].copy_from_slice(&name[..len]);
    }

    pub(crate) fn get(&self, group: i32, index: u8) -> Option<i32> {
        let (offset, width) = Self::field(group, index)?;
        Some(if width == 4 {
            i32::from_le_bytes(self.0[offset..offset + 4].try_into().unwrap())
        } else {
            i32::from(self.0[offset])
        })
    }

    pub(crate) fn set(&mut self, group: i32, index: u8, value: i32) -> bool {
        let Some((offset, width)) = Self::field(group, index) else {
            return false;
        };
        self.0[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
        true
    }

    fn field(group: i32, index: u8) -> Option<(usize, usize)> {
        let i = usize::from(index);
        Some(match (group, i) {
            (0, 0..=6) => (3 + i, 1),
            (0, 8..=11) => (10 + i - 8, 1),
            (0, 12) => (16, 4),
            (0, 13) => (20, 1),
            (1, 0..=6) => (21 + i, 1),
            (1, 7) => (28, 4),
            (2, 1..=7) => (64 + i - 1, 1),
            (2, 8) => (72, 4),
            (3, 0..=4) => (76 + i, 1),
            (3, 5..=8) => (84 + (i - 5) * 4, 4),
            (3, 9..=14) => (100 + i - 9, 1),
            (3, 15) => (81, 1),
            (4, 0..=1) => (120 + i, 1),
            (4, 2..=15) => (106 + i - 2, 1),
            (5, 0) => (122, 1),
            (5, 1..=5) => (137 + i - 1, 1),
            (5, 6..=19) => (123 + i - 6, 1),
            (6, 0..=4) => (142 + i, 1),
            (6, 5) => (148, 4),
            (6, 6..=8) => (152 + i - 6, 1),
            (7, 0) => (158, 1),
            (8, 0..=3) => (159 + i, 1),
            _ => return None,
        })
    }
}
