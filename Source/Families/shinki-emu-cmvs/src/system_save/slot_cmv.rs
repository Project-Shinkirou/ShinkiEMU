use super::slot_io::{Reader, invalid};
use crate::vm::{
    CmvsPs2aVmState,
    cmv::{CmvController, CmvRange},
};
use shinki_emu_sdk::CoreError;

/// One native CImageVideo range, in repeat/start/end wire order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SavedCmvRange {
    pub repeat: u32,
    pub start: u32,
    pub end: u32,
}

/// Tag 2280 describes the three controllers in byte table 3184. It is unrelated
/// to the eight movie objects in byte table 3100. No retained-root or clock data
/// is present in this original-file record; private VM snapshots retain those.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SavedCmv {
    pub version: u16,
    pub logical_index: u32,
    pub parent_slot: u32,
    pub source: [u8; 64],
    pub active_range: u32,
    pub pending_range: u32,
    pub playing: u32,
    pub paused: u32,
    pub ranges: [SavedCmvRange; 16],
    pub current_frame: u32,
    pub mode: u32,
    pub open_mode: u32,
    pub stop_frame: u32,
}

impl SavedCmv {
    pub(crate) fn decode(payload: &[u8]) -> Result<Self, CoreError> {
        let mut input = Reader::new(payload);
        let version = input.u16()?;
        if version > 4 {
            return Err(invalid("unsupported native CMV record version"));
        }
        let logical_index = input.u32()?;
        let parent_slot = input.u32()?;
        let source = input.bytes(64)?.try_into().unwrap();
        let active_range = input.u32()?;
        let pending_range = input.u32()?;
        let playing = input.u32()?;
        let paused = input.u32()?;
        let mut ranges = [SavedCmvRange::default(); 16];
        for range in &mut ranges {
            *range = SavedCmvRange {
                repeat: input.u32()?,
                start: input.u32()?,
                end: input.u32()?,
            };
        }
        let current_frame = input.u32()?;
        let mode = if version >= 3 { input.u32()? } else { 0 };
        let open_mode = if version >= 4 { input.u32()? } else { 2 };
        let stop_frame = if version >= 4 { input.u32()? } else { 0 };
        input.finish()?;
        let saved = Self {
            version,
            logical_index,
            parent_slot,
            source,
            active_range,
            pending_range,
            playing,
            paused,
            ranges,
            current_frame,
            mode,
            open_mode,
            stop_frame,
        };
        saved.validate()?;
        Ok(saved)
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        self.validate()?;
        let mut output = Vec::with_capacity(298);
        output.extend_from_slice(&self.version.to_le_bytes());
        for word in [self.logical_index, self.parent_slot] {
            output.extend_from_slice(&word.to_le_bytes());
        }
        output.extend_from_slice(&self.source);
        for word in [
            self.active_range,
            self.pending_range,
            self.playing,
            self.paused,
        ] {
            output.extend_from_slice(&word.to_le_bytes());
        }
        for range in &self.ranges {
            for word in [range.repeat, range.start, range.end] {
                output.extend_from_slice(&word.to_le_bytes());
            }
        }
        output.extend_from_slice(&self.current_frame.to_le_bytes());
        if self.version >= 3 {
            output.extend_from_slice(&self.mode.to_le_bytes());
        }
        if self.version >= 4 {
            output.extend_from_slice(&self.open_mode.to_le_bytes());
            output.extend_from_slice(&self.stop_frame.to_le_bytes());
        }
        Ok(output)
    }

    pub(crate) fn resource(&self) -> Result<String, CoreError> {
        self.resource_text().map(|source| source.into_owned())
    }

