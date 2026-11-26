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
    assert_eq!(&session.scene.pixels()[pixel..pixel + 4], &[220, 30, 40, 255]);
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

#[test]
fn authored_gallery_hit_focus_paging_and_activation_share_geometry() {
    use serde_json::json;

    let _lock = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    title::game(root.path());
    let blue = title::png(1280, 720, [20, 40, 160, 255]);
    let green = title::png(1280, 720, [40, 160, 20, 255]);
    let red = title::png(1280, 720, [160, 20, 40, 255]);
    let note = title::png(11, 11, [240, 200, 30, 255]);
    let wave = fixture::wave();
    fixture::assets(
        root.path(),
        "bg",
        &[("gallery.png", &blue), ("sheet.png", &green), ("hover.png", &red), ("note.png", &note)],
    );
    fixture::assets(root.path(), "bgm", &[("z.ogg", &wave), ("a.ogg", &wave)]);
    let layout = json!({"screens": [
        {"id":"tabs", "layers":[{"resource":"musica:/bg/gallery.png"}],
         "hover":"musica:/bg/hover.png", "buttons":[
             {"rect":[20,40,64,32], "action":{"kind":"open","screen":"pictures"}},
             {"rect":[140,40,64,32], "action":{"kind":"open","screen":"music1"}},
             {"rect":[260,40,64,32], "action":{"kind":"open","screen":"music2"},
              "requires":{"bonus":1}},
             {"rect":[10,200,64,32], "action":{"kind":"back"}}
         ]},
        {"id":"pictures", "layers":[{"resource":"musica:/bg/sheet.png"}],
         "hover":"musica:/bg/hover.png", "buttons":[
             {"rect":[32,20,70,35], "action":{"kind":"view","images":["musica:/bg/gallery.png","musica:/bg/hover.png"]}},
             {"rect":[32,72,70,35], "action":{"kind":"back"}}
         ]},
        {"id":"music1", "layers":[{"resource":"musica:/bg/sheet.png"}],
         "track_group":"album", "initial_selection":0,
         "playing":{"resource":"musica:/bg/note.png","at":[-12,0]}, "buttons":[
             {"rect":[32,21,80,33], "action":{"kind":"track","resource":"musica:/bgm/z.ogg","select_only":true}},
             {"rect":[180,520,70,70], "action":{"kind":"next_page","screen":"music2"}},
             {"rect":[900,520,70,70], "action":{"kind":"back"}}
         ]},
        {"id":"music2", "layers":[{"resource":"musica:/bg/gallery.png"}],
         "track_group":"album", "initial_selection":0, "buttons":[
             {"rect":[690,98,80,42], "action":{"kind":"track","resource":"musica:/bgm/a.ogg"}},
             {"rect":[180,520,70,70], "action":{"kind":"previous_page","screen":"music1"}},
             {"rect":[900,520,70,70], "action":{"kind":"back"}}
         ]}
    ]});
    let path = root.path().join("gallery-layout.json");
    std::fs::write(&path, layout.to_string()).unwrap();
    let mut request = title::title_request(root.path());
    let mut data = title::menu_data(false);
    data["gallery_layout"] = json!("gallery-layout.json");
    request.configuration.iter_mut().find(|entry| entry.id == "frontend").unwrap().value =
        ConfigValue::String(data.to_string());
    let mut provider = fixture::provider();
    let (_, mut session) = provider.open_session(request).unwrap();
    session.vm.replace_global_variables([("TOHKA_CLEAR".into(), 1)].into());
    let tap = |x, y| [
        RuntimeEvent::PointerMove { x, y },
        RuntimeEvent::PointerButton { button: PointerButton::Primary, state: KeyState::Pressed },
        RuntimeEvent::PointerButton { button: PointerButton::Primary, state: KeyState::Released },
    ];
    let pixel = |session: &crate::session::MusicaSession, x: usize, y: usize| -> [u8; 4] {
        let index = (y * 1280 + x) * 4;
        session.scene.pixels()[index..index + 4].try_into().unwrap()
    };

    session.advance(0, &tap(1050.0, 183.0)).unwrap();
    assert_eq!(session.vm.state().system_ui.page, Page::Memories);
    session.advance(0, &[key(KeyCode::ArrowDown), key(KeyCode::ArrowRight)]).unwrap();
    assert_eq!(pixel(&session, 145, 50), [160, 20, 40, 255]);
    assert_eq!(pixel(&session, 139, 50), [20, 40, 160, 255]);
    session.advance(0, &tap(270.0, 45.0)).unwrap();
    assert_eq!(session.gallery_cursor.as_ref().unwrap().screen, 0);
    let mut drag = tap(145.0, 50.0).to_vec();
    drag.insert(2, RuntimeEvent::PointerMove { x: 100.0, y: 50.0 });
    session.advance(0, &drag).unwrap();
    assert_eq!(session.gallery_cursor.as_ref().unwrap().screen, 0);

    session.advance(0, &tap(145.0, 50.0)).unwrap();
    session.advance(0, &tap(40.0, 35.0)).unwrap();
    assert!(session.vm.state().audio.values().all(|stream| !stream.playing));
    session.advance(0, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().audio[&0].resource_uri, "musica:/bgm/z.ogg");
    assert_eq!(pixel(&session, 20, 21), [240, 200, 30, 255]);
    session.advance(0, &[key(KeyCode::ArrowDown)]).unwrap();
    assert_eq!(session.gallery_cursor.as_ref().unwrap().screen, 3);
    assert_eq!(session.vm.state().audio[&0].resource_uri, "musica:/bgm/z.ogg");
    session.advance(0, &[key(KeyCode::ArrowLeft)]).unwrap();
    assert_eq!(session.gallery_cursor.as_ref().unwrap().screen, 2);
    session.advance(0, &tap(181.0, 550.0)).unwrap();
    assert_eq!(session.gallery_cursor.as_ref().unwrap().screen, 3);
    session.advance(0, &tap(700.0, 100.0)).unwrap();
    assert_eq!(session.vm.state().audio[&0].resource_uri, "musica:/bgm/a.ogg");
    session.advance(0, &[key(KeyCode::Escape)]).unwrap();
    assert!(!session.vm.state().audio[&0].playing);
    assert_eq!(session.gallery_cursor.as_ref().unwrap().screen, 0);
    assert_eq!(pixel(&session, 145, 50), [160, 20, 40, 255]);

    session.advance(0, &tap(30.0, 50.0)).unwrap();
    session.advance(0, &[key(KeyCode::ArrowRight), key(KeyCode::ArrowDown)]).unwrap();
    assert_eq!(pixel(&session, 40, 73), [160, 20, 40, 255]);
    session.advance(0, &tap(33.0, 25.0)).unwrap();
    assert_eq!(pixel(&session, 0, 0), [20, 40, 160, 255]);
    session.advance(0, &[key(KeyCode::ArrowRight)]).unwrap();
    assert_eq!(pixel(&session, 0, 0), [160, 20, 40, 255]);
    session.advance(0, &[key(KeyCode::Escape)]).unwrap();
    assert_eq!(pixel(&session, 0, 0), [40, 160, 20, 255]);
    assert_eq!(session.vm.state().instruction_count, 0);
    Box::new(session).close().unwrap();
}

