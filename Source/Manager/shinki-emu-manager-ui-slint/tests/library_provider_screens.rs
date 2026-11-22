//! Android provider-only folders in the library: cards carry a status icon
//! instead of repeating the cause, and the selected game's detail panel states
//! the cause. Only a provider that all-files access can never map gets a
//! definite note; no grant button or granted wording is inferred.
//!
//! Renders the real `ManagerWindow` through the software renderer with a
//! 15-game library of mixed launch states and runs the assertions for every
//! drawn combination. Set `SHINKI_UI_SCREENSHOT_DIR` to keep representative
//! `<dir>/library-provider/<state>-<theme>-<language>-<width>x<height>.png`
//! (zh-CN light 360, en dark 1280, ja dark 360, ja light 1280); without it
//! dark ja is checked at both sizes. All titles and covers are synthetic.

use std::{
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use i_slint_core::items::AccessibleRole;
use shinki_emu_manager_ui_slint::{
    App, GameCardViewModel, Library, ManagerViewModel, MaterialWindowAdapter,
    SelectedGameViewModel, Settings, SlintManagerAdapter, i18n, tr,
};
use slint::{
    ComponentHandle,
    platform::{
        EventLoopProxy, Platform, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
    private_unstable_api::re_exports::{AccessibleStringProperty, ItemRc, WindowInner},
};

type Job = Box<dyn FnOnce() + Send>;

/// Cover decoding reports back through `invoke_from_event_loop`.
static JOBS: std::sync::Mutex<Vec<Job>> = std::sync::Mutex::new(Vec::new());

struct Proxy;

impl EventLoopProxy for Proxy {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
        Ok(())
    }
    fn invoke_from_event_loop(&self, event: Job) -> Result<(), slint::EventLoopError> {
        JOBS.lock().expect("job queue").push(event);
        Ok(())
    }
}

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
    fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
        Some(Box::new(Proxy))
    }
}

fn render_once(software: &MinimalSoftwareWindow, width: u32, height: u32) -> Vec<u8> {
    software.set_size(slint::PhysicalSize::new(width, height));
    software.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    software.dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(width as f32, height as f32),
    });
    let mut pixels = vec![slint::Rgb8Pixel { r: 0, g: 0, b: 0 }; width as usize * height as usize];
    for frame in 0..120 {
        let jobs = std::mem::take(&mut *JOBS.lock().expect("job queue"));
        for job in jobs {
            job();
        }
        slint::platform::update_timers_and_animations();
        software.request_redraw();
        software.draw_if_needed(|renderer| renderer.render(&mut pixels, width as usize));
        let idle = JOBS.lock().expect("job queue").is_empty();
        if frame >= 8 && idle && !software.window().has_active_animations() {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    pixels.iter().flat_map(|p| [p.r, p.g, p.b, 255]).collect()
}

/// Covers decode off-thread in any order; capture once the frame stops changing.
fn render(software: &MinimalSoftwareWindow, width: u32, height: u32) -> Vec<u8> {
    let mut last = render_once(software, width, height);
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(25));
        let next = render_once(software, width, height);
        if next == last && JOBS.lock().expect("job queue").is_empty() {
            break;
        }
        last = next;
    }
    last
}

fn write_cover(dir: &Path, name: &str, a: [u8; 3]) -> String {
    let path = dir.join(format!("{name}.png"));
    let image = image::RgbaImage::from_fn(240, 320, |x, y| {
        let t = (x + y) as f32 / (240 + 320) as f32;
        let mix = |i: usize| (a[i] as f32 * (1.0 - t) + 24.0 * t) as u8;
        image::Rgba([mix(0), mix(1), mix(2), 255])
    });
    image.save(&path).expect("write cover");
    path.to_string_lossy().into_owned()
}

const LONG_JA: &str =
    "とても長いタイトルのビジュアルノベル　～星降る夜に交わした約束と、もう一つの物語～　完全版";
const LONG_EN: &str =
    "The Extraordinarily Long Chronicle of the Lighthouse Keeper Who Waited Through Every Winter";

fn provider_problem() -> String {
    tr!("此游戏文件夹由系统文件提供程序管理，当前核心无法直接读取。").into()
}

