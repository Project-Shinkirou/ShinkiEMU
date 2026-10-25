use super::*;
use crate::test_fixture as fixture;

fn setup() -> (tempfile::TempDir, MusicaMountedVfs, Mixer) {
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), &[0]);
    let archive = crate::mount_musica(
        root.path(),
        &root.path().join(shinki_emu_musica_keys::MUSICA_KEY_FILE),
    )
    .unwrap();
    let mut mixer = Mixer::new().unwrap();
    mixer
        .load(1, "musica:/bgm/tone.ogg", &archive, &AtomicBool::new(false))
        .unwrap();
    (root, archive, mixer)
}

fn rendered(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
    let mut samples = vec![0.0; frames * 2];
    mixer.render(&mut samples);
    samples
}

#[test]
fn restore_rejects_cursor_beyond_asset_without_changing_live_audio() {
    let (_root, archive, mut mixer) = setup();
    mixer.play(1, 0.5, 0.0, true, 0).unwrap();
    rendered(&mut mixer, 512);
    let before = postcard::to_allocvec(&mixer.snapshot()).unwrap();
    for playing in [false, true] {
        let mut snapshot: Vec<SoundSnapshot> = postcard::from_bytes(&before).unwrap();
        snapshot[0].position = 1_000_000.0;
        snapshot[0].playing = playing;
        let failure = mixer
            .restore(snapshot, &archive, &AtomicBool::new(false))
            .unwrap_err();
        assert_eq!(failure.code.as_str(), "SHINKI_EMU_MUSICA_AUDIO_SNAPSHOT");
        assert_eq!(postcard::to_allocvec(&mixer.snapshot()).unwrap(), before);
    }
    assert!(
        rendered(&mut mixer, 512)
            .iter()
            .any(|sample| sample.abs() > 0.001)
    );
}

#[test]
fn restore_accepts_a_stopped_sound_at_its_exact_endpoint() {
    let (_root, archive, mut mixer) = setup();
    let data = &mixer.sounds[&1].data;
    let endpoint = data.num_frames() as f64 / f64::from(data.sample_rate);
    let mut snapshot = mixer.snapshot();
    snapshot[0].position = endpoint;
    mixer
        .restore(snapshot, &archive, &AtomicBool::new(false))
        .unwrap();
    let restored = mixer.snapshot();
    assert_eq!(restored[0].position, endpoint);
    assert!(!restored[0].playing);
}

#[test]
fn cancelled_empty_restore_cannot_clear_the_live_mixer() {
    let (_root, archive, mut mixer) = setup();
    mixer.play(1, 0.5, 0.0, true, 0).unwrap();
    rendered(&mut mixer, 512);
    let before = postcard::to_allocvec(&mixer.snapshot()).unwrap();
    assert!(
        mixer
            .restore(Vec::new(), &archive, &AtomicBool::new(true))
            .unwrap()
            .is_none()
    );
    assert_eq!(postcard::to_allocvec(&mixer.snapshot()).unwrap(), before);
    assert!(
        rendered(&mut mixer, 512)
            .iter()
            .any(|sample| sample.abs() > 0.001)
    );
}

