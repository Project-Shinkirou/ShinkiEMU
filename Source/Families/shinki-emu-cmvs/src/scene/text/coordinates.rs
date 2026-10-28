use super::*;

/// Native glyphs are rasterized in captured MODE pixels, then their CBasic
/// reference dimensions map those pixels to the current logical stage.
#[derive(Clone, Copy)]
pub(super) struct NativeCoordinates {
    numerator: u32,
    denominator: u32,
    logical_scale: f32,
}

impl NativeCoordinates {
    pub(super) fn new(
        surface: &crate::CmvsNativeTextSurface,
        stage_width: u32,
    ) -> Result<Self, CoreError> {
        let numerator = surface.physical_size[0];
        let denominator = surface.virtual_size[0];
        if numerator == 0 || denominator == 0 || stage_width == 0 {
            return Err(error("SHINKI_EMU_CMVS_TEXT_GEOMETRY"));
        }
        Ok(Self {
            numerator,
            denominator,
            logical_scale: stage_width as f32 / numerator as f32,
        })
    }

    pub(super) fn logical_scale(self) -> f32 {
        self.logical_scale
    }

    pub(super) fn font_size(self, size: u32) -> Result<u32, CoreError> {
        let pixels = self.numerator.wrapping_mul(size) / self.denominator;
        if !(1..=4096).contains(&pixels) {
            return Err(error("SHINKI_EMU_CMVS_TEXT_GEOMETRY"));
        }
        Ok(pixels)
    }

    pub(super) fn position(self, origin: [i32; 2], local: [i32; 2]) -> Result<[i32; 2], CoreError> {
        let axis = |origin, local| {
            // 466290 and 4662D0 use wrapping DWORD products and DIV.
            let scale =
                |value: i32| (self.numerator.wrapping_mul(value as u32) / self.denominator) as i32;
            scale(origin)
                .checked_add(scale(local))
                .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_GEOMETRY"))
        };
        Ok([axis(origin[0], local[0])?, axis(origin[1], local[1])?])
    }

    pub(super) fn virtual_advance(self, pixels: u32) -> Result<u32, CoreError> {
        let width =
            (u64::from(pixels) * u64::from(self.denominator)).div_ceil(u64::from(self.numerator));
        u32::try_from(width).map_err(|_| error("SHINKI_EMU_CMVS_TEXT_ADVANCE"))
    }
}
