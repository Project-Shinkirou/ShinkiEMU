use super::*;
use crate::{MusicaLaunchMode, MusicaSystemPage};
pub(super) fn game(root: &std::path::Path) {
    game_with_title_case(root, false);
}

fn game_with_lowercase_title(root: &std::path::Path) {
    game_with_title_case(root, true);
}

fn game_with_title_case(root: &std::path::Path, lowercase_title: bool) {
    fixture::game(
        root,
        b".setglobal custom = custom + 1\r\n.message 1   First\r\n.setglobal AYAME_CLEAR = 1\r\n.end\r\n",
    );
    let mut assets = Vec::new();
    for (name, w, h) in [
        ("saveloadBase.png", 1280, 720),
        ("saveloadLoad.png", 352, 48),
        ("saveloadSelect.png", 344, 98),
        ("saveloadButtons.png", 356, 48),
        ("notsaved.png", 106, 60),
        ("configBase.png", 1280, 720),
        ("knob.png", 15, 25),
        ("checkmark.png", 21, 32),
        ("circle.png", 74, 74),
    ] {
        assets.push((
            name.to_owned(),
            png(
                w,
                h,
                if name == "knob.png" {
                    [200, 100, 50, 255]
                } else {
                    [20, 30, 40, 255]
                },
            ),
        ));
    }
    for page in 0..10 {
        assets.push((
            format!("saveload_Page{page}.png"),
            png(208, 48, [30, 40, 50, 255]),
        ));
    }
    for name in ["BGMTest.wav", "VOICEtest.wav", "SEtest.wav"] {
        assets.push((name.to_owned(), fixture::wave()));
    }
    for variant in 0..3 {
        let stem = if lowercase_title {
            format!("topmenu{variant}")
        } else {
            format!("topMenu{variant}")
        };
        let over = if lowercase_title {
            format!("{stem}over")
        } else {
            format!("{stem}Over")
        };
        assets.push((
            format!("{stem}.png"),
            png(1280, 720, [20 + variant * 20, 30, 40, 255]),
        ));
        assets.push((format!("{over}.png"), png(1280, 720, [100, 150, 200, 255])));
    }
    fixture::assets(
        root,
        "sys",
        &assets
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect::<Vec<_>>(),
    );
    std::fs::write(
        root.join("frontend-fixture.json"),
        menu_data(lowercase_title).to_string(),
    )
    .unwrap();
}
pub(super) fn png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(w, h, image::Rgba(color))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}
pub(super) fn title_request(root: &std::path::Path) -> OpenRequest {
    let mut req = request(root, Sink::default());
    req.configuration
        .iter_mut()
        .find(|entry| entry.id == "launch_mode")
        .unwrap()
        .value = ConfigValue::Enum("title".into());
    req.configuration.push(ConfigEntry {
        id: "frontend".into(),
        value: ConfigValue::String(
            std::fs::read_to_string(root.join("frontend-fixture.json"))
                .unwrap_or_else(|_| menu_data(false).to_string()),
        ),
    });
    req
}

pub(super) fn menu_data(lowercase: bool) -> serde_json::Value {
    use serde_json::json;
    let titles = (0..3).map(|variant| {
        let stem = if lowercase { format!("topmenu{variant}") } else { format!("topMenu{variant}") };
        let hover = if lowercase { format!("{stem}over") } else { format!("{stem}Over") };
        let actions = if variant == 2 { vec!["new_game", "load", "config", "memories", "exit"] } else { vec!["new_game", "load", "config", "exit"] };
        let buttons = actions.iter().enumerate().map(|(index, action)| {
            let top = if *action == "exit" { 216 } else { 24 + index * 48 };
            json!({"action": action, "rect": [1024, top, 256, 48]})
        }).collect::<Vec<_>>();
        let requires = match variant {
            1 => json!({"AYAME_CLEAR":1,"SUI_CLEAR":1,"REN_CLEAR":1}),
            2 => json!({"TOHKA_CLEAR":1}),
            _ => json!({}),
        };
        json!({"background":format!("musica:/sys/{stem}.png"),"hover":format!("musica:/sys/{hover}.png"),"requires":requires,"buttons":buttons})
    }).collect::<Vec<_>>();
    json!({"titles":titles})
}

fn omit_texture_overrides(root: &std::path::Path) {
    let profile_path = root.join(MUSICA_KEY_FILE);
    let mut key: toml::Table =
        toml::from_str(&std::fs::read_to_string(&profile_path).unwrap()).unwrap();
    key.remove("texture_overrides");
    std::fs::write(&profile_path, toml::to_string_pretty(&key).unwrap()).unwrap();
}

