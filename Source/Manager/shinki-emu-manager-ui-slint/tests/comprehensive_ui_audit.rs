//! Comprehensive UX audit with realistic data and stress tests.

use std::{path::PathBuf, rc::Rc, time::{Duration, Instant}};
use shinki_emu_manager_ui_slint::{
    App, ChoiceViewModel, GameCardViewModel, GameSettingsViewModel, ManagerViewModel,
    Onboarding, PatchGameViewModel, PatchStateViewModel, RecipeViewModel,
    SlintManagerAdapter, Store, StoreGameViewModel, StoreLoadState, StoreViewModel, i18n,
};
use slint::{ComponentHandle, platform::{Platform, WindowAdapter, WindowEvent,
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType}}};

struct SoftwarePlatform { window: Rc<MinimalSoftwareWindow>, start: Instant }
impl Platform for SoftwarePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> { Ok(self.window.clone()) }
    fn duration_since_start(&self) -> Duration { self.start.elapsed() }
}

fn render(software: &MinimalSoftwareWindow, width: u32, height: u32) -> Vec<u8> {
    software.set_size(slint::PhysicalSize::new(width, height));
    software.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    software.dispatch_event(WindowEvent::Resized { size: slint::LogicalSize::new(width as f32, height as f32) });
    let mut pixels = vec![slint::Rgb8Pixel { r: 0, g: 0, b: 0 }; width as usize * height as usize];
    for frame in 0..120 {
        slint::platform::update_timers_and_animations();
        software.request_redraw();
        software.draw_if_needed(|renderer| { renderer.render(&mut pixels, width as usize); });
        if frame >= 2 && !software.window().has_active_animations() { break; }
        std::thread::sleep(Duration::from_millis(16));
    }
    pixels.iter().flat_map(|p| [p.r, p.g, p.b, 255]).collect()
}

