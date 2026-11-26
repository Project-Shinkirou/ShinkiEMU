//! Exercises pointer events on both live editors and renders the actual ManagerWindow.
//! `SHINKI_UI_SCREENSHOT_DIR` retains the PNGs for every locale and viewport.

use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Instant};

use shinki_emu_manager_ui_slint::{
    App, ConfigFieldViewModel, FilterStageViewModel, Filters, FiltersViewModel, ManagerViewModel,
    Session, SlintManagerAdapter, i18n, tr,
};
use slint::{
    ComponentHandle, LogicalPosition, LogicalSize, PhysicalSize, Rgb8Pixel, SharedString,
    platform::{
        Key, Platform, PointerEventButton, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
};

struct SoftwarePlatform(Rc<MinimalSoftwareWindow>, Instant);

impl Platform for SoftwarePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }

    fn duration_since_start(&self) -> std::time::Duration {
        self.1.elapsed()
    }
}

#[derive(Debug, PartialEq)]
enum Action {
    Select(i32),
    Move(i32, i32),
    Remove(i32),
    Add(SharedString),
    Replace(SharedString),
    LoadSource,
    Parameter(SharedString, SharedString),
    Reset,
}

fn record_actions(filters: &Filters<'_>) -> Rc<RefCell<Vec<Action>>> {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let log = actions.clone();
    filters.on_select_stage(move |index| log.borrow_mut().push(Action::Select(index)));
    let log = actions.clone();
    filters.on_move_stage(move |from, to| log.borrow_mut().push(Action::Move(from, to)));
    let log = actions.clone();
    filters.on_remove_stage(move |index| log.borrow_mut().push(Action::Remove(index)));
    let log = actions.clone();
    filters.on_add_stage(move |id| log.borrow_mut().push(Action::Add(id)));
    let log = actions.clone();
    filters.on_set_preset(move |id| log.borrow_mut().push(Action::Replace(id)));
    let log = actions.clone();
    filters.on_load_source(move || log.borrow_mut().push(Action::LoadSource));
    let log = actions.clone();
    filters
        .on_field_changed(move |key, value| log.borrow_mut().push(Action::Parameter(key, value)));
    let log = actions.clone();
    filters.on_reset_parameters(move || log.borrow_mut().push(Action::Reset));
    actions
}

fn stage(id: &str, label: &str, reason: &str) -> FilterStageViewModel {
    FilterStageViewModel {
        id: id.into(),
        label: label.into(),
        available: reason.is_empty(),
        unavailable_reason: reason.into(),
    }
}

fn catalog(dxc_reason: &str) -> Vec<FilterStageViewModel> {
    vec![
        stage("scale", tr!("平滑放大"), dxc_reason),
        stage("sharpen", tr!("锐化"), dxc_reason),
        stage("lanczos3", "Lanczos3", ""),
        stage("scale2x", "Scale2x", ""),
        stage("anime4k_original_restore", tr!("原版 Anime4K 修复"), ""),
        stage(
            "anime4k_original_x2",
            tr!("原版 Anime4K 修复 + 2× 放大"),
            "",
        ),
        stage("anime4k", tr!("Magpie Anime4K 2×（旧版）"), dxc_reason),
    ]
}

fn field(
    key: &str,
    label: &str,
    value: &str,
    min: f64,
    max: f64,
    step: f64,
) -> ConfigFieldViewModel {
    ConfigFieldViewModel {
        key: key.into(),
        label: label.into(),
        value: value.into(),
        kind: "float".into(),
        min,
        max,
        step,
        ..Default::default()
    }
}

fn click(window: &MinimalSoftwareWindow, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
    window.dispatch_event(WindowEvent::PointerMoved { position });
    window.dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    window.dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
    window.dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(0.0, 0.0),
    });
}

fn press(window: &MinimalSoftwareWindow, key: Key) {
    let text: SharedString = key.into();
    window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    window.dispatch_event(WindowEvent::KeyReleased { text });
}

fn render(window: &MinimalSoftwareWindow, (width, height): (u32, u32)) -> Vec<u8> {
    let mut pixels = vec![Rgb8Pixel::default(); (width * height) as usize];
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        });
    }
    pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect()
}