fn single_pixel_ani() -> Vec<u8> {
    let mut bytes = vec![0, 1, 1, 0, 0, 0, 0, 0, b'f', 0];
    for value in [1u16, 1, 32, 0, 0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([32, 96, 224, 255]);
    bytes
}

fn single_pixel_sqz() -> Vec<u8> {
    use std::io::Write;

    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&[32, 96, 224, 255]).unwrap();
    let frame = encoder.finish().unwrap();
    let mut bytes = b"SQZ1".to_vec();
    for value in [
        32u32,
        1,
        1,
        1,
        36,
        frame.len() as u32,
        36,
        frame.len() as u32,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(frame);
    bytes
}

#[test]
fn native_gpu_title_without_texture_overrides_starts_loads_returns_and_exits() {
    let _lock = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    game(root.path());
    omit_texture_overrides(root.path());
    let mut provider = fixture::provider();
    let (_, mut session) = provider.open_session(title_request(root.path())).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Title);
    assert_eq!(&session.scene.pixels[..4], &[20, 30, 40, 255]);
    session.advance(500_000_000, &[]).unwrap();
    assert_eq!(session.vm.state().instruction_count, 0);
    session
        .advance(
            0,
            &[
                key(KeyCode::ArrowDown),
                RuntimeEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Pressed,
                },
            ],
        )
        .unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Title);
    session.advance(0, &[key(KeyCode::ArrowUp)]).unwrap();
    session
        .advance(0, &[RuntimeEvent::PointerMove { x: 1100.0, y: 80.0 }])
        .unwrap();
    let pixel = ((80 * 1280 + 1100) * 4) as usize;
    assert_eq!(
        &session.scene.pixels[pixel..pixel + 4],
        &[100, 150, 200, 255]
    );
    session.advance(0, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Load);
    session.advance(0, &[key(KeyCode::Escape)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Title);
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "First");
    session.save(20).unwrap();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Title);
    assert!(!session.finished);
    assert_eq!(session.vm.state().gallery_unlocks.len(), 2);
    session
        .advance(0, &[key(KeyCode::ArrowDown), key(KeyCode::Enter)])
        .unwrap();
    let frontend_identity = session.vm.frontend_identity();
    session
        .advance(
            0,
            &[
                key(KeyCode::ArrowRight),
                key(KeyCode::ArrowRight),
                key(KeyCode::Enter),
            ],
        )
        .unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::None);
    assert_eq!(session.vm.state().launch_mode, MusicaLaunchMode::Title);
    assert_eq!(session.vm.state().gallery_unlocks.len(), 2);
    assert_eq!(session.message.as_ref().unwrap().0, "First");
    assert_eq!(session.vm.frontend_identity(), frontend_identity);
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Title);
    let previous_reads = session.vm.state().read_message_identities.clone();
    let previous_tick = session.vm.state().fixed_tick;
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().global_variables["custom"], 2);
    assert_eq!(session.vm.state().read_message_identities, previous_reads);
    assert!(session.vm.state().fixed_tick > previous_tick);
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Title);
    session
        .advance(0, &[key(KeyCode::ArrowUp), key(KeyCode::Enter)])
        .unwrap();
    assert!(session.finished);
    Box::new(session).close().unwrap();
}

#[test]
fn native_gpu_lowercase_title_without_texture_overrides_starts_and_enters_story() {
    let _lock = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    game_with_lowercase_title(root.path());
    omit_texture_overrides(root.path());
    let mut provider = fixture::provider();
    let (_, mut session) = provider.open_session(title_request(root.path())).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Title);
    assert_eq!(&session.scene.pixels[..4], &[20, 30, 40, 255]);
    session.advance(500_000_000, &[]).unwrap();
    session
        .advance(0, &[RuntimeEvent::PointerMove { x: 1100.0, y: 80.0 }])
        .unwrap();
    session.advance(0, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Load);
    session.advance(0, &[key(KeyCode::Escape)]).unwrap();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "First");
    Box::new(session).close().unwrap();
}

#[test]
fn native_gpu_session_container_images_without_texture_overrides_render() {
    let _lock = PROVIDER_SESSION.lock();
    for (name, payload) in [
        ("BG.ANI", single_pixel_ani()),
        ("BG.SQZ", single_pixel_sqz()),
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = format!(".stage * {name} 2 3\r\n.end\r\n");
        fixture::game(root.path(), source.as_bytes());
        fixture::asset(root.path(), "bg", name, &payload);
        omit_texture_overrides(root.path());
        let mut provider = fixture::provider();
        let mut opened = provider
            .open(request(root.path(), Sink::default()))
            .unwrap();
        opened.session.advance(16_666_667, &[]).unwrap();
        let mut frame = Capture(Vec::new());
        frame
            .read(opened.session.acquire_frame(None).unwrap())
            .unwrap();
        let pixel = (3 * 1280 + 2) * 4;
        assert_eq!(&frame.0[pixel..pixel + 4], &[224, 96, 32, 255]);
        opened.session.close().unwrap();
    }
}

