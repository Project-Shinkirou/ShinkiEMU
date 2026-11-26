//! The desktop 打开光驱 toolbar action and its disc choice dialog, driven
//! through the real `ManagerWindow` with the software renderer.
//!
//! Checks that the action is absent while the host leaves
//! `Library.optical-available` false (mobile), that a click on it reaches
//! `Library.open-optical`, that it waits while the host lists discs, that a
//! picked row reports its index and that the dialog closes through
//! `Library.close-optical`. Set `SHINKI_UI_SCREENSHOT_DIR` to keep PNGs in
//! zh-CN, en and ja at 1280x800 and 360x780. Volume names are synthetic.

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use shinki_emu_manager_ui_slint::{
    Choice, GameCardViewModel, Library, ManagerViewModel, Notices, SlintManagerAdapter, i18n,
    report_error, tr,
};
use slint::{
    ComponentHandle, LogicalPosition, ModelRc, SharedString, VecModel,
    platform::{
        Platform, PointerEventButton, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
    private_unstable_api::re_exports::{AccessibleStringProperty, ItemRc, Key, WindowInner},
};

struct SoftwarePlatform {
    window: Rc<MinimalSoftwareWindow>,
    start: Instant,
}

impl Platform for SoftwarePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
    fn duration_since_start(&self) -> Duration {
        self.start.elapsed()
    }
}

fn render(software: &MinimalSoftwareWindow, width: u32, height: u32) -> Vec<u8> {
    software.set_size(slint::PhysicalSize::new(width, height));
    software.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    software.dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(width as f32, height as f32),
    });
    let mut pixels = vec![slint::Rgb8Pixel { r: 0, g: 0, b: 0 }; width as usize * height as usize];
    for frame in 0..120 {
        slint::platform::update_timers_and_animations();
        software.request_redraw();
        software.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        });
        if frame >= 2 && !software.window().has_active_animations() {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    pixels.iter().flat_map(|p| [p.r, p.g, p.b, 255]).collect()
}

fn labeled_item(software: &MinimalSoftwareWindow, label: &str) -> Option<ItemRc> {
    let window = WindowInner::from_pub(software.window());
    let mut stack = vec![window.window_item_rc()?];
    while let Some(item) = stack.pop() {
        stack.extend(std::iter::successors(
            item.first_child(),
            ItemRc::next_sibling,
        ));
        if item.is_visible()
            && item
                .accessible_string_property(AccessibleStringProperty::Label)
                .is_some_and(|text| text.as_str() == label)
        {
            return Some(item);
        }
    }
    None
}

fn center(software: &MinimalSoftwareWindow, label: &str) -> LogicalPosition {
    let item = labeled_item(software, label)
        .unwrap_or_else(|| panic!("visible control not found: {label}"));
    let center = item.map_to_native_window(item.geometry().center());
    LogicalPosition::new(center.x, center.y)
}

fn click(software: &MinimalSoftwareWindow, label: &str) -> LogicalPosition {
    let position = center(software, label);
    software.dispatch_event(WindowEvent::PointerMoved { position });
    software.dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    software.dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
    software.dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(0.0, 0.0),
    });
    position
}

fn press(software: &MinimalSoftwareWindow, key: impl Into<SharedString>) {
    let text = key.into();
    software.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    software.dispatch_event(WindowEvent::KeyReleased { text });
}

fn games() -> Vec<GameCardViewModel> {
    [
        ("g1", "星降る坂道の約束", "Studio Hoshizora"),
        (
            "g2",
            "Lanterns Over the Quiet Harbor: Extended Memorial Edition",
            "Tidewater Works",
        ),
        ("g3", "白い栞", "Shiori Soft"),
        ("g4", "夏の終わりの手紙", "Kaze Lab"),
    ]
    .into_iter()
    .map(|(id, title, subtitle)| GameCardViewModel {
        id: id.into(),
        title: title.into(),
        subtitle: subtitle.into(),
        ready: true,
        ..Default::default()
    })
    .collect()
}

/// (volume root, volume name): one unlabeled volume and one long name.
const DISCS: [(&str, &str); 3] = [
    ("D:\\", "SHINKI_DISC1"),
    ("E:\\", ""),
    ("F:\\", "MEMORIAL_EDITION_INSTALL_AND_DATA_DISC_2_OF_2"),
];

fn choices() -> ModelRc<Choice> {
    ModelRc::new(VecModel::from(
        DISCS
            .iter()
            .map(|(root, label)| Choice {
                id: (*root).into(),
                label: (*label).into(),
            })
            .collect::<Vec<_>>(),
    ))
}