/// (id, title, engine, version, cover index, problem, provider access, play time, compat)
type Row = (&'static str, &'static str, &'static str, &'static str, Option<usize>, String, &'static str, &'static str, &'static str);

fn rows() -> Vec<Row> {
    let saf = provider_problem;
    vec![
        ("g01", "Harbor Lights", "SiglusEngine · siglus-rs", "", Some(0), String::new(), "", "12.5 h", "perfect"),
        ("g02", "Snowfield Letters", "KiriKiri Z · krkr2", "Steam", None, saf(), "provider", "45 h", "completable"),
        ("g03", "Clockwork Garden", "SiglusEngine", "", Some(1), tr!("已识别引擎，缺少兼容核心").into(), "", "", "unknown"),
        ("g04", LONG_JA, "Artemis · artemis-rs", "初回限定版", Some(2), saf(), "provider", "1 h", "flawed"),
        ("g05", "Ten", "", "", None, tr!("未识别到可用的模拟器").into(), "", "", "unknown"),
        ("g06", "Moonlit Station", "FVP · rfvp", "", Some(3), saf(), "unmappable", "65 h", "perfect"),
        ("g07", "Paper Wings", "CatSystem2 · cs2", "", None, String::new(), "", "42 h", "boot_only"),
        ("g08", LONG_EN, "Ren'Py · renpy", "Definitive Edition", Some(4), saf(), "unmappable", "", "completable"),
        ("g09", "Stardust", "KiriKiri Z · krkr2", "HD", None, tr!("缺少所选核心，请安装后重启应用").into(), "", "5 h", "unplayable"),
        ("g10", "Quiet Library", "SiglusEngine · siglus-rs", "", Some(0), String::new(), "", "120 h", "perfect"),
        ("g11", "Glass Ocean", "Musica · musica", "", None, saf(), "provider", "95 h", "unknown"),
        ("g12", "Autumn Bells", "FVP · rfvp", "Director's Cut", Some(1), String::new(), "", "38 h", "perfect"),
        ("g13", "Lantern Festival", "KiriKiri Z · krkr2", "", Some(2), tr!("游戏目录当前不可用").into(), "", "52 h", "completable"),
        ("g14", "Summer Rain", "Artemis · artemis-rs", "Remastered", Some(3), String::new(), "", "80 h", "flawed"),
        ("g15", "Night Ferry", "", "", None, saf(), "unmappable", "", "unknown"),
    ]
}

fn library(covers: &[String], theme: &str) -> ManagerViewModel {
    let mut m = ManagerViewModel::default();
    m.app.version = "0.1.0-beta.9".into();
    m.app.appearance.theme_mode = theme.into();
    m.app.appearance.reduce_motion = true;
    m.library.prepare_state = "ready".into();
    m.library.can_refresh = true;
    m.library.prepare_label = tr!("刷新资料库").into();
    m.settings.roots = vec!["content://com.android.externalstorage.documents/tree/primary%3AGames".into()];
    m.library.games = rows()
        .into_iter()
        .map(|(id, title, engine, version, cover, problem, access, play_time, compat)| GameCardViewModel {
            id: id.into(),
            title: title.into(),
            subtitle: engine.into(),
            version: version.into(),
            cover_uri: cover.map(|i| covers[i].clone()).unwrap_or_default(),
            ready: problem.is_empty(),
            problem,
            provider_access: access.into(),
            play_time: play_time.into(),
            last_played: String::new(),
            compat: compat.into(),
        })
        .collect();
    m
}

fn select(m: &mut ManagerViewModel, id: &str) {
    let game = m.library.games.iter().find(|game| game.id == id).expect("fixture game").clone();
    m.library.selected_id = id.into();
    m.selection.game = SelectedGameViewModel {
        id: game.id,
        title: game.title,
        engine: game.subtitle,
        launchable: game.ready,
        problem: game.problem,
        provider_access: game.provider_access,
        play_time: game.play_time,
        ..Default::default()
    };
}

fn labeled(software: &MinimalSoftwareWindow, label: &str) -> Vec<ItemRc> {
    let window = WindowInner::from_pub(software.window());
    let mut stack = vec![window.window_item_rc().expect("window item")];
    let mut found = Vec::new();
    while let Some(item) = stack.pop() {
        stack.extend(std::iter::successors(item.first_child(), ItemRc::next_sibling));
        if item.is_visible()
            && item
                .accessible_string_property(AccessibleStringProperty::Label)
                .is_some_and(|text| text.as_str() == label)
        {
            found.push(item);
        }
    }
    found
}

/// Cards in the viewport show the provider cause only as an icon's accessible name,
/// while a missing-core card keeps its text. `labeled` sees only unclipped items.
fn assert_cards(software: &MinimalSoftwareWindow, context: &str) {
    let cause = labeled(software, &provider_problem());
    assert!(!cause.is_empty(), "{context}: provider-only cards in view carry a status icon");
    assert!(
        cause.iter().all(|item| item.accessible_role() == AccessibleRole::Image),
        "{context}: provider cause must not be repeated as card text"
    );
    let missing = tr!("已识别引擎，缺少兼容核心");
    assert!(
        labeled(software, missing).iter().any(|item| item.accessible_role() == AccessibleRole::Text),
        "{context}: {missing} keeps its card text"
    );
}

#[test]
fn provider_only_games_use_card_icon_and_detail_cause() {
    let software = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(SoftwarePlatform { window: software.clone(), start: Instant::now() }))
        .expect("software platform");
    let adapter = SlintManagerAdapter::new().expect("manager window");
    let window = adapter.window();
    let covers_dir = tempfile::tempdir().expect("cover dir");
    let covers: Vec<String> = [[60, 90, 160], [170, 70, 90], [40, 140, 120], [190, 140, 60], [110, 80, 170]]
        .iter()
        .enumerate()
        .map(|(i, &color)| write_cover(covers_dir.path(), &format!("cover{i}"), color))
        .collect();
    let output = std::env::var_os("SHINKI_UI_SCREENSHOT_DIR").map(PathBuf::from);
    let combinations: &[(&str, &str, (u32, u32))] = if output.is_some() {
        &[
            ("zh-CN", "light", (360, 780)),
            ("en", "dark", (1280, 800)),
            ("ja", "dark", (360, 780)),
            ("ja", "light", (1280, 800)),
        ]
    } else {
        &[("ja", "dark", (360, 780)), ("ja", "dark", (1280, 800))]
    };
    let settings = window.global::<Settings>();
    window.show().expect("show");

    for &(language, theme, (width, height)) in combinations {
        i18n::apply(language);
        let allow = tr!("允许访问共享存储");
        let granted = tr!("已允许访问共享存储");
        let unmappable = tr!("共享存储权限不适用于这个文件提供程序。");
        let install = tr!("安装核心");
        let context = format!("{theme} {language} {width}x{height}");
        window.global::<MaterialWindowAdapter>().set_touch(width < 600);
        let shown = |label: &str| !labeled(&software, label).is_empty();
        // Cards name the cause only on their icon; the detail panel writes it out.
        let cause_text = || {
            labeled(&software, &provider_problem()).iter().any(|item| item.accessible_role() == AccessibleRole::Text)
        };
        let no_grant_claim = || !shown(allow) && !shown(granted);
        let save = |state: &str, rgba: Vec<u8>| {
            let Some(dir) = &output else { return };
            let dir = dir.join("library-provider");
            std::fs::create_dir_all(&dir).expect("screenshot dir");
            image::RgbaImage::from_raw(width, height, rgba)
                .expect("frame size")
                .save(dir.join(format!("{state}-{theme}-{language}-{width}x{height}.png")))
                .expect("write png");
        };
        let show = |id: Option<&str>, view: &str| {
            let mut m = library(&covers, theme);
            m.app.appearance.view_mode = view.into();
            if let Some(id) = id {
                select(&mut m, id);
            }
            window.global::<Library>().set_detail_open(false);
            adapter.apply(&ManagerViewModel::default());
            adapter.apply(&m);
            window.global::<App>().set_page("library".into());
            window.global::<Library>().set_cover_loading(true);
            window.global::<Library>().set_detail_open(id.is_some());
            render(&software, width, height)
        };
        settings.set_android_storage_access_available(true);
        settings.set_android_storage_access_granted(false);

        for view in ["grid", "list"] {
            let rgba = show(None, view);
            assert_cards(&software, &format!("{view} {context}"));
            save(view, rgba);
        }

        // External-storage provider: whether all-files access would map it is unknown here.
        let rgba = show(Some("g04"), "grid");
        assert!(cause_text(), "{context}: detail states the cause");
        assert!(no_grant_claim() && !shown(unmappable), "{context}: undetermined mapping shows the cause only");
        save("detail-provider", rgba);
        settings.set_android_storage_access_granted(true);
        render(&software, width, height);
        assert!(no_grant_claim() && !shown(unmappable), "{context}: a granted permission is not claimed to help");
        settings.set_android_storage_access_granted(false);

        let rgba = show(Some("g08"), "grid");
        assert!(cause_text(), "{context}: unmappable cause");
        assert!(shown(unmappable) && no_grant_claim(), "{context}: definite note, no grant");
        save("detail-unmappable", rgba);

        let rgba = show(Some("g03"), "grid");
        assert!(shown(install), "{context}: missing core keeps its install action");
        assert!(!cause_text() && !shown(unmappable) && no_grant_claim(), "{context}: missing core is not a provider state");
        save("detail-missing-core", rgba);

        // Before Android 11 (or off Android) there is no all-files permission to mention.
        settings.set_android_storage_access_available(false);
        show(Some("g08"), "grid");
        assert!(cause_text(), "{context}: cause without permission support");
        assert!(!shown(unmappable) && no_grant_claim(), "{context}: no permission note");
    }
    settings.set_android_storage_access_available(false);
    window.global::<MaterialWindowAdapter>().set_touch(false);
    window.hide().expect("hide");
}
