//! Camera owners serialized by 456E80/457CB0 and 458380/458B70.
//! The v3 plane record is 30, 90, or 154 bytes according to DWORD38;
//! the independent look-at/FOV owner has a fixed 34-byte v2 record.
use super::slot_io::{Reader, invalid};
use shinki_emu_sdk::CoreError;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SavedProjection {
    pub cameras: [SavedCamera; 2],
    pub views: [SavedView; 2],
}

impl Default for SavedProjection {
    fn default() -> Self {
        let mut cameras = [SavedCamera::default(), SavedCamera::default()];
        cameras[0].enabled = 1;
        let mut views = [SavedView::default(), SavedView::default()];
        views[0].enabled = 1;
        Self { cameras, views }
    }
}

impl SavedProjection {
    pub(crate) fn decode_record(&mut self, tag: u16, payload: &[u8]) -> Result<(), CoreError> {
        match tag {
            2176..=2177 => self.cameras[usize::from(tag - 2176)] = SavedCamera::decode(payload)?,
            2178..=2179 => self.views[usize::from(tag - 2178)] = SavedView::decode(payload)?,
            _ => return Err(invalid("unknown native projection record")),
        }
        Ok(())
    }

    pub(crate) fn encode_record(&self, tag: u16) -> Result<Vec<u8>, CoreError> {
        match tag {
            2176..=2177 => self.cameras[usize::from(tag - 2176)].encode(),
            2178..=2179 => Ok(self.views[usize::from(tag - 2178)].encode()),
            _ => Err(invalid("unknown native projection record")),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        for camera in &self.cameras {
            camera.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SavedCamera {
    pub enabled: u32,
    /// Writer emits zero; paired loader skips this DWORD.
    pub reserved: u32,
    /// Native DWORD38 selects the record body, not projection mode DWORD0.
    pub kind: u32,
    pub plane: Option<SavedPlane>,
    pub transition: Option<SavedCameraTransition>,
    pub center: [f32; 2],
    pub scale: [f32; 2],
}

impl Default for SavedCamera {
    fn default() -> Self {
        Self {
            enabled: 0,
            reserved: 0,
            kind: 0,
            plane: Some(SavedPlane::default()),
            transition: None,
            center: [512.0, 307.0],
            scale: [1.0, 1.0],
        }
    }
}

impl SavedCamera {
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if self.plane.is_some() != matches!(self.kind, 0 | 1)
            || self.transition.is_some() != (self.kind == 1)
        {
            return Err(invalid("native camera body does not match its kind"));
        }
        Ok(())
    }

    fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        let mut r = Reader::new(bytes);
        if r.u16()? != 3 {
            return Err(invalid("native plane camera version is not 3"));
        }
        let enabled = r.u32()?;
        let reserved = r.u32()?;
        let kind = r.u32()?;
        let plane = if matches!(kind, 0 | 1) {
            Some(SavedPlane::decode(&mut r)?)
        } else {
            None
        };
        let transition = if kind == 1 {
            Some(SavedCameraTransition::decode(&mut r)?)
        } else {
            None
        };
        let value = Self {
            enabled,
            reserved,
            kind,
            plane,
            transition,
            center: [r.f32()?, r.f32()?],
            scale: [r.f32()?, r.f32()?],
        };
        r.finish()?;
        value.validate()?;
        Ok(value)
    }

    fn encode(&self) -> Result<Vec<u8>, CoreError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(match self.kind {
            0 => 90,
            1 => 154,
            _ => 30,
        });
        bytes.extend_from_slice(&3u16.to_le_bytes());
        for word in [self.enabled, self.reserved, self.kind] {
            put_word(&mut bytes, word);
        }
        if let Some(plane) = &self.plane {
            plane.encode(&mut bytes);
        }
        if let Some(transition) = &self.transition {
            transition.encode(&mut bytes);
        }
        put_floats(&mut bytes, &self.center);
        put_floats(&mut bytes, &self.scale);
        Ok(bytes)
    }
}

/// Native DWORD0..14. Auxiliary words are retained in the codec, but the VM
/// bridge rejects nonzero values where the scene has no corresponding consumer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SavedPlane {
    pub mode: u32,
    pub position: [f32; 3],
    pub auxiliary: [f32; 2],
    pub depth: f32,
    pub extent: [f32; 2],
    pub output: [f32; 2],
    pub slope: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
}

impl Default for SavedPlane {
    fn default() -> Self {
        Self {
            mode: 1,
            position: [0.0, 3.0, 0.0],
            auxiliary: [0.0; 2],
            depth: 10.0,
            extent: [10.0, 6.0],
            output: [1024.0, 614.0],
            slope: 24.0,
            pitch: 0.0,
            yaw: 0.0,
            roll: 0.0,
        }
    }
}

impl SavedPlane {
    fn decode(r: &mut Reader<'_>) -> Result<Self, CoreError> {
        Ok(Self {
            mode: r.u32()?,
            position: [r.f32()?, r.f32()?, r.f32()?],
            auxiliary: [r.f32()?, r.f32()?],
            depth: r.f32()?,
            extent: [r.f32()?, r.f32()?],
            output: [r.f32()?, r.f32()?],
            slope: r.f32()?,
            pitch: r.f32()?,
            yaw: r.f32()?,
            roll: r.f32()?,
        })
    }

    fn encode(&self, bytes: &mut Vec<u8>) {
        put_word(bytes, self.mode);
        put_floats(bytes, &self.position);
        put_floats(bytes, &self.auxiliary);
        put_floats(bytes, &[self.depth]);
        put_floats(bytes, &self.extent);
        put_floats(bytes, &self.output);
        put_floats(bytes, &[self.slope, self.pitch, self.yaw, self.roll]);
    }
}

