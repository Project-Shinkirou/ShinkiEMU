//! Comprehensive UX audit: renders all major Manager UI states for visual inspection.
//!
//! Set `SHINKI_UI_SCREENSHOT_DIR` to generate PNGs of every state in zh-CN, en
//! and ja at 1280x800 and 360x780. Screenshots are organized by page/state.

use std::{
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use shinki_emu_manager_ui_slint::{
    App, GameCardViewModel, ManagerViewModel, Onboarding, SlintManagerAdapter, Store, i18n,
};
use slint::{
    ComponentHandle,
    platform::{
        Platform, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
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

/// Screenshot descriptor: (page/state ID, setup function)
type ScreenState = (&'static str, fn(&SlintManagerAdapter));

fn states() -> Vec<ScreenState> {
    vec![
        // Onboarding
        ("onboarding/step0-welcome", |adapter| {
            let mut model = base_model();
            model.resale_notice.acknowledged = false;
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("onboarding".into());
            adapter.window().global::<Onboarding>().set_step(0);
        }),
        ("onboarding/step1-roots", |adapter| {
            let model = base_model();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("onboarding".into());
            adapter.window().global::<Onboarding>().set_step(1);
        }),
        ("onboarding/step2-network", |adapter| {
            let model = base_model();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("onboarding".into());
            adapter.window().global::<Onboarding>().set_step(2);
        }),
        ("onboarding/step3-appearance", |adapter| {
            let model = base_model();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("onboarding".into());
            adapter.window().global::<Onboarding>().set_step(3);
        }),
        ("onboarding/step4-prepare", |adapter| {
            let model = base_model();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("onboarding".into());
            adapter.window().global::<Onboarding>().set_step(4);
        }),
        // Library - empty
        ("library/empty-unprepared", |adapter| {
            let mut model = base_model();
            model.library.prepare_state = "unprepared".into();
            model.library.games = vec![];
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("library".into());
        }),
        ("library/empty-no-roots", |adapter| {
            let mut model = base_model();
            model.library.prepare_state = "ready".into();
            model.library.games = vec![];
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("library".into());
        }),
        // Library - with games (grid)
        ("library/grid-with-games", |adapter| {
            let mut model = base_model();
            model.library.games = sample_games();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("library".into());
        }),
        // Library - list view
        ("library/list-with-games", |adapter| {
            let mut model = base_model();
            model.app.appearance.view_mode = "list".into();
            model.library.games = sample_games();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("library".into());
        }),
        // Library - preparing
        ("library/preparing", |adapter| {
            let mut model = base_model();
            model.library.prepare_state = "scanning".into();
            model.library.prepare_label = "正在扫描".into();
            model.library.prepare_detail = "正在扫描游戏目录...".into();
            model.library.prepare_progress = Some(0.3);
            model.library.can_cancel_prepare = true;
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("library".into());
        }),
        // Library - search no results
        ("library/search-no-results", |adapter| {
            let mut model = base_model();
            model.library.query = "不存在的游戏".into();
            model.library.games = vec![];
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("library".into());
        }),
        // Settings pages
        ("settings/appearance", |adapter| {
            let model = base_model();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("settings".into());
            adapter.window().global::<App>().set_settings_category("appearance".into());
        }),
        ("settings/roots", |adapter| {
            let model = base_model();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("settings".into());
            adapter.window().global::<App>().set_settings_category("library-roots".into());
        }),
        ("settings/network", |adapter| {
            let model = base_model();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("settings".into());
            adapter.window().global::<App>().set_settings_category("network".into());
        }),
        ("settings/input", |adapter| {
            let model = base_model();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("settings".into());
            adapter.window().global::<App>().set_settings_category("input".into());
        }),
        ("settings/about", |adapter| {
            let model = base_model();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("settings".into());
            adapter.window().global::<App>().set_settings_category("about".into());
        }),
        // Patch page
        ("patch/select-game", |adapter| {
            let mut model = base_model();
            model.library.games = sample_games();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("patch".into());
        }),
        // Filters page
        ("filters/main", |adapter| {
            let model = base_model();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("filters".into());
        }),
        // Game settings
        ("game-settings/main", |adapter| {
            let mut model = base_model();
            model.game_settings.game_id = "game1".into();
            model.game_settings.title = "White Album 2".into();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("game-settings".into());
        }),
        // Store - login
        ("store/login", |adapter| {
            let mut model = base_model();
            model.store.provider_name = "Steam".into();
            model.store.can_credentials = true;
            model.store.can_qr = true;
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("add-game".into());
            adapter.window().global::<App>().set_page("store".into());
            adapter.window().global::<Store>().set_login_state("signed-out".into());
        }),
        // Add game
        ("add-game/main", |adapter| {
            let model = base_model();
            adapter.apply(&model);
            adapter.window().global::<App>().set_page("add-game".into());
        }),
    ]
}

fn base_model() -> ManagerViewModel {
    let mut model = ManagerViewModel::default();
    model.app.version = "0.1.0-beta.7".into();
    model.app.appearance.theme_mode = "dark".into();
    model.app.appearance.reduce_motion = true;
    model.resale_notice.acknowledged = true;
    model.library.prepare_state = "ready".into();
    model.network.allowed = true;
    model
}

fn sample_games() -> Vec<GameCardViewModel> {
    vec![
        GameCardViewModel {
            id: "game1".into(),
            title: "White Album 2".into(),
            subtitle: "白色相簿2".into(),
            version: "".into(),
            cover_uri: "".into(),
            ready: true,
            problem: "".into(),
            play_time: "12.5 小时".into(),
            last_played: "2 天前".into(),
            compat: "verified".into(),
        },
        GameCardViewModel {
            id: "game2".into(),
            title: "CLANNAD".into(),
            subtitle: "クラナド".into(),
            version: "Steam 版".into(),
            cover_uri: "".into(),
            ready: true,
            problem: "".into(),
            play_time: "45.2 小时".into(),
            last_played: "1 周前".into(),
            compat: "playable".into(),
        },
        GameCardViewModel {
            id: "game3".into(),
            title: "Steins;Gate".into(),
            subtitle: "命运石之门".into(),
            version: "".into(),
            cover_uri: "".into(),
            ready: false,
            problem: "缺少核心".into(),
            play_time: "".into(),
            last_played: "".into(),
            compat: "unknown".into(),
        },
    ]
}

#[test]
fn comprehensive_ui_audit() {
    let software = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(SoftwarePlatform {
        window: software.clone(),
        start: Instant::now(),
    }))
    .expect("software platform");
    let adapter = SlintManagerAdapter::new().expect("manager window");

    let output = std::env::var_os("SHINKI_UI_SCREENSHOT_DIR")
        .map(PathBuf::from)
        .expect("SHINKI_UI_SCREENSHOT_DIR required for audit");

    let languages = ["zh-CN", "en", "ja"];
    let sizes = [(1280, 800), (360, 780)];

    adapter.window().show().expect("show");

    for &language in &languages {
        i18n::apply(language);
        for &(width, height) in &sizes {
            for (id, setup) in states() {
                setup(&adapter);
                let rgba = render(&software, width, height);

                let dir = output.join(id.split('/').next().unwrap());
                std::fs::create_dir_all(&dir).expect("create screenshot dir");

                let filename = format!(
                    "{}-{}-{}x{}.png",
                    id.replace('/', "-"),
                    language,
                    width,
                    height
                );
                let path = output.join(filename);
                image::RgbaImage::from_raw(width, height, rgba)
                    .expect("frame size")
                    .save(&path)
                    .expect("write png");
            }
        }
    }

    adapter.window().hide().expect("hide");
}
