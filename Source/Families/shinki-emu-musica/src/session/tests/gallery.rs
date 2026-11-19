use super::*;
use crate::MusicaSystemPage as Page;
#[test]
fn native_gpu_gallery_pages_music_and_script_return_share_session_lifecycle() {
    let _lock = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    title::game(root.path());
    fixture::asset(
        root.path(),
        "bg",
        "memory.png",
        &title::png(1280, 720, [220, 30, 40, 255]),
    );
    let wave = fixture::wave();
    fixture::assets(
        root.path(),
        "bgm",
        &[("arbitrary-song.ogg", &wave), ("other-song.ogg", &wave)],
    );
    fixture::assets(
        root.path(),
        "scr",
        &[
            ("test.sc", b".message 1   Main\r\n.end\r\n"),
            ("memory-alpha.sc", b".message 1   Gallery\r\n.end\r\n"),
            ("credits.sc", b".message 1   Gallery\r\n.end\r\n"),
            ("original-index.sc", b".chain memory-alpha.sc\r\n.end\r\n"),
        ],
    );
    let mut req = title::title_request(root.path());
    let mut data = title::menu_data(false);
    data["replay_index"] = serde_json::json!("musica:/scr/original-index.sc");
    data["movies"] = serde_json::json!([{"label":"Credits","resource":"musica:/scr/credits.sc"}]);
    data["cg"] = serde_json::json!([{"label":"Memory","resource":"musica:/bg/memory.png"}]);
    req.configuration
        .iter_mut()
        .find(|entry| entry.id == "frontend")
        .unwrap()
        .value = ConfigValue::String(data.to_string());
    let sink = Sink::default();
    req.audio_output = Some(Box::new(sink.clone()));
    let mut provider = fixture::provider();
    let (_, mut session) = provider.open_session(req).unwrap();
    session
        .vm
        .replace_global_variables(std::collections::BTreeMap::from([(
            "TOHKA_CLEAR".into(),
            1,
        )]));
    session
        .advance(
            0,
            &[
                key(KeyCode::ArrowDown),
                key(KeyCode::ArrowDown),
                key(KeyCode::ArrowDown),
                key(KeyCode::Enter),
            ],
        )
        .unwrap();
    assert_eq!(session.vm.state().system_ui.page, Page::Memories);
    session.advance(0, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, Page::GalleryCg);
    let pixel = ((97 * 1280 + 65) * 4) as usize;
    assert_eq!(&session.scene.pixels[pixel..pixel + 4], &[220, 30, 40, 255]);
    session
        .advance(500_000_000, &[key(KeyCode::ArrowDown)])
        .unwrap();
    assert_eq!(session.vm.state().instruction_count, 0);
    session
        .advance(
            0,
            &[
                key(KeyCode::Escape),
                key(KeyCode::ArrowDown),
                key(KeyCode::ArrowDown),
                key(KeyCode::Enter),
            ],
        )
        .unwrap();
    assert_eq!(session.vm.state().system_ui.page, Page::GalleryBgm);
    session
        .advance(0, &[key(KeyCode::ArrowUp), key(KeyCode::Enter)])
        .unwrap();
    assert_eq!(
        session.vm.state().audio[&0].resource_uri,
        "musica:/bgm/other-song.ogg"
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while !sink.nonzero.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(sink.nonzero.load(Ordering::Acquire));
    session.advance(0, &[key(KeyCode::Backspace)]).unwrap();
    assert!(!session.vm.state().audio[&0].playing);
    session
        .advance(
            0,
            &[
                RuntimeEvent::PointerMove { x: 180.0, y: 80.0 },
                RuntimeEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Pressed,
                },
            ],
        )
        .unwrap();
    assert_eq!(
        session.vm.state().audio[&0].resource_uri,
        "musica:/bgm/arbitrary-song.ogg"
    );
    session.advance(0, &[key(KeyCode::Escape)]).unwrap();
    assert!(!session.vm.state().audio[&0].playing);
    for (focus, expected) in [(1, "memory-alpha.sc"), (3, "credits.sc")] {
        session.vm.set_gallery_page(Page::Memories, focus).unwrap();
        session.advance(0, &[key(KeyCode::Enter)]).unwrap();
        session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
        assert_eq!(
            session.vm.state().script_uri,
            format!("musica:/scr/{expected}")
        );
        assert_eq!(session.message.as_ref().unwrap().0, "Gallery");
        session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
        assert_eq!(session.vm.state().system_ui.page, Page::Title);
    }
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "Main");
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    let before = session.vm.state().clone();
    assert!(session.vm.set_gallery_page(Page::GalleryBgm, 2).is_err());
    assert_eq!(session.vm.state(), &before);
    Box::new(session).close().unwrap();
}