type ScreenState = (&'static str, fn(&SlintManagerAdapter));

fn states() -> Vec<ScreenState> {
    vec![
        ("onboarding/step0", |a| {
            let mut m = base(); m.resale_notice.acknowledged = false; a.apply(&m);
            a.window().global::<App>().set_page("onboarding".into());
            a.window().global::<Onboarding>().set_step(0);
        }),
        ("onboarding/step1", |a| { let m = base(); a.apply(&m); a.window().global::<App>().set_page("onboarding".into()); a.window().global::<Onboarding>().set_step(1); }),
        ("onboarding/step2", |a| { let m = base(); a.apply(&m); a.window().global::<App>().set_page("onboarding".into()); a.window().global::<Onboarding>().set_step(2); }),
        ("onboarding/step3", |a| { let m = base(); a.apply(&m); a.window().global::<App>().set_page("onboarding".into()); a.window().global::<Onboarding>().set_step(3); }),
        ("onboarding/step4", |a| { let m = base(); a.apply(&m); a.window().global::<App>().set_page("onboarding".into()); a.window().global::<Onboarding>().set_step(4); }),
        ("library/empty-unprepared", |a| { let mut m = base(); m.library.prepare_state = "unprepared".into(); m.library.games = vec![]; a.apply(&m); a.window().global::<App>().set_page("library".into()); }),
        ("library/empty-no-roots", |a| { let mut m = base(); m.library.games = vec![]; a.apply(&m); a.window().global::<App>().set_page("library".into()); }),
        ("library/grid-games", |a| { let mut m = base(); m.library.games = games(); a.apply(&m); a.window().global::<App>().set_page("library".into()); }),
        ("library/list-games", |a| { let mut m = base(); m.app.appearance.view_mode = "list".into(); m.library.games = games(); a.apply(&m); a.window().global::<App>().set_page("library".into()); }),
        ("library/preparing", |a| {
            let mut m = base(); m.library.prepare_state = "scanning".into(); m.library.prepare_label = "扫描中".into();
            m.library.prepare_detail = "扫描游戏目录...".into(); m.library.prepare_progress = Some(0.3); m.library.can_cancel_prepare = true;
            a.apply(&m); a.window().global::<App>().set_page("library".into());
        }),
        ("library/search-empty", |a| { let mut m = base(); m.library.query = "不存在".into(); m.library.games = vec![]; a.apply(&m); a.window().global::<App>().set_page("library".into()); }),
        ("settings/appearance", |a| { let m = base(); a.apply(&m); a.window().global::<App>().set_page("settings".into()); a.window().global::<App>().set_settings_category("appearance".into()); }),
        ("settings/roots", |a| {
            let mut m = base(); m.settings.roots = vec!["D:/Games".into(), "E:/Gal".into()];
            a.apply(&m); a.window().global::<App>().set_page("settings".into()); a.window().global::<App>().set_settings_category("library-roots".into());
        }),
        ("settings/network", |a| { let m = base(); a.apply(&m); a.window().global::<App>().set_page("settings".into()); a.window().global::<App>().set_settings_category("network".into()); }),
        ("settings/input", |a| { let m = base(); a.apply(&m); a.window().global::<App>().set_page("settings".into()); a.window().global::<App>().set_settings_category("input".into()); }),
        ("settings/about", |a| { let m = base(); a.apply(&m); a.window().global::<App>().set_page("settings".into()); a.window().global::<App>().set_settings_category("about".into()); }),
        ("patch/select", |a| {
            let mut m = base(); m.library.games = games();
            m.patch.games = games().into_iter().map(|g| {
                let linked = g.id == "game1" || g.id == "game2";
                PatchGameViewModel { id: g.id, title: g.title, subtitle: g.subtitle, cover_uri: "".into(), linked }
            }).collect();
            a.apply(&m); a.window().global::<App>().set_page("patch".into());
        }),
        ("patch/recipes", |a| {
            let mut m = base(); m.library.games = games();
            m.patch.game = PatchGameViewModel { id: "game1".into(), title: "WA2".into(), subtitle: "白2".into(), cover_uri: "".into(), linked: true };
            m.patch.recipes = vec![
                RecipeViewModel { id: "p1".into(), title: "官方补丁 1.01".into(), recommended: true },
                RecipeViewModel { id: "p2".into(), title: "汉化补丁 v2.0".into(), recommended: false },
            ];
            a.apply(&m); a.window().global::<App>().set_page("patch".into());
        }),
        ("patch/running", |a| {
            let mut m = base();
            m.patch.game = PatchGameViewModel { id: "game1".into(), title: "WA2".into(), subtitle: "白2".into(), cover_uri: "".into(), linked: true };
            m.patch.state = PatchStateViewModel {
                step: 3, running: true, phase: "应用中...".into(), completed: 3, total: Some(10),
                output: "".into(), error: "".into(), can_start: false, script: "".into(),
                patch_directory: "".into(), output_directory: "".into(), in_place: false,
                has_backup: false, downloads: vec![], checks: vec![], rolled_back: false,
            };
            a.apply(&m); a.window().global::<App>().set_page("patch".into());
        }),
        ("patch/failed", |a| {
            let mut m = base();
            m.patch.game = PatchGameViewModel { id: "game1".into(), title: "WA2".into(), subtitle: "白2".into(), cover_uri: "".into(), linked: true };
            m.patch.state = PatchStateViewModel {
                step: 3, running: false, phase: "".into(), completed: 0, total: None,
                output: "".into(), error: "校验失败：scenario.xp3 哈希不匹配".into(), can_start: true,
                script: "".into(), patch_directory: "".into(), output_directory: "".into(),
                in_place: false, has_backup: false, downloads: vec![], checks: vec![], rolled_back: false,
            };
            a.apply(&m); a.window().global::<App>().set_page("patch".into());
        }),
        ("filters/main", |a| { let m = base(); a.apply(&m); a.window().global::<App>().set_page("filters".into()); }),
        ("game-settings/main", |a| {
            let mut m = base();
            m.game_settings = GameSettingsViewModel {
                game_id: "game1".into(), title: "WA2".into(),
                emulator_choices: vec![
                    ChoiceViewModel { id: "xp3".into(), label: "xp3dec 1.2.0".into() },
                    ChoiceViewModel { id: "krz".into(), label: "KRKrZ 2.3.1".into() },
                ],
                emulator_id: "xp3".into(), ..Default::default()
            };
            a.apply(&m); a.window().global::<App>().set_page("game-settings".into());
        }),
        ("store/login", |a| {
            let mut m = base(); m.store = StoreViewModel { provider_name: "Steam".into(), can_credentials: true, can_qr: true, ..Default::default() };
            a.apply(&m); a.window().global::<App>().set_page("add-game".into());
            a.window().global::<App>().set_page("store".into()); a.window().global::<Store>().set_login_state("signed-out".into());
        }),
        ("store/games", |a| {
            let mut m = base();
            m.store = StoreViewModel {
                provider_name: "Steam".into(), account_name: "user@e.com".into(), library_state: StoreLoadState::Ready,
                games: vec![
                    StoreGameViewModel { title_id: "g1".into(), name: "STEINS;GATE".into(), size_bytes: 2684354560,
                        platforms: vec!["Win".into()], languages: vec!["EN".into(), "JA".into(), "ZH".into()],
                        installed: false, update_available: false, update_unknown: false, unavailable_reason: "".into() },
                    StoreGameViewModel { title_id: "g2".into(), name: "CLANNAD".into(), size_bytes: 5153960756,
                        platforms: vec!["Win".into()], languages: vec!["EN".into(), "JA".into()],
                        installed: true, update_available: false, update_unknown: false, unavailable_reason: "".into() },
                ],
                ..Default::default()
            };
            a.apply(&m); a.window().global::<App>().set_page("store".into());
        }),
        ("add-game/main", |a| { let m = base(); a.apply(&m); a.window().global::<App>().set_page("add-game".into()); }),
    ]
}

fn base() -> ManagerViewModel {
    let mut m = ManagerViewModel::default();
    m.app.version = "0.1.0-beta.7".into(); m.app.appearance.theme_mode = "dark".into();
    m.app.appearance.reduce_motion = true; m.resale_notice.acknowledged = true;
    m.library.prepare_state = "ready".into(); m.network.allowed = true; m
}

fn games() -> Vec<GameCardViewModel> {
    vec![
        gcard("game1", "White Album 2", "白色相簿2", "", true, "", "12.5h", "2d", "verified"),
        gcard("game2", "CLANNAD", "クラナド", "Steam", true, "", "45h", "1w", "playable"),
        gcard("game3", "Steins;Gate", "命运石之门", "", false, "缺核心", "", "", "unknown"),
        gcard("game4", "Fate/stay night", "Fate", "RN", true, "", "80h", "3w", "verified"),
        gcard("game5", "Ever17", "Ever17", "", true, "", "28h", "1mo", "playable"),
        gcard("game6", "Dies irae", "Dies", "AeF", true, "", "65h", "2mo", "verified"),
        gcard("game7", "Muv-Luv Alt", "ML Alt", "", true, "", "42h", "3mo", "verified"),
        gcard("game8", "Rewrite", "リライト", "", true, "", "52h", "4mo", "playable"),
        gcard("game9", "Planetarian", "星の人", "HD", true, "", "5h", "5mo", "verified"),
        gcard("game10", "Higurashi", "ひぐらし", "Hou", true, "", "120h", "6mo", "verified"),
        gcard("game11", "Umineko", "うみねこ", "", true, "", "95h", "7mo", "playable"),
        gcard("game12", "LB!", "リトバス", "Ecstasy", true, "", "38h", "8mo", "verified"),
        gcard("stress_ja", "超長いタイトルのビジュアルノベルゲームでテキストの折り返しと省略記号の動作を確認するためのテストデータ",
            "これは非常に長い副題でレイアウトが崩れないかチェックします", "特別限定版", true, "", "1h", "昨日", "verified"),
        gcard("stress_en", "An Extremely Long Visual Novel Title To Test Text Wrapping And Ellipsis Behavior In The Card Layout Component",
            "This is a very long subtitle to check if the layout breaks", "Def Ed", true, "", "2h", "yesterday", "playable"),
        gcard("stress_zh", "这是一个非常非常非常非常非常非常非常非常非常非常非常非常长的游戏标题用来测试文本折行和省略号显示",
            "超级长的副标题也需要测试看看布局会不会崩溃或者出现意外的换行和截断问题", "完全版", false,
            "模拟器核心不兼容当前版本，请更新到最新版本或联系开发者获取支持", "", "", "unknown"),
    ]
}

fn gcard(id: &str, t: &str, st: &str, v: &str, r: bool, p: &str, pt: &str, lp: &str, c: &str) -> GameCardViewModel {
    GameCardViewModel {
        id: id.into(), title: t.into(), subtitle: st.into(), version: v.into(), cover_uri: "".into(),
        ready: r, problem: p.into(), play_time: pt.into(), last_played: lp.into(), compat: c.into()
    }
}

#[test]
fn comprehensive_ui_audit() {
    let sw = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(SoftwarePlatform { window: sw.clone(), start: Instant::now() })).expect("platform");
    let adapter = SlintManagerAdapter::new().expect("adapter");
    let output = std::env::var_os("SHINKI_UI_SCREENSHOT_DIR").map(PathBuf::from).expect("SHINKI_UI_SCREENSHOT_DIR required");
    adapter.window().show().expect("show");
    for &lang in &["zh-CN", "en", "ja"] {
        i18n::apply(lang);
        for &(w, h) in &[(1280, 800), (360, 780)] {
            for (id, setup) in states() {
                setup(&adapter);
                let rgba = render(&sw, w, h);
                let dir = output.join(id.split('/').next().unwrap());
                std::fs::create_dir_all(&dir).expect("mkdir");
                let path = output.join(format!("{}-{}-{}x{}.png", id.replace('/', "-"), lang, w, h));
                image::RgbaImage::from_raw(w, h, rgba).expect("img").save(&path).expect("save");
            }
        }
    }
    adapter.window().hide().expect("hide");
}
