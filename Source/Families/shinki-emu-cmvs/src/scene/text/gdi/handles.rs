use super::*;
use sha2::{Digest, Sha256};
use windows::{
    Win32::{
        Foundation::HANDLE,
        Globalization::{CP_ACP, GetACP, WideCharToMultiByte},
    },
    core::{PCSTR, PCWSTR, w},
};

pub(super) struct MemoryFont {
    pub asset_id: String,
    pub face_index: u32,
    pub hash: roukaku_core::Hash256,
    pub family: Vec<u8>,
    tables: Vec<(u32, u32, [u8; 32])>,
    registration: HANDLE,
}

impl MemoryFont {
    pub fn new(font: &PackagedFont) -> Result<Self, CoreError> {
        let family_w: Vec<u16> = font.family.encode_utf16().collect();
        if family_w.is_empty() || family_w.contains(&0) {
            return Err(error("SHINKI_EMU_CMVS_GDI_FAMILY"));
        }
        let mut substituted = windows::core::BOOL(0);
        // UTF-8 system ACP forbids lpUsedDefaultChar, while legacy ACPs use
        // it to reject replacement names rather than selecting another face.
        let used_default = if unsafe { GetACP() } == 65001 {
            None
        } else {
            Some(&mut substituted as *mut _)
        };
        let count =
            unsafe { WideCharToMultiByte(CP_ACP, 0, &family_w, None, PCSTR::null(), used_default) };
        if count <= 0 {
            return Err(win_error("WideCharToMultiByte"));
        }
        let mut family = vec![0; count as usize + 1];
        if unsafe {
            WideCharToMultiByte(
                CP_ACP,
                0,
                &family_w,
                Some(&mut family[..count as usize]),
                PCSTR::null(),
                used_default,
            )
        } != count
            || substituted.as_bool()
        {
            return Err(error("SHINKI_EMU_CMVS_GDI_FAMILY_ENCODING"));
        }
        let tables = face_tables(&font.bytes, font.face_index)?;
        let byte_count =
            u32::try_from(font.bytes.len()).map_err(|_| error("SHINKI_EMU_CMVS_GDI_FONT_BYTES"))?;
        let mut faces = 0;
        // GDI copies the font data; the PackagedFont remains the asset owner.
        let registration = unsafe {
            AddFontMemResourceEx(font.bytes.as_ptr().cast(), byte_count, None, &raw mut faces)
        };
        if registration.is_invalid() || faces == 0 {
            if !registration.is_invalid() {
                unsafe {
                    let _ = RemoveFontMemResourceEx(registration);
                }
            }
            return Err(win_error("AddFontMemResourceEx"));
        }
        Ok(Self {
            asset_id: font.asset_id.clone(),
            face_index: font.face_index,
            hash: font.hash,
            family,
            tables,
            registration,
        })
    }

    pub fn matches(&self, font: &PackagedFont) -> bool {
        self.asset_id == font.asset_id
            && self.face_index == font.face_index
            && self.hash == font.hash
    }

    pub fn verify(&self, dc: HDC) -> Result<(), CoreError> {
        // Compare every sfnt table from the requested TTC face. Family aliases
        // alone cannot establish that GDI selected the supplied asset/face.
        let mut buffer = [0u8; 4096];
        for (tag, length, expected) in &self.tables {
            let actual = unsafe { GetFontData(dc, *tag, 0, None, 0) };
            if actual == u32::MAX {
                return Err(win_error("GetFontData"));
            }
            if actual != *length {
                return Err(error("SHINKI_EMU_CMVS_GDI_FONT_IDENTITY"));
            }
            let mut hash = Sha256::new();
            let mut offset = 0;
            while offset < *length {
                let count = (*length - offset).min(buffer.len() as u32);
                if unsafe { GetFontData(dc, *tag, offset, Some(buffer.as_mut_ptr().cast()), count) }
                    != count
                {
                    return Err(win_error("GetFontData"));
                }
                hash.update(&buffer[..count as usize]);
                offset += count;
            }
            if hash.finalize().as_slice() != expected {
                return Err(error("SHINKI_EMU_CMVS_GDI_FONT_IDENTITY"));
            }
        }
        Ok(())
    }
}