#[test]
fn optical_action_and_choice_dialog() {
    let software = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(SoftwarePlatform {
        window: software.clone(),
        start: Instant::now(),
    }))
    .expect("software platform");
    let adapter = SlintManagerAdapter::new().expect("manager window");
    let window = adapter.window();
    let library = window.global::<Library>();

    let opened = Rc::new(Cell::new(0));
    let added = Rc::new(Cell::new(0));
    let imported = Rc::new(RefCell::new(Vec::new()));
    let closed = Rc::new(Cell::new(0));
    {
        let opened = opened.clone();
        library.on_open_optical(move || opened.set(opened.get() + 1));
        let added = added.clone();
        library.on_add_directory(move || added.set(added.get() + 1));
        let imported = imported.clone();
        library.on_import_optical(move |index| imported.borrow_mut().push(index));
        // Like the host: closing replaces the snapshot with nothing.
        let closed = closed.clone();
        let weak = window.as_weak();
        library.on_close_optical(move || {
            closed.set(closed.get() + 1);
            if let Some(window) = weak.upgrade() {
                window
                    .global::<Library>()
                    .set_optical_choices(ModelRc::default());
            }
        });
    }

    let output = std::env::var_os("SHINKI_UI_SCREENSHOT_DIR").map(PathBuf::from);
    if let Some(dir) = &output {
        std::fs::create_dir_all(dir).expect("screenshot dir");
    }
    let save = |id: &str, language: &str, (width, height): (u32, u32), rgba: Vec<u8>| {
        if let Some(dir) = &output {
            image::RgbaImage::from_raw(width, height, rgba)
                .expect("frame size")
                .save(dir.join(format!("optical-{id}-{language}-{width}x{height}.png")))
                .expect("write png");
        }
    };

    window.show().expect("show");
    for language in ["zh-CN", "en", "ja"] {
        i18n::apply(language);
        let mut model = ManagerViewModel::default();
        model.app.appearance.language = language.into();
        model.app.appearance.theme_mode = "dark".into();
        model.app.appearance.reduce_motion = true;
        model.library.prepare_state = "ready".into();
        model.library.games = games();
        adapter.apply(&model);
        let action = tr!("打开光驱");
        let add_folder = tr!("添加文件夹");
        for size in [(1280, 800), (360, 780)] {
            let (width, height) = size;
            let at = format!("{language} {width}x{height}");

            // Mobile: the host leaves the action unavailable and the toolbar as before.
            library.set_optical_available(false);
            library.set_optical_listing(false);
            library.set_optical_choices(ModelRc::default());
            let mobile = render(&software, width, height);
            assert!(
                labeled_item(&software, action).is_none(),
                "{at}: action shown on mobile"
            );
            assert!(
                labeled_item(&software, add_folder).is_some(),
                "{at}: 添加文件夹 missing"
            );
            save("toolbar-mobile", language, size, mobile.clone());

            // Desktop: the action sits in the same row, right of 添加文件夹.
            library.set_optical_available(true);
            let desktop = render(&software, width, height);
            assert!(desktop != mobile, "{at}: action did not change the toolbar");
            let disc = center(&software, action);
            let folder = center(&software, add_folder);
            assert!(
                (disc.y - folder.y).abs() < 1.0,
                "{at}: action left the toolbar row"
            );
            assert!(disc.x > folder.x, "{at}: action not after 添加文件夹");
            save("toolbar-desktop", language, size, desktop);
            let before = (opened.get(), added.get());
            click(&software, action);
            assert_eq!(
                opened.get(),
                before.0 + 1,
                "{at}: click did not reach open-optical"
            );
            assert_eq!(added.get(), before.1, "{at}: click also hit 添加文件夹");

            // While the host lists discs a second click waits.
            library.set_optical_listing(true);
            save("listing", language, size, render(&software, width, height));
            click(&software, action);
            assert_eq!(
                opened.get(),
                before.0 + 1,
                "{at}: listing accepted a second click"
            );
            library.set_optical_listing(false);

            // Several discs: rows show the name with the path, or the path alone.
            library.set_optical_choices(choices());
            let dialog = render(&software, width, height);
            assert!(
                labeled_item(&software, tr!("选择光盘")).is_some(),
                "{at}: no dialog title"
            );
            for (root, label) in DISCS {
                let row = if label.is_empty() { root } else { label };
                assert!(
                    labeled_item(&software, row).is_some(),
                    "{at}: row {row} missing"
                );
            }
            save("choices", language, size, dialog);
            imported.borrow_mut().clear();
            click(&software, DISCS[1].0);
            assert_eq!(
                *imported.borrow(),
                [1],
                "{at}: picked row reported a different index"
            );
            let closes = closed.get();
            click(&software, tr!("取消"));
            assert_eq!(closed.get(), closes + 1, "{at}: 取消 did not close");
            render(&software, width, height);
            assert!(
                labeled_item(&software, DISCS[0].1).is_none(),
                "{at}: dialog stayed open"
            );
            library.set_optical_choices(choices());
            render(&software, width, height);
            press(&software, Key::Escape);
            assert_eq!(closed.get(), closes + 2, "{at}: Escape did not close");
            render(&software, width, height);
            assert!(
                imported.borrow().len() == 1,
                "{at}: closing imported a disc"
            );

            // No disc and replaced disc use the host notice path.
            for (id, code, title) in [
                (
                    "no-media",
                    "SHINKI_EMU_OPTICAL_NO_MEDIA",
                    tr!("没有找到光盘"),
                ),
                (
                    "replaced",
                    "SHINKI_EMU_OPTICAL_MEDIA_UNAVAILABLE: Reinsert the original disc or open the currently mounted disc from the library.",
                    tr!("光盘已更换或取出"),
                ),
            ] {
                report_error(window, code);
                let frame = render(&software, width, height);
                assert!(
                    labeled_item(&software, title).is_some(),
                    "{at}: {id} notice missing"
                );
                save(id, language, size, frame);
                let notice = code.split(':').next().unwrap_or(code);
                window
                    .global::<Notices>()
                    .invoke_dismiss(format!("host:{notice}").into());
            }
        }
    }
    window.hide().expect("hide");
}
