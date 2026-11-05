//! Visual regression for every Manager page with realistic synthetic data.
//!
//! Renders the real `ManagerWindow` through the software renderer. Each state
//! is drawn in zh-CN, en and ja at 1280x800 and 360x780 and must repeat the
//! same frame when applied twice. Set `SHINKI_UI_SCREENSHOT_DIR` to keep PNGs
//! as `<dir>/<page>/<state>-<language>-<width>x<height>.png`.
//! All titles, developers and covers are synthetic. `SHINKI_UI_SCREENS_ONLY`
//! (comma-separated id prefixes such as `patch/failed`) limits the states drawn.

use std::{
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use shinki_emu_manager_ui_slint::{
    App, ChoiceViewModel, ConfigFieldViewModel, GameCardViewModel, GameSettingsViewModel, Library,
    ManagerViewModel, Onboarding, Patch, PatchGameViewModel, PatchStateViewModel, RecipeViewModel,
    RenderViewModel, SelectedGameViewModel, Session, SlintManagerAdapter, Store,
    StoreDepotViewModel, StoreGameViewModel, StoreLoadState, StoreLoginState, i18n,
};
use slint::{
    ComponentHandle,
    platform::{
        EventLoopProxy, Platform, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
};

type Job = Box<dyn FnOnce() + Send>;

/// Cover decoding hands results back through `invoke_from_event_loop`; this
/// queue is drained before every frame so covers appear in the capture.
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

fn run_jobs() {
    let jobs = std::mem::take(&mut *JOBS.lock().expect("job queue"));
    for job in jobs {
        job();
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

fn render(software: &MinimalSoftwareWindow, width: u32, height: u32) -> Vec<u8> {
    software.set_size(slint::PhysicalSize::new(width, height));
    software.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    software.dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(width as f32, height as f32),
    });
    let mut pixels = vec![slint::Rgb8Pixel { r: 0, g: 0, b: 0 }; width as usize * height as usize];
    for frame in 0..120 {
        run_jobs();
        slint::platform::update_timers_and_animations();
        software.request_redraw();
        software.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        });
        // Covers arrive from worker threads; settle until a few frames pass with no new jobs.
        let idle = JOBS.lock().expect("job queue").is_empty();
        if frame >= 8 && idle && !software.window().has_active_animations() {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    pixels.iter().flat_map(|p| [p.r, p.g, p.b, 255]).collect()
}

/// Synthetic cover art: a two-tone gradient with a frame, so cards with and
/// without covers sit side by side.
fn write_cover(dir: &Path, name: &str, a: [u8; 3], b: [u8; 3]) -> String {
    let path = dir.join(format!("{name}.png"));
    let image = image::RgbaImage::from_fn(240, 320, |x, y| {
        let t = (x + y) as f32 / (240 + 320) as f32;
        let mix = |i: usize| (a[i] as f32 * (1.0 - t) + b[i] as f32 * t) as u8;
        if x < 6 || y < 6 || x > 233 || y > 313 {
            image::Rgba([255, 255, 255, 90])
        } else {
            image::Rgba([mix(0), mix(1), mix(2), 255])
        }
    });
    image.save(&path).expect("write cover");
    path.to_string_lossy().into_owned()
}

struct Data {
    covers: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
fn card(
    id: &str,
    title: &str,
    subtitle: &str,
    version: &str,
    cover: &str,
    ready: bool,
    problem: &str,
    play_time: &str,
    last_played: &str,
    compat: &str,
) -> GameCardViewModel {
    GameCardViewModel {
        id: id.into(),
        title: title.into(),
        subtitle: subtitle.into(),
        version: version.into(),
        cover_uri: cover.into(),
        ready,
        problem: problem.into(),
        play_time: play_time.into(),
        last_played: last_played.into(),
        compat: compat.into(),
    }
}

const LONG_JA: &str =
    "とても長いタイトルのビジュアルノベル　～星降る夜に交わした約束と、もう一つの物語～　完全版";
const LONG_EN: &str =
    "The Extraordinarily Long Chronicle of the Lighthouse Keeper Who Waited Through Every Winter";
const LONG_DEV_JA: &str = "株式会社とても長い名前のゲーム開発スタジオ・第二制作部";
const LONG_DEV_EN: &str = "Northern Lights Interactive Entertainment Visual Novel Division";

fn games(d: &Data) -> Vec<GameCardViewModel> {
    let c = |i: usize| d.covers[i % d.covers.len()].as_str();
    vec![
        card(
            "g01",
            "Harbor Lights",
            "港の灯り",
            "",
            c(0),
            true,
            "",
            "12.5 h",
            "2 天前",
            "perfect",
        ),
        card(
            "g02",
            "Snowfield Letters",
            "雪原の手紙",
            "Steam",
            "",
            true,
            "",
            "45 h",
            "1 周前",
            "completable",
        ),
        card(
            "g03",
            "Clockwork Garden",
            "机械庭园",
            "",
            c(1),
            false,
            "缺少可用的模拟器核心",
            "",
            "",
            "unknown",
        ),
        card(
            "g04",
            "Summer Rain",
            "夏の雨",
            "Remastered",
            c(2),
            true,
            "",
            "80 h",
            "3 周前",
            "flawed",
        ),
        card(
            "g05",
            "Ten",
            "十",
            "",
            "",
            true,
            "",
            "28 h",
            "1 个月前",
            "completable",
        ),
        card(
            "g06",
            "Moonlit Station",
            "月夜の駅",
            "",
            c(3),
            true,
            "",
            "65 h",
            "2 个月前",
            "perfect",
        ),
        card(
            "g07",
            "Paper Wings",
            "纸翼",
            "",
            "",
            true,
            "",
            "42 h",
            "3 个月前",
            "boot_only",
        ),
        card(
            "g08",
            "Lantern Festival",
            "灯籠祭",
            "",
            c(4),
            true,
            "",
            "52 h",
            "4 个月前",
            "completable",
        ),
        card(
            "g09",
            "Stardust",
            "星屑",
            "HD",
            "",
            true,
            "",
            "5 h",
            "5 个月前",
            "unplayable",
        ),
        card(
            "g10",
            "Quiet Library",
            "静かな図書館",
            "",
            c(0),
            true,
            "",
            "120 h",
            "6 个月前",
            "perfect",
        ),
        card(
            "g11",
            "Glass Ocean",
            "玻璃之海",
            "",
            "",
            true,
            "",
            "95 h",
            "7 个月前",
            "unknown",
        ),
        card(
            "g12",
            "Autumn Bells",
            "秋の鐘",
            "Director's Cut",
            c(1),
            true,
            "",
            "38 h",
            "8 个月前",
            "perfect",
        ),
        card(
            "g13",
            LONG_JA,
            LONG_DEV_JA,
            "初回限定版",
            c(2),
            true,
            "",
            "1 h",
            "昨天",
            "completable",
        ),
        card(
            "g14",
            LONG_EN,
            LONG_DEV_EN,
            "Definitive Edition",
            "",
            false,
            "The configured core failed to start: missing archive plugin for this engine version",
            "",
            "",
            "flawed",
        ),
    ]
}

fn base() -> ManagerViewModel {
    let mut m = ManagerViewModel::default();
    m.app.version = "0.1.0-beta.7".into();
    m.app.appearance.theme_mode = "dark".into();
    m.app.appearance.reduce_motion = true;
    m.resale_notice.acknowledged = true;
    m.library.prepare_state = "ready".into();
    m.network.allowed = true;
    m.settings.roots = vec!["D:/Games/Visual Novels".into(), "E:/Archive/Galgame".into()];
    m
}

fn library_model(d: &Data) -> ManagerViewModel {
    let mut m = base();
    m.library.games = games(d);
    m.library.prepare_label = shinki_emu_manager_ui_slint::tr!("刷新资料库").into();
    m
}

fn patch_games(d: &Data) -> Vec<PatchGameViewModel> {
    games(d)
        .into_iter()
        .map(|g| PatchGameViewModel {
            linked: g.compat != "unknown",
            id: g.id,
            title: g.title,
            subtitle: g.subtitle,
            cover_uri: g.cover_uri,
        })
        .collect()
}

fn patch_selected(d: &Data) -> ManagerViewModel {
    let mut m = library_model(d);
    m.patch.games = patch_games(d);
    m.patch.game = m.patch.games[0].clone();
    m.patch.recipes = vec![
        RecipeViewModel {
            id: "r1".into(),
            title: "简体中文化补丁 v2.1（对应 Steam 版 1.04）".into(),
            recommended: true,
        },
        RecipeViewModel {
            id: "r2".into(),
            title: "Official update 1.04".into(),
            recommended: false,
        },
        RecipeViewModel {
            id: "r3".into(),
            title: LONG_JA.into(),
            recommended: false,
        },
    ];
    m.patch.state.script = "r1".into();
    m.patch.state.output_directory = "D:/Games/Visual Novels/Harbor Lights (zh)".into();
    m.patch.state.patch_directory = "D:/Downloads/harbor-lights-zh".into();
    m
}

fn field(
    key: &str,
    label: &str,
    kind: &str,
    value: &str,
    options: &[&str],
) -> ConfigFieldViewModel {
    ConfigFieldViewModel {
        key: key.into(),
        label: label.into(),
        description: String::new(),
        group: String::new(),
        kind: kind.into(),
        value: value.into(),
        display_value: value.into(),
        options: options.iter().map(|o| o.to_string()).collect(),
        option_values: options.iter().map(|o| o.to_string()).collect(),
        min: 0.0,
        max: 100.0,
        step: 1.0,
        unit: String::new(),
        advanced: false,
        overridden: false,
        error: String::new(),
    }
}

fn selection(d: &Data, m: &mut ManagerViewModel) {
    m.library.selected_id = "g13".into();
    m.selection.game = SelectedGameViewModel {
        id: "g13".into(),
        title: LONG_JA.into(),
        engine: "KiriKiri Z".into(),
        emulator: "krkr2".into(),
        emulator_id: "krkr2".into(),
        launchable: true,
        problem: String::new(),
        play_time: "1 h".into(),
        last_played: "2026-11-03 21:40".into(),
        developer: LONG_DEV_JA.into(),
        release_date: "2019-08-30".into(),
        platforms: "Windows".into(),
        description: "合成的作品简介，用于检查长段落在详情面板中的折行与留白。".into(),
        tags: "恋爱, 奇幻, 群像".into(),
        aliases: "長いタイトル, LongTitle".into(),
    };
    m.selection.emulator_choices = vec![ChoiceViewModel {
        id: "krkr2".into(),
        label: "KiriKiri 2 (krkr2)".into(),
    }];
    let _ = d;
}

type Setup = fn(&SlintManagerAdapter, &Data);

fn states() -> Vec<(&'static str, Setup)> {
    vec![
        // 首次引导：每一步
        ("onboarding/step0-notice", |a, _| {
            let mut m = base();
            m.resale_notice.acknowledged = false;
            a.apply(&m);
            page(a, "onboarding");
            a.window().global::<Onboarding>().set_step(0);
        }),
        ("onboarding/step1-roots", |a, _| {
            let mut m = base();
            m.onboarding.roots = vec![
                "D:/Games/Visual Novels".into(),
                "E:/Archive/とても長いフォルダー名のゲームライブラリ".into(),
            ];
            a.apply(&m);
            page(a, "onboarding");
            a.window().global::<Onboarding>().set_step(1);
        }),
        ("onboarding/step2-network", |a, _| {
            a.apply(&base());
            page(a, "onboarding");
            a.window().global::<Onboarding>().set_step(2);
        }),
        ("onboarding/step3-appearance", |a, _| {
            a.apply(&base());
            page(a, "onboarding");
            a.window().global::<Onboarding>().set_step(3);
        }),
        ("onboarding/step4-scanning", |a, _| {
            let mut m = base();
            m.library.prepare_state = "scanning".into();
            m.library.prepare_detail = "D:/Games/Visual Novels/Harbor Lights".into();
            m.library.prepare_progress = Some(0.42);
            m.library.can_cancel_prepare = true;
            a.apply(&m);
            page(a, "onboarding");
            a.window().global::<Onboarding>().set_step(4);
        }),
        // 游戏库
        ("library/empty-unprepared", |a, _| {
            let mut m = base();
            m.library.prepare_state = "unprepared".into();
            a.apply(&m);
            page(a, "library");
        }),
        ("library/empty-no-roots", |a, _| {
            let mut m = base();
            m.settings.roots.clear();
            a.apply(&m);
            page(a, "library");
        }),
        ("library/grid", |a, d| {
            a.apply(&library_model(d));
            page(a, "library");
        }),
        ("library/list", |a, d| {
            let mut m = library_model(d);
            m.app.appearance.view_mode = "list".into();
            a.apply(&m);
            page(a, "library");
        }),
        ("library/filter-no-results", |a, _| {
            let mut m = base();
            m.library.compat_filter = "unplayable".into();
            a.apply(&m);
            page(a, "library");
        }),
        ("library/search-no-results", |a, _| {
            let mut m = base();
            m.library.query = "存在しない作品".into();
            a.apply(&m);
            page(a, "library");
        }),
        ("library/loading-scan", |a, d| {
            let mut m = library_model(d);
            m.library.prepare_state = "scanning".into();
            m.library.prepare_detail = "E:/Archive/Galgame/Autumn Bells".into();
            m.library.prepare_progress = Some(0.6);
            m.library.can_cancel_prepare = true;
            a.apply(&m);
            page(a, "library");
        }),
        ("library/error-prepare-failed", |a, _| {
            let mut m = base();
            m.library.prepare_state = "failed".into();
            m.library.prepare_detail = "核心 krkr2 加载失败：缺少 plugin/xp3filter.dll".into();
            m.library.prepare_label = shinki_emu_manager_ui_slint::tr!("准备").into();
            a.apply(&m);
            page(a, "library");
        }),
        ("library/detail", |a, d| {
            let mut m = library_model(d);
            selection(d, &mut m);
            a.apply(&m);
            page(a, "library");
            a.window().global::<Library>().set_detail_open(true);
        }),
        // 游戏设置：有模拟器、有参数
        ("game-settings/emulator", |a, _| {
            let mut m = base();
            m.game_settings = GameSettingsViewModel {
                game_id: "g13".into(),
                title: LONG_JA.into(),
                emulator_choices: vec![
                    ChoiceViewModel {
                        id: "krkr2".into(),
                        label: "KiriKiri 2 (krkr2) 1.4.0".into(),
                    },
                    ChoiceViewModel {
                        id: "krkrz".into(),
                        label: "KiriKiri Z 1.2.0".into(),
                    },
                ],
                emulator_id: "krkr2".into(),
                render: RenderViewModel {
                    supported: true,
                    options: vec![
                        ChoiceViewModel {
                            id: "native".into(),
                            label: "原生".into(),
                        },
                        ChoiceViewModel {
                            id: "2x".into(),
                            label: "2×".into(),
                        },
                    ],
                    mode: "native".into(),
                    custom_width: 1280,
                    custom_height: 720,
                    min: "640x480".into(),
                    max: "3840x2160".into(),
                    note: String::new(),
                    error: String::new(),
                },
                fields: vec![
                    field("text_speed", "文字速度", "integer", "60", &[]),
                    field(
                        "font",
                        "字体",
                        "enum",
                        "Noto Sans CJK",
                        &["Noto Sans CJK", "Source Han Serif"],
                    ),
                    field("skip_unread", "允许跳过未读文本", "bool", "false", &[]),
                ],
                ..Default::default()
            };
            a.apply(&m);
            page(a, "game-settings");
        }),
        // 补丁：列表、选补丁、执行中、失败
        ("patch/select-game", |a, d| {
            let mut m = library_model(d);
            m.patch.games = patch_games(d);
            a.apply(&m);
            page(a, "patch");
            a.window().global::<Patch>().set_step(1);
        }),
        ("patch/empty-library", |a, _| {
            a.apply(&base());
            page(a, "patch");
        }),
        ("patch/choose-recipe", |a, d| {
            a.apply(&patch_selected(d));
            page(a, "patch");
            a.window().global::<Patch>().set_step(2);
        }),
        ("patch/running", |a, d| {
            let mut m = patch_selected(d);
            m.patch.state = PatchStateViewModel {
                step: 4,
                running: true,
                phase: "正在写入文件".into(),
                completed: 312,
                total: Some(1024),
                output: "data/scenario/ch03.ks".into(),
                ..m.patch.state.clone()
            };
            a.apply(&m);
            page(a, "patch");
            a.window().global::<Patch>().set_step(4);
        }),
        ("patch/failed", |a, d| {
            let mut m = patch_selected(d);
            m.patch.state = PatchStateViewModel {
                step: 4,
                error: "校验失败：data.xp3 的 SHA-256 与补丁要求的版本不一致。请确认游戏是 Steam 版 1.04。".into(),
                can_start: true,
                rolled_back: true,
                ..m.patch.state.clone()
            };
            a.apply(&m);
            page(a, "patch");
            a.window().global::<Patch>().set_step(4);
        }),
        // 滤镜
        ("filters/none", |a, _| {
            a.apply(&base());
            page(a, "filters");
        }),
        ("filters/sharpen-params", |a, _| {
            let mut m = base();
            m.filters.preset = "sharpen".into();
            m.filters.fields = vec![
                field("strength", "锐化强度", "float", "0.6", &[]),
                field("radius", "半径", "integer", "2", &[]),
            ];
            a.apply(&m);
            page(a, "filters");
        }),
        // 设置：除 translation / updates 外的每个分类
        ("settings/appearance", |a, _| {
            a.apply(&base());
            settings(a, "appearance");
        }),
        ("settings/library-roots", |a, _| {
            a.apply(&base());
            settings(a, "library-roots");
        }),
        ("settings/network", |a, _| {
            a.apply(&base());
            settings(a, "network");
        }),
        ("settings/input", |a, _| {
            a.apply(&base());
            settings(a, "input");
        }),
        ("settings/about", |a, _| {
            a.apply(&base());
            settings(a, "about");
        }),
        // 添加游戏、商店：登录、列表、详情、加载失败
        ("add-game/main", |a, _| {
            a.apply(&base());
            page(a, "add-game");
        }),
        ("store/login", |a, _| {
            a.apply(&store_model(StoreLoginState::SignedOut));
            page(a, "store");
        }),
        ("store/library", |a, _| {
            a.apply(&store_model(StoreLoginState::SignedIn));
            page(a, "store");
        }),
        ("store/detail", |a, _| {
            let mut m = store_model(StoreLoginState::SignedIn);
            m.store.selected_title_id = "1001".into();
            m.store.depots = vec![
                StoreDepotViewModel {
                    depot_id: "1".into(),
                    name: "Base game".into(),
                    platform: "windows".into(),
                    language: "english".into(),
                    size_bytes: 3_221_225_472,
                    selected: true,
                },
                StoreDepotViewModel {
                    depot_id: "2".into(),
                    name: "日本語ボイス".into(),
                    platform: "windows".into(),
                    language: "japanese".into(),
                    size_bytes: 1_073_741_824,
                    selected: false,
                },
            ];
            m.store.library_root = "D:/Games/Visual Novels".into();
            a.apply(&m);
            page(a, "store");
            a.window().global::<Store>().set_detail_open(true);
        }),
        ("store/error-library", |a, _| {
            let mut m = store_model(StoreLoginState::SignedIn);
            m.store.games.clear();
            m.store.library_state = StoreLoadState::Error;
            m.store.error = "无法连接商店服务器（连接超时）。请检查网络后重试。".into();
            a.apply(&m);
            page(a, "store");
        }),
        // 游戏内菜单
        ("in-game/menu", |a, _| {
            let mut m = base();
            m.session.phase = "running".into();
            m.session.launch_title = LONG_EN.into();
            a.apply(&m);
            page(a, "library");
            a.set_session_active(true);
            let s = a.window().global::<Session>();
            s.set_title(LONG_EN.into());
            s.set_menu_open(true);
            s.set_menu_page("main".into());
        }),
    ]
}

fn store_model(login: StoreLoginState) -> ManagerViewModel {
    let mut m = base();
    m.store.provider_id = "steam".into();
    m.store.provider_name = "Steam".into();
    m.store.providers = vec![ChoiceViewModel {
        id: "steam".into(),
        label: "Steam".into(),
    }];
    m.store.can_credentials = true;
    m.store.can_qr = true;
    m.store.login_state = login;
    m.store.library_state = StoreLoadState::Ready;
    if login == StoreLoginState::SignedIn {
        m.store.account_name = "synthetic_user_2026".into();
    }
    let game = |id: &str, name: &str, size: u64, installed: bool, update: bool, reason: &str| {
        StoreGameViewModel {
            title_id: id.into(),
            name: name.into(),
            size_bytes: size,
            platforms: vec!["windows".into()],
            languages: vec!["english".into(), "japanese".into(), "schinese".into()],
            installed,
            update_available: update,
            update_unknown: false,
            unavailable_reason: reason.into(),
        }
    };
    m.store.games = vec![
        game("1001", "Harbor Lights", 4_294_967_296, false, false, ""),
        game("1002", "Snowfield Letters", 2_147_483_648, true, true, ""),
        game("1003", LONG_EN, 9_663_676_416, false, false, ""),
        game(
            "1004",
            LONG_JA,
            1_073_741_824,
            false,
            false,
            "此游戏没有 Windows 版本",
        ),
        game("1005", "Ten", 734_003_200, true, false, ""),
    ];
    m
}

fn page(a: &SlintManagerAdapter, page: &str) {
    a.window().global::<App>().set_page(page.into());
}

fn settings(a: &SlintManagerAdapter, category: &str) {
    let app = a.window().global::<App>();
    app.set_page("settings".into());
    app.set_settings_category(category.into());
}

fn base_empty() -> ManagerViewModel {
    let mut m = base();
    m.settings.roots.clear();
    m
}

/// Returns the window to a neutral state so no state leaks into the next one.
/// An empty model makes the adapter rebuild every list (and request its covers) on the next apply.
fn reset(a: &SlintManagerAdapter) {
    a.set_session_active(false);
    a.apply(&base_empty());
    let w = a.window();
    w.global::<Session>().set_menu_open(false);
    w.global::<Library>().set_detail_open(false);
    w.global::<Store>().set_detail_open(false);
    w.global::<Patch>().set_step(1);
    w.global::<App>().set_page("library".into());
}

#[test]
fn every_page_renders_stable_frames() {
    let software = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(SoftwarePlatform {
        window: software.clone(),
        start: Instant::now(),
    }))
    .expect("software platform");
    let adapter = SlintManagerAdapter::new().expect("manager window");
    adapter
        .window()
        .global::<Library>()
        .set_cover_loading(false);

    let covers_dir = tempfile::tempdir().expect("cover dir");
    let palette = [
        [60, 90, 160],
        [170, 70, 90],
        [40, 140, 120],
        [190, 140, 60],
        [110, 80, 170],
    ];
    let data = Data {
        covers: palette
            .iter()
            .enumerate()
            .map(|(i, &c)| write_cover(covers_dir.path(), &format!("cover{i}"), c, [20, 20, 30]))
            .collect(),
    };

    let output = std::env::var_os("SHINKI_UI_SCREENSHOT_DIR").map(PathBuf::from);
    let (languages, sizes): (&[&str], &[(u32, u32)]) = if output.is_some() {
        (&["zh-CN", "en", "ja"], &[(1280, 800), (360, 780)])
    } else {
        (&["ja"], &[(360, 780)])
    };
    // `SHINKI_UI_SCREENS_ONLY=patch/failed,store` renders only ids with one of these prefixes.
    let only: Vec<String> = std::env::var("SHINKI_UI_SCREENS_ONLY")
        .map(|v| v.split(',').map(str::to_owned).collect())
        .unwrap_or_default();
    adapter.window().show().expect("show");
    for &language in languages {
        i18n::apply(language);
        for &(width, height) in sizes {
            for (id, setup) in states() {
                if !only.is_empty() && !only.iter().any(|p| id.starts_with(p.as_str())) {
                    continue;
                }
                let draw = || {
                    reset(&adapter);
                    setup(&adapter, &data);
                    adapter.window().global::<Library>().set_cover_loading(true);
                    // Covers decode off-thread in any order; capture once the frame stops changing.
                    let mut last = render(&software, width, height);
                    for _ in 0..40 {
                        std::thread::sleep(Duration::from_millis(25));
                        let next = render(&software, width, height);
                        if next == last && JOBS.lock().expect("job queue").is_empty() {
                            break;
                        }
                        last = next;
                    }
                    last
                };
                let rgba = draw();
                let animated = id.ends_with("scanning")
                    || id.ends_with("loading-scan")
                    || id.ends_with("running");
                if !animated {
                    let again = draw();
                    if rgba != again
                        && let Some(dir) = &output
                    {
                        for (n, f) in [("a", &rgba), ("b", &again)] {
                            image::RgbaImage::from_raw(width, height, f.clone())
                                .expect("frame size")
                                .save(dir.join(format!("mismatch-{n}.png")))
                                .expect("write png");
                        }
                    }
                    assert!(
                        rgba == again,
                        "{id} {language} {width}x{height}: same model drew a different frame"
                    );
                }
                if let Some(dir) = &output {
                    let (group, state) = id.split_once('/').expect("page/state id");
                    let dir = dir.join(group);
                    std::fs::create_dir_all(&dir).expect("screenshot dir");
                    image::RgbaImage::from_raw(width, height, rgba)
                        .expect("frame size")
                        .save(dir.join(format!("{state}-{language}-{width}x{height}.png")))
                        .expect("write png");
                }
            }
        }
    }
    reset(&adapter);
    adapter.window().hide().expect("hide");
}