/// DWORD21..36: 4570F0 advances this transition. 456E80 saves its restart
/// position and zero elapsed time, rather than serializing the visible instant.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SavedCameraTransition {
    pub active: u32,
    pub flags: u32,
    pub target_position: [f32; 3],
    pub target_angles: [f32; 3],
    pub duration: u32,
    pub origin_position: [f32; 3],
    pub origin_angles: [f32; 3],
    pub elapsed: u32,
}

impl SavedCameraTransition {
    fn decode(r: &mut Reader<'_>) -> Result<Self, CoreError> {
        Ok(Self {
            active: r.u32()?,
            flags: r.u32()?,
            target_position: [r.f32()?, r.f32()?, r.f32()?],
            target_angles: [r.f32()?, r.f32()?, r.f32()?],
            duration: r.u32()?,
            origin_position: [r.f32()?, r.f32()?, r.f32()?],
            origin_angles: [r.f32()?, r.f32()?, r.f32()?],
            elapsed: r.u32()?,
        })
    }

    fn encode(&self, bytes: &mut Vec<u8>) {
        put_word(bytes, self.active);
        put_word(bytes, self.flags);
        put_floats(bytes, &self.target_position);
        put_floats(bytes, &self.target_angles);
        put_word(bytes, self.duration);
        put_floats(bytes, &self.origin_position);
        put_floats(bytes, &self.origin_angles);
        put_word(bytes, self.elapsed);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SavedView {
    pub enabled: u32,
    pub position: [f32; 3],
    pub angles: [f32; 3],
    pub field_of_view: f32,
}

impl Default for SavedView {
    fn default() -> Self {
        Self {
            enabled: 0,
            position: [0.0, 1.0, 0.0],
            angles: [0.0; 3],
            field_of_view: 45.0,
        }
    }
}

impl SavedView {
    fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        let mut r = Reader::new(bytes);
        if r.u16()? != 2 {
            return Err(invalid("native look-at camera version is not 2"));
        }
        let value = Self {
            enabled: r.u32()?,
            position: [r.f32()?, r.f32()?, r.f32()?],
            angles: [r.f32()?, r.f32()?, r.f32()?],
            field_of_view: r.f32()?,
        };
        r.finish()?;
        Ok(value)
    }

    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(34);
        bytes.extend_from_slice(&2u16.to_le_bytes());
        put_word(&mut bytes, self.enabled);
        put_floats(&mut bytes, &self.position);
        put_floats(&mut bytes, &self.angles);
        put_floats(&mut bytes, &[self.field_of_view]);
        bytes
    }
}

fn put_word(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn put_floats(bytes: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        put_word(bytes, value.to_bits());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independently follows native byte offsets: v3 prefix14, DWORD0..14,
    // optional DWORD21..36, then DWORD15..18. Not produced by the codec.
    fn native_camera_fixture(kind: u32) -> Vec<u8> {
        let mut bytes = 3u16.to_le_bytes().to_vec();
        for word in [1u32, 0, kind] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        if matches!(kind, 0 | 1) {
            let body = [
                3u32,
                1.0f32.to_bits(),
                2.0f32.to_bits(),
                4.0f32.to_bits(),
                0,
                0,
                10.0f32.to_bits(),
                8.0f32.to_bits(),
                6.0f32.to_bits(),
                32.0f32.to_bits(),
                24.0f32.to_bits(),
                0,
                0,
                0,
                90.0f32.to_bits(),
            ];
            for word in body {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        if kind == 1 {
            for word in [
                1u32,
                0x2000_0000,
                5.0f32.to_bits(),
                6.0f32.to_bits(),
                7.0f32.to_bits(),
                0,
                0,
                180.0f32.to_bits(),
                1000,
                1.0f32.to_bits(),
                2.0f32.to_bits(),
                4.0f32.to_bits(),
                0,
                0,
                90.0f32.to_bits(),
                0,
            ] {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        for value in [16.0f32, 12.0, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn native_v3_camera_forms_decode_their_distinct_bodies() {
        for (kind, length) in [(0, 90), (1, 154), (2, 30)] {
            let bytes = native_camera_fixture(kind);
            assert_eq!(bytes.len(), length);
            let camera = SavedCamera::decode(&bytes).unwrap();
            assert_eq!(camera.kind, kind);
            assert_eq!(camera.center, [16.0, 12.0]);
            assert_eq!(camera.scale, [2.0, 3.0]);
            if let Some(plane) = &camera.plane {
                assert_eq!(plane.position, [1.0, 2.0, 4.0]);
                assert_eq!(plane.roll, 90.0);
            }
            if let Some(tween) = &camera.transition {
                assert_eq!(tween.duration, 1000);
                assert_eq!(tween.target_position, [5.0, 6.0, 7.0]);
                assert_eq!(tween.origin_angles, [0.0, 0.0, 90.0]);
            }
            assert_eq!(camera.encode().unwrap(), bytes);
            assert!(SavedCamera::decode(&bytes[..bytes.len() - 1]).is_err());
        }
    }

    #[test]
    fn native_v2_view_fields_are_position_angles_and_field_of_view() {
        let mut bytes = 2u16.to_le_bytes().to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        for value in [2.0f32, 3.0, 4.0, 10.0, 20.0, 30.0, 60.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let view = SavedView::decode(&bytes).unwrap();
        assert_eq!(bytes.len(), 34);
        assert_eq!(view.position, [2.0, 3.0, 4.0]);
        assert_eq!(view.angles, [10.0, 20.0, 30.0]);
        assert_eq!(view.field_of_view, 60.0);
        assert_eq!(view.encode(), bytes);
    }
}
