//! Native plane-camera worker (457620..457BE5), shared by both camera owners.
use super::CmvsPs2aVmState;
use shinki_emu_sdk::CoreError;
use std::collections::BTreeMap;

type Words = BTreeMap<u32, u32>;
const AXES: [u32; 6] = [1, 2, 3, 12, 13, 14];

fn invalid() -> CoreError {
    CoreError::invalid(
        "SHINKI_EMU_CMVS_CAMERA_TRANSITION",
        "native camera transition contains non-finite geometry",
    )
}

pub(crate) fn default_word(index: u32) -> u32 {
    match index {
        0 => 1,
        2 => 3.0_f32.to_bits(),
        6 | 7 => 10.0_f32.to_bits(),
        8 => 6.0_f32.to_bits(),
        9 => 1024.0_f32.to_bits(),
        10 => 614.0_f32.to_bits(),
        11 => 24.0_f32.to_bits(),
        15 => 512.0_f32.to_bits(),
        16 => 307.0_f32.to_bits(),
        17 | 18 => 1.0_f32.to_bits(),
        _ => 0,
    }
}

fn raw(words: &Words, index: u32) -> u32 {
    words
        .get(&index)
        .copied()
        .unwrap_or_else(|| default_word(index))
}

fn number(words: &Words, index: u32) -> f32 {
    f32::from_bits(raw(words, index))
}

// CVTTSS2SI/CVTTSD2SI yield integer-indefinite rather than Rust's saturation.
pub(super) fn cvtt(value: f64) -> i32 {
    if value >= 2147483648.0 || value < -2147483648.0 || !value.is_finite() {
        i32::MIN
    } else {
        value as i32
    }
}

pub(super) fn hundredths(value: f32) -> f32 {
    cvtt(f64::from(value * 100.0)) as f32 / 100.0
}

fn enabled_axis(flags: u32, axis: usize) -> bool {
    axis < 3 || flags & [0x8000_0000, 0x4000_0000, 0x2000_0000][axis - 3] != 0
}

