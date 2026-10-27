use super::*;
use std::fs::{File, OpenOptions};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

const HEADER_BYTES: usize = 600;
const MAX_PREVIEW_BYTES: usize = 64 * 1024 * 1024;

fn native_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && &bytes[..3] == b"CSV" && (b'2'..=b'9').contains(&bytes[3])
}

pub(super) fn probe(root: &Path, slot: u32) -> bool {
    if slot > 999 {
        return false;
    }
    let Ok(mut file) = File::open(
        root.join("save")
            .join(crate::system_save::slot::slot_file_name(slot)),
    ) else {
        return false;
    };
    let mut prefix = [0; 64];
    file.read_exact(&mut prefix).is_ok() && native_magic(&prefix)
}

fn read_header(root: &Path, slot: u32, write: bool) -> Option<(File, [u8; HEADER_BYTES])> {
    if (slot as i32) < 0 {
        return None;
    }
    let path = root
        .join("save")
        .join(crate::system_save::slot::slot_file_name(slot));
    let mut file = OpenOptions::new().read(true).write(write).open(path).ok()?;
    let mut header = [0; HEADER_BYTES];
    file.read_exact(&mut header).ok()?;
    native_magic(&header).then_some((file, header))
}

pub(super) fn set_title(root: &Path, slot: u32, title: &str) -> bool {
    if slot > 999 {
        return false;
    }
    let (bytes, _, malformed) = encoding_rs::SHIFT_JIS.encode(title);
    if malformed || bytes.len() >= 256 || bytes.contains(&0) {
        return false;
    }
    let Some((mut file, mut header)) = read_header(root, slot, true) else {
        return false;
    };
    header[272..528].fill(0);
    header[272..272 + bytes.len()].copy_from_slice(&bytes);
    file.seek(SeekFrom::Start(0)).is_ok() && file.write_all(&header).is_ok()
}

pub(super) fn set_metadata(root: &Path, slot: u32, metadata: [u32; 2]) -> bool {
    if slot > 999 {
        return false;
    }
    let Some((mut file, mut header)) = read_header(root, slot, true) else {
        return false;
    };
    header[532..536].copy_from_slice(&metadata[0].to_le_bytes());
    header[536..540].copy_from_slice(&metadata[1].to_le_bytes());
    file.seek(SeekFrom::Start(0)).is_ok() && file.write_all(&header).is_ok()
}

fn header_string(header: &[u8], start: usize) -> Option<&[u8]> {
    let field = header.get(start..start + 256)?;
    let length = field.iter().position(|byte| *byte == 0)?;
    let value = &field[..length];
    let (_, malformed) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(value);
    (!malformed).then_some(value)
}

fn bitmap(file: &mut File, header: &[u8; HEADER_BYTES]) -> Option<(TextureFrame, bool)> {
    let word = |at| u32::from_le_bytes(header[at..at + 4].try_into().unwrap()) as usize;
    let packed = word(564);
    let decoded = word(584);
    if packed == 0 {
        return None;
    }
    if packed > MAX_PREVIEW_BYTES || !(54..=MAX_PREVIEW_BYTES).contains(&decoded) {
        return None;
    }
    let start = HEADER_BYTES.checked_add(word(560))?;
    let end = start.checked_add(packed)?;
    if end as u64 > file.metadata().ok()?.len() {
        return None;
    }
    file.seek(SeekFrom::Start(start as u64)).ok()?;
    let mut source = vec![0; packed];
    file.read_exact(&mut source).ok()?;
    let bmp = crate::system_save::lzss::decompress(&source, decoded).ok()?;
    if bmp.len() < 54 || &bmp[..2] != b"BM" {
        return None;
    }
    let bits = u16::from_le_bytes(bmp[28..30].try_into().unwrap());
    if !matches!(bits, 24 | 32) {
        return None;
    }
    let field = |at| u32::from_le_bytes(bmp[at..at + 4].try_into().unwrap());
    let width = field(18);
    let height = field(22);
    if field(10) != 54
        || field(14) != 40
        || field(30) != 0
        || width == 0
        || height == 0
        || width > 4096
        || height > 4096
    {
        return None;
    }
    let stride = (width as usize)
        .checked_mul(usize::from(bits) / 8)?
        .checked_add(3)?
        & !3;
    let extent = stride.checked_mul(height as usize)?.checked_add(54)?;
    if extent > bmp.len() {
        return None;
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(MAX_PREVIEW_BYTES as u64);
    let mut reader =
        image::ImageReader::with_format(Cursor::new(bmp.as_slice()), image::ImageFormat::Bmp);
    reader.limits(limits);
    let image = reader.decode().ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    let mut rgba = image.into_raw();
    if bits == 32 {
        // 43F8E0 preserves BI_RGB's fourth byte; the BMP standard decoder
        // treats it as padding. Keep mature RGB decoding and restore only
        // this native alpha-plane interpretation, without copying pixels.
        for (y, row) in rgba.chunks_exact_mut(width as usize * 4).enumerate() {
            let source = 54 + (height as usize - 1 - y) * stride;
            for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                pixel[3] = bmp[source + x * 4 + 3];
            }
        }
    }
    Some((
        TextureFrame::from_vec(width, height, rgba).ok()?,
        bits == 32,
    ))
}

