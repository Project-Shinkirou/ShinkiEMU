use super::*;

fn stretcher(rate: f32, looping: bool) -> NativeMono175 {
    NativeMono175::new(22050, rate, looping, 44100, 8192).unwrap()
}

#[test]
fn unsupported_rates_and_notification_sizes_are_not_guessed() {
    assert!(NativeMono175::new(48000, 2.0, false, 44100, 8192).is_err());
    assert!(NativeMono175::new(22050, 2.0, false, 48000, 8192).is_err());
    assert!(NativeMono175::new(22050, 2.0, false, 44100, 9600).is_err());
    assert!(NativeMono175::new(22050, f32::NAN, false, 44100, 8192).is_err());
    assert!(NativeMono175::new(22050, 1.0, false, 44100, 8192).is_err());
}

#[test]
fn rate_three_excludes_excess_blend_tail_from_logical_output() {
    let mut state = stretcher(3.0, false);
    state.input[..1000].fill(Some(0.25));
    state.input_write = 1000;
    state.generate_block().unwrap();
    // Constant correlation ties preserve Pmin55. k=trunc(55/2+0.5)=28.
    assert_eq!(state.min_period, 55);
    assert_eq!(state.window, 110);
    assert_eq!(state.max_period, 220);
    assert_eq!(state.input_read, 83);
    assert_eq!(state.output_write, 28);
    assert_eq!(state.output[0], Some(0.25));
    assert_eq!(state.output[54], Some(0.25));
}

#[test]
fn pcm_conversion_uses_native_32767_scale_and_truncation() {
    assert_eq!(NativeMono175::to_native_i16(1.0), 32767);
    assert_eq!(NativeMono175::to_native_i16(-1.0), -32767);
    assert_eq!(NativeMono175::to_native_i16(0.5), 16383);
    assert_eq!(NativeMono175::to_native_i16(-0.5), -16383);
    assert_eq!(NativeMono175::to_native_i16(2.0), 32767);
    assert_eq!(NativeMono175::to_native_i16(-2.0), -32768);
    assert_eq!(NativeMono175::to_native_i16(f32::NAN), -32768);
}

#[test]
fn output_queue_changes_next_decoder_request_and_retains_source_progress() {
    let mut state = stretcher(2.0, false);
    let pcm = vec![32767; 22050];
    let mut output = vec![f32::NAN; state.source_chunk_samples()];
    let first = state.next_chunk(&pcm, None, &mut output);
    assert_eq!(first.initialized_samples, 1024);
    assert_eq!(first.decoder_requested_bytes, 10240);
    assert_eq!(first.decoder_returned_bytes, 10240);
    assert_eq!(first.native_decoded_bytes, 2048);
    assert!(!first.finished);
    assert!(!first.indeterminate_tail);
    assert_eq!(output[0], 32766.0 / 32768.0);
    assert_eq!(state.source_cursor, 5120);
    assert_eq!(state.output_write - state.output_read, 21);
    let second = state.next_chunk(&pcm, None, &mut output);
    assert_eq!(second.decoder_requested_bytes, (2048 - 42) * 5);
    assert_eq!(second.decoder_returned_bytes, (2048 - 42) * 5);
    assert_eq!(state.source_cursor, 10135);
    assert_eq!(second.initialized_samples, 1024);
    assert!(!second.indeterminate_tail);
}

#[test]
fn input_multiplier_compares_promoted_rate_to_double_2_point_1() {
    let pcm = vec![1000; 22050];
    let mut low = stretcher(2.1_f32, false);
    let mut high = stretcher(f32::from_bits(2.1_f32.to_bits() + 1), false);
    let mut output = vec![0.0; low.source_chunk_samples()];
    assert_eq!(
        low.next_chunk(&pcm, None, &mut output)
            .decoder_requested_bytes,
        2048 * 5
    );
    assert_eq!(
        high.next_chunk(&pcm, None, &mut output)
            .decoder_requested_bytes,
        2048 * 8
    );
}

#[test]
fn short_input_plays_initialized_prefix_then_marks_unknown_heap_tail() {
    let mut state = stretcher(2.0, false);
    let mut output = vec![f32::NAN; state.source_chunk_samples()];
    let chunk = state.next_chunk(&[32767], None, &mut output);
    assert_eq!(chunk.decoder_requested_bytes, 10240);
    assert_eq!(chunk.decoder_returned_bytes, 2);
    assert_eq!(chunk.native_decoded_bytes, 2048);
    assert_eq!(chunk.initialized_samples, 55);
    assert!(chunk.finished);
    assert!(chunk.indeterminate_tail);
    assert_eq!(output[0], 32766.0 / 32768.0);
    assert!(output[1..55].iter().all(|sample| *sample == 0.0));
    assert!(output[55..].iter().all(|sample| sample.is_nan()));
    let next = state.next_chunk(&[32767], None, &mut output);
    assert_eq!(next.initialized_samples, 0);
    assert_eq!(next.native_decoded_bytes, 0);
    assert!(next.indeterminate_tail);
}

