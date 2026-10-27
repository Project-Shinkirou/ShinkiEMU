use super::*;

const CONTROLLER_TABLE: u16 = 796 * 4;
const PARENT_TABLE: u16 = 481 * 4;
const RANGE_COUNT: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct CmvRange {
    pub repeat: bool,
    pub start: u32,
    pub end: u32,
}

/// CImageVideo's sixteen range records and independent playback clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct CmvController {
    pub logical_index: u32,
    pub parent_slot: u8,
    pub parent_identity: u64,
    pub resource: String,
    pub source_uri: Option<String>,
    pub source_frame: u16,
    pub open_mode: u32,
    pub initial_frame: u32,
    pub current_frame: u32,
    pub width: u32,
    pub height: u32,
    pub frame_count: u32,
    pub rate_num: u32,
    pub rate_den: u32,
    pub has_alpha: bool,
    pub opened: bool,
    pub playing: bool,
    pub paused: bool,
    pub mode: u32,
    pub stop_frame: u32,
    pub active_range: u8,
    pub pending_range: Option<u8>,
    pub ranges: [CmvRange; RANGE_COUNT],
    /// Fractional frame clock: milliseconds * rate_num, modulo 1000*rate_den.
    pub clock_remainder: u64,
    pub elapsed_ms: u64,
    /// Native audio channel 11's playback cursor; None selects the host-clock fallback.
    #[serde(skip)]
    pub audio_clock_ms: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CmvTarget {
    Live(u8),
    ReloadPrevious(u8),
    SceneImage(u8),
    SceneReloadPrevious(u8),
}

/// 4430E0 pins the CImage object, not its table slot. A composite 708 keeps
/// that object alive at the replacement root's +12408 until release.
pub(crate) fn bound_target(state: &CmvsPs2aVmState, slot: u8) -> Option<CmvTarget> {
    resolve_target(
        state.cmv_controllers.get(&slot)?,
        &state.slot_objects,
        &state.texture_reloads,
        &state.native_scene_workers,
    )
}

fn resolve_target(
    controller: &CmvController,
    slot_objects: &BTreeMap<u32, u64>,
    reloads: &BTreeMap<u8, super::texture_reload::TextureReload>,
    workers: &BTreeMap<u8, CmvsNativeSceneWorker>,
) -> Option<CmvTarget> {
    let current_key = u32::from(PARENT_TABLE) << 8 | u32::from(controller.parent_slot);
    if slot_objects.get(&current_key) == Some(&controller.parent_identity) {
        return Some(CmvTarget::Live(controller.parent_slot));
    }
    if let Some(parent) = slot_objects.iter().find_map(|(key, identity)| {
        ((*key >> 8) == u32::from(PARENT_TABLE) && *identity == controller.parent_identity)
            .then_some(*key as u8)
    }) {
        return Some(CmvTarget::Live(parent));
    }
    if let Some(parent) = reloads.iter().find_map(|(parent, reload)| {
        retained_identity(reload, controller.parent_identity).then_some(*parent)
    }) {
        return Some(CmvTarget::ReloadPrevious(parent));
    }
    workers.iter().find_map(|(&slot, worker)| {
        let image = worker.image_shards()?;
        if image.identity == controller.parent_identity {
            Some(CmvTarget::SceneImage(slot))
        } else if image
            .reload
            .as_ref()
            .is_some_and(|reload| retained_identity(reload, controller.parent_identity))
        {
            Some(CmvTarget::SceneReloadPrevious(slot))
        } else {
            None
        }
    })
}

fn retained_identity(reload: &super::texture_reload::TextureReload, identity: u64) -> bool {
    reload.previous_identity == identity
        && matches!(
            reload.mode,
            super::texture_reload::ReloadMode::Composite
                | super::texture_reload::ReloadMode::Failed
        )
}

