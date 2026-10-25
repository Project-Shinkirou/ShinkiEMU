use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStatus {
    Running,
    Waiting,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowCommand {
    /// Request the host-owned game window's fullscreen mode. No native handle
    /// crosses the ABI. Hosts must execute this on their window thread or fail
    /// the session explicitly when window control is unavailable.
    SetFullscreen(bool),
    /// UTF-8 window caption. Empty clears it; native encodings are decoded by the family.
    SetTitle(String),
    /// Owned, tightly packed straight-alpha RGBA8 small window icon.
    SetIcon(WindowIcon),
    /// Update the host-owned pointer in logical stage coordinates. Omitted
    /// properties retain their current values; no native cursor handle is passed.
    SetPointer(PointerRequest),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PointerRequest {
    pub logical_x: Option<i32>,
    pub logical_y: Option<i32>,
    pub visible: Option<bool>,
    /// Idle time in milliseconds; zero disables automatic hiding. Actual
    /// pointer activity restores visibility unless explicitly hidden.
    pub autohide_ms: Option<u64>,
}

impl PointerRequest {
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

pub const MAX_WINDOW_TITLE_BYTES: usize = 4096;
pub const MAX_WINDOW_ICON_DIMENSION: u32 = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl WindowIcon {
    pub fn validate(&self) -> SessionResult<()> {
        let expected = self
            .width
            .checked_mul(self.height)
            .and_then(|n| n.checked_mul(4));
        if self.width == 0
            || self.height == 0
            || self.width > MAX_WINDOW_ICON_DIMENSION
            || self.height > MAX_WINDOW_ICON_DIMENSION
            || expected.map(|n| n as usize) != Some(self.rgba.len())
        {
            return Err(SessionError::invalid(
                "SHINKI_EMU_FAMILY_WINDOW_ICON",
                "window icon dimensions or RGBA8 byte length are invalid",
            ));
        }
        Ok(())
    }
}

impl WindowCommand {
    pub fn validate(&self) -> SessionResult<()> {
        if let Self::SetIcon(icon) = self {
            return icon.validate();
        }
        if let Self::SetTitle(title) = self
            && (title.len() > MAX_WINDOW_TITLE_BYTES || title.chars().any(char::is_control))
        {
            return Err(SessionError::invalid(
                "SHINKI_EMU_FAMILY_WINDOW_TITLE",
                "window title exceeds its UTF-8 bound or contains control characters",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvanceResponse {
    pub status: RuntimeStatus,
    pub window_command: Option<WindowCommand>,
    /// A successful restore, explicit session restart, or Family-defined
    /// pause/resume transition established a new playback timeline. Start the
    /// next wall-time interval after this call and its frame capture complete;
    /// do not replay loading or paused time.
    pub reset_clock: bool,
}

impl AdvanceResponse {
    pub fn validate(&self) -> SessionResult<()> {
        if let Some(command) = &self.window_command {
            command.validate()?;
        }
        Ok(())
    }

    pub fn running() -> Self {
        Self {
            status: RuntimeStatus::Running,
            window_command: None,
            reset_clock: false,
        }
    }
}
pub(crate) fn validate_dimensions(width: u32, height: u32) -> SessionResult<()> {
    if width == 0 || height == 0 {
        return Err(SessionError::invalid(
            "SHINKI_EMU_FAMILY_FRAME_SIZE",
            "dimensions must be non-zero",
        ));
    }
    Ok(())
}

pub(crate) fn validate_window_size(width: u32, height: u32) -> SessionResult<()> {
    if width == 0 || height == 0 {
        return Err(SessionError::invalid(
            "SHINKI_EMU_FAMILY_WINDOW_SIZE",
            "window dimensions must be non-zero",
        ));
    }
    Ok(())
}