#[test]
fn native_gpu_title_config_returns_and_missing_art_fails_explicitly() {
    let _lock = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    game(root.path());
    let mut provider = fixture::provider();
    let (_, mut session) = provider.open_session(title_request(root.path())).unwrap();
    session
        .advance(
            0,
            &[
                key(KeyCode::ArrowDown),
                key(KeyCode::ArrowDown),
                key(KeyCode::Enter),
            ],
        )
        .unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Config);
    session.advance(0, &[key(KeyCode::Escape)]).unwrap();
    assert_eq!(session.vm.state().system_ui.page, MusicaSystemPage::Title);
    Box::new(session).close().unwrap();
    let missing = tempfile::tempdir().unwrap();
    fixture::game(missing.path(), b".end\r\n");
    assert!(
        provider
            .open_session(title_request(missing.path()))
            .is_err()
    );
    let opened = provider
        .open(request(missing.path(), Sink::default()))
        .unwrap();
    opened.session.close().unwrap();
}

#[test]
fn native_gpu_unknown_release_uses_frontend_data_and_opens_without_optional_archives() {
    let _lock = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".end\r\n");
    fixture::assets(
        root.path(),
        "scr",
        &[
            (
                "opening.sc",
                b".message 1 visitor-001.ogg Guest Welcome\r\n.end\r\n",
            ),
            ("memory.sc", b".message 1   Memory\r\n.end\r\n"),
            ("memory-index.sc", b".chain memory.sc\r\n.end\r\n"),
            (
                "movie-wrapper.sc",
                b".movie 1 finale.avi 640 360 t\r\n.end\r\n",
            ),
        ],
    );
    fixture::assets(
        root.path(),
        "sys",
        &[
            ("welcome.png", &png(640, 360, [30, 60, 90, 255])),
            ("highlight.png", &png(640, 360, [100, 150, 200, 255])),
        ],
    );
    fixture::asset(root.path(), "voice", "visitor-001.ogg", &fixture::wave());
    let mut profile =
        shinki_emu_musica_keys::load_musica_key_file(&root.path().join(MUSICA_KEY_FILE)).unwrap();
    for role in ["bg", "bgm", "st", "se", "mov"] {
        profile.archive_keys.get_mut(role).unwrap().required = false;
        std::fs::remove_file(root.path().join(format!("{role}.paz"))).unwrap();
    }
    std::fs::write(
        root.path().join(MUSICA_KEY_FILE),
        toml::to_string(&profile).unwrap(),
    )
    .unwrap();
    assert!(crate::provider::probe_musica(root.path()));
    let data = serde_json::json!({
        "titles":[{"background":"musica:/sys/welcome.png","hover":"musica:/sys/highlight.png","buttons":[
            {"rect":[10,20,120,40],"action":"new_game"},
            {"rect":[10,80,120,40],"action":"memories"},
            {"rect":[10,140,120,40],"action":"exit"}
        ]}],
        "replay_index":"musica:/scr/memory-index.sc"
    });
    let mut req = request(root.path(), Sink::default());
    req.configuration[0].value = ConfigValue::Enum("title".into());
    req.configuration.extend([
        ConfigEntry {
            id: "entry_script".into(),
            value: ConfigValue::String("opening.sc".into()),
        },
        ConfigEntry {
            id: "logical_width".into(),
            value: ConfigValue::Integer(640),
        },
        ConfigEntry {
            id: "logical_height".into(),
            value: ConfigValue::Integer(360),
        },
        ConfigEntry {
            id: "frontend".into(),
            value: ConfigValue::String(data.to_string()),
        },
    ]);
    let mut provider = fixture::provider();
    let (_, mut session) = provider.open_session(req).unwrap();
    assert_eq!(session.vm.config().character_voices[0].prefix, "visitor");
    assert_eq!(
        session
            .vm
            .gallery_entries(&MusicaSystemPage::GalleryReplay)
            .next()
            .unwrap()
            .resource,
        "musica:/scr/memory.sc"
    );
    assert!(
        session
            .vm
            .gallery_entries(&MusicaSystemPage::GalleryMovie)
            .next()
            .is_none()
    );
    assert_eq!(&session.scene.pixels[..4], &[30, 60, 90, 255]);
    session
        .advance(
            0,
            &[
                RuntimeEvent::PointerMove { x: 20.0, y: 90.0 },
                RuntimeEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Pressed,
                },
            ],
        )
        .unwrap();
    assert_eq!(
        session.vm.state().system_ui.page,
        MusicaSystemPage::Memories
    );
    assert_eq!(session.vm.gallery_pages().count(), 2); // replay, return
    session.advance(0, &[key(KeyCode::Enter)]).unwrap();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "Memory");
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "Welcome");
    Box::new(session).close().unwrap();
    std::fs::remove_file(root.path().join("scr.paz")).unwrap();
    assert!(!crate::provider::probe_musica(root.path()));
    let failure = crate::mount_musica(root.path(), std::path::Path::new(MUSICA_KEY_FILE))
        .err()
        .unwrap();
    assert_eq!(failure.code(), "SHINKI_EMU_MUSICA_ARCHIVE_MISSING");
    assert!(failure.message().contains("scr"));
}