#[test]
fn looping_source_continues_across_notifications_without_offline_expansion() {
    let mut state = stretcher(2.0, true);
    let mut output = vec![0.0; state.source_chunk_samples()];
    let input_pointer = state.input.as_ptr();
    let output_pointer = state.output.as_ptr();
    let scratch_pointer = state.scratch.as_ptr();
    for _ in 0..3 {
        let chunk = state.next_chunk(&[32767], None, &mut output);
        assert_eq!(chunk.initialized_samples, 1024);
        assert!(!chunk.finished);
        assert!(!chunk.indeterminate_tail);
        assert_eq!(state.source_cursor, 0);
        assert_eq!(state.input.as_ptr(), input_pointer);
        assert_eq!(state.output.as_ptr(), output_pointer);
        assert_eq!(state.scratch.as_ptr(), scratch_pointer);
    }
}

#[test]
fn decoder_restart_preserves_stretch_queues_and_initialized_scratch() {
    let mut state = stretcher(2.0, false);
    let pcm = vec![32767; 22050];
    let mut output = vec![0.0; state.source_chunk_samples()];
    state.next_chunk(&pcm, None, &mut output);
    let before = (
        state.input_read,
        state.input_write,
        state.output_read,
        state.output_write,
    );
    let scratch = state.scratch[0];
    state.restart(true);
    assert_eq!(state.source_cursor, 0);
    assert!(state.looping);
    assert_eq!(
        (
            state.input_read,
            state.input_write,
            state.output_read,
            state.output_write
        ),
        before
    );
    assert_eq!(state.scratch[0], scratch);
    state.set_looping(false);
    assert!(!state.looping);
    assert_eq!(
        (
            state.input_read,
            state.input_write,
            state.output_read,
            state.output_write
        ),
        before
    );
}

#[test]
fn unknown_correlation_does_not_turn_into_silence() {
    let mut state = stretcher(2.0, true);
    state.input_write = 0x4000;
    let mut output = vec![f32::NAN; state.source_chunk_samples()];
    let chunk = state.next_chunk(&[32767], None, &mut output);
    assert_eq!(chunk.decoder_requested_bytes, 0);
    assert_eq!(chunk.initialized_samples, 0);
    assert!(chunk.indeterminate_tail);
    assert_eq!(state.blocked, Some(Boundary::Correlation));
    assert!(output.iter().all(|sample| sample.is_nan()));
}

#[test]
fn loop_seam_matches_contiguous_pcm_without_restarting_correlation_phase() {
    let cycle: Vec<i16> = (0..317)
        .map(|i| (((i * 257) % 60000) - 30000) as i16)
        .collect();
    let contiguous: Vec<i16> = cycle
        .iter()
        .copied()
        .cycle()
        .take(cycle.len() * 100)
        .collect();
    let mut looping = stretcher(1.5, true);
    let mut expanded = stretcher(1.5, false);
    let mut loop_output = vec![0.0; looping.source_chunk_samples()];
    let mut expanded_output = vec![0.0; expanded.source_chunk_samples()];
    for _ in 0..3 {
        let from_loop = looping.next_chunk(&cycle, None, &mut loop_output);
        let from_contiguous = expanded.next_chunk(&contiguous, None, &mut expanded_output);
        assert_eq!(from_loop.initialized_samples, loop_output.len());
        assert_eq!(from_contiguous.initialized_samples, expanded_output.len());
        assert!(!from_loop.indeterminate_tail);
        assert!(!from_contiguous.indeterminate_tail);
        assert_eq!(
            from_loop.decoder_requested_bytes,
            from_contiguous.decoder_requested_bytes
        );
        assert_eq!(loop_output, expanded_output);
        assert_eq!(looping.input_read, expanded.input_read);
        assert_eq!(looping.output_read, expanded.output_read);
        assert_eq!(looping.output_write, expanded.output_write);
    }
}

#[test]
fn native_repeat_source_matches_contiguous_pcm_without_resetting_stretch_queues() {
    let first: Vec<i16> = (0..317)
        .map(|i| (((i * 257) % 60000) - 30000) as i16)
        .collect();
    let repeat: Vec<i16> = first.iter().map(|sample| sample / 2).collect();
    let contiguous: Vec<i16> = first
        .iter()
        .copied()
        .chain(repeat.iter().copied().cycle().take(repeat.len() * 100))
        .collect();
    let mut looping = stretcher(2.0, true);
    let mut expanded = stretcher(2.0, false);
    let mut loop_output = vec![0.0; looping.source_chunk_samples()];
    let mut expanded_output = vec![0.0; expanded.source_chunk_samples()];
    for _ in 0..3 {
        let from_loop = looping.next_chunk(&first, Some(&repeat), &mut loop_output);
        let from_contiguous = expanded.next_chunk(&contiguous, None, &mut expanded_output);
        assert_eq!(from_loop.initialized_samples, loop_output.len());
        assert_eq!(from_contiguous.initialized_samples, expanded_output.len());
        assert!(!from_loop.indeterminate_tail);
        assert_eq!(loop_output, expanded_output);
    }
    assert!(looping.decoder_repeated());
}