fn validate_words(words: &Words) -> Result<(), CoreError> {
    if raw(words, 21) == 0 {
        return Ok(());
    }
    let flags = raw(words, 22);
    for (axis, current) in AXES.into_iter().enumerate() {
        if enabled_axis(flags, axis)
            && [
                number(words, current),
                number(words, 23 + axis as u32),
                number(words, 30 + axis as u32),
            ]
            .iter()
            .any(|value| !value.is_finite())
        {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(crate) fn validate(vm: &CmvsPs2aVmState) -> Result<(), CoreError> {
    validate_words(&vm.screen_words)?;
    if let Some(words) = vm.screen_camera_words.get(&1) {
        validate_words(words)?;
    }
    Ok(())
}

/// 457BF0 resets the body and worker, but not owner enable37 or save policy38.
pub(crate) fn reset(vm: &mut CmvsPs2aVmState) {
    fn reset_words(words: &mut Words) {
        for index in (0..=18).chain(21..=36) {
            words.insert(index, default_word(index));
        }
    }
    reset_words(&mut vm.screen_words);
    reset_words(vm.screen_camera_words.entry(1).or_default());
}

pub(crate) fn pending(vm: &CmvsPs2aVmState) -> bool {
    raw(&vm.screen_words, 21) != 0
}

/// 457B90: cancellation preserves the current visible plane unless snap=true.
pub(crate) fn commit(vm: &mut CmvsPs2aVmState, snap: bool) {
    finish(&mut vm.screen_words, snap);
}

fn finish(words: &mut Words, snap: bool) {
    if raw(words, 21) == 0 {
        return;
    }
    let flags = raw(words, 22);
    words.insert(21, 0);
    if snap {
        for (axis, current) in AXES.into_iter().enumerate() {
            if enabled_axis(flags, axis) {
                words.insert(current, raw(words, 23 + axis as u32));
            }
        }
    }
}

/// Actual script operands are top-first; no renderer-field shadow is retained.
pub(crate) fn start(
    vm: &mut CmvsPs2aVmState,
    command: u16,
    values: &[u32],
) -> Result<(), CoreError> {
    let expected = match command {
        107 => 4,
        108 => 5,
        109..=111 => 3,
        125 => 7,
        _ => 0,
    };
    if expected == 0 || values.len() != expected {
        return Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_CAMERA_TRANSITION",
            "native camera command operands are invalid",
        ));
    }
    let words = &mut vm.screen_words;
    let origin = AXES.map(|index| number(words, index));
    let mut target = origin;
    let mut flags = match values[0] {
        0 => 0,
        1 => {
            if command == 125 {
                0x0101_0101
            } else {
                0x0001_0101
            }
        }
        2 => {
            if command == 125 {
                0x0202_0202
            } else {
                0x0002_0202
            }
        }
        _ => raw(words, 22), // unsupported modes leave the prior native flags unchanged.
    };
    //4576F0 quantizes raw script floats through CVTTSS2SI, including NaN,
    //infinity and finite overflow. Integer-indefinite becomes a finite target.
    let mut put = |axis: usize, word: u32| {
        target[axis] = hundredths(f32::from_bits(word));
    };
    match command {
        107 => {
            put(0, values[1]);
            put(2, values[2]);
        }
        108 => {
            for axis in 0..3 {
                put(axis, values[axis + 1]);
            }
        }
        109 => put(1, values[1]),
        110 => put(0, values[1]),
        111 => put(2, values[1]),
        125 => {
            for axis in 0..3 {
                put(axis, values[axis + 2]);
            }
            put(5, values[5]);
            flags |= 0x2000_0000;
            if values[1] != 0 {
                flags |= 0x0400_0000;
            }
        }
        _ => unreachable!(),
    }
    for (axis, value) in origin.iter().enumerate() {
        if enabled_axis(flags, axis) && !value.is_finite() {
            return Err(invalid());
        }
    }
    // Origins and targets for unchanged axes remain the live object's values.
    for (axis, current) in AXES.into_iter().enumerate() {
        words.entry(current).or_insert(origin[axis].to_bits());
        words.insert(23 + axis as u32, target[axis].to_bits());
        words.insert(30 + axis as u32, origin[axis].to_bits());
    }
    words.insert(22, flags);
    words.insert(29, values[values.len() - 1]);
    words.insert(36, 0);
    words.insert(21, 1);
    Ok(())
}

/// 456DF0 writes XYZ without cancelling an already-running 4576F0 worker.
pub(crate) fn set_position(vm: &mut CmvsPs2aVmState, values: [u32; 3]) -> Result<(), CoreError> {
    let values = values.map(f32::from_bits);
    for (axis, value) in values.into_iter().enumerate() {
        vm.screen_words
            .insert(axis as u32 + 1, hundredths(value).to_bits());
    }
    Ok(())
}

fn interpolate(origin: f32, target: f32, elapsed: u32, duration: u32, mode: u8) -> Option<f32> {
    if origin == target {
        return None;
    }
    let e = elapsed as f32;
    let d = duration as f32;
    let delta = target - origin;
    match mode {
        0 => Some(hundredths(delta * e / d) + origin),
        1 => {
            // Native DIVSD promotes each rounded f32 operand before division.
            let remaining = 1.0 - f64::from(e) / f64::from(d);
            let distance = cvtt(remaining * remaining * f64::from(delta) * 100.0) as f32 / 100.0;
            Some(target - distance)
        }
        2 => {
            let degrees = hundredths(e * 90.0 / d);
            let radians = (degrees / 180.0) * std::f32::consts::PI;
            // 452A60 multiplies the double cosine by the promoted f32 radius.
            let cosine = hundredths((f64::from(radians).cos() * f64::from(delta)) as f32);
            Some((delta - cosine) + origin)
        }
        _ => None,
    }
}

fn advance_words(words: &mut Words, delta_ms: u32) {
    if raw(words, 21) == 0 {
        return;
    }
    let elapsed = raw(words, 36).wrapping_add(delta_ms);
    words.insert(36, elapsed);
    let duration = raw(words, 29);
    if elapsed >= duration {
        finish(words, true);
        return;
    }
    let flags = raw(words, 22);
    for (axis, current) in [(0usize, 1u32), (1, 2), (2, 3), (5, 14)] {
        if !enabled_axis(flags, axis) {
            continue;
        }
        let mode = ((flags
            >> match axis {
                0 => 16,
                1 => 8,
                _ => 0,
            })
            & 255) as u8;
        if let Some(value) = interpolate(
            number(words, 30 + axis as u32),
            number(words, 23 + axis as u32),
            elapsed,
            duration,
            mode,
        ) {
            words.insert(current, value.to_bits());
        }
    }
}

/// 4262A0 calls both owners once per frame, using 419AA0 without a 200ms cap.
pub(crate) fn advance(vm: &mut CmvsPs2aVmState, delta_ms: u32) -> Result<(), CoreError> {
    validate(vm)?;
    advance_words(&mut vm.screen_words, delta_ms);
    if let Some(words) = vm.screen_camera_words.get_mut(&1) {
        advance_words(words, delta_ms);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_camera_easing_quantization_and_uncapped_delta_reach_the_exact_endpoint() {
        for (mode, midpoint) in [(0, 4.0), (1, 6.0), (2, 2.35)] {
            let mut vm = CmvsPs2aVmState::new(0);
            start(&mut vm, 110, &[mode, 8.0f32.to_bits(), 1000]).unwrap();
            advance(&mut vm, 500).unwrap();
            assert_eq!(number(&vm.screen_words, 1), midpoint);
            assert!(pending(&vm));
            advance(&mut vm, 500).unwrap();
            assert_eq!(number(&vm.screen_words, 1), 8.0);
            assert!(!pending(&vm));
        }
    }
    #[test]
    fn native_zero_duration_and_cancel_keep_distinct_visible_results() {
        let mut vm = CmvsPs2aVmState::new(0);
        start(&mut vm, 110, &[0, 8.0f32.to_bits(), 1000]).unwrap();
        advance(&mut vm, 250).unwrap();
        commit(&mut vm, false);
        advance(&mut vm, 1000).unwrap();
        assert_eq!(number(&vm.screen_words, 1), 2.0);
        start(&mut vm, 110, &[0, 9.0f32.to_bits(), 0]).unwrap();
        assert!(pending(&vm));
        advance(&mut vm, 0).unwrap();
        assert_eq!(number(&vm.screen_words, 1), 9.0);
        assert!(!pending(&vm));
    }
    #[test]
    fn immediate_position_quantizes_invalid_raw_floats_without_cancelling_the_worker() {
        let mut vm = CmvsPs2aVmState::new(0);
        start(&mut vm, 110, &[0, 8.0f32.to_bits(), 1000]).unwrap();
        set_position(
            &mut vm,
            [
                f32::NAN.to_bits(),
                f32::INFINITY.to_bits(),
                f32::MAX.to_bits(),
            ],
        )
        .unwrap();
        for axis in 1..=3 {
            assert_eq!(number(&vm.screen_words, axis), -21_474_836.0);
        }
        assert!(pending(&vm));
        advance(&mut vm, 1000).unwrap();
        assert_eq!(number(&vm.screen_words, 1), 8.0);
    }
}