fn face_tables(bytes: &[u8], index: u32) -> Result<Vec<(u32, u32, [u8; 32])>, CoreError> {
    let invalid = || error("SHINKI_EMU_CMVS_GDI_FONT_BYTES");
    let read_u32 = |at: usize| -> Result<u32, CoreError> {
        Ok(u32::from_be_bytes(
            bytes
                .get(at..at + 4)
                .ok_or_else(invalid)?
                .try_into()
                .map_err(|_| invalid())?,
        ))
    };
    let base = if bytes.get(..4) == Some(b"ttcf") {
        if index >= read_u32(8)? {
            return Err(invalid());
        }
        read_u32(
            12usize
                .checked_add(index as usize * 4)
                .ok_or_else(invalid)?,
        )? as usize
    } else if index == 0 {
        0
    } else {
        return Err(invalid());
    };
    let count_at = base.checked_add(4).ok_or_else(invalid)?;
    let count = u16::from_be_bytes(
        bytes
            .get(count_at..count_at + 2)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    );
    if count == 0 {
        return Err(invalid());
    }
    let mut tables = Vec::with_capacity(count as usize);
    for index in 0..usize::from(count) {
        let record = base.checked_add(12 + index * 16).ok_or_else(invalid)?;
        let tag = u32::from_le_bytes(
            bytes
                .get(record..record + 4)
                .ok_or_else(invalid)?
                .try_into()
                .map_err(|_| invalid())?,
        );
        let start = read_u32(record + 8)? as usize;
        let length = read_u32(record + 12)?;
        let end = start.checked_add(length as usize).ok_or_else(invalid)?;
        let table = bytes.get(start..end).ok_or_else(invalid)?;
        tables.push((tag, length, Sha256::digest(table).into()));
    }
    Ok(tables)
}

impl Drop for MemoryFont {
    fn drop(&mut self) {
        unsafe {
            let _ = RemoveFontMemResourceEx(self.registration);
        }
    }
}

pub(super) struct Dc(HDC);
impl Dc {
    pub fn new() -> Result<Self, CoreError> {
        // A NULL-compatible DC is invalidated when its creating thread exits.
        // A display-compatible memory DC has independent thread-safe lifetime;
        // no HWND, capture, desktop bitmap, or visible surface is involved.
        let display = unsafe { CreateDCW(w!("DISPLAY"), PCWSTR::null(), PCWSTR::null(), None) };
        if display.is_invalid() {
            return Err(win_error("CreateDCW"));
        }
        let dc = unsafe { CreateCompatibleDC(Some(display)) };
        unsafe {
            let _ = DeleteDC(display);
        }
        if dc.is_invalid() {
            return Err(win_error("CreateCompatibleDC"));
        }
        Ok(Self(dc))
    }
    pub fn handle(&self) -> HDC {
        self.0
    }
}
impl Drop for Dc {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

pub(super) struct Font(pub HFONT);
impl Font {
    pub fn new(assets: &[MemoryFont], key: FontKey, dc: &Dc) -> Result<Self, CoreError> {
        let size = i32::try_from(key.size).map_err(|_| error("SHINKI_EMU_CMVS_GDI_SIZE"))?;
        if size <= 0 || key.weight > i32::MAX as u32 {
            return Err(error("SHINKI_EMU_CMVS_GDI_SIZE"));
        }
        let handle = unsafe {
            CreateFontA(
                -size,
                0,
                0,
                0,
                key.weight as i32,
                0,
                0,
                0,
                FONT_CHARSET(if key.shift_jis { 128 } else { 0 }),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                PROOF_QUALITY,
                0x30,
                PCSTR(assets[key.assets[0]].family.as_ptr()),
            )
        };
        if handle.is_invalid() {
            return Err(win_error("CreateFontA"));
        }
        let font = Self(handle);
        {
            let _selection = Selection::new(dc.handle(), font.0)?;
            // GDI chooses the physical face, not fontdb's nearest-weight
            // heuristic. For example Arial600 uses its regular sfnt tables.
            // Both candidates belong to this bound family; never accept a
            // charset-induced fallback to an unrelated registered family.
            let requested = usize::from(key.weight == 600);
            let expected = &assets[key.assets[requested]];
            if let Err(failure) = expected.verify(dc.handle()) {
                let other = key.assets[1 - requested];
                if other == key.assets[requested] {
                    return Err(failure);
                }
                assets[other].verify(dc.handle())?;
            }
        }
        Ok(font)
    }
}
impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0.0));
        }
    }
}