#[test]
fn cancelled_load_keeps_the_existing_stream_and_playback() {
    let (_root, archive, mut mixer) = setup();
    mixer.play(1, 0.5, 0.0, true, 0).unwrap();
    rendered(&mut mixer, 512);
    let before = postcard::to_allocvec(&mixer.snapshot()).unwrap();
    assert!(
        mixer
            .load(
                1,
                "musica:/bgm/not-present.ogg",
                &archive,
                &AtomicBool::new(true)
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(postcard::to_allocvec(&mixer.snapshot()).unwrap(), before);
    assert!(
        rendered(&mut mixer, 512)
            .iter()
            .any(|sample| sample.abs() > 0.001)
    );
}

#[test]
fn fade_in_snapshot_roundtrip_resumes_gain_and_remaining_duration() {
    let (_root, archive, mut mixer) = setup();
    mixer.play(1, 0.8, 0.0, true, 100).unwrap();
    rendered(&mut mixer, 2048);
    let encoded = postcard::to_allocvec(&mixer.snapshot()).unwrap();
    let snapshot: Vec<SoundSnapshot> = postcard::from_bytes(&encoded).unwrap();
    assert_eq!(snapshot[0].fade.as_ref().unwrap().elapsed, 2048);
    let mut restored = Mixer::new().unwrap();
    restored
        .restore(snapshot, &archive, &AtomicBool::new(false))
        .unwrap();
    let expected = rendered(&mut mixer, 1024);
    let actual = rendered(&mut restored, 1024);
    // A newly created resampler warms up its first few samples. Compare the
    // remaining waveform, including its evolving gain rather than a meter.
    let difference = expected[16..]
        .iter()
        .zip(&actual[16..])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(difference < 0.002, "waveform difference={difference}");
    assert!(restored.snapshot()[0].fade.is_some());
    rendered(&mut restored, 4800 - 3072);
    assert!(restored.snapshot()[0].fade.is_none());
    assert!(restored.snapshot()[0].playing);
}

#[test]
fn restored_fade_out_stops_at_remaining_sample_boundary() {
    let (_root, archive, mut mixer) = setup();
    mixer.play(1, 1.0, 0.0, true, 0).unwrap();
    rendered(&mut mixer, 512);
    mixer
        .apply(
            MusicaAudioCommand::Stop {
                sequence: 1,
                stream_id: 1,
                fade_ms: 100,
            },
            &archive,
            &AtomicBool::new(false),
        )
        .unwrap();
    rendered(&mut mixer, 1024);
    let snapshot = mixer.snapshot();
    let mut restored = Mixer::new().unwrap();
    restored
        .restore(snapshot, &archive, &AtomicBool::new(false))
        .unwrap();
    rendered(&mut restored, 3775);
    assert!(restored.snapshot()[0].playing);
    rendered(&mut restored, 1);
    assert!(!restored.snapshot()[0].playing);
    assert!(
        rendered(&mut restored, 512)
            .iter()
            .all(|s| s.abs() < 0.00001)
    );
}

#[test]
fn invalid_fade_does_not_replace_current_mixer() {
    let (_root, archive, mut mixer) = setup();
    mixer.play(1, 0.5, 0.0, true, 100).unwrap();
    rendered(&mut mixer, 512);
    let mut snapshot = mixer.snapshot();
    snapshot[0].fade.as_mut().unwrap().frames = 0;
    assert!(
        mixer
            .restore(snapshot, &archive, &AtomicBool::new(false))
            .is_err()
    );
    assert_eq!(mixer.snapshot()[0].fade.as_ref().unwrap().elapsed, 512);
    assert!(rendered(&mut mixer, 512).iter().any(|s| s.abs() > 0.0001));
}

#[test]
fn new_volume_command_replaces_fade_without_stopping_stream() {
    let (_root, archive, mut mixer) = setup();
    mixer.play(1, 1.0, 0.0, true, 100).unwrap();
    rendered(&mut mixer, 512);
    mixer
        .apply(
            MusicaAudioCommand::SetParams {
                sequence: 2,
                stream_id: 1,
                volume: 0.5,
                pan: 0.0,
                repeat: true,
            },
            &archive,
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(mixer.snapshot()[0].fade.is_none());
    assert!(rendered(&mut mixer, 512).iter().any(|v| v.abs() > 0.1));
    assert!(mixer.snapshot()[0].playing);
}

#[test]
fn stopping_unplayed_resource_keeps_a_restorable_snapshot() {
    let (_root, archive, mut mixer) = setup();
    mixer
        .apply(
            MusicaAudioCommand::Stop {
                sequence: 1,
                stream_id: 1,
                fade_ms: 100,
            },
            &archive,
            &AtomicBool::new(false),
        )
        .unwrap();
    let snapshot = mixer.snapshot();
    assert!(!snapshot[0].playing);
    assert!(snapshot[0].fade.is_none());
    mixer
        .restore(snapshot, &archive, &AtomicBool::new(false))
        .unwrap();
}

#[test]
fn bus_gain_scales_waveform_without_changing_authored_volume_or_fade() {
    let (_root, archive, mut full) = setup();
    let mut quiet = Mixer::new().unwrap();
    quiet
        .set_preferences(AudioPreferences {
            volume: [25, 100, 100],
            muted: [false; 3],
        })
        .unwrap();
    quiet
        .load(1, "musica:/bgm/tone.ogg", &archive, &AtomicBool::new(false))
        .unwrap();
    for mixer in [&mut full, &mut quiet] {
        mixer.play(1, 0.8, 0.0, true, 100).unwrap();
    }
    let expected = rendered(&mut full, 2048);
    let actual = rendered(&mut quiet, 2048);
    assert!(expected.iter().any(|v| v.abs() > 0.001));
    let difference = expected[512..]
        .iter()
        .zip(&actual[512..])
        .map(|(a, b)| (a * 0.25 - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(difference < 0.0001, "bus gain difference={difference}");
    assert_eq!(
        postcard::to_allocvec(&full.snapshot()).unwrap(),
        postcard::to_allocvec(&quiet.snapshot()).unwrap()
    );
    let snapshot = full.snapshot();
    quiet
        .restore(snapshot, &archive, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(quiet.preferences.volume, [25, 100, 100]);
    let expected = rendered(&mut full, 1024);
    let actual = rendered(&mut quiet, 1024);
    let difference = expected[32..]
        .iter()
        .zip(&actual[32..])
        .map(|(a, b)| (a * 0.25 - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(difference < 0.002, "restored gain difference={difference}");
}

#[test]
fn bus_mute_preserves_cursor_and_does_not_mute_other_buses() {
    let (_root, archive, mut mixer) = setup();
    mixer
        .set_preferences(AudioPreferences {
            volume: [100; 3],
            muted: [false, true, true],
        })
        .unwrap();
    mixer.play(1, 0.8, 0.0, true, 0).unwrap();
    assert!(rendered(&mut mixer, 1024).iter().any(|v| v.abs() > 0.001));
    mixer
        .set_preferences(AudioPreferences {
            volume: [100; 3],
            muted: [true; 3],
        })
        .unwrap();
    rendered(&mut mixer, 512);
    assert!(rendered(&mut mixer, 1024).iter().all(|v| v.abs() < 0.00001));
    let snapshot = mixer.snapshot();
    assert!(snapshot[0].playing && snapshot[0].position > 0.0);
    assert_eq!(snapshot[0].volume, 0.8);
    mixer
        .restore(snapshot, &archive, &AtomicBool::new(false))
        .unwrap();
    assert!(rendered(&mut mixer, 1024).iter().all(|v| v.abs() < 0.00001));
    mixer.set_preferences(AudioPreferences::default()).unwrap();
    assert!(rendered(&mut mixer, 1024).iter().any(|v| v.abs() > 0.001));
}

#[test]
fn voice_peak_samples_the_current_bounded_window() {
    let (_root, archive, mut mixer) = setup();
    mixer
        .load(4, "musica:/bgm/tone.ogg", &archive, &AtomicBool::new(false))
        .unwrap();
    let data = &mut mixer.sounds.get_mut(&4).unwrap().data;
    let midpoint = data.frames.len() / 2;
    for (index, frame) in Arc::make_mut(&mut data.frames).iter_mut().enumerate() {
        let level = if index < midpoint { 0.9 } else { 0.2 };
        frame.left = level;
        frame.right = -level;
    }
    mixer.play(4, 1.0, 0.0, false, 0).unwrap();
    rendered(&mut mixer, 4096);
    // After an 85 ms sampling gap, the last 30 ms are quiet; sampling the
    // oldest pending samples would incorrectly report the opening loud half.
    assert!((199..=201).contains(&mixer.voice_level_milli()));
    assert!(mixer.voice_level_cursor > 0.07);
}

#[test]
fn voice_peak_scales_authored_volume_and_resets_on_replay_and_load() {
    let (_root, archive, mut mixer) = setup();
    mixer
        .load(4, "musica:/bgm/tone.ogg", &archive, &AtomicBool::new(false))
        .unwrap();
    for frame in Arc::make_mut(&mut mixer.sounds.get_mut(&4).unwrap().data.frames) {
        frame.left = 0.8;
        frame.right = -0.8;
    }
    mixer.play(4, 0.5, 0.0, true, 0).unwrap();
    rendered(&mut mixer, 1024);
    assert!((399..=401).contains(&mixer.voice_level_milli()));
    mixer.play(4, 0.1, 0.0, true, 0).unwrap();
    assert_eq!(mixer.voice_level_milli(), 0);
    rendered(&mut mixer, 1024);
    assert!((79..=81).contains(&mixer.voice_level_milli()));
    mixer
        .load(4, "musica:/bgm/tone.ogg", &archive, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(mixer.voice_level_milli(), 0);
    mixer.play(4, 1.0, 0.0, true, 100).unwrap();
    rendered(&mut mixer, 1024);
    assert!(mixer.voice_level_milli() < 100);
}