#[test]
fn live_filter_chain_screens_and_actions() {
    let software = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(SoftwarePlatform(software.clone(), Instant::now())))
        .expect("software platform");
    let adapter = SlintManagerAdapter::new().expect("manager window");
    let window = adapter.window();
    let app = window.global::<App>();
    let session = window.global::<Session>();
    let filters = window.global::<Filters>();
    let actions = record_actions(&filters);
    let output = std::env::var_os("SHINKI_UI_SCREENSHOT_DIR").map(PathBuf::from);
    if let Some(dir) = &output {
        std::fs::create_dir_all(dir).expect("screenshot directory");
    }
    let (languages, sizes): (&[&str], &[(u32, u32)]) = if output.is_some() {
        (&["zh-CN", "en", "ja"], &[(1280, 800), (360, 780)])
    } else {
        (&["en"], &[(360, 780)])
    };
    window.show().expect("show");
    for &language in languages {
        i18n::apply(language);
        for &(width, height) in sizes {
            let size = (width, height);
            software.set_size(PhysicalSize::new(width, height));
            software.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
            software.dispatch_event(WindowEvent::Resized {
                size: LogicalSize::new(width as f32, height as f32),
            });
            for game in [false, true] {
                let surface = if game { "quick-menu" } else { "settings" };
                let case = format!("{surface}/{language}/{width}x{height}");
                let capture =
                    |state: &str| {
                        let rgb = render(&software, size);
                        if let Some(dir) = &output {
                            image::RgbImage::from_raw(width, height, rgb.clone())
                                .expect("frame size")
                                .save(dir.join(format!(
                                    "{surface}-{state}-{language}-{width}x{height}.png"
                                )))
                                .expect("save screenshot");
                        }
                        rgb
                    };
                let expect = |action| assert_eq!(actions.take(), [action], "{case}");
                let mount = || {
                    adapter.set_session_active(false);
                    app.set_page("library".into());
                    render(&software, size);
                    if game {
                        adapter.set_session_active(true);
                        session.set_menu_open(true);
                        session.set_menu_page("filters".into());
                    } else {
                        app.set_page("filters".into());
                    }
                };
                let narrow = width < 600;
                let origin_x = if game {
                    width as f32 - 360.0 + 16.0
                } else if narrow {
                    16.0
                } else {
                    104.0
                };
                let content_width = if game || narrow { 322.0 } else { 760.0 };
                // The quick menu omits the explanatory line below its chain title.
                let toolbar_y = if game {
                    134.0
                } else if narrow {
                    164.0
                } else {
                    172.0
                };
                let mut model = ManagerViewModel::default();
                model.app.appearance.language = language.into();
                model.app.appearance.theme_mode = "dark".into();
                model.app.appearance.reduce_motion = true;
                model.library.prepare_state = "ready".into();
                model.session.phase = "running".into();
                model.filters = FiltersViewModel {
                    preset: "chain".into(),
                    stages: vec![
                        stage("lanczos3", "Lanczos3", ""),
                        stage(
                            "external",
                            match language {
                                "zh-CN" => "手绘背景与字幕叠加画面的轻量柔化和边缘锐化（HLSL）",
                                "ja" => {
                                    "手描き背景と字幕を重ねた映像の輪郭を整えるソフトフィルター（HLSL）"
                                }
                                _ => {
                                    "Soft edge reconstruction for hand-painted backgrounds and subtitles (HLSL)"
                                }
                            },
                            "",
                        ),
                        stage("scale2x", "Scale2x", ""),
                    ],
                    choices: catalog(""),
                    selected_stage: 0,
                    fields: vec![field("filter.scale", tr!("缩放倍率"), "2", 1.0, 4.0, 0.25)],
                    ..Default::default()
                };
                adapter.apply(&model);
                mount();
                let populated = capture("populated");
                click(&software, origin_x + 80.0, toolbar_y + 96.0);
                expect(Action::Select(1));
                // Reflect the committed host view; a stage's parameters must follow selection.
                model.filters.selected_stage = 1;
                model.filters.fields = vec![field(
                    "filter.strength",
                    tr!("锐化强度"),
                    "0.35",
                    0.0,
                    1.0,
                    0.05,
                )];
                adapter.apply(&model);
                render(&software, size);
                click(&software, origin_x + 32.0, toolbar_y);
                expect(Action::Move(1, 0));
                model.filters.stages.swap(0, 1);
                model.filters.selected_stage = 0;
                adapter.apply(&model);
                render(&software, size);
                click(
                    &software,
                    origin_x + content_width * 0.75,
                    toolbar_y + 335.0,
                );
                let changes = actions.take();
                let [Action::Parameter(key, value)] = changes.as_slice() else {
                    panic!("{case}: slider did not issue a parameter edit: {changes:?}");
                };
                assert_eq!(key, "filter.strength", "{case}");
                assert!(
                    value.parse::<f32>().unwrap() > 0.35,
                    "{case}: slider did not increase strength"
                );
                model.filters.fields[0].value = value.to_string();
                adapter.apply(&model);
                let edited = capture("edited");
                assert!(
                    edited != populated,
                    "{case}: reorder and selected parameters did not render"
                );
                click(&software, origin_x + 32.0, toolbar_y);
                assert!(actions.take().is_empty(), "{case}: first stage can move up");
                click(&software, origin_x + 80.0, toolbar_y);
                expect(Action::Move(0, 1));
                click(&software, origin_x + 128.0, toolbar_y);
                expect(Action::Remove(0));
                click(&software, origin_x + 64.0, toolbar_y + 392.0);
                expect(Action::Reset);
                click(&software, origin_x + 64.0, toolbar_y + 216.0);
                let catalog_open = render(&software, size);
                let position =
                    LogicalPosition::new(origin_x + content_width / 2.0, toolbar_y + 360.0);
                software.dispatch_event(WindowEvent::PointerScrolled {
                    position,
                    delta_x: 0.0,
                    delta_y: -820.0,
                });
                let scrolled = capture("catalog-scrolled");
                assert!(
                    scrolled != catalog_open,
                    "{case}: wheel could not reveal the last catalog items"
                );
                software.dispatch_event(WindowEvent::PointerScrolled {
                    position,
                    delta_x: 0.0,
                    delta_y: 820.0,
                });
                render(&software, size);
                click(&software, origin_x + 70.0, toolbar_y + 290.0);
                expect(Action::LoadSource);

                // Missing DXC blocks only the format-4 entries and source picker.
                let reason = tr!("找不到 DXC，无法编译 HLSL 滤镜。");
                model.filters = FiltersViewModel {
                    choices: catalog(reason),
                    source_unavailable_reason: reason.into(),
                    ..Default::default()
                };
                adapter.apply(&model);
                mount();
                let empty = render(&software, size);
                let action_y = toolbar_y + 58.0;
                click(&software, origin_x + 64.0, action_y);
                let unavailable = capture("unavailable-empty");
                assert!(
                    unavailable != empty,
                    "{case}: opening the add catalog changed no pixels"
                );
                click(&software, origin_x + 70.0, action_y + 72.0);
                assert!(
                    actions.take().is_empty(),
                    "{case}: missing DXC opened the file picker"
                );
                // At compact width, the en/ja DXC reason takes two lines.
                let blocked_rows_wrap = (game || narrow) && language != "zh-CN";
                let lanczos_y = action_y + if blocked_rows_wrap { 258.0 } else { 216.0 };
                click(&software, origin_x + 70.0, lanczos_y);
                expect(Action::Add("lanczos3".into()));

                model.filters.stages.push(model.filters.choices[2].clone());
                model.filters.selected_stage = 0;
                model.filters.preset = "lanczos3".into();
                model.filters.fields =
                    vec![field("filter.scale", tr!("缩放倍率"), "2", 1.0, 4.0, 0.25)];
                adapter.apply(&model);
                render(&software, size);
                let replace_y = toolbar_y + 120.0;
                click(&software, origin_x + content_width - 64.0, replace_y);
                capture("presets");
                click(&software, origin_x + 70.0, replace_y + 96.0);
                expect(Action::Replace("none".into()));
                if game {
                    press(&software, Key::Escape);
                    assert_eq!(
                        session.get_menu_page(),
                        "main",
                        "{case}: Esc must return to the quick menu"
                    );
                }
                eprintln!(
                    "{case}: selected, moved both ways, removed, edited and reset parameters, scrolled, added, loaded HLSL, blocked unavailable HLSL, and replaced chain"
                );
            }
        }
    }
    window.hide().expect("hide");
}