impl CmvsSession {
    pub(super) fn load_native_slot_info(
        &mut self,
        slot: u32,
        parent: u32,
        child: i32,
        title_target: u32,
        secondary_target: u32,
    ) -> SessionResult<bool> {
        if (slot as i32) < 0 || parent > 255 {
            return Ok(false);
        }
        let Some((mut file, header)) = read_header(&self.game_root, slot, false) else {
            return Ok(false);
        };
        let Some(title) = header_string(&header, 16) else {
            return Ok(false);
        };
        let Some(secondary) = header_string(&header, 272) else {
            return Ok(false);
        };
        let preview_enabled = u16::from_le_bytes(header[548..550].try_into().unwrap()) != 0;
        let target_exists =
            child < 0 || (child < 1024 && self.vm.texture_parents.contains_key(&(parent as u8)));
        if preview_enabled && target_exists {
            let packed = u32::from_le_bytes(header[564..568].try_into().unwrap());
            let preview = if packed == 0 {
                // 444D90/43CD50 allocates an opaque black CPU image.
                let width = self.vm.interpreter_words.get(&10296).copied().unwrap_or(0);
                let height = self.vm.interpreter_words.get(&10300).copied().unwrap_or(0);
                if width > 4096 || height > 4096 {
                    return Ok(false);
                }
                if width == 0 || height == 0 {
                    None
                } else {
                    Some((
                        TextureFrame::from_vec(
                            width,
                            height,
                            [0, 0, 0, 255].repeat((width * height) as usize),
                        )
                        .map_err(|_| {
                            error(
                                "SHINKI_EMU_CMVS_SAVE_PREVIEW",
                                "native blank preview is invalid",
                            )
                        })?,
                        true,
                    ))
                }
            } else {
                match bitmap(&mut file, &header) {
                    Some(frame) => Some(frame),
                    None => return Ok(false),
                }
            };
            if let Some((frame, alpha)) = preview {
                self.install_native_preview(slot, parent as u8, child, frame, alpha)?;
            }
        }
        self.vm
            .write_native_string_reference(title_target, title)
            .map_err(core)?;
        self.vm
            .write_native_string_reference(secondary_target, secondary)
            .map_err(core)?;
        for (at, field) in [(528, 81236), (532, 81240), (536, 81244)] {
            self.vm.write_interpreter_word(
                field,
                u32::from_le_bytes(header[at..at + 4].try_into().unwrap()),
            );
        }
        self.vm.write_interpreter_word(
            81224,
            crate::archive::save_catalog::native_timestamp(&header[16..272]),
        );
        Ok(true)
    }

    fn install_native_preview(
        &mut self,
        slot: u32,
        parent: u8,
        child: i32,
        frame: TextureFrame,
        alpha: bool,
    ) -> SessionResult<()> {
        let info = [frame.width, frame.height, u32::from(alpha)];
        let target = if child < 0 {
            let identity = self.vm.next_slot_object_id;
            let next = identity.checked_add(1).ok_or_else(|| {
                error(
                    "SHINKI_EMU_CMVS_VM_SLOT_OBJECTS",
                    "preview identity space overflowed",
                )
            })?;
            self.scene.release_parent_surface(parent);
            self.vm.texture_reloads.remove(&parent);
            if self
                .vm
                .texture_presentation
                .as_ref()
                .is_some_and(|owner| owner.parent == parent)
            {
                self.vm.texture_presentation = None;
            }
            self.vm.texture_parents.insert(
                parent,
                crate::CmvsTextureParentState {
                    resource: Some(String::new()),
                    image_info: info,
                    ..Default::default()
                },
            );
            self.vm.texture_children.insert(parent, BTreeMap::new());
            let key = 1924_u32 << 8 | u32::from(parent);
            self.vm.slot_objects.insert(key, identity);
            self.vm.slot_object_seed_words.remove(&key);
            self.vm.next_slot_object_id = next;
            CmvsTextureSlot::Parent(parent)
        } else {
            let child = child as u16;
            self.scene.release_child_surface(parent, child);
            self.vm.texture_children.entry(parent).or_default().insert(
                child,
                crate::CmvsTextureChildState {
                    resource: Some(String::new()),
                    image_info: info,
                    ..Default::default()
                },
            );
            CmvsTextureSlot::Child { parent, child }
        };
        let uri = format!("cmvs:/native-previews/save{:03}.bmp", slot % 1000);
        self.scene.bind_memory_image(target, &uri, frame);
        Ok(())
    }
}

#[cfg(test)]
#[path = "native_slot_info_tests.rs"]
mod tests;
