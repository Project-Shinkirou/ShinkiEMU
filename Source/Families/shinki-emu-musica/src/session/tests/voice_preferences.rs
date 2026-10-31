use super::*;

#[test]
fn manager_voice_preferences_keep_history_and_authored_voice_wait() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".message 1 aya-missing.ogg speaker First\r\n.message 2 aya-tone.ogg speaker Second\\v\r\n.end\r\n");
    fixture::asset(root.path(), "voice", "aya-tone.ogg", &fixture::wave());
    let sink = Sink::default();
    let mut provider = fixture::provider();
    let mut req = request(root.path(), sink.clone());
    req.configuration.push(ConfigEntry {
        id: "voice_aya".into(),
        value: ConfigValue::Bool(false),
    });
    let mut opened = provider.open(req).unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    opened
        .session
        .advance(16_666_667, &[key(KeyCode::Enter)])
        .unwrap();
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    let saved =
        crate::native_directory::NativeFiles::open(root.path(), crate::ScriptEncoding::ShiftJis)
            .unwrap()
            .slot(0)
            .unwrap()
            .unwrap();
    let checkpoint = saved.checkpoint().unwrap();
    assert_eq!(checkpoint.history.len(), 2);
    assert_eq!(checkpoint.history[0].voice, "aya-missing.ogg");
    assert_eq!(checkpoint.history[1].voice, "aya-tone.ogg");
    opened
        .session
        .advance(
            0,
            &[
                key(KeyCode::F9),
                key(KeyCode::PageUp),
                key(KeyCode::ArrowUp),
                key(KeyCode::Enter),
            ],
        )
        .unwrap();
    opened.session.advance(0, &[key(KeyCode::Escape)]).unwrap();
    assert_eq!(
        opened.session.advance(50_000_000, &[]).unwrap().status,
        RuntimeStatus::Waiting
    );
    assert!(!sink.nonzero.load(Ordering::Acquire));
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if opened.session.advance(16_666_667, &[]).unwrap().status == RuntimeStatus::Finished {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "restarted voice duration did not release the message"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    opened.session.close().unwrap();
}

#[test]
fn voice_preferences_are_typed_and_resolved_from_descriptor() {
    let descriptor = crate::musica_descriptor();
    descriptor.validate().unwrap();
    let defaults = resolve_config(&descriptor.configuration, &[]).unwrap();
    let preferences = crate::voice_preferences::VoicePreferences::resolve(&defaults).unwrap();
    assert!(preferences.backlog_voice_playback);
    for prefix in ["ren", "sui", "aya", "tou", "mot", "other"] {
        assert!(preferences.enabled(&format!("musica:/voice/{prefix}-test.ogg")));
    }
    assert!(
        resolve_config(
            &descriptor.configuration,
            &[ConfigEntry {
                id: "voice_aya".into(),
                value: ConfigValue::String("false".into())
            }]
        )
        .is_err()
    );
}

#[test]
fn current_manager_preferences_override_saved_voice_playback() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".message 1 aya-tone.ogg speaker First\r\n.end\r\n",
    );
    fixture::asset(root.path(), "voice", "aya-tone.ogg", &fixture::wave());
    let mut provider = fixture::provider();
    let mut initial = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    initial
        .session
        .advance(16_666_667, &[key(KeyCode::F5)])
        .unwrap();
    initial.session.close().unwrap();
    let sink = Sink::default();
    let mut req = request(root.path(), sink.clone());
    req.configuration.push(ConfigEntry {
        id: "voice_aya".into(),
        value: ConfigValue::Bool(false),
    });
    let mut loaded = provider.open(req).unwrap();
    loaded.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    loaded.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    let saved =
        crate::native_directory::NativeFiles::open(root.path(), crate::ScriptEncoding::ShiftJis)
            .unwrap()
            .slot(1)
            .unwrap()
            .unwrap();
    assert_eq!(
        saved.checkpoint().unwrap().history.last().unwrap().voice,
        "aya-tone.ogg"
    );
    loaded
        .session
        .advance(0, &[key(KeyCode::PageUp), key(KeyCode::Enter)])
        .unwrap();
    assert!(!sink.nonzero.load(Ordering::Acquire));
    loaded.session.close().unwrap();
}

#[test]
fn backlog_voice_preference_does_not_remove_history() {
    let source = b".message 1 aya-tone.ogg speaker First\r\n.end\r\n";
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        roukaku_core::Hash256::from_sha256(source),
        crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
        1,
    )
    .unwrap();
    vm.set_voice_preferences(crate::voice_preferences::VoicePreferences {
        backlog_voice_playback: false,
        ..Default::default()
    });
    vm.step(1).unwrap();
    vm.open_backlog().unwrap();
    let commands = vm.replay_backlog_voice().unwrap();
    assert!(!commands.iter().any(|command| matches!(
        command,
        crate::MusicaAudioCommand::Play { .. } | crate::MusicaAudioCommand::LoadResource { .. }
    )));
    assert_eq!(vm.state().backlog.len(), 1);
    assert!(vm.state().backlog[0].voice.is_some());
}

#[test]
fn manager_bus_mute_is_applied_before_first_pcm_and_survives_restore() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".message 1 aya-tone.ogg speaker Voice\\v\r\n.end\r\n",
    );
    fixture::asset(root.path(), "voice", "aya-tone.ogg", &fixture::wave());
    let sink = Sink::default();
    let mut req = request(root.path(), sink.clone());
    req.configuration.push(ConfigEntry {
        id: "voice_muted".into(),
        value: ConfigValue::Bool(true),
    });
    let mut provider = fixture::provider();
    let mut opened = provider.open(req).unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    let saved =
        crate::native_directory::NativeFiles::open(root.path(), crate::ScriptEncoding::ShiftJis)
            .unwrap()
            .slot(0)
            .unwrap()
            .unwrap();
    assert_eq!(
        saved.checkpoint().unwrap().history.last().unwrap().voice,
        "aya-tone.ogg"
    );
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    assert!(!sink.nonzero.load(Ordering::Acquire));
    opened.session.close().unwrap();
}
