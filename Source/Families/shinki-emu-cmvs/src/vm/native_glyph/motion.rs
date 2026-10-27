use super::{CmvsNativeGlyphGraph, CmvsNativeGlyphMotion};
use crate::vm::CmvsPs2aVmState;
use shinki_emu_sdk::CoreError;

/// 41A000's frozen value or the wrapping, offset-adjusted session clock.
pub(crate) fn clock(vm: &CmvsPs2aVmState) -> u32 {
    let word = |address| vm.process_global_words.get(&address).copied().unwrap_or(0);
    if word(0x004f_052c) != 0 {
        word(0x004f_0530)
    } else {
        vm.session_clock_millis.wrapping_sub(word(0x004f_0554))
    }
}

// CVTTSS2SI and CVTTSD2SI return integer-indefinite, not a saturating cast.
pub(super) fn cvtt(value: f64) -> i32 {
    if !value.is_finite() || !(-2147483648.0..2147483648.0).contains(&value) {
        i32::MIN
    } else {
        value as i32
    }
}

/// 924/927 refuse the entire setup if either requested axis points out of bounds.
pub(crate) fn start(
    graph: &mut CmvsNativeGlyphGraph,
    curve: u32,
    delta: [i32; 2],
    extra: u32,
    duration: u32,
    now: u32,
) -> bool {
    let start = graph
        .position
        .map(|word| cvtt(f64::from(f32::from_bits(word))));
    for axis in 0..2 {
        if (delta[axis] > 0 && start[axis] >= graph.bounds[axis * 2 + 1])
            || (delta[axis] < 0 && start[axis] <= graph.bounds[axis * 2])
        {
            return false;
        }
    }
    graph.motion = Some(CmvsNativeGlyphMotion {
        start,
        end: [
            start[0].wrapping_add(delta[0]),
            start[1].wrapping_add(delta[1]),
        ],
        curve,
        extra,
        duration,
        timestamp: now,
    });
    true
}

fn divide(numerator: i32, denominator: u32) -> Result<i32, CoreError> {
    numerator.checked_div(denominator as i32).ok_or_else(|| {
        CoreError::invalid(
            "SHINKI_EMU_CMVS_GLYPH_MOTION_DIVIDE",
            "native standalone glyph motion reaches an integer divide fault",
        )
    })
}

