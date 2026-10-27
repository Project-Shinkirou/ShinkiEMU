use super::*;

fn single() -> CmvsNativeImageShardGrid {
    CmvsNativeImageShardGrid::new([1, 1], [20, 0], &mut 1).unwrap()
}

#[test]
fn grid_ceil_uses_twenty_while_tile_parameter_controls_crop_step_and_pivot() {
    let value = CmvsNativeImageShardGrid::new([41, 21], [7, 99], &mut 1).unwrap();
    assert_eq!(value.particle_capacity(), 6);
    let last = value.draw_basic(5).unwrap();
    assert_eq!(last.reference_size, Some([41, 21]));
    assert_eq!(last.rect_words, Some([14, 7, 7, 7]));
    assert_eq!(last.position_words, Some([3, 3]));
    assert_eq!(last.auxiliary_pair_words, Some([17, 10]));
    assert!(last.parent_context_detached);
    assert!(last.screen_shake_disabled);
    assert_eq!(value.draw_basic(6), None);

    let unused = CmvsNativeImageShardGrid::new([41, 21], [7, u32::MAX], &mut 1).unwrap();
    assert_eq!(value.particles, unused.particles);
    for index in 0..6 {
        assert_eq!(value.draw_basic(index), unused.draw_basic(index));
    }
    let zero = CmvsNativeImageShardGrid::new([41, 21], [0, 0], &mut 1).unwrap();
    assert_eq!(zero.particle_capacity(), 6);
    assert_eq!(zero.draw_basic(5).unwrap().rect_words, Some([0; 4]));
    assert_eq!(
        zero.draw_basic(5).unwrap().auxiliary_pair_words,
        Some([0; 2])
    );
}

#[test]
fn row_major_births_consume_four_crt_draws_including_the_unused_heading() {
    let mut rng = 1;
    let value = CmvsNativeImageShardGrid::new([21, 1], [20, 0], &mut rng).unwrap();
    //CRT41,18467,6334,26500;19169,15724,11478,29358.
    assert_eq!(rng, 0x72ae7869);
    assert_eq!(value.particles[0].delay_ms, 117);
    assert_eq!(value.particles[1].delay_ms, 74);
    assert_eq!(value.particles[0].speed_word, 3.84_f32.to_bits());
    assert_eq!(
        value.particles[1].speed_word,
        (78.0_f32 / 100.0 - 0.5 + 4.0).to_bits()
    );
    assert_eq!(value.particles[0].drift_per_call_word, (-1.0_f32).to_bits());
    assert_eq!(
        value.particles[1].drift_per_call_word,
        (58.0_f32 / 50.0 - 1.0).to_bits()
    );
    assert_eq!(value.particles[1].base_position_words, [30, 10]);

    let rows = CmvsNativeImageShardGrid::new([1, 21], [20, 0], &mut 1).unwrap();
    assert_eq!(rows.particles[0].delay_ms, 187);
    assert_eq!(rows.particles[1].delay_ms, 74);
    assert_eq!(rows.particles[1].base_position_words, [10, 30]);
}

#[test]
fn delay_equality_performs_first_drift_even_at_zero_motion_time() {
    let mut value = single();
    let initial = value.draw_basic(0).unwrap();
    value.advance(116).unwrap();
    assert_eq!(value.draw_basic(0), Some(initial));
    assert_eq!(value.status(), 0);
    value.advance(1).unwrap();
    let first = value.draw_basic(0).unwrap();
    assert_eq!(first.auxiliary_pair_words, Some([9, 10]));
    assert_eq!(first.rotation_hundredths, 0);
    assert_eq!(first.scale_words, Some([1.0_f32.to_bits(); 2]));
    value.advance(0).unwrap();
    assert_eq!(
        value.draw_basic(0).unwrap().auxiliary_pair_words,
        Some([8, 10])
    );
}

#[test]
fn drift_counts_eligible_calls_while_y_rotation_and_scale_use_full_elapsed() {
    let mut once = single();
    once.advance(117).unwrap();
    let mut twice = once.clone();
    once.advance(1000).unwrap();
    twice.advance(500).unwrap();
    twice.advance(500).unwrap();
    let a = once.draw_basic(0).unwrap();
    let b = twice.draw_basic(0).unwrap();
    assert_eq!(a.auxiliary_pair_words, Some([8, 270]));
    assert_eq!(b.auxiliary_pair_words, Some([7, 270]));
    assert_eq!(a.rotation_hundredths, 3600);
    assert_eq!(a.rotation_hundredths, b.rotation_hundredths);
    assert_eq!(a.scale_words, b.scale_words);
    assert_eq!(once.clock_ms, twice.clock_ms);

    let mut half = single();
    half.advance(117 + 500).unwrap();
    assert_eq!(
        half.draw_basic(0).unwrap().auxiliary_pair_words,
        Some([9, 140])
    );
}