pub(super) struct Selection {
    dc: HDC,
    previous: HGDIOBJ,
}
impl Selection {
    pub fn new(dc: HDC, font: HFONT) -> Result<Self, CoreError> {
        let previous = unsafe { SelectObject(dc, HGDIOBJ(font.0)) };
        if previous.is_invalid() {
            return Err(win_error("SelectObject"));
        }
        Ok(Self { dc, previous })
    }
}
impl Drop for Selection {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
        }
    }
}

pub(super) fn raster(dc: &Dc, font: &Font, key: CellKey) -> Result<cell::Cell, CoreError> {
    let _selection = Selection::new(dc.handle(), font.0)?;
    let identity = MAT2 {
        eM11: FIXED { fract: 0, value: 1 },
        eM22: FIXED { fract: 0, value: 1 },
        ..Default::default()
    };
    let mut gm = GLYPHMETRICS::default();
    let count = unsafe {
        GetGlyphOutlineA(
            dc.handle(),
            u32::from(key.code),
            GGO_GRAY4_BITMAP,
            &mut gm,
            0,
            None,
            &identity,
        )
    };
    if count == u32::MAX {
        return Err(win_error("GetGlyphOutlineA"));
    }
    let mut tm = TEXTMETRICA::default();
    if !unsafe { GetTextMetricsA(dc.handle(), &mut tm) }.as_bool() {
        return Err(win_error("GetTextMetricsA"));
    }
    let mut bytes = vec![0; count as usize];
    if count != 0 {
        let written = unsafe {
            GetGlyphOutlineA(
                dc.handle(),
                u32::from(key.code),
                GGO_GRAY4_BITMAP,
                &mut gm,
                count,
                Some(bytes.as_mut_ptr().cast()),
                &identity,
            )
        };
        if written == u32::MAX || written != count {
            return Err(win_error("GetGlyphOutlineA"));
        }
    }
    cell::compose(key.font.size, key.code, &gm, &tm, &bytes, key.settings())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sfnt_identity_uses_selected_collection_face_and_table_endianness() {
        let mut bytes = vec![0; 80];
        bytes[..4].copy_from_slice(b"ttcf");
        bytes[8..12].copy_from_slice(&2u32.to_be_bytes());
        bytes[12..16].copy_from_slice(&24u32.to_be_bytes());
        bytes[16..20].copy_from_slice(&44u32.to_be_bytes());
        bytes[48..50].copy_from_slice(&1u16.to_be_bytes());
        bytes[56..60].copy_from_slice(b"head");
        bytes[64..68].copy_from_slice(&76u32.to_be_bytes());
        bytes[68..72].copy_from_slice(&4u32.to_be_bytes());
        bytes[76..80].copy_from_slice(&[1, 2, 3, 4]);
        let tables = face_tables(&bytes, 1).unwrap();
        assert_eq!(tables[0].0, u32::from_le_bytes(*b"head"));
        assert_eq!(tables[0].1, 4);
        assert_eq!(tables[0].2, <[u8; 32]>::from(Sha256::digest([1, 2, 3, 4])));
        assert!(face_tables(&bytes, 0).is_err());
        assert!(face_tables(&bytes, 2).is_err());
    }
}
