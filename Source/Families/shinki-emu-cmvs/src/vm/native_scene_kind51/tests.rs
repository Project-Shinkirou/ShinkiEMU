use super::*;

fn start(selector: u32) -> CmvsNativeSceneKind51 {
    let mut owner = CmvsNativeSceneKind51::new(0x8123_4567);
    owner.request(selector);
    assert_eq!(
        owner.advance(u32::MAX, 256).unwrap(),
        NativeSceneFrame::default()
    );
    owner
}

fn advance_by(owner: &mut CmvsNativeSceneKind51, mut milliseconds: u32) -> NativeSceneFrame {
    let mut frame = NativeSceneFrame::default();
    while milliseconds != 0 {
        let delta = milliseconds.min(200);
        frame = owner.advance(delta, 193).unwrap();
        milliseconds -= delta;
    }
    frame
}

fn position(frame: NativeSceneFrame) -> [f32; 3] {
    frame.position_words.unwrap().map(f32::from_bits)
}

#[test]
fn idle_keeps_native_absolute_initial_word_until_request_transition() {
    let mut owner = CmvsNativeSceneKind51::new(u32::MAX - 7);
    assert_eq!(owner.status(), u32::MAX);
    assert_eq!(
        owner.advance(u32::MAX, 0).unwrap(),
        NativeSceneFrame::default()
    );
    assert_eq!(owner.timer_ms, u32::MAX - 7);
    owner.request(1);
    assert_eq!(owner.status(), u32::MAX);
    let frame = owner.advance(200, 0).unwrap();
    assert!(frame.position_words.is_none());
    assert_eq!(owner.status(), 1);
    assert_eq!(owner.timer_ms, 0);
    assert_eq!(position(owner.advance(0, 0).unwrap()), [0.0, 1.5, 0.0]);
}

#[test]
fn selectors_overwrite_one_pending_slot_and_invalid_selectors_do_not_clear_it() {
    let mut owner = start(1);
    owner.request(2);
    owner.request(3);
    for selector in [0, 4, 90, u32::MAX] {
        owner.request(selector);
    }
    advance_by(&mut owner, 1499);
    assert_eq!(owner.status(), 1);
    let frame = owner.advance(1, 256).unwrap();
    assert_eq!(position(frame), [0.0, 1.5, 0.2]);
    assert_eq!(owner.status(), 3);
    assert_eq!(owner.pending_state, None);
    assert_eq!(owner.timer_ms, 0);
    assert!(!owner.event_fired);
}

#[test]
fn slow_step_has_quantized_half_sine_then_two_distinct_landing_phases() {
    let mut owner = start(1);
    assert_eq!(position(advance_by(&mut owner, 500)), [0.0, 1.6, 0.1]);
    let landing = advance_by(&mut owner, 500);
    assert_eq!(position(landing), [0.0, 1.5, 0.2]);
    assert_eq!(
        landing.audio[0],
        Some(NativeSceneSound::PlayStatic {
            slot: 16,
            volume: 193
        })
    );
    assert_eq!(position(advance_by(&mut owner, 250)), [0.0, 1.48, 0.2]);
    let circle = advance_by(&mut owner, 125);
    assert_eq!(position(circle), [0.0, 1.494, 0.2]);
    assert_eq!(circle.audio, [None; 4]);
    assert_eq!(owner.status(), 1);
    assert_eq!(position(advance_by(&mut owner, 125)), [0.0, 1.5, 0.2]);
    assert_eq!(owner.status(), 90);
}

#[test]
fn sound_gate_uses_threshold_equality_and_reopens_after_each_repeated_fast_step() {
    let mut owner = start(2);
    advance_by(&mut owner, 799);
    assert!(!owner.event_fired);
    let sound = owner.advance(1, 0x00ab_cdef).unwrap();
    assert_eq!(
        sound.audio[0],
        Some(NativeSceneSound::PlayStatic {
            slot: 17,
            volume: 0x00ab_cdef
        })
    );
    assert_eq!(owner.advance(0, 77).unwrap().audio, [None; 4]);
    assert_eq!(owner.advance(99, 77).unwrap().audio, [None; 4]);
    let complete = owner.advance(1, 77).unwrap();
    assert_eq!(position(complete), [0.0, 1.5, 0.4]);
    assert_eq!(owner.status(), 2);
    assert!(!owner.event_fired);
    assert_eq!(owner.timer_ms, 0);
    let sound = advance_by(&mut owner, 800);
    assert_eq!(
        sound.audio[0],
        Some(NativeSceneSound::PlayStatic {
            slot: 17,
            volume: 193
        })
    );
}

