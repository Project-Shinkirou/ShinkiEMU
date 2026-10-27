//! Native 424120/424750/4247D0 timeline state and recovered consumers.
use super::*;
use crate::system_save::slot_globals::{NativeTimeline, NativeTimelineEntry};

fn failure(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_TIMELINE", message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeTimelineTarget {
    Parent(u8),
    ChannelElement(u32),
}

pub(crate) struct NativeTimelineReload {
    pub target: NativeTimelineTarget,
    pub resource: String,
    pub duration: u32,
}

pub(crate) fn validate(timeline: &NativeTimeline) -> Result<(), CoreError> {
    timeline.validate()?;
    // 424480 and the native wire preserve raw values. A forced mode2 does not
    // read them; the eventual camera/motion/geometry consumer defines their use.
    for entry in &timeline.records[..timeline.count as usize] {
        match entry.kind {
            1 if entry.mode == 0
                && entry.target == 0
                && entry.name.is_empty()
                && entry.values == [0; 3]
                && entry.curve == 0 => {}
            3 if entry.mode == 0
                && entry.target < 256
                && entry.values == [0; 3]
                && entry.curve == 0 => {}
            5 => {}
            3 if (1..=2).contains(&entry.mode) && entry.target < 256 && entry.name.is_empty() => {}
            4 if entry.target == 0 && entry.name.is_empty() && entry.curve == 0 => {}
            _ => {
                return Err(failure(
                    "native timeline record kind, mode or target is unsupported",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn capture(vm: &CmvsPs2aVmState) -> Result<NativeTimeline, CoreError> {
    let timeline = vm.native_timeline.clone().unwrap_or_default();
    validate(&timeline)?;
    Ok(timeline)
}

/// Apply only to the detached candidate VM. 424750 rebases the origin from
/// the cumulative completed delay, not the wall-clock instant of the save.
pub(crate) fn apply(vm: &mut CmvsPs2aVmState, saved: &NativeTimeline) -> Result<(), CoreError> {
    validate(saved)?;
    let mut timeline = saved.clone();
    if timeline.active != 0 && timeline.count != 0 {
        timeline.start_millis = vm
            .session_clock_millis
            .wrapping_sub(timeline.elapsed_millis);
    }
    vm.native_timeline = Some(timeline);
    Ok(())
}

pub(crate) fn reset(vm: &mut CmvsPs2aVmState) {
    // 424790 clears only five header dwords, leaving stale table entries.
    if let Some(timeline) = &mut vm.native_timeline {
        timeline.active = 0;
        timeline.count = 0;
        timeline.cursor = 0;
        timeline.start_millis = 0;
        timeline.elapsed_millis = 0;
    }
}

pub(crate) fn start(vm: &mut CmvsPs2aVmState) -> Result<(), CoreError> {
    if let Some(timeline) = &mut vm.native_timeline {
        validate(timeline)?;
        if timeline.active == 0 && timeline.count != 0 {
            timeline.active = 1;
            timeline.cursor = 0;
            timeline.start_millis = vm.session_clock_millis;
            timeline.elapsed_millis = 0;
        }
    }
    Ok(())
}

pub(crate) fn stop(vm: &mut CmvsPs2aVmState) {
    if let Some(timeline) = &mut vm.native_timeline
        && timeline.active != 0
        && timeline.count != 0
    {
        timeline.active = 0;
    }
}

pub(crate) fn cursor(vm: &CmvsPs2aVmState) -> u32 {
    vm.native_timeline
        .as_ref()
        .filter(|timeline| timeline.active != 0)
        .map_or(u32::MAX, |timeline| timeline.cursor)
}

pub(crate) fn count(vm: &CmvsPs2aVmState) -> u32 {
    vm.native_timeline
        .as_ref()
        .map_or(0, |timeline| timeline.count)
}

fn push(vm: &mut CmvsPs2aVmState, entry: NativeTimelineEntry) -> Result<(), CoreError> {
    if count(vm) >= 16 {
        return Ok(());
    }
    // Native string buffers hold 63 bytes plus NUL. Validate before mutation.
    let (name, _, malformed) = encoding_rs::SHIFT_JIS.encode(&entry.name);
    if malformed || name.len() >= 64 || entry.name.contains('\0') {
        return Err(failure("native timeline name exceeds its CP932 buffer"));
    }
    let timeline = vm
        .native_timeline
        .get_or_insert_with(NativeTimeline::default);
    timeline.records[timeline.count as usize] = entry;
    timeline.count += 1;
    Ok(())
}

pub(crate) fn push_delay(vm: &mut CmvsPs2aVmState, duration: u32) -> Result<(), CoreError> {
    push(
        vm,
        NativeTimelineEntry {
            kind: 1,
            duration,
            ..NativeTimelineEntry::default()
        },
    )
}

pub(crate) fn push_channel(
    vm: &mut CmvsPs2aVmState,
    channel: u32,
    name: String,
    duration: u32,
) -> Result<(), CoreError> {
    if count(vm) < 16 && channel >= 256 {
        return Err(failure("native timeline parent slot exceeds 255"));
    }
    push(
        vm,
        NativeTimelineEntry {
            kind: 3,
            target: channel,
            name,
            duration,
            ..NativeTimelineEntry::default()
        },
    )
}

/// 47C0D0 -> 4243F0: the target is a child of channel zero's CImage,
/// not a channel index. Lookup guards belong to the consumer.
pub(crate) fn push_element(
    vm: &mut CmvsPs2aVmState,
    target: u32,
    name: String,
    duration: u32,
) -> Result<(), CoreError> {
    push(
        vm,
        NativeTimelineEntry {
            kind: 5,
            target,
            name,
            duration,
            ..Default::default()
        },
    )
}

pub(crate) fn push_camera(
    vm: &mut CmvsPs2aVmState,
    mode: u32,
    values: [u32; 3],
    duration: u32,
) -> Result<(), CoreError> {
    if count(vm) >= 16 {
        return Ok(());
    }
    push(
        vm,
        NativeTimelineEntry {
            kind: 4,
            mode,
            values,
            duration,
            ..Default::default()
        },
    )
}

pub(crate) fn push_transform(
    vm: &mut CmvsPs2aVmState,
    mode: u8,
    target: u32,
    values: [u32; 3],
    curve: u32,
    duration: u32,
) -> Result<(), CoreError> {
    if count(vm) >= 16 {
        return Ok(());
    }
    if !(1..=2).contains(&mode) || target >= 256 {
        return Err(failure(
            "native timeline transform mode or target is invalid",
        ));
    }
    push(
        vm,
        NativeTimelineEntry {
            kind: 3,
            mode: u32::from(mode),
            target,
            values,
            curve,
            duration,
            ..Default::default()
        },
    )
}

/// One renderer-tick pass. Yield each resource action before the next record
/// so multiple replacements of one parent retain native owner ordering.
pub(crate) fn advance(
    vm: &mut CmvsPs2aVmState,
    skip: bool,
) -> Result<Option<NativeTimelineReload>, CoreError> {
    let Some(timeline) = &vm.native_timeline else {
        return Ok(None);
    };
    validate(timeline)?;
    if timeline.active == 0 || timeline.cursor >= timeline.count {
        return Ok(None);
    }
    loop {
        let timeline = vm.native_timeline.as_ref().unwrap();
        let entry = &timeline.records[timeline.cursor as usize];
        let (kind, mode, target, duration, values, curve) = (
            entry.kind,
            entry.mode,
            entry.target,
            entry.duration,
            entry.values,
            entry.curve,
        );
        let mut reload = None;
        if kind == 1 && !skip {
            if vm
                .session_clock_millis
                .wrapping_sub(timeline.start_millis)
                .wrapping_sub(timeline.elapsed_millis)
                < duration
            {
                return Ok(None);
            }
            let timeline = vm.native_timeline.as_mut().unwrap();
            timeline.elapsed_millis = timeline.elapsed_millis.wrapping_add(duration);
        } else if kind == 4 {
            if skip || duration == 0 {
                super::camera_transition::set_position(vm, values)?;
            } else {
                super::camera_transition::start(
                    vm,
                    108,
                    &[mode, values[0], values[1], values[2], duration],
                )?;
            }
        } else if kind == 3 && vm.slot_objects.contains_key(&((1924u32 << 8) | target)) {
            if mode == 0 {
                reload = Some(NativeTimelineReload {
                    target: NativeTimelineTarget::Parent(target as u8),
                    resource: entry.name.clone(),
                    duration: if skip { 0 } else { duration },
                });
            } else if !skip || mode == 1 {
                let image = vm
                    .texture_parents
                    .get_mut(&(target as u8))
                    .filter(|image| image.surface_initialized)
                    .ok_or_else(|| {
                        failure("native timeline transform target has no basic surface")
                    })?;
                if skip {
                    // 4247D0 mode1 flushes the old worker, then writes literal XYZ.
                    // Mode2 intentionally has no forced-completion branch in native.
                    image.basic_effects.motion = None;
                    image.float_position_words = values;
                } else if mode == 2 || image.basic_effects.motion.is_some() {
                    if mode == 2 {
                        image.basic_effects.motion = Some(Default::default());
                    }
                    // 42BBB0/42BC50 deliberately ignore a null DWORD33.
                    // Mode1 preserves that guard; only mode2 allocates it.
                    let motion = image.basic_effects.motion.as_mut().unwrap();
                    motion.active = 1;
                    motion.origin = image.float_position_words;
                    motion.start = 0;
                    motion.current = 0;
                    motion.elapsed_ms = 0;
                    motion.segments[0] = super::image_effects::CmvsFloatSegment {
                        next: -1,
                        target: values,
                        curve,
                        duration_ms: duration,
                    };
                }
            }
        } else if kind == 5
            && mode == 0
            && vm.slot_objects.contains_key(&(742u32 << 8))
            && vm.effect_channels.contains_key(&0)
            && vm.effect_elements.contains_key(&(0, u32::MAX))
        {
            if (target as i32) < 0 {
                return Err(failure(
                    "native timeline element replacement cannot replace the container root",
                ));
            }
            if target < 1024 && vm.effect_elements.contains_key(&(0, target)) {
                reload = Some(NativeTimelineReload {
                    target: NativeTimelineTarget::ChannelElement(target),
                    resource: entry.name.clone(),
                    duration: if skip { 0 } else { duration },
                });
            }
        }
        let timeline = vm.native_timeline.as_mut().unwrap();
        timeline.cursor += 1;
        if timeline.cursor == timeline.count {
            timeline.active = 0;
            return Ok(reload);
        }
        if reload.is_some() {
            return Ok(reload);
        }
    }
}

/// 4262A0 -> 445660 -> 4448A0, once per renderer frame, after timeline
/// dispatch. Like parent reloads, the unsigned clock is not capped at 200ms.
pub(crate) fn advance_channel_image_reloads(
    vm: &mut CmvsPs2aVmState,
    delta_ms: u32,
) -> Result<(), CoreError> {
    for image in vm.effect_elements.values_mut() {
        let Some(reload) = &mut image.image_reload else {
            continue;
        };
        if !reload.busy() {
            continue;
        }
        reload.elapsed_ms = reload.elapsed_ms.wrapping_add(delta_ms);
        if reload.elapsed_ms >= reload.duration_ms {
            image.image_reload = None;
            continue;
        }
        if reload.mode == super::texture_reload::ReloadMode::Composite {
            reload.previous_parent.auxiliary_pair_words =
                image.position.map(|words| words.map(|word| word as u32));
            reload.previous_parent.auxiliary_word = Some(image.order as u32);
            reload.previous_parent.float_position_words = image.float_position_words;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(vm: &mut CmvsPs2aVmState, id: u16, words: &[u32]) -> Option<CmvsPs2aVmAction> {
        for word in words.iter().rev() {
            super::super::stack::push_word(vm, *word).unwrap();
        }
        let pc = vm.program_counter;
        execute_cmvs390_frame(
            vm,
            &CmvsPs2aInstructionFrame::Command {
                span: crate::CmvsPs2aSourceSpan {
                    offset: pc,
                    byte_length: 2,
                },
                command_id: id,
            },
        )
        .unwrap()
    }

    #[test]
    fn recovered_opcodes_read_real_count_cursor_and_emit_host_flush() {
        let mut vm = CmvsPs2aVmState::new(0);
        command(&mut vm, 646, &[1000]);
        command(&mut vm, 645, &[]);
        assert_eq!(vm.interpreter_words[&81220], 1);
        command(&mut vm, 641, &[]);
        assert_eq!(vm.interpreter_words[&81220], u32::MAX);
        command(&mut vm, 642, &[]);
        command(&mut vm, 641, &[]);
        assert_eq!(vm.interpreter_words[&81220], 0);
        command(&mut vm, 643, &[]);
        command(&mut vm, 641, &[]);
        assert_eq!(vm.interpreter_words[&81220], u32::MAX);
        assert_eq!(
            command(&mut vm, 644, &[]),
            Some(CmvsPs2aVmAction::FlushNativeTimeline)
        );
        command(&mut vm, 640, &[]);
        command(&mut vm, 645, &[]);
        assert_eq!(vm.interpreter_words[&81220], 0);
    }

    #[test]
    fn saved_delay_rebases_clock_then_restores_sparse_parent_owner() {
        let mut saved = NativeTimeline {
            active: 1,
            count: 2,
            start_millis: 900,
            elapsed_millis: 40,
            ..Default::default()
        };
        saved.records[0] = NativeTimelineEntry {
            kind: 1,
            duration: 25,
            ..Default::default()
        };
        saved.records[1] = NativeTimelineEntry {
            kind: 3,
            target: 231,
            name: "stage.pb3".into(),
            duration: 700,
            ..Default::default()
        };
        let mut vm = CmvsPs2aVmState::new(0);
        vm.session_clock_millis = 500;
        vm.slot_objects.insert((1924 << 8) | 231, 1);
        apply(&mut vm, &saved).unwrap();
        assert_eq!(vm.native_timeline.as_ref().unwrap().start_millis, 460);
        assert!(advance(&mut vm, false).unwrap().is_none());
        vm.session_clock_millis = 525;
        let request = advance(&mut vm, false).unwrap().unwrap();
        assert_eq!(
            (request.target, request.resource.as_str(), request.duration),
            (NativeTimelineTarget::Parent(231), "stage.pb3", 700)
        );
        let captured = capture(&vm).unwrap();
        assert_eq!(
            (captured.active, captured.cursor, captured.elapsed_millis),
            (0, 2, 65)
        );
    }

    #[test]
    fn camera_commands_wait_for_delay_and_yield_between_resource_owners() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.slot_objects.insert(1924 << 8, 1);
        command(&mut vm, 646, &[50]);
        command(&mut vm, 647, &[1, 8.0f32.to_bits(), 0, 0, 400]);
        push_channel(&mut vm, 0, "middle.pb3".into(), 10).unwrap();
        command(&mut vm, 647, &[0, 12.0f32.to_bits(), 0, 0, 400]);
        start(&mut vm).unwrap();
        vm.session_clock_millis = 49;
        assert!(advance(&mut vm, false).unwrap().is_none());
        assert!(!super::super::camera_transition::pending(&vm));
        vm.session_clock_millis = 50;
        assert_eq!(
            advance(&mut vm, false).unwrap().unwrap().resource,
            "middle.pb3"
        );
        assert_eq!(f32::from_bits(vm.screen_words[&23]), 8.0);
        assert_eq!(cursor(&vm), 3);
        assert!(advance(&mut vm, false).unwrap().is_none());
        assert_eq!(f32::from_bits(vm.screen_words[&23]), 12.0);
        super::super::camera_transition::advance(&mut vm, 200).unwrap();
        assert_eq!(f32::from_bits(vm.screen_words[&1]), 6.0);
    }

    #[test]
    fn immediate_camera_and_forced_timeline_write_preserve_active_worker() {
        for skip in [false, true] {
            let mut vm = CmvsPs2aVmState::new(0);
            super::super::camera_transition::start(&mut vm, 108, &[0, 8.0f32.to_bits(), 0, 0, 800])
                .unwrap();
            super::super::camera_transition::advance(&mut vm, 200).unwrap();
            push_camera(
                &mut vm,
                2,
                [20.009f32.to_bits(), 0, 0],
                if skip { 999 } else { 0 },
            )
            .unwrap();
            start(&mut vm).unwrap();
            assert!(advance(&mut vm, skip).unwrap().is_none());
            assert_eq!(f32::from_bits(vm.screen_words[&1]), 20.0);
            assert!(super::super::camera_transition::pending(&vm));
            super::super::camera_transition::advance(&mut vm, 200).unwrap();
            assert_eq!(f32::from_bits(vm.screen_words[&1]), 4.0);
        }
    }

    #[test]
    fn transforms_restore_native_blob_clock_and_distinguish_forced_modes() {
        use crate::system_save::slot_image::SavedBasic;
        for command_id in [649, 650] {
            let mut vm = CmvsPs2aVmState::new(0);
            vm.slot_objects.insert((1924 << 8) | 231, 1);
            vm.texture_parents.insert(
                231,
                CmvsTextureParentState {
                    surface_initialized: true,
                    ..Default::default()
                },
            );
            // 649 reuses the owner allocated by 650, rather than allocating one.
            if command_id == 649 {
                vm.texture_parents
                    .get_mut(&231)
                    .unwrap()
                    .basic_effects
                    .motion = Some(Default::default());
            }
            command(&mut vm, command_id, &[231, 8.0f32.to_bits(), 0, 0, 0, 800]);
            start(&mut vm).unwrap();
            advance(&mut vm, false).unwrap();
            let image = vm.texture_parents.get_mut(&231).unwrap();
            image
                .basic_effects
                .advance_motion(999, &mut image.float_position_words)
                .unwrap();
            assert_eq!(f32::from_bits(image.float_position_words[0]), 2.0);
            let mut saved = SavedBasic::default();
            saved.capture_effects(&image.basic_effects).unwrap();
            assert_eq!(saved.effects[&1941].len(), 222);
            let decoded = SavedBasic::decode(&saved.encode().unwrap()).unwrap();
            image.basic_effects = decoded.restore_effects().unwrap();
            image
                .basic_effects
                .advance_motion(200, &mut image.float_position_words)
                .unwrap();
            assert_eq!(f32::from_bits(image.float_position_words[0]), 4.0);
            reset(&mut vm);
            command(&mut vm, command_id, &[231, 20.0f32.to_bits(), 0, 0, 0, 800]);
            start(&mut vm).unwrap();
            advance(&mut vm, true).unwrap();
            let image = &vm.texture_parents[&231];
            assert_eq!(
                f32::from_bits(image.float_position_words[0]),
                if command_id == 649 { 20.0 } else { 4.0 }
            );
            assert_eq!(image.basic_effects.motion.is_some(), command_id == 650);
        }
    }

    #[test]
    fn native_mode1_null_owner_consumes_record_while_mode2_allocates() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.slot_objects.insert(1924 << 8, 1);
        vm.texture_parents.insert(
            0,
            CmvsTextureParentState {
                surface_initialized: true,
                ..Default::default()
            },
        );
        command(&mut vm, 649, &[0, 8.0f32.to_bits(), 0, 0, 0, 400]);
        start(&mut vm).unwrap();
        advance(&mut vm, false).unwrap();
        assert_eq!(vm.native_timeline.as_ref().unwrap().cursor, 1);
        assert!(vm.texture_parents[&0].basic_effects.motion.is_none());
        assert_eq!(vm.texture_parents[&0].float_position_words, [0; 3]);
        reset(&mut vm);
        command(&mut vm, 650, &[0, 8.0f32.to_bits(), 0, 0, 0, 400]);
        start(&mut vm).unwrap();
        advance(&mut vm, false).unwrap();
        assert!(vm.texture_parents[&0].basic_effects.motion.is_some());
    }

    #[test]
    fn forced_flush_requires_native_active_latch() {
        let mut vm = CmvsPs2aVmState::new(0);
        push_camera(&mut vm, 0, [8.0f32.to_bits(), 0, 0], 400).unwrap();
        assert!(advance(&mut vm, true).unwrap().is_none());
        assert!(!vm.screen_words.contains_key(&1));
        assert_eq!(vm.native_timeline.as_ref().unwrap().cursor, 0);
        start(&mut vm).unwrap();
        stop(&mut vm);
        advance(&mut vm, true).unwrap();
        assert!(!vm.screen_words.contains_key(&1));
        start(&mut vm).unwrap();
        advance(&mut vm, true).unwrap();
        assert_eq!(vm.screen_words[&1], 8.0f32.to_bits());
    }

    #[test]
    fn queued_unknown_camera_mode_reuses_the_live_easing_flags() {
        let mut vm = CmvsPs2aVmState::new(0);
        super::super::camera_transition::start(&mut vm, 110, &[1, 8.0f32.to_bits(), 1000]).unwrap();
        let flags = vm.screen_words[&22];
        command(&mut vm, 647, &[99, 20.0f32.to_bits(), 0, 0, 1000]);
        start(&mut vm).unwrap();
        advance(&mut vm, false).unwrap();
        assert_eq!(vm.screen_words[&22], flags);
        super::super::camera_transition::advance(&mut vm, 500).unwrap();
        assert_eq!(f32::from_bits(vm.screen_words[&1]), 15.0);
        assert!(super::super::camera_transition::pending(&vm));
    }

    #[test]
    fn kind5_opcode_uses_channel_zero_child_owner_and_keeps_record_chronology() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.frame_string_pool_bytes
            .insert(0, b"child.pb3\0".to_vec());
        command(&mut vm, 651, &[7, 0, 600]);
        command(&mut vm, 646, &[40]);
        command(&mut vm, 651, &[7, 0, 900]);
        vm.slot_objects.insert(742 << 8, 1);
        vm.effect_channels.insert(0, Default::default());
        vm.effect_elements.insert((0, u32::MAX), Default::default());
        vm.effect_elements.insert((0, 7), Default::default());
        start(&mut vm).unwrap();
        let first = advance(&mut vm, false).unwrap().unwrap();
        assert_eq!(
            (first.target, first.duration),
            (NativeTimelineTarget::ChannelElement(7), 600)
        );
        assert!(advance(&mut vm, false).unwrap().is_none());
        vm.session_clock_millis = 40;
        let second = advance(&mut vm, false).unwrap().unwrap();
        assert_eq!(second.duration, 900);
        assert_eq!(capture(&vm).unwrap().elapsed_millis, 40);
        reset(&mut vm);
        push_element(&mut vm, 7, "child.pb3".into(), 900).unwrap();
        start(&mut vm).unwrap();
        assert_eq!(advance(&mut vm, true).unwrap().unwrap().duration, 0);
    }

    #[test]
    fn kind5_absent_and_overflow_targets_consume_but_live_negative_fails_atomically() {
        let mut vm = CmvsPs2aVmState::new(0);
        push_element(&mut vm, u32::MAX, "child.pb3".into(), 500).unwrap();
        start(&mut vm).unwrap();
        assert!(advance(&mut vm, false).unwrap().is_none());
        reset(&mut vm);
        vm.slot_objects.insert(742 << 8, 1);
        vm.effect_channels.insert(0, Default::default());
        vm.effect_elements.insert((0, u32::MAX), Default::default());
        push_element(&mut vm, 1024, "child.pb3".into(), 500).unwrap();
        start(&mut vm).unwrap();
        assert!(advance(&mut vm, false).unwrap().is_none());
        reset(&mut vm);
        push_element(&mut vm, u32::MAX, "child.pb3".into(), 500).unwrap();
        start(&mut vm).unwrap();
        let before = vm.clone();
        assert!(advance(&mut vm, false).is_err());
        assert_eq!(vm, before);
        let mut invalid = capture(&vm).unwrap();
        invalid.records[0].kind = 99;
        assert!(apply(&mut vm, &invalid).is_err());
        assert_eq!(vm, before);
    }

    #[test]
    fn ordinary_transform_unknown_curves_survive_queued_save_and_finish_literal_geometry() {
        for command_id in [649, 650] {
            for curve in [2, 0xffff_7f02, 0xa55a_a5fe] {
                let mut vm = CmvsPs2aVmState::new(0);
                let held = [(-0.0f32).to_bits(), 3.0f32.to_bits(), (-2.0f32).to_bits()];
                let target = [8.0f32.to_bits(), 4.0f32.to_bits(), 1.0f32.to_bits()];
                vm.slot_objects.insert(1924 << 8, 1);
                vm.texture_parents.insert(
                    0,
                    CmvsTextureParentState {
                        surface_initialized: true,
                        float_position_words: held,
                        ..Default::default()
                    },
                );
                if command_id == 649 {
                    vm.texture_parents.get_mut(&0).unwrap().basic_effects.motion =
                        Some(Default::default());
                }
                command(
                    &mut vm,
                    command_id,
                    &[0, target[0], target[1], target[2], curve, 400],
                );
                start(&mut vm).unwrap();
                let saved =
                    NativeTimeline::decode(&capture(&vm).unwrap().encode().unwrap()).unwrap();
                apply(&mut vm, &saved).unwrap();
                advance(&mut vm, false).unwrap();
                let image = vm.texture_parents.get_mut(&0).unwrap();
                assert!(
                    !image
                        .basic_effects
                        .advance_motion(200, &mut image.float_position_words)
                        .unwrap()
                );
                assert_eq!(image.float_position_words, held);
                assert!(
                    !image
                        .basic_effects
                        .advance_motion(200, &mut image.float_position_words)
                        .unwrap()
                );
                assert_eq!(image.float_position_words, target);
                assert!(image.basic_effects.motion.is_none());
            }
        }
    }
}
