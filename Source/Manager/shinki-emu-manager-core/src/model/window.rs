use super::{FamilyError, FamilyResult};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FamilyStatus {
    Running,
    Waiting,
    Finished,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FamilyPointerRequest {
    pub logical_x: Option<i32>,
    pub logical_y: Option<i32>,
    pub visible: Option<bool>,
    /// Idle time in milliseconds; zero disables automatic hiding. Actual
    /// pointer activity restores visibility unless explicitly hidden.
    pub autohide_ms: Option<u64>,
}

impl FamilyPointerRequest {
    /// Coalesce pending updates without discarding unchanged properties.
    pub fn merge(&mut self, next: Self) {
        if next.logical_x.is_some() {
            self.logical_x = next.logical_x;
        }
        if next.logical_y.is_some() {
            self.logical_y = next.logical_y;
        }
        if next.visible.is_some() {
            self.visible = next.visible;
        }
        if next.autohide_ms.is_some() {
            self.autohide_ms = next.autohide_ms;
        }
    }
}

pub const MAX_WINDOW_ICON_DIMENSION: u32 = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyWindowIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl FamilyWindowIcon {
    pub fn validate(&self) -> FamilyResult<()> {
        Self::validate_rgba8(self.width, self.height, self.rgba.len())
    }

    pub fn validate_rgba8(width: u32, height: u32, byte_len: usize) -> FamilyResult<()> {
        let expected = width.checked_mul(height).and_then(|n| n.checked_mul(4));
        if width == 0
            || height == 0
            || width > MAX_WINDOW_ICON_DIMENSION
            || height > MAX_WINDOW_ICON_DIMENSION
            || expected.map(|n| n as usize) != Some(byte_len)
        {
            return Err(FamilyError::invalid(
                "SHINKI_EMU_FAMILY_WINDOW_ICON",
                "window icon dimensions or RGBA8 byte length are invalid",
            ));
        }
        Ok(())
    }
}
