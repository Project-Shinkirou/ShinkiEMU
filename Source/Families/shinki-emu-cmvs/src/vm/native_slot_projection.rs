//! Maps native slot camera owners onto the maps used by scene::projection.
//! No native payload is retained as an opaque VM record.
use super::CmvsPs2aVmState;
use crate::system_save::slot_projection::{
    SavedCamera, SavedCameraTransition, SavedPlane, SavedProjection,
};
use shinki_emu_sdk::CoreError;
use std::collections::BTreeMap;

const POLICY_512: u32 = (1920 << 16) | 76;
const POLICY_515: u32 = (1920 << 16) | 72;

pub(crate) fn capture(vm: &CmvsPs2aVmState) -> Result<(SavedProjection, [u32; 2]), CoreError> {
    super::camera_transition::validate(vm)?;
    let mut saved = SavedProjection::default();
    saved.cameras[0] = capture_camera(Some(&vm.screen_words), 1)?;
    saved.cameras[1] = capture_camera(vm.screen_camera_words.get(&1), 0)?;
    saved.views = super::independent_view::capture_pair(vm)?;
    let scalars = [
        vm.component_words.get(&POLICY_512).copied().unwrap_or(0),
        vm.component_words.get(&POLICY_515).copied().unwrap_or(0),
    ];
    validate_runtime(&saved)?;
    Ok((saved, scalars))
}

/// All unsupported state is checked before the first write. 512/515 are the
/// graphics component's DWORD19/18, shared with script commands 715/592.
pub(crate) fn apply(
    vm: &mut CmvsPs2aVmState,
    saved: &SavedProjection,
    scalars: [u32; 2],
) -> Result<(), CoreError> {
    validate_runtime(saved)?;
    super::independent_view::restore_pair(vm, &saved.views)?;
    apply_camera(&mut vm.screen_words, &saved.cameras[0]);
    apply_camera(
        vm.screen_camera_words.entry(1).or_default(),
        &saved.cameras[1],
    );
    vm.component_words.insert(POLICY_512, scalars[0]);
    vm.component_words.insert(POLICY_515, scalars[1]);
    Ok(())
}

fn capture_camera(
    words: Option<&BTreeMap<u32, u32>>,
    enabled: u32,
) -> Result<SavedCamera, CoreError> {
    let raw = |word, default| {
        words
            .and_then(|words| words.get(&word))
            .copied()
            .unwrap_or(default)
    };
    let active = raw(21, 0) != 0;
    let flags = raw(22, 0);
    let f = |word, default: f32| f32::from_bits(raw(word, default.to_bits()));
    let kind = raw(38, 0);
    let mut camera = SavedCamera {
        enabled: raw(37, enabled),
        reserved: 0,
        kind,
        plane: matches!(kind, 0 | 1).then(|| SavedPlane {
            mode: raw(0, 1),
            position: [f(1, 0.0), f(2, 3.0), f(3, 0.0)],
            auxiliary: [f(4, 0.0), f(5, 0.0)],
            depth: f(6, 10.0),
            extent: [f(7, 10.0), f(8, 6.0)],
            output: [f(9, 1024.0), f(10, 614.0)],
            slope: f(11, 24.0),
            pitch: f(12, 0.0),
            yaw: f(13, 0.0),
            roll: f(14, 0.0),
        }),
        transition: (kind == 1).then(|| {
            if !active {
                return SavedCameraTransition::default();
            }
            SavedCameraTransition {
                active: 1,
                flags,
                target_position: [f(23, 0.0), f(24, 0.0), f(25, 0.0)],
                target_angles: [f(26, 0.0), f(27, 0.0), f(28, 0.0)],
                duration: raw(29, 0),
                origin_position: [f(30, 0.0), f(31, 0.0), f(32, 0.0)],
                origin_angles: [f(33, 0.0), f(34, 0.0), f(35, 0.0)],
                // Native456E80 restarts kind1; the live worker is untouched.
                elapsed: 0,
            }
        }),
        center: [f(15, 512.0), f(16, 307.0)],
        scale: [f(17, 1.0), f(18, 1.0)],
    };
    if active && let Some(plane) = &mut camera.plane {
        // Kind0 serializes the endpoint; kind1 serializes the origin.
        let position = if kind == 0 { 23 } else { 30 };
        plane.position = [f(position, 0.0), f(position + 1, 0.0), f(position + 2, 0.0)];
        let angles = if kind == 0 { 26 } else { 33 };
        for (axis, value) in [&mut plane.pitch, &mut plane.yaw, &mut plane.roll]
            .into_iter()
            .enumerate()
        {
            if flags & [0x8000_0000, 0x4000_0000, 0x2000_0000][axis] != 0 {
                *value = f(angles + axis as u32, 0.0);
            }
        }
    }
    Ok(camera)
}