#[test]
fn authored_gallery_cg_groups_keep_slots_and_labelled_replay_returns_to_sheet() {
    use serde_json::json;

    let _lock = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    title::game(root.path());
    let encoding = crate::ScriptEncoding::ShiftJis;
    let mut saved = crate::native_directory::NativeFiles::open(root.path(), encoding).unwrap();
    for name in ["shown-a.png", "shown-b.png"] {
        saved.record_cg(name).unwrap();
    }
    drop(saved);

    let sheet = [30, 60, 90, 255];
    let a = [120, 30, 60, 255];
    let b = [60, 120, 30, 255];
    let locked = [70, 70, 70, 255];
    let unlocked = [230, 120, 30, 255];
    let assets = [
        ("sheet.png", title::png(1280, 720, sheet)),
        ("Shown-A.png", title::png(1280, 720, a)),
        ("Shown-B.png", title::png(1280, 720, b)),
        ("Unseen.png", title::png(1280, 720, [13, 130, 200, 255])),
        ("locked.png", title::png(50, 40, locked)),
        ("unlocked.png", title::png(50, 40, unlocked)),
    ];
    fixture::assets(
        root.path(),
        "bg",
        &assets.iter().map(|(name, data)| (*name, data.as_slice())).collect::<Vec<_>>(),
    );
    fixture::assets(
        root.path(),
        "scr",
        &[
            ("test.sc", b".end\r\n"),
            ("flash.sc", b".message 1   Ordinary entry\r\n.end\r\n.label Again\r\n.set NOADDCGV = 1\r\n.stage * Unseen.png 0 0\r\n.message 2   Label replay\r\n.end\r\n"),
        ],
    );
    std::fs::write(root.path().join("layout.json"), json!({"screens":[
        {"id":"root", "layers":[{"resource":"musica:/bg/sheet.png"}], "buttons":[
            {"rect":[10,20,60,38], "action":{"kind":"open","screen":"cg-one"}},
            {"rect":[80,20,60,38], "action":{"kind":"open","screen":"flash"}}
        ]},
        {"id":"cg-one", "layers":[{"resource":"musica:/bg/sheet.png"}], "buttons":[
            {"rect":[20,60,50,40], "thumbnail":"musica:/bg/unlocked.png", "locked_thumbnail":"musica:/bg/locked.png",
             "action":{"kind":"view","images":["musica:/bg/Unseen.png"],"viewed_only":true}},
            {"rect":[100,60,50,40], "thumbnail":"musica:/bg/unlocked.png", "locked_thumbnail":"musica:/bg/locked.png",
             "action":{"kind":"view","images":["musica:/bg/Unseen.png","musica:/bg/Shown-A.png","musica:/bg/Unseen.png","musica:/bg/Shown-B.png"],"viewed_only":true}},
            {"rect":[250,80,50,40], "action":{"kind":"next_page","screen":"cg-two"}}
        ]},
        {"id":"cg-two", "layers":[{"resource":"musica:/bg/sheet.png"}], "buttons":[
            {"rect":[50,40,50,45], "action":{"kind":"view","images":["musica:/bg/Shown-B.png"]}},
            {"rect":[250,80,50,40], "action":{"kind":"previous_page","screen":"cg-one"}}
        ]},
        {"id":"flash", "layers":[{"resource":"musica:/bg/sheet.png"}], "buttons":[
            {"rect":[190,80,70,40], "action":{"kind":"next_page","screen":"flash-two"}}
        ]},
        {"id":"flash-two", "layers":[{"resource":"musica:/bg/sheet.png"}], "buttons":[
            {"rect":[190,80,70,40], "thumbnail":"musica:/bg/unlocked.png", "locked_thumbnail":"musica:/bg/locked.png",
             "requires_viewed":["musica:/bg/Unseen.png","musica:/bg/Shown-A.png"],
             "action":{"kind":"script","resource":"musica:/scr/flash.sc","label":"Again"}},
            {"rect":[270,80,70,40],
             "action":{"kind":"script","resource":"musica:/scr/flash.sc","label":"Absent"}}
        ]}
    ]}).to_string()).unwrap();
    let mut request = title::title_request(root.path());
    let mut frontend = title::menu_data(false);
    frontend["gallery_layout"] = json!("layout.json");
    request.configuration.iter_mut().find(|entry| entry.id == "frontend").unwrap().value =
        ConfigValue::String(frontend.to_string());
    let mut provider = fixture::provider();
    let (_, mut session) = provider.open_session(request).unwrap();
    session.vm.replace_global_variables([("TOHKA_CLEAR".into(), 1)].into());
    let tap = |x, y| [
        RuntimeEvent::PointerMove { x, y },
        RuntimeEvent::PointerButton { button: PointerButton::Primary, state: KeyState::Pressed },
        RuntimeEvent::PointerButton { button: PointerButton::Primary, state: KeyState::Released },
    ];
    let pixel = |session: &crate::session::MusicaSession, x: usize, y: usize| -> [u8; 4] {
        let index = (y * 1280 + x) * 4;
        session.scene.pixels()[index..index + 4].try_into().unwrap()
    };

    session.advance(0, &tap(1050.0, 183.0)).unwrap();
    session.advance(0, &tap(20.0, 35.0)).unwrap();
    assert_eq!(pixel(&session, 25, 65), locked);
    assert_eq!(pixel(&session, 105, 65), unlocked);
    session.advance(0, &tap(25.0, 65.0)).unwrap();
    assert!(session.gallery_cursor.as_ref().unwrap().focus.is_none());
    assert_eq!(pixel(&session, 25, 65), locked);
    session.advance(0, &[key(KeyCode::ArrowRight), key(KeyCode::Enter)]).unwrap();
    assert_eq!(pixel(&session, 0, 0), a);
    session.advance(0, &tap(20.0, 20.0)).unwrap();
    assert_eq!(pixel(&session, 0, 0), b);
    session.advance(0, &[key(KeyCode::ArrowLeft)]).unwrap();
    assert_eq!(pixel(&session, 0, 0), a);
    session.advance(0, &[key(KeyCode::Escape), key(KeyCode::PageDown)]).unwrap();
    assert_eq!(session.gallery_cursor.as_ref().unwrap().screen, 2);
    session.advance(0, &tap(55.0, 55.0)).unwrap();
    assert_eq!(pixel(&session, 0, 0), b);
    session.advance(0, &tap(55.0, 55.0)).unwrap();
    assert_eq!(session.gallery_cursor.as_ref().unwrap().screen, 2);
    assert_eq!(pixel(&session, 0, 0), sheet);
    session.advance(0, &[key(KeyCode::Escape)]).unwrap();

    session.advance(0, &tap(100.0, 35.0)).unwrap();
    session.advance(0, &[key(KeyCode::PageDown)]).unwrap();
    assert_eq!(pixel(&session, 195, 85), unlocked);
    session.advance(0, &tap(195.0, 85.0)).unwrap();
    for _ in 0..4 {
        session.advance(16_666_667, &[]).unwrap();
    }
    assert_eq!(session.message.as_ref().unwrap().0, "Label replay");
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, Page::Memories);
    assert_eq!(session.gallery_cursor.as_ref().unwrap().screen, 4);
    assert_eq!(pixel(&session, 195, 85), unlocked);
    let error = session.advance(0, &tap(275.0, 85.0)).err().expect("missing replay label");
    assert_eq!(error.code.as_str(), "SHINKI_EMU_MUSICA_RUNTIME_LABEL");
    assert_eq!(session.vm.state().system_ui.page, Page::Memories);
    assert_eq!(session.gallery_cursor.as_ref().unwrap().screen, 4);
    Box::new(session).close().unwrap();

    let saved = crate::native_directory::NativeFiles::open(root.path(), encoding).unwrap();
    assert!(saved.has_viewed_cg("musica:/bg/Shown-A.png"));
    assert!(!saved.has_viewed_cg("musica:/bg/Unseen.png"));
}
