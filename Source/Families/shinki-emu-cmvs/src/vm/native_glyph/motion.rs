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
    fn integer_product_wraps_before_signed_division_instead_of_float_interpolation() {
        let mut graph = graph();
        graph.bounds = [i32::MIN, i32::MAX, i32::MIN, i32::MAX];
        assert!(start(&mut graph, 0, [100_000, -100_000], 0, 100_000, 0));
        advance(&mut graph, 50_000).unwrap();
        assert_eq!(graph.position.map(f32::from_bits), [7050.0, -7050.0]);
        assert!(start(&mut graph, 0, [100, -100], 0, 0x8000_0000, 0));
        advance(&mut graph, 1).unwrap();
        assert_eq!(graph.position.map(f32::from_bits), [7050.0, -7050.0]);
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
    fn sine_amplitude_and_angle_products_wrap_as_i32_before_binary64_math() {
        let mut graph = graph();
        graph.bounds = [i32::MIN, i32::MAX, i32::MIN, i32::MAX];
        assert!(start(&mut graph, 1, [30_000_000, 0], 0, 1_000_000, 0));
        advance(&mut graph, 500_000).unwrap();
        let angle = 205; // wrapping(9000 * 500000) / 1000000
        let expected = cvtt(
            ((angle as f64 / 18000.0) * std::f64::consts::PI).sin()
                * f64::from(30_000_000i32.wrapping_mul(100)),
        ) / 100;
        assert_eq!(graph.position[0], (expected as f32).to_bits());
        assert!(f32::from_bits(graph.position[0]) < 0.0);
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
    fn private_resume_preserves_motion_descriptor_pixels_occupancy_and_resident_child() {
        use crate::vm::native_glyph::{CmvsNativeGlyphPainted, CmvsNativeGlyphResident};
        let mut graph = graph();
        crate::vm::native_glyph::layout::append(&mut graph, b"A", false, |_, _| Ok(21)).unwrap();
        graph.atlas.occupancy[7] = 1;
        graph.records[0].painted = Some(CmvsNativeGlyphPainted {
            bgra: vec![0x5a; 48 * 48 * 4],
            offset: [-2, 4],
            physical_size: 42,
            cell_size: 48,
        });
        graph.records[0].resident = Some(CmvsNativeGlyphResident {
            slot: 7,
            cell_size: 48,
            child: Default::default(),
        });
        assert!(start(&mut graph, 1, [100, -100], 0xfeedbeef, 1000, 100));
        advance(&mut graph, 350).unwrap();
        let bytes = postcard::to_allocvec(&graph).unwrap();
        let mut restored: CmvsNativeGlyphGraph = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(restored, graph);
        advance(&mut graph, 600).unwrap();
        advance(&mut restored, 600).unwrap();
        assert_eq!(restored, graph);
        assert_eq!(restored.atlas.occupancy[7], 1);
        assert_eq!(restored.records[0].resident.as_ref().unwrap().slot, 7);
        assert_eq!(
            restored.records[0].painted.as_ref().unwrap().bgra.len(),
            48 * 48 * 4
        );
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

    #[test]
    fn cvtt_preserves_native_integer_indefinite() {
        assert_eq!(cvtt(f64::NAN), i32::MIN);
        assert_eq!(cvtt(2147483648.0), i32::MIN);
        assert_eq!(cvtt(-2147483648.0), i32::MIN);
        assert_eq!(cvtt(-2.9), -2);
    }

    #[test]
    fn signed_division_reports_native_overflow_instead_of_panicking_or_saturating() {
        assert_eq!(divide(-7, 3).unwrap(), -2);
        assert_eq!(divide(7, (-3i32) as u32).unwrap(), -2);
        assert!(divide(1, 0).is_err());
        assert!(divide(i32::MIN, u32::MAX).is_err());
    }

    #[test]
    fn setup_wraps_targets_and_no_motion_preserves_position_and_root_bits() {
        let mut graph = graph();
        graph.bounds = [i32::MIN, i32::MAX, i32::MIN, i32::MAX];
        graph.position = [2147483520.0, -0.0].map(f32::to_bits);
        graph.root.float_position_words = [0x7fc01234, 0xffc05678, 0x80000000];
        let before = graph.clone();
        advance(&mut graph, 100).unwrap();
        assert_eq!(graph, before);
        assert!(start(&mut graph, 0, [1000, 0], 0, 0, 100));
        assert_eq!(
            graph.motion.as_ref().unwrap().end[0],
            2147483520i32.wrapping_add(1000)
        );
        advance(&mut graph, 100).unwrap();
        assert_eq!(
            graph.position[0],
            (2147483520i32.wrapping_add(1000) as f32).to_bits()
        );
        assert_eq!(graph.root.float_position_words[2], 0x80000000);
    }
}