fn validate_runtime(saved: &SavedProjection) -> Result<(), CoreError> {
    saved.validate()?;
    for camera in &saved.cameras {
        if let Some(worker) = camera
            .transition
            .as_ref()
            .filter(|worker| worker.active != 0)
            && (worker
                .target_position
                .iter()
                .chain(worker.origin_position.iter())
                .any(|value| !value.is_finite())
                || (0..3).any(|axis| {
                    worker.flags & [0x8000_0000, 0x4000_0000, 0x2000_0000][axis] != 0
                        && (!worker.target_angles[axis].is_finite()
                            || !worker.origin_angles[axis].is_finite())
                }))
        {
            return Err(CoreError::invalid(
                "SHINKI_EMU_CMVS_NATIVE_PROJECTION",
                "native camera worker contains non-finite geometry",
            ));
        }
        if camera
            .center
            .iter()
            .chain(camera.scale.iter())
            .any(|value| !value.is_finite())
        {
            return Err(CoreError::invalid(
                "SHINKI_EMU_CMVS_NATIVE_PROJECTION",
                "native camera contains non-finite geometry",
            ));
        }
        let Some(plane) = &camera.plane else {
            continue;
        };
        if plane
            .position
            .iter()
            .chain(plane.extent.iter())
            .chain(plane.auxiliary.iter())
            .chain(plane.output.iter())
            .chain(camera.center.iter())
            .chain(camera.scale.iter())
            .chain([
                &plane.depth,
                &plane.slope,
                &plane.pitch,
                &plane.yaw,
                &plane.roll,
            ])
            .any(|value| !value.is_finite())
        {
            return Err(CoreError::invalid(
                "SHINKI_EMU_CMVS_NATIVE_PROJECTION",
                "native camera contains non-finite geometry",
            ));
        }
    }
    Ok(())
}

