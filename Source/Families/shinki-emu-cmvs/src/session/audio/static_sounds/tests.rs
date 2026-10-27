use super::*;

fn worker() -> Worker {
    Worker {
        bgm: [None, None],
        active_bgm: 0,
        channels: std::array::from_fn(|_| None),
        static_voices: std::array::from_fn(|_| Vec::new()),
        retired_stream_markers: [NativeStreamMarker::default(); CHANNEL_SOUNDS],
    }
}
fn clip(values: &[f32]) -> DecodedClip {
    DecodedClip {
        sample_rate: OUTPUT_RATE,
        frames: Arc::new(values.iter().flat_map(|value| [*value; 2]).collect()),
        source: None,
        native_channels: 2,
        indeterminate_tail: false,
        native_restart_rewinds: true,
        native_repeat: None,
        native_pcm: None,
    }
}
fn load(worker: &mut Worker, values: &[f32]) {
    worker.static_command(StaticCommand::Load {
        slot: 16,
        clip: clip(values),
    });
}
fn duplicate(worker: &mut Worker) {
    worker.static_command(StaticCommand::Duplicate {
        slot: 16,
        amplitude: 1.0,
    });
}
#[test]
fn duplicate_voices_overlap_but_the_thirty_third_is_dropped() {
    let mut worker = worker();
    load(&mut worker, &[0.01; CHUNK_FRAMES * 2]);
    for _ in 0..33 {
        duplicate(&mut worker);
    }
    let mut mix = [0.0; CHUNK_FRAMES * 2];
    worker.mix_static_duplicates(&mut mix);
    assert!((mix[0] - 0.32).abs() < 0.000001);
    assert_eq!(mix[0], mix[1]);
    worker.mix_static_duplicates(&mut [0.0; CHUNK_FRAMES * 2]);
    assert!(!worker.any_active());
    duplicate(&mut worker);
    let mut mix = [0.0; CHUNK_FRAMES * 2];
    worker.mix_static_duplicates(&mut mix);
    assert!(mix.iter().all(|sample| *sample == 0.01));
}
#[test]
fn main_stop_retains_cursor_and_does_not_stop_duplicates() {
    let mut worker = worker();
    let mut samples = vec![0.1; CHUNK_FRAMES * 4];
    samples[CHUNK_FRAMES..].fill(0.2);
    load(&mut worker, &samples);
    worker.static_command(StaticCommand::Main {
        slot: 16,
        amplitude: 1.0,
        looping: false,
    });
    let mut first = [0.0; CHUNK_FRAMES * 2];
    mix_stream(worker.channels[31].as_mut().unwrap(), &mut first);
    assert!(first.iter().all(|sample| *sample == 0.1));
    duplicate(&mut worker);
    worker.static_command(StaticCommand::Stop { slot: 16 });
    let mut mix = [0.0; CHUNK_FRAMES * 2];
    worker.mix_static_duplicates(&mut mix);
    assert!(mix.iter().all(|sample| *sample == 0.1));
    worker.static_command(StaticCommand::Main {
        slot: 16,
        amplitude: 1.0,
        looping: false,
    });
    let mut resumed = [0.0; CHUNK_FRAMES * 2];
    mix_stream(worker.channels[31].as_mut().unwrap(), &mut resumed);
    assert!(resumed.iter().all(|sample| *sample == 0.2));
}
#[test]
fn replacement_and_clear_destroy_main_and_duplicate_voices() {
    let mut worker = worker();
    load(&mut worker, &[0.1; CHUNK_FRAMES * 2]);
    duplicate(&mut worker);
    load(&mut worker, &[0.7; CHUNK_FRAMES * 2]);
    let mut mix = [0.0; CHUNK_FRAMES * 2];
    worker.mix_static_duplicates(&mut mix);
    assert!(mix.iter().all(|sample| *sample == 0.0));
    duplicate(&mut worker);
    worker.mix_static_duplicates(&mut mix);
    assert!(mix.iter().all(|sample| *sample == 0.7));
    worker.static_command(StaticCommand::Clear { slot: 16 });
    worker.static_command(StaticCommand::Main {
        slot: 16,
        amplitude: 1.0,
        looping: true,
    });
    duplicate(&mut worker);
    assert!(!worker.any_active());
    let mut mix = [0.0; CHUNK_FRAMES * 2];
    worker.mix_static_duplicates(&mut mix);
    assert!(mix.iter().all(|sample| *sample == 0.0));
}
