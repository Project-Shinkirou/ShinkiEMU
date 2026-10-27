use super::*;

/// Renderer-global flags behind `429290`, independent of the host raster size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeViewState {
    pub fullscreen: bool,
    pub suppress_alternate: bool,
    pub alternate_enabled: bool,
    /// Renderer DWORD52/53, when configuration alone determines the mode.
    /// Saved multi-mode selection also needs the host adapter's mode list.
    pub fullscreen_size: Option<[u32; 2]>,
    /// Cached DWORD12/13. Changing DWORD43 alone does not recompute these.
    pub viewport_origin: [u32; 2],
}

impl Default for CmvsNativeViewState {
    fn default() -> Self {
        Self {
            fullscreen: false,
            // `41C57B` calls `42B490(1)`, which initializes renderer DWORD43.
            suppress_alternate: true,
            // `41DA70` explicitly clears renderer DWORD47 through `429B20`.
            alternate_enabled: false,
            fullscreen_size: None,
            viewport_origin: [0, 0],
        }
    }
}

impl CmvsNativeViewState {
    /// `429290`: DWORD46 && !DWORD43 && DWORD47.
    pub(crate) fn alternate_active(&self) -> bool {
        self.fullscreen && !self.suppress_alternate && self.alternate_enabled
    }

    /// The bounded, monitor-independent branches of `429370`.
    pub(crate) fn reset_viewport(&mut self, render_size: [u32; 2]) {
        if !self.fullscreen {
            self.viewport_origin = [0, 0];
        } else if !self.suppress_alternate {
            self.recenter_viewport(render_size);
        }
    }

    fn recenter_viewport(&mut self, render_size: [u32; 2]) {
        if self.alternate_enabled {
            self.viewport_origin = [0, 0];
        } else if let Some(size) = self.fullscreen_size {
            // Native performs unsigned subtraction and logical SHR, not
            // signed division or a floating-point letterbox calculation.
            self.viewport_origin = [
                size[0].wrapping_sub(render_size[0]) >> 1,
                size[1].wrapping_sub(render_size[1]) >> 1,
            ];
        }
    }

    /// `429B20` changes DWORD12/13 only in unsuppressed fullscreen.
    pub(crate) fn set_alternate(&mut self, enabled: bool, render_size: [u32; 2]) {
        self.alternate_enabled = enabled;
        if self.fullscreen && !self.suppress_alternate {
            self.recenter_viewport(render_size);
        }
    }

    /// `4291F0`/`429220`: windowed resize and policy scaling apply their
    /// viewport during the later presentation pass, not inside each quad.
    pub(crate) fn geometry_origin(&self) -> [u32; 2] {
        if self.fullscreen && !self.suppress_alternate {
            self.viewport_origin
        } else {
            [0, 0]
        }
    }
}

/// `4266C0` runs before script dispatch; `467C60` updates both live surfaces
/// only when an enabled channel observes a different cached view predicate.
pub(crate) fn apply_channel_origins(state: &mut CmvsPs2aVmState) -> Result<(), CoreError> {
    let alternate = state.native_view.alternate_active();
    let predicate = u32::from(alternate);
    for index in 0..8_u8 {
        if !state
            .slot_objects
            .contains_key(&(742 << 8 | u32::from(index)))
        {
            continue;
        }
        let channel = state.effect_channels.get_mut(&index).ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_VIEW_CHANNEL",
                "native view channel has no live state",
            )
        })?;
        if channel.read_dword(1244) == 0 || channel.read_dword(1246) == predicate {
            continue;
        }
        let text_slot = channel.read_dword(0) as u8;
        if !state
            .slot_objects
            .contains_key(&(750 << 8 | u32::from(text_slot)))
        {
            return Err(invalid(
                "SHINKI_EMU_CMVS_VIEW_TEXT",
                "native view channel has no live text owner",
            ));
        }
        let text = state.native_text.get_mut(&text_slot).ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_VIEW_TEXT",
                "native view channel has no text owner",
            )
        })?;
        let origin_word = if alternate { 1236 } else { 1240 };
        let x = channel.read_dword(origin_word);
        let y = channel.read_dword(origin_word + 2);
        // Do not call set_origin: the view consumer preserves the default
        // pair at words1240/1242 while updating only current words6/8.
        channel.write_dword(6, x);
        channel.write_dword(8, y);
        channel.write_dword(1246, predicate);
        let position = [x as i32, y as i32];
        if let Some(root) = state.effect_elements.get_mut(&(index, u32::MAX)) {
            root.position = Some(position);
        }
        text.position = position;
    }
    Ok(())
}