fn apply_camera(words: &mut BTreeMap<u32, u32>, camera: &SavedCamera) {
    if let Some(plane) = &camera.plane {
        words.insert(0, plane.mode);
        for (index, value) in [
            plane.position[0],
            plane.position[1],
            plane.position[2],
            plane.auxiliary[0],
            plane.auxiliary[1],
            plane.depth,
            plane.extent[0],
            plane.extent[1],
            plane.output[0],
            plane.output[1],
            plane.slope,
            plane.pitch,
            plane.yaw,
            plane.roll,
        ]
        .into_iter()
        .enumerate()
        {
            words.insert(index as u32 + 1, value.to_bits());
        }
        // Native457CB0 resets kind0's worker and restores kind1's entire worker.
        let inactive = SavedCameraTransition::default();
        let worker = camera.transition.as_ref().unwrap_or(&inactive);
        for (word, value) in (21..=36).zip([
            worker.active,
            worker.flags,
            worker.target_position[0].to_bits(),
            worker.target_position[1].to_bits(),
            worker.target_position[2].to_bits(),
            worker.target_angles[0].to_bits(),
            worker.target_angles[1].to_bits(),
            worker.target_angles[2].to_bits(),
            worker.duration,
            worker.origin_position[0].to_bits(),
            worker.origin_position[1].to_bits(),
            worker.origin_position[2].to_bits(),
            worker.origin_angles[0].to_bits(),
            worker.origin_angles[1].to_bits(),
            worker.origin_angles[2].to_bits(),
            worker.elapsed,
        ]) {
            words.insert(word, value);
        }
    }
    // The 30-byte form intentionally leaves DWORD0..14 and the worker alone.
    for (word, value) in [15, 16, 17, 18].into_iter().zip([
        camera.center[0],
        camera.center[1],
        camera.scale[0],
        camera.scale[1],
    ]) {
        words.insert(word, value.to_bits());
    }
    words.insert(37, camera.enabled);
    words.insert(38, camera.kind);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CmvsPs2aControlInstruction, CmvsPs2aInstructionFrame, CmvsPs2aSourceSpan,
        execute_cmvs390_frame,
    };

    const SPAN: CmvsPs2aSourceSpan = CmvsPs2aSourceSpan {
        offset: 0,
        byte_length: 2,
    };

    fn command(vm: &mut CmvsPs2aVmState, id: u16, values: &[u32]) {
        for value in values {
            execute_cmvs390_frame(
                vm,
                &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                    span: SPAN,
                    stack_bytes: 4,
                    value: *value,
                }),
            )
            .unwrap();
        }
        execute_cmvs390_frame(
            vm,
            &CmvsPs2aInstructionFrame::Command {
                span: SPAN,
                command_id: id,
            },
        )
        .unwrap();
    }

    #[test]
    fn restored_scalars_share_the_native_graphics_command_owners() {
        let mut source = CmvsPs2aVmState::new(0);
        source.settings_words.insert(72, 23);
        command(&mut source, 715, &[7]);
        command(&mut source, 592, &[42]);
        assert_eq!(source.settings_words[&72], 23);
        let (saved, scalars) = capture(&source).unwrap();
        assert_eq!(scalars, [7, 1]);
        let mut candidate = CmvsPs2aVmState::new(0);
        candidate.native_view.viewport_origin = [13, 17];
        apply(&mut candidate, &saved, scalars).unwrap();
        assert_eq!(candidate.component_words[&POLICY_512], 7);
        assert_eq!(candidate.component_words[&POLICY_515], 1);
        assert_eq!(candidate.native_view.viewport_origin, [13, 17]);
        assert!(!candidate.interpreter_words.contains_key(&512));
        assert!(!candidate.interpreter_words.contains_key(&515));
        command(&mut candidate, 715, &[3]);
        command(&mut candidate, 592, &[0]);
        assert_eq!(capture(&candidate).unwrap().1, [3, 0]);
    }

    #[test]
    fn malformed_view_or_camera_worker_is_rejected_before_mutation() {
        let mut candidate = CmvsPs2aVmState::new(0);
        let before = candidate.clone();
        let mut saved = SavedProjection::default();
        saved.views[0].angles[2] = f32::NAN;
        assert_eq!(
            apply(&mut candidate, &saved, [12, 1]).unwrap_err().code(),
            "SHINKI_EMU_CMVS_INDEPENDENT_VIEW"
        );
        assert_eq!(candidate, before);
        saved = SavedProjection::default();
        saved.cameras[1].kind = 1;
        saved.cameras[1].transition =
            Some(crate::system_save::slot_projection::SavedCameraTransition {
                active: 1,
                flags: 0x8000_0000,
                target_angles: [f32::NAN, 0.0, 0.0],
                ..Default::default()
            });
        assert_eq!(
            apply(&mut candidate, &saved, [12, 1]).unwrap_err().code(),
            "SHINKI_EMU_CMVS_NATIVE_PROJECTION"
        );
        assert_eq!(candidate, before);
    }
}