#[test]
fn fast_step_uses_eight_tenths_amplitude_and_fifty_millisecond_landing_halves() {
    let mut owner = start(2);
    assert_eq!(position(advance_by(&mut owner, 400)), [0.0, 1.58, 0.2]);
    assert_eq!(position(advance_by(&mut owner, 400)), [0.0, 1.5, 0.4]);
    assert_eq!(position(advance_by(&mut owner, 50)), [0.0, 1.48, 0.4]);
    assert_eq!(position(advance_by(&mut owner, 25)), [0.0, 1.494, 0.4]);
    owner.request(1);
    advance_by(&mut owner, 25);
    assert_eq!(owner.status(), 1);
    assert_eq!(owner.pending_state, None);
    assert_eq!(owner.base_position_words[2], 0.4_f32.to_bits());
}

#[test]
fn movement_caps_each_frame_at_two_hundred_and_unsigned_huge_spans_are_capped() {
    for selector in [1, 2, 3] {
        let mut huge = start(selector);
        let mut exact = huge.clone();
        assert_eq!(
            huge.advance(u32::MAX, 17).unwrap(),
            exact.advance(200, 17).unwrap()
        );
        assert_eq!(huge, exact);
        assert_eq!(huge.timer_ms, 200);
    }
}

#[test]
fn cooldown_is_uncapped_strictly_greater_than_six_hundred_and_can_be_overridden() {
    let mut owner = start(1);
    advance_by(&mut owner, 1500);
    assert_eq!(owner.status(), 90);
    owner.advance(600, 0).unwrap();
    assert_eq!(owner.status(), 90);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 90);
    owner.advance(1, 0).unwrap();
    assert_eq!(owner.status(), 1);
    assert_eq!(owner.timer_ms, 0);
    advance_by(&mut owner, 1500);
    owner.request(2);
    owner.advance(0, 0).unwrap();
    assert_eq!(owner.status(), 2);
    assert_eq!(owner.pending_state, None);
    assert_eq!(owner.timer_ms, 0);
}

#[test]
fn cooldown_and_active_timers_use_unsigned_wrapping_not_signed_or_saturating_add() {
    let mut owner = start(1);
    advance_by(&mut owner, 1500);
    owner.advance(0x8000_0000, 0).unwrap();
    assert_eq!(owner.status(), 1);
    owner.state = 90;
    owner.timer_ms = u32::MAX - 99;
    owner.advance(100, 0).unwrap();
    assert_eq!(owner.status(), 90);
    assert_eq!(owner.timer_ms, 0);
    owner.state = 2;
    owner.timer_ms = u32::MAX - 99;
    assert_eq!(position(owner.advance(100, 0).unwrap()), [0.0, 1.5, 0.2]);
    assert_eq!(owner.timer_ms, 0);
}

#[test]
fn descent_changes_pitch_not_fov_and_completion_native_immediate_is_eighty_degrees() {
    let mut owner = start(3);
    let first = advance_by(&mut owner, 500);
    assert_eq!(position(first), [0.0, 1.6, 0.25]);
    assert_eq!(first.pitch_word, None);
    let midpoint = advance_by(&mut owner, 500);
    assert_eq!(position(midpoint), [0.0, 1.5, 0.5]);
    assert_eq!(midpoint.pitch_word, Some(35.0_f32.to_bits()));
    let falling = advance_by(&mut owner, 500);
    assert_eq!(position(falling), [0.0, f32::from_bits(0x3f68_f5c2), 0.75]);
    assert_eq!(falling.pitch_word, Some(47.5_f32.to_bits()));
    assert_eq!(falling.field_of_view_word, None);
    assert_eq!(falling.angle_words, None);
    let complete = advance_by(&mut owner, 500);
    assert_eq!(position(complete), [0.0, -0.5, 1.0]);
    assert_eq!(complete.pitch_word, Some(80.0_f32.to_bits()));
    assert_eq!(complete.field_of_view_word, None);
    assert_eq!(
        complete.audio[0],
        Some(NativeSceneSound::StopStream { slot: 10 })
    );
    assert_eq!(owner.status(), u32::MAX);
}