/// 437DF0 advances before viewport residency. This is not CBasic float motion.
pub(crate) fn advance(graph: &mut CmvsNativeGlyphGraph, now: u32) -> Result<(), CoreError> {
    let Some(motion) = graph.motion.as_ref() else {
        return Ok(());
    };
    // Native's switch reaches uninitialized stack coordinates for every other
    // curve, before even inspecting elapsed time or selecting the endpoint.
    if !matches!(motion.curve, 0 | 1) {
        return Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_GLYPH_MOTION_UNINITIALIZED",
            "native standalone glyph motion curve reads uninitialized coordinates",
        ));
    }
    let elapsed = now.wrapping_sub(motion.timestamp);
    let mut finished = elapsed >= motion.duration;
    let mut position = if finished {
        motion.end
    } else {
        let mut position = motion.start;
        for (axis, coordinate) in position.iter_mut().enumerate() {
            let delta = motion.end[axis].wrapping_sub(motion.start[axis]);
            if delta == 0 {
                continue;
            }
            let offset = if motion.curve == 0 {
                divide((elapsed as i32).wrapping_mul(delta), motion.duration)?
            } else {
                let angle = divide((elapsed as i32).wrapping_mul(9000), motion.duration)?;
                let radians = (f64::from(angle) / 18000.0) * std::f64::consts::PI;
                cvtt(radians.sin() * f64::from(delta.wrapping_mul(100))) / 100
            };
            *coordinate = motion.start[axis].wrapping_add(offset);
        }
        position
    };
    // Separate comparisons reproduce native even if a script reverses bounds.
    for (axis, coordinate) in position.iter_mut().enumerate() {
        if *coordinate < graph.bounds[axis * 2] {
            *coordinate = graph.bounds[axis * 2];
            finished = true;
        }
        if *coordinate > graph.bounds[axis * 2 + 1] {
            *coordinate = graph.bounds[axis * 2 + 1];
            finished = true;
        }
    }
    graph.position = position.map(|value| (value as f32).to_bits());
    let m = graph.physical_size[0] as f32;
    let v = graph.virtual_size[0] as f32;
    for (target, value) in graph.root.float_position_words[..2]
        .iter_mut()
        .zip(position)
    {
        *target = ((m * value as f32) / v).to_bits();
    }
    if finished {
        graph.motion = None;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::native_glyph::layout::tests::graph;

    #[test]
    fn setup_truncates_positions_captures_raw_extra_and_preserves_old_motion_on_refusal() {
        let mut graph = graph();
        graph.position = [2.9, -3.9].map(f32::to_bits);
        assert!(start(&mut graph, 0, [5, -7], 0xdeadbeef, 100, 42));
        let motion = graph.motion.clone().unwrap();
        assert_eq!(
            (motion.start, motion.end, motion.extra, motion.timestamp),
            ([2, -3], [7, -10], 0xdeadbeef, 42)
        );
        graph.position[0] = 1000.75f32.to_bits();
        assert!(!start(&mut graph, 1, [1, 0], 0, 99, 99));
        assert_eq!(graph.motion, Some(motion));
    }

    #[test]
    fn linear_signed_truncation_wrapping_clock_and_width_scaled_both_axes() {
        let mut graph = graph();
        graph.root.float_position_words[2] = 9.0f32.to_bits();
        assert!(start(&mut graph, 0, [-11, 7], 0, 100, u32::MAX - 49));
        advance(&mut graph, 0).unwrap();
        assert_eq!(graph.position.map(f32::from_bits), [-5.0, 3.0]);
        assert_eq!(
            graph.root.float_position_words.map(f32::from_bits),
            [-10.0, 6.0, 9.0]
        );
        assert!(graph.motion.is_some());
        advance(&mut graph, 50).unwrap();
        assert_eq!(graph.position.map(f32::from_bits), [-11.0, 7.0]);
        assert!(graph.motion.is_none());
    }

    #[test]
    fn sine_curve_uses_integer_hundredth_degree_angle_and_signed_amplitude() {
        let mut graph = graph();
        assert!(start(&mut graph, 1, [100, -100], 77, 1000, 0));
        advance(&mut graph, 500).unwrap();
        assert_eq!(graph.position.map(f32::from_bits), [70.0, -70.0]);
        advance(&mut graph, 999).unwrap();
        assert_eq!(graph.position.map(f32::from_bits), [99.0, -99.0]);
        advance(&mut graph, 1000).unwrap();
        assert_eq!(graph.position.map(f32::from_bits), [100.0, -100.0]);
        assert!(graph.motion.is_none());
    }

    #[test]
    fn strict_bound_clamps_cancel_motion_and_zero_duration_selects_endpoint() {
        let mut graph = graph();
        graph.bounds = [-5, 5, -10, 10];
        assert!(start(&mut graph, 0, [20, 4], 0, 100, 0));
        advance(&mut graph, 25).unwrap();
        assert_eq!(graph.position.map(f32::from_bits), [5.0, 1.0]);
        assert!(graph.motion.is_some());
        advance(&mut graph, 30).unwrap();
        assert_eq!(graph.position.map(f32::from_bits), [5.0, 1.0]);
        assert!(graph.motion.is_none());
        assert!(start(&mut graph, 0, [-20, 4], 0, 0, 100));
        advance(&mut graph, 100).unwrap();
        assert_eq!(graph.position.map(f32::from_bits), [-5.0, 5.0]);
        assert!(graph.motion.is_none());
    }

    #[test]
    fn unknown_curve_is_retained_then_fails_at_native_uninitialized_use_even_after_duration() {
        let mut graph = graph();
        assert!(start(&mut graph, 0x100, [2, 3], 0xaabbccdd, 0, 5));
        let before = graph.clone();
        assert!(advance(&mut graph, 500).is_err());
        assert_eq!(graph, before);
    }

    #[test]
    fn frozen_native_clock_and_offset_are_not_frame_ticks_or_elapsed_deltas() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.session_clock_millis = 3;
        vm.process_global_words.insert(0x004f_0554, 5);
        assert_eq!(clock(&vm), u32::MAX - 1);
        vm.process_global_words.insert(0x004f_052c, 2);
        vm.process_global_words.insert(0x004f_0530, 12345);
        assert_eq!(clock(&vm), 12345);
        vm.session_clock_millis = 9000;
        assert_eq!(clock(&vm), 12345);
        let mut resumed: CmvsPs2aVmState =
            postcard::from_bytes(&postcard::to_allocvec(&vm).unwrap()).unwrap();
        assert_eq!(clock(&resumed), 12345);
        resumed.process_global_words.insert(0x004f_052c, 0);
        assert_eq!(clock(&resumed), 8995);
    }
}