    fn resource_text(&self) -> Result<std::borrow::Cow<'_, str>, CoreError> {
        let end = self
            .source
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| invalid("native CMV source is unterminated"))?;
        if end == 0 {
            return Err(invalid("native CMV source is empty"));
        }
        encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(&self.source[..end])
            .ok_or_else(|| invalid("native CMV source has invalid Shift-JIS"))
    }

    fn validate(&self) -> Result<(), CoreError> {
        if self.version > 4
            || self.logical_index > 2
            || self.parent_slot > 255
            || self.active_range >= 16
            || (self.pending_range != u32::MAX && self.pending_range >= 16)
            || self.open_mode > 3
            || (self.version < 3 && self.mode != 0)
            || (self.version < 4 && (self.open_mode != 2 || self.stop_frame != 0))
        {
            return Err(invalid("native CMV descriptor is invalid"));
        }
        self.resource_text()?;
        Ok(())
    }

    pub(crate) fn capture(state: &CmvsPs2aVmState, slot: u16) -> Result<Self, CoreError> {
        let slot = u8::try_from(slot)
            .ok()
            .filter(|slot| *slot <= 2)
            .ok_or_else(|| invalid("native CMV controller slot exceeds table 3184"))?;
        let controller = state
            .cmv_controllers
            .get(&slot)
            .ok_or_else(|| invalid("native CMV capture has no controller"))?;
        if !controller.opened
            || !state
                .slot_objects
                .contains_key(&((3184u32 << 8) | u32::from(slot)))
        {
            return Err(invalid(
                "native CMV capture requires an opened table-3184 controller",
            ));
        }
        let (name, _, malformed) = encoding_rs::SHIFT_JIS.encode(&controller.resource);
        if malformed || name.len() >= 64 || name.contains(&0) {
            return Err(invalid("native CMV source exceeds its 64-byte field"));
        }
        let mut source = [0; 64];
        source[..name.len()].copy_from_slice(&name);
        let saved = Self {
            version: 4,
            logical_index: controller.logical_index,
            parent_slot: u32::from(controller.parent_slot),
            source,
            active_range: u32::from(controller.active_range),
            pending_range: controller.pending_range.map_or(u32::MAX, u32::from),
            playing: u32::from(controller.playing),
            paused: u32::from(controller.paused),
            ranges: controller.ranges.map(|range| SavedCmvRange {
                repeat: u32::from(range.repeat),
                start: range.start,
                end: range.end,
            }),
            current_frame: controller.current_frame,
            mode: controller.mode,
            open_mode: controller.open_mode,
            stop_frame: controller.stop_frame,
        };
        saved.validate()?;
        Ok(saved)
    }

    /// 442FA0 reopens mode 3 as mode 2 and restarts audio-clock mode 15 at zero.
    /// The saved mode itself remains 15; only its initial decode frame changes.
    pub(crate) fn reopen_mode(&self) -> u32 {
        if self.open_mode == 3 {
            2
        } else {
            self.open_mode
        }
    }

    pub(crate) fn decode_frame(&self) -> u32 {
        if self.mode == 15 {
            0
        } else {
            self.current_frame
        }
    }

    /// Build only the controller. The caller must supply a reconstructed LIVE
    /// parent identity and publish its frame without replacing the parent tree.
    pub(crate) fn reconstruct(
        &self,
        parent_identity: u64,
        source_frame: u16,
        source_uri: String,
        metadata: crate::cmv::CmvMetadata,
    ) -> Result<CmvController, CoreError> {
        self.validate()?;
        let frame = self.decode_frame();
        if parent_identity == 0
            || metadata.width == 0
            || metadata.height == 0
            || metadata.frame_count == 0
            || metadata.rate_num == 0
            || metadata.rate_den == 0
            || frame >= metadata.frame_count
            || u128::from(metadata.frame_count) * 1000 * u128::from(metadata.rate_den)
                / u128::from(metadata.rate_num)
                == 0
            || self.ranges.iter().any(|range| {
                range.end >= metadata.frame_count
                    || (range.start != u32::MAX
                        && (range.start >= metadata.frame_count || range.start > range.end))
            })
        {
            return Err(invalid(
                "native CMV state is outside the reopened stream metadata",
            ));
        }
        Ok(CmvController {
            logical_index: self.logical_index,
            parent_slot: self.parent_slot as u8,
            parent_identity,
            resource: self.resource()?,
            source_uri: Some(source_uri),
            source_frame,
            open_mode: self.reopen_mode(),
            initial_frame: frame,
            current_frame: frame,
            width: metadata.width,
            height: metadata.height,
            frame_count: metadata.frame_count,
            rate_num: metadata.rate_num,
            rate_den: metadata.rate_den,
            has_alpha: metadata.has_alpha,
            opened: true,
            playing: self.playing != 0,
            paused: self.paused != 0,
            mode: self.mode,
            stop_frame: self.stop_frame,
            active_range: self.active_range as u8,
            pending_range: (self.pending_range != u32::MAX).then_some(self.pending_range as u8),
            ranges: self.ranges.map(|range| CmvRange {
                repeat: range.repeat != 0,
                start: range.start,
                end: range.end,
            }),
            clock_remainder: 0,
            elapsed_ms: 0,
            audio_clock_ms: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0u8; 298];
        bytes[0..2].copy_from_slice(&4u16.to_le_bytes());
        bytes[2..6].copy_from_slice(&2u32.to_le_bytes());
        bytes[6..10].copy_from_slice(&7u32.to_le_bytes());
        bytes[10..20].copy_from_slice(b"sample.cmv");
        // Preserve non-string tail bytes too: 4429E0 copies all 64 source bytes.
        bytes[73] = 0xa5;
        bytes[78..82].copy_from_slice(&u32::MAX.to_le_bytes());
        bytes[82..86].copy_from_slice(&1u32.to_le_bytes());
        bytes[98..102].copy_from_slice(&9u32.to_le_bytes());
        bytes[282..286].copy_from_slice(&5u32.to_le_bytes());
        bytes[286..290].copy_from_slice(&15u32.to_le_bytes());
        bytes[290..294].copy_from_slice(&3u32.to_le_bytes());
        bytes[294..298].copy_from_slice(&8u32.to_le_bytes());
        bytes
    }

    #[test]
    fn native_v4_exact_fixture_and_reopening_normalization() {
        let bytes = fixture();
        let saved = SavedCmv::decode(&bytes).unwrap();
        assert_eq!(saved.encode().unwrap(), bytes);
        assert_eq!(saved.current_frame, 5);
        assert_eq!(saved.reopen_mode(), 2);
        assert_eq!(saved.decode_frame(), 0);
        let restored = saved
            .reconstruct(
                91,
                4,
                "fixture:sample.cmv".into(),
                crate::cmv::CmvMetadata {
                    width: 8,
                    height: 6,
                    frame_count: 10,
                    rate_num: 25,
                    rate_den: 1,
                    has_alpha: false,
                },
            )
            .unwrap();
        assert_eq!(restored.parent_identity, 91);
        assert_eq!(restored.parent_slot, 7);
        assert_eq!(restored.logical_index, 2);
        assert_eq!(restored.current_frame, 0);
        assert_eq!(restored.mode, 15);
        assert_eq!(restored.open_mode, 2);
        assert_eq!(
            (
                restored.clock_remainder,
                restored.elapsed_ms,
                restored.audio_clock_ms
            ),
            (0, 0, None)
        );
        assert_eq!(restored.ranges[0].end, 9);
    }

    #[test]
    fn original_record_drops_retained_identity_and_private_clock_only_on_reconstruction() {
        let saved = SavedCmv::decode(&fixture()).unwrap();
        let metadata = crate::cmv::CmvMetadata {
            width: 8,
            height: 6,
            frame_count: 10,
            rate_num: 25,
            rate_den: 1,
            has_alpha: false,
        };
        let mut exact = saved
            .reconstruct(33, 4, "cmvs:/source/sample.cmv".into(), metadata)
            .unwrap();
        exact.current_frame = 5;
        exact.clock_remainder = 377;
        exact.elapsed_ms = 802;
        exact.audio_clock_ms = Some(900);
        let mut vm = CmvsPs2aVmState::new(0);
        vm.slot_objects.insert(3184 << 8, 101);
        vm.cmv_controllers.insert(0, exact.clone());
        let original_file = SavedCmv::capture(&vm, 0).unwrap();
        assert_eq!(vm.cmv_controllers[&0], exact);
        let reopened = original_file
            .reconstruct(91, 4, "cmvs:/source/sample.cmv".into(), metadata)
            .unwrap();
        assert_eq!(reopened.parent_identity, 91);
        assert_eq!(reopened.current_frame, 0);
        assert_eq!(
            (
                reopened.clock_remainder,
                reopened.elapsed_ms,
                reopened.audio_clock_ms
            ),
            (0, 0, None)
        );
        assert_eq!(
            (
                exact.parent_identity,
                exact.current_frame,
                exact.clock_remainder
            ),
            (33, 5, 377)
        );
    }

    #[test]
    fn native_older_versions_have_native_defaults_and_exact_lengths() {
        for version in 0..=3u16 {
            let mut bytes = fixture();
            let length = if version < 3 { 286 } else { 290 };
            bytes.truncate(length);
            bytes[..2].copy_from_slice(&version.to_le_bytes());
            let saved = SavedCmv::decode(&bytes).unwrap();
            assert_eq!(saved.open_mode, 2);
            assert_eq!(saved.stop_frame, 0);
            assert_eq!(saved.mode, if version < 3 { 0 } else { 15 });
            assert_eq!(saved.encode().unwrap(), bytes);
        }
        let bytes = fixture();
        assert!(SavedCmv::decode(&bytes[..297]).is_err());
        let mut trailing = bytes;
        trailing.push(0);
        assert!(SavedCmv::decode(&trailing).is_err());
    }
}