#[test]
fn descent_plays_twenty_then_stops_ten_once_and_stops_ten_again_at_completion() {
    let mut owner = start(3);
    advance_by(&mut owner, 1799);
    assert!(!owner.event_fired);
    let threshold = owner.advance(1, 177).unwrap();
    assert_eq!(
        threshold.audio,
        [
            Some(NativeSceneSound::PlayStatic {
                slot: 20,
                volume: 177
            }),
            Some(NativeSceneSound::StopStream { slot: 10 }),
            None,
            None,
        ]
    );
    assert_eq!(owner.advance(0, 177).unwrap().audio, [None; 4]);
    assert_eq!(owner.advance(199, 177).unwrap().audio, [None; 4]);
    assert_eq!(
        owner.advance(1, 177).unwrap().audio,
        [
            Some(NativeSceneSound::StopStream { slot: 10 }),
            None,
            None,
            None
        ]
    );
}

#[test]
fn descent_retains_pending_until_the_next_idle_callback_without_advancing_it() {
    let mut owner = start(3);
    owner.request(2);
    advance_by(&mut owner, 2000);
    assert_eq!(owner.status(), u32::MAX);
    assert_eq!(owner.pending_state, Some(2));
    assert!(owner.advance(200, 0).unwrap().position_words.is_none());
    assert_eq!(owner.status(), 2);
    assert_eq!(owner.timer_ms, 0);
    assert_eq!(owner.pending_state, None);
    assert_eq!(
        position(advance_by(&mut owner, 400)),
        [0.0, f32::from_bits(0xbed7_0a3e), 1.2]
    );
}

#[test]
fn constructor_camera_writes_preserve_unrelated_roll_fov_and_do_not_start_sound() {
    let frame = CmvsNativeSceneKind51::new(37).initial_frame();
    assert_eq!(frame.position_words, Some([0, 1.5_f32.to_bits(), 0]));
    assert_eq!(frame.pitch_word, Some(35.0_f32.to_bits()));
    assert_eq!(frame.yaw_word, Some(0));
    assert_eq!(frame.angle_words, None);
    assert_eq!(frame.field_of_view_word, None);
    assert_eq!(frame.audio, [None; 4]);
}

#[test]
fn native_pi_and_float_narrowing_precede_hundredths_truncation() {
    // Degrees=90 narrows radians to float PI/2, but sin narrows back to 1.0.
    assert_eq!(landing_sine(90.0).to_bits(), 0.2_f32.to_bits());
    assert_eq!(landing_sine(-45.0).to_bits(), (-0.14_f32).to_bits());
    assert_eq!(landing_sine(0.0).to_bits(), 0);
    // At 180 degrees the native float PI produces a negative sine, truncated to zero.
    assert_eq!(landing_sine(180.0).to_bits(), 0);
    assert_eq!(radians(180.0), f32::from_bits(0x4049_0fdb) as f64);
    assert_ne!(radians(180.0), std::f64::consts::PI);
}

#[test]
fn private_continuation_preserves_queued_transition_clock_and_once_only_audio() {
    for (selector, elapsed) in [(1, 1103), (2, 821), (3, 1837)] {
        let mut owner = start(selector);
        advance_by(&mut owner, elapsed);
        owner.request(if selector == 3 { 1 } else { 3 });
        let bytes = serde_json::to_vec(&owner).unwrap();
        let mut restored: CmvsNativeSceneKind51 = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored, owner);
        for delta in [0, 17, 199, 200, 3, 700, 1, 0, 200, 200, 200, 200] {
            assert_eq!(
                restored.advance(delta, 163).unwrap(),
                owner.advance(delta, 163).unwrap()
            );
            assert_eq!(restored, owner);
        }
    }
}