/// Called only after the host has decoded and bound the requested first frame.
pub(crate) fn opened(
    state: &mut CmvsPs2aVmState,
    slot: u8,
    metadata: crate::cmv::CmvMetadata,
) -> Result<(), CoreError> {
    let controller = state.cmv_controllers.get_mut(&slot).ok_or_else(|| {
        invalid(
            "SHINKI_EMU_CMVS_CMV_OWNER",
            "CMV open callback has no controller",
        )
    })?;
    if metadata.width == 0
        || metadata.height == 0
        || metadata.frame_count == 0
        || metadata.rate_num == 0
        || metadata.rate_den == 0
        || controller.initial_frame >= metadata.frame_count
        || u128::from(metadata.frame_count) * 1000 * u128::from(metadata.rate_den)
            / u128::from(metadata.rate_num)
            == 0
    {
        return Err(invalid(
            "SHINKI_EMU_CMVS_CMV_METADATA",
            "CMV metadata or initial frame is invalid",
        ));
    }
    if controller.opened {
        if controller.width != metadata.width
            || controller.height != metadata.height
            || controller.frame_count != metadata.frame_count
            || controller.rate_num != metadata.rate_num
            || controller.rate_den != metadata.rate_den
            || controller.has_alpha != metadata.has_alpha
        {
            return Err(invalid(
                "SHINKI_EMU_CMVS_CMV_METADATA",
                "restored CMV stream metadata changed",
            ));
        }
        return Ok(());
    }
    controller.width = metadata.width;
    controller.height = metadata.height;
    controller.frame_count = metadata.frame_count;
    controller.rate_num = metadata.rate_num;
    controller.rate_den = metadata.rate_den;
    controller.has_alpha = metadata.has_alpha;
    controller.ranges[0] = CmvRange {
        repeat: false,
        start: controller.initial_frame,
        end: metadata.frame_count - 1,
    };
    controller.opened = true;
    Ok(())
}

pub(super) fn validate(state: &CmvsPs2aVmState) -> Result<(), CoreError> {
    for (slot, controller) in &state.cmv_controllers {
        if *slot > 2
            || controller.active_range as usize >= RANGE_COUNT
            || controller
                .pending_range
                .is_some_and(|range| range as usize >= RANGE_COUNT)
            || controller.resource.is_empty()
            || controller.open_mode > 3
            || controller.parent_identity == 0
            || !state
                .slot_objects
                .contains_key(&slot_object_key(CONTROLLER_TABLE, u32::from(*slot))?)
            || (controller.opened
                && (controller.width == 0
                    || controller.height == 0
                    || controller.frame_count == 0
                    || controller.rate_num == 0
                    || controller.rate_den == 0
                    || u128::from(controller.frame_count) * 1000 * u128::from(controller.rate_den)
                        / u128::from(controller.rate_num)
                        == 0
                    || controller.ranges.iter().any(|range| {
                        range.end >= controller.frame_count
                            || (range.start != u32::MAX
                                && (range.start >= controller.frame_count
                                    || range.start > range.end))
                    })
                    || controller.clock_remainder >= 1000 * u64::from(controller.rate_den)))
        {
            return Err(invalid(
                "SHINKI_EMU_CMVS_CMV_STATE",
                "invalid CMV controller snapshot",
            ));
        }
    }
    Ok(())
}