#[test]
fn rotation_wraps_its_integer_product_before_unsigned_division() {
    let mut value = single();
    value.advance(117 + 12_000_000).unwrap();
    let basic = value.draw_basic(0).unwrap();
    assert_eq!(basic.rotation_hundredths, 34300);
    assert_eq!(basic.scale_words, Some([0; 2]));
    assert!(value.finished());
    assert_eq!(value.particle_capacity(), 1);
    value.advance(1000).unwrap();
    assert_eq!(value.draw_basic(0).unwrap().rotation_hundredths, 1900);
    assert_eq!(value.particle_capacity(), 1);
}

#[test]
fn scale_is_one_at_delay_half_at_3750_and_zero_from_7500() {
    let mut value = single();
    value.advance(117).unwrap();
    assert_eq!(
        value.draw_basic(0).unwrap().scale_words,
        Some([1.0_f32.to_bits(); 2])
    );
    value.advance(3750).unwrap();
    assert_eq!(
        value.draw_basic(0).unwrap().scale_words,
        Some([0.5_f32.to_bits(); 2])
    );
    value.advance(3750).unwrap();
    assert_eq!(value.draw_basic(0).unwrap().scale_words, Some([0; 2]));
    value.advance(20_000).unwrap();
    assert_eq!(value.draw_basic(0).unwrap().scale_words, Some([0; 2]));
}

#[test]
fn finished_uses_inclusive_signed_y_boundary_and_latches_without_stopping() {
    let mut value = CmvsNativeImageShardGrid::new([1, 1], [2, 0], &mut 1).unwrap();
    value.particles[0].delay_ms = 0;
    value.particles[0].speed_word = 4.0_f32.to_bits();
    value.advance(128).unwrap();
    assert_eq!(
        value.draw_basic(0).unwrap().auxiliary_pair_words.unwrap()[1],
        33
    );
    assert!(!value.finished());
    value.advance(4).unwrap();
    assert_eq!(value.status(), -1);
    value.advance(0).unwrap();
    assert_eq!(f32::from_bits(value.particles[0].drift_word), -3.0);
    //Wrapping(now-birth) returns to0; a visible shard does not clear the latch.
    value.advance(u32::MAX - 131).unwrap();
    assert_eq!(
        value.draw_basic(0).unwrap().auxiliary_pair_words.unwrap()[1],
        1
    );
    assert!(value.finished());
    value.advance(136).unwrap();
    assert_eq!(
        value.draw_basic(0).unwrap().auxiliary_pair_words.unwrap()[1],
        35
    );
    assert_eq!(value.status(), -1);
}

#[test]
fn waiting_particles_prevent_completion_and_empty_grids_finish_on_first_tick() {
    let mut value = CmvsNativeImageShardGrid::new([21, 1], [20, 0], &mut 1).unwrap();
    value.particles[0].delay_ms = 0;
    value.particles[1].delay_ms = 1000;
    value.advance(500).unwrap();
    assert!(value.particles[0].destination_words[1] > 33);
    assert_eq!(value.particles[1].destination_words, [30, 10]);
    assert!(!value.finished());
    value.advance(1000).unwrap();
    assert!(value.finished());

    let mut rng = 123;
    for extent in [[0, 0], [0, u32::MAX], [u32::MAX, 0]] {
        let mut empty = CmvsNativeImageShardGrid::new(extent, [0, u32::MAX], &mut rng).unwrap();
        empty.validate().unwrap();
        assert_eq!(empty.status(), 0);
        empty.advance(0).unwrap();
        assert_eq!(empty.status(), -1);
        assert_eq!(empty.draw_basic(0), None);
    }
    assert_eq!(rng, 123);
}

