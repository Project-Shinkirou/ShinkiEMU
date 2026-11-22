// SPDX-License-Identifier: MIT OR Apache-2.0

use shinki_emu_abi as abi;
use std::{
    fs::{self, File},
    io::{self, Read},
    path::Path,
};

// TILE v1: magic (4 bytes), version (1), LE width and height (2 each), RGB (3).
const RESOURCE_BYTES: usize = 12;
const MAX_SIDE: u16 = 256;
const TILE_SIDE: usize = 8;

pub(super) struct Tile {
    width: u16,
    height: u16,
    rgb: [u8; 3],
}

impl Tile {
    pub(super) fn load(directory: &Path) -> io::Result<Option<Self>> {
        let path = directory.join("tilefix.dat");
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.len() != RESOURCE_BYTES as u64 {
            return Ok(None);
        }
        let mut source = File::open(path)?;
        let mut bytes = [0; RESOURCE_BYTES];
        match source.read_exact(&mut bytes) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(error) => return Err(error),
        }
        // Bound the read even if the file changes after the metadata check.
        if source.read(&mut [0])? != 0 {
            return Ok(None);
        }
        let [b'T', b'I', b'L', b'E', 1, w0, w1, h0, h1, r, g, b] = bytes else {
            return Ok(None);
        };
        let width = u16::from_le_bytes([w0, w1]);
        let height = u16::from_le_bytes([h0, h1]);
        if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
            return Ok(None);
        }
        Ok(Some(Self {
            width,
            height,
            rgb: [r, g, b],
        }))
    }

    pub(super) fn game_id(&self, directory: &Path) -> String {
        if let Some(name) = directory.file_name().and_then(|name| name.to_str())
            && name.len() <= 255
            && !name.chars().any(char::is_control)
        {
            return name.to_owned();
        }
        let [r, g, b] = self.rgb;
        format!("tile-{}x{}-{r:02x}{g:02x}{b:02x}", self.width, self.height)
    }

    pub(super) fn surface(&self) -> abi::SurfaceInfo {
        let size = abi::Extent {
            width: self.width.into(),
            height: self.height.into(),
        };
        abi::SurfaceInfo {
            kind: abi::SURFACE_CPU,
            pixel_format: abi::PIXEL_RGBA8,
            color: abi::COLOR_SRGB,
            alpha: abi::ALPHA_OPAQUE,
            rows: abi::ROWS_TOP_DOWN,
            logical: size,
            raster: size,
            stride: u64::from(self.width) * 4,
        }
    }

    pub(super) fn render(&self, flipped: bool) -> Vec<u8> {
        let [r, g, b] = self.rgb;
        let colors = [[r, g, b, 255], [!r, !g, !b, 255]];
        let width = usize::from(self.width);
        let height = usize::from(self.height);
        let mut pixels = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            let row = y / TILE_SIDE + usize::from(flipped);
            for x in 0..width {
                pixels.extend_from_slice(&colors[(row + x / TILE_SIDE) % 2]);
            }
        }
        pixels
    }
}