fn present(controller: &CmvController, slot: u8, previous: u32) -> Option<CmvsPs2aVmAction> {
    (controller.opened
        && controller.current_frame != previous
        && controller.current_frame < controller.frame_count)
        .then_some(CmvsPs2aVmAction::PresentCmvFrame {
            controller_slot: slot,
            frame: controller.current_frame,
        })
}

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    command_id: u16,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    let slot = values
        .first()
        .ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                "CMV command has no slot operand",
            )
        })?
        .1;
    if slot > 2 {
        raise_error_flag(state, 10532, 0x0100_0000)?;
        return Ok(None);
    }
    let slot = slot as u8;
    if command_id == 776 || command_id == 782 {
        let parent = values[1].1;
        let parent_slot = u8::try_from(parent).map_err(|_| {
            invalid(
                "SHINKI_EMU_CMVS_CMV_PARENT",
                "CMV parent exceeds native texture table",
            )
        })?;
        let resource = resolve_resource_name(state, values[2].1)?;
        let start_frame = values[3].1;
        let open_mode = if command_id == 782 { values[4].1 } else { 2 };
        if open_mode > 3 {
            return Err(invalid(
                "SHINKI_EMU_CMVS_CMV_OPEN_MODE",
                "unsupported native CMV stream open mode",
            ));
        }
        let controller_key = slot_object_key(CONTROLLER_TABLE, u32::from(slot))?;
        let parent_key = slot_object_key(PARENT_TABLE, parent)?;
        let identity = state.next_slot_object_id;
        state.next_slot_object_id = identity.checked_add(2).ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_VM_SLOT_OBJECTS",
                "CMV object identity space overflowed",
            )
        })?;
        state.slot_objects.insert(controller_key, identity);
        state.slot_objects.insert(parent_key, identity + 1);
        state
            .texture_parents
            .insert(parent_slot, CmvsTextureParentState::default());
        state.texture_children.insert(parent_slot, BTreeMap::new());
        state.texture_reloads.remove(&parent_slot);
        state.cmv_controllers.insert(
            slot,
            CmvController {
                logical_index: u32::from(slot),
                parent_slot,
                parent_identity: identity + 1,
                resource: resource.clone(),
                source_uri: None,
                source_frame: state.current_frame,
                open_mode,
                initial_frame: start_frame,
                current_frame: start_frame,
                width: 0,
                height: 0,
                frame_count: 0,
                rate_num: 0,
                rate_den: 0,
                has_alpha: false,
                opened: false,
                playing: false,
                paused: false,
                mode: 0,
                stop_frame: 0,
                active_range: 0,
                pending_range: None,
                ranges: [CmvRange::default(); RANGE_COUNT],
                clock_remainder: 0,
                elapsed_ms: 0,
                audio_clock_ms: None,
            },
        );
        return Ok(Some(CmvsPs2aVmAction::OpenCmvResource {
            controller_slot: slot,
            parent_slot,
            resource,
            start_frame,
        }));
    }
    if command_id == 778 {
        state
            .slot_objects
            .remove(&slot_object_key(CONTROLLER_TABLE, u32::from(slot))?);
        return Ok(state.cmv_controllers.remove(&slot).map(|_| {
            CmvsPs2aVmAction::ReleaseCmvResource {
                controller_slot: slot,
            }
        }));
    }
    let Some(controller) = state.cmv_controllers.get_mut(&slot) else {
        if command_id == 783 {
            state.write_interpreter_word(81220, u32::MAX);
            state.write_interpreter_word(81236, 0);
        } else if matches!(command_id, 794 | 795 | 796 | 798) {
            state.write_interpreter_word(81220, 0);
        }
        return Ok(None);
    };
    let previous = controller.current_frame;
    let mut result = None;
    let mut secondary = None;
    match command_id {
        777 => {
            let range = usize::try_from(values[1].1)
                .ok()
                .filter(|range| *range < RANGE_COUNT)
                .ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_CMV_RANGE",
                        "CMV range index exceeds sixteen native records",
                    )
                })?;
            let start = values[2].1;
            let end = if values[3].1 == u32::MAX {
                controller.frame_count.saturating_sub(1)
            } else {
                values[3].1
            };
            if !controller.opened
                || (start != u32::MAX && start >= controller.frame_count)
                || end >= controller.frame_count
                || (start != u32::MAX && start > end)
            {
                return Err(invalid(
                    "SHINKI_EMU_CMVS_CMV_RANGE",
                    "CMV range is outside the decoded stream",
                ));
            }
            controller.ranges[range] = CmvRange {
                start,
                end,
                repeat: values[4].1 != 0,
            };
            if controller.playing {
                controller.pending_range = Some(range as u8);
            } else {
                controller.active_range = range as u8;
                controller.clock_remainder = 0;
                controller.elapsed_ms = 0;
                controller.pending_range = None;
                controller.playing = true;
            }
        }
        779 => {
            if controller.playing {
                controller.clock_remainder = 0;
                controller.elapsed_ms = 0;
            }
            controller.paused = false;
        }
        780 => controller.paused = true,
        781 => controller.mode = values[1].1,
        783 => {
            result = Some(u32::from(controller.active_range));
            secondary = Some(controller.current_frame);
        }
        794 => {
            result = Some(u32::from(controller.playing));
        }
        795 => {
            controller.current_frame = values[1].1;
            controller.playing = false;
            result = Some(1);
        }
        796 => {
            let queued = controller.pending_range.take();
            result = Some(u32::from(queued.is_some()));
            if let Some(range) = queued {
                controller.active_range = range;
                let start = controller.ranges[usize::from(range)].start;
                controller.current_frame = if start == u32::MAX {
                    controller.current_frame.wrapping_add(1)
                } else {
                    start
                };
            }
        }
        797 => controller.stop_frame = values[1].1,
        798 => {
            result = Some(controller.current_frame);
        }
        _ => {
            return Err(invalid(
                "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
                "unknown CMV controller command",
            ));
        }
    }
    let action = present(controller, slot, previous);
    if let Some(value) = result {
        state.write_interpreter_word(81220, value);
    }
    if let Some(value) = secondary {
        state.write_interpreter_word(81236, value);
    }
    Ok(action.filter(|_| bound_target(state, slot).is_some()))
}