#[test]
fn large_tile_coordinates_and_y_addition_wrap_with_signed_completion_comparison() {
    let grid = CmvsNativeImageShardGrid::new([21, 21], [u32::MAX, 0], &mut 1).unwrap();
    let corner = grid.draw_basic(3).unwrap();
    assert_eq!(corner.rect_words, Some([u32::MAX; 4]));
    assert_eq!(corner.position_words, Some([u32::MAX >> 1; 2]));
    assert_eq!(corner.auxiliary_pair_words, Some([2_147_483_646; 2]));

    let mut value = CmvsNativeImageShardGrid::new([1, 1], [u32::MAX, 0], &mut 1).unwrap();
    value.advance(117).unwrap();
    assert!(value.finished());
    value.advance(4).unwrap();
    assert_eq!(value.particles[0].destination_words[1], 0x80000000);
    assert!(value.finished());
    //At the same wrapped destination an unlatched worker remains pending.
    value.finished = false;
    value.advance(0).unwrap();
    assert!(!value.finished());
}

#[test]
fn unsigned_elapsed_rounds_to_f32_before_division_and_cvtt_uses_integer_indefinite() {
    let mut value = single();
    value.particles[0].delay_ms = 0;
    value.particles[0].speed_word = 4.0_f32.to_bits();
    value.advance(16_777_219).unwrap();
    assert_eq!(value.particles[0].destination_words[1], 4_194_315);
    value.advance(u32::MAX - 16_777_219).unwrap();
    assert_eq!(value.particles[0].destination_words[1], 1_073_741_834);

    let mut drift = single();
    drift.particles[0].drift_word = 2_147_483_648.0_f32.to_bits();
    drift.advance(117).unwrap();
    assert_eq!(drift.particles[0].destination_words[0], 0x8000000a);
    let mut negative = single();
    negative.particles[0].drift_word = (-0.99_f32).to_bits();
    negative.advance(117).unwrap();
    assert_eq!(negative.particles[0].destination_words[0], 9);
}

#[test]
fn absolute_clock_seed_and_elapsed_wrap_preserve_waiting_child_draw_state() {
    let mut value = single();
    value.clock_ms = u32::MAX - 5;
    value.advance(117).unwrap();
    assert_eq!(value.clock_ms, 111);
    assert_eq!(value.particles[0].destination_words, [9, 10]);
    value.advance(1000).unwrap();
    let previous = value.particles[0].clone();
    value.advance(u32::MAX - 1116).unwrap();
    assert_eq!(value.elapsed_ms, 0);
    assert_eq!(value.particles[0], previous);
    assert!(value.finished());
    value.advance(117).unwrap();
    assert_eq!(value.particles[0].destination_words, [7, 10]);
}

#[test]
fn private_snapshot_continues_drift_absolute_age_draw_state_and_finished_latch() {
    let mut original = CmvsNativeImageShardGrid::new([41, 21], [7, 19], &mut 1).unwrap();
    original.clock_ms = 123_456;
    for delta in [50, 200, 0, 10_000] {
        original.advance(delta).unwrap();
    }
    assert!(original.finished());
    let bytes = serde_json::to_vec(&original).unwrap();
    let mut restored: CmvsNativeImageShardGrid = serde_json::from_slice(&bytes).unwrap();
    restored.validate().unwrap();
    for delta in [0, 17, u32::MAX, 300, 15_000] {
        original.advance(delta).unwrap();
        restored.advance(delta).unwrap();
        assert_eq!(restored, original);
        for index in 0..original.particle_capacity() {
            assert_eq!(restored.draw_basic(index), original.draw_basic(index));
        }
    }
}

#[test]
fn unsafe_allocation_and_invalid_private_state_fail_before_clock_or_rng_changes() {
    let mut rng = 123;
    assert!(CmvsNativeImageShardGrid::new([u32::MAX; 2], [0; 2], &mut rng).is_err());
    assert_eq!(rng, 123);
    let mut value = single();
    value.particles[0].speed_word = f32::NAN.to_bits();
    let previous = value.clone();
    assert!(value.advance(1000).is_err());
    assert_eq!(value, previous);
    let mut count = single();
    count.particles.clear();
    assert!(count.validate().is_err());
    let mut invalid_drift = single();
    invalid_drift.particles[0].drift_per_call_word = f32::INFINITY.to_bits();
    assert!(invalid_drift.validate().is_err());

    let above_queue = CmvsNativeImageShardGrid::new([20 * 8193, 1], [0, 0], &mut 1).unwrap();
    assert_eq!(above_queue.particle_capacity(), 8193);
    assert!(above_queue.draw_basic(8192).is_some());
}