/// Native 442CC0: consume real elapsed time; a terminal boundary clears playing
/// only on the step after reaching the inclusive end frame.
pub(crate) fn advance(state: &mut CmvsPs2aVmState, delta_ms: u32) -> Vec<CmvsPs2aVmAction> {
    let mut actions = Vec::new();
    for (&slot, controller) in &mut state.cmv_controllers {
        if !controller.opened
            || controller.paused
            || !controller.playing
            || !matches!(controller.mode, 0 | 10 | 15)
        {
            continue;
        }
        let previous = controller.current_frame;
        let delta = if controller.mode == 0 {
            delta_ms.min(1000)
        } else {
            delta_ms
        };
        controller.elapsed_ms = controller.elapsed_ms.saturating_add(u64::from(delta));
        let denominator = 1000 * u64::from(controller.rate_den);
        let numerator = u128::from(controller.clock_remainder)
            + u128::from(delta) * u128::from(controller.rate_num);
        let mut steps = (numerator / u128::from(denominator)) as u64;
        controller.clock_remainder = (numerator % u128::from(denominator)) as u64;
        match controller.mode {
            15 => {
                let duration_ms = u128::from(controller.frame_count) * u128::from(denominator)
                    / u128::from(controller.rate_num);
                let clock = controller
                    .audio_clock_ms
                    .map_or(controller.elapsed_ms, u64::from);
                controller.current_frame = ((u128::from(clock) % duration_ms)
                    * u128::from(controller.rate_num)
                    / u128::from(denominator)) as u32;
            }
            0 | 10 => {
                while steps != 0 {
                    let range = controller.ranges[usize::from(controller.active_range)];
                    if controller.current_frame < range.end {
                        let mut count = u64::from(range.end - controller.current_frame).min(steps);
                        if controller.mode == 10 && controller.stop_frame != 0 {
                            if controller.current_frame == controller.stop_frame {
                                break;
                            }
                            if controller.current_frame < controller.stop_frame {
                                count = count.min(u64::from(
                                    controller.stop_frame - controller.current_frame,
                                ));
                            }
                        }
                        controller.current_frame += count as u32;
                        steps -= count;
                        continue;
                    }
                    if controller.mode == 0 {
                        if let Some(next) = controller.pending_range.take() {
                            controller.active_range = next;
                            let start = controller.ranges[usize::from(next)].start;
                            controller.current_frame =
                                if start == u32::MAX || start == controller.current_frame {
                                    controller.current_frame.wrapping_add(1)
                                } else {
                                    start
                                };
                            steps -= 1;
                            continue;
                        }
                        if range.repeat {
                            controller.current_frame = range.start;
                            steps -= 1;
                            if range.start <= range.end {
                                steps %= u64::from(range.end - range.start) + 1;
                            } else {
                                steps = 0;
                            }
                            continue;
                        }
                    }
                    controller.paused = true;
                    controller.playing = false;
                    break;
                }
            }
            _ => {}
        }
        if let Some(action) = present(controller, slot, previous)
            && resolve_target(
                controller,
                &state.slot_objects,
                &state.texture_reloads,
                &state.native_scene_workers,
            )
            .is_some()
        {
            actions.push(action);
        }
    }
    actions
}

#[cfg(test)]
mod tests;
