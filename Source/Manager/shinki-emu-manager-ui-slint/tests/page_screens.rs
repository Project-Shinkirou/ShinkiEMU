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
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use i_slint_core::input::TouchPhase;
use shinki_emu_manager_ui_slint::{
    App, ChoiceViewModel, ConfigFieldViewModel, GameCardViewModel, GameSettingsViewModel, Library,
    ManagerViewModel, NoticeViewModel, Onboarding, Patch, PatchGameViewModel, PatchStateViewModel, RecipeViewModel,
    RenderViewModel, SelectedGameViewModel, Session, SlintManagerAdapter, Store,
    StoreDepotViewModel, StoreGameViewModel, StoreLoadState, StoreLoginState, i18n, report_error, tr,
};
use slint::{
    ComponentHandle, Model,
    platform::{
        EventLoopProxy, InternalEvent, Platform, PointerEventButton, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
    private_unstable_api::re_exports::{AccessibleStringProperty, ItemRc, Key, WindowInner},
    LogicalPosition, SharedString,
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
    opened_urls: Rc<RefCell<Vec<String>>>,
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
    fn open_url(&self, url: &str) -> Result<(), slint::PlatformError> {
        self.opened_urls.borrow_mut().push(url.to_owned());
        Ok(())
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
    m.library.prepare_state = "ready".into();
    m.library.can_refresh = true;
    m.library.prepare_label = shinki_emu_manager_ui_slint::tr!("刷新资料库").into();
    m.network.allowed = true;
    m.settings.roots = vec!["D:/Games/Visual Novels".into(), "E:/Archive/Galgame".into()];
    m
}

fn library_model(d: &Data) -> ManagerViewModel {
    let mut m = base();
    m.library.games = games(d);
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
        ("onboarding/step0-roots", |a, _| {
            let mut m = base();
            m.onboarding.roots = vec![
                "D:/Games/Visual Novels".into(),
                "E:/Archive/とても長いフォルダー名のゲームライブラリ".into(),
            ];
            a.apply(&m);
            page(a, "onboarding");
            a.window().global::<Onboarding>().set_step(0);
        }),
        ("onboarding/step1-network", |a, _| {
            let mut m = base();
            m.network.allowed = false;
            a.apply(&m);
            page(a, "onboarding");
            a.window().global::<Onboarding>().set_step(1);
        }),
        ("onboarding/step2-appearance", |a, _| {
            a.apply(&base());
            page(a, "onboarding");
            a.window().global::<Onboarding>().set_step(2);
        }),
        // 游戏库
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
        ("library/scanning", |a, d| {
            let mut m = library_model(d);
            m.library.prepare_state = "scanning".into();
            m.library.prepare_detail = "E:/Archive/Galgame/Autumn Bells".into();
            m.library.prepare_progress = Some(0.6);
            m.library.can_cancel_prepare = true;
            m.library.can_refresh = false;
            a.apply(&m);
            page(a, "library");
        }),
        ("library/scan-failed", |a, _| {
            let mut m = base();
            m.library.prepare_state = "failed".into();
            m.library.prepare_detail = shinki_emu_manager_ui_slint::tr!("请检查文件夹权限，或选择其他文件夹。").into();
            m.library.prepare_label = shinki_emu_manager_ui_slint::tr!("重试").into();
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
                configuration_state: "ready".into(),
                ..Default::default()
            };
            a.apply(&m);
            page(a, "game-settings");
        }),
        ("game-settings/unprepared", |a, _| {
            let mut m = base();
            m.game_settings.game_id = "g03".into();
            m.game_settings.title = "Clockwork Garden".into();
            m.game_settings.emulator_id = "siglus".into();
            m.game_settings.emulator_choices = vec![ChoiceViewModel {
                id: "siglus".into(),
                label: "siglus-rs 0.2.0".into(),
            }];
            m.game_settings.configuration_state = "unprepared".into();
            a.apply(&m);
            page(a, "game-settings");
        }),
        ("game-settings/loading", |a, _| {
            let mut m = base();
            m.game_settings.game_id = "g03".into();
            m.game_settings.title = "Clockwork Garden".into();
            m.game_settings.emulator_id = "siglus".into();
            m.game_settings.configuration_state = "loading".into();
            a.apply(&m);
            page(a, "game-settings");
        }),
        ("game-settings/failed", |a, _| {
            let mut m = base();
            m.game_settings.game_id = "g14".into();
            m.game_settings.title = LONG_EN.into();
            m.game_settings.emulator_id = "krkrz".into();
            m.game_settings.emulator_choices = vec![ChoiceViewModel {
                id: "krkrz".into(),
                label: "KiriKiri Z 1.2.0".into(),
            }];
            m.game_settings.configuration_state = "failed".into();
            m.game_settings.configuration_error = "KiriKiri Z 初始化失败\n缺少 plugin/xp3filter.dll。修复这个文件后，重试只会初始化所选核心。".into();
            m.notices.items = vec![NoticeViewModel {
                id: "unrelated".into(),
                severity: "warning".into(),
                title: "元数据同步推迟".into(),
                detail: "这条通知与当前配置失败无关。".into(),
                code: String::new(),
                core: String::new(),
            }];
            a.apply(&m);
            page(a, "game-settings");
        }),
        ("game-settings/missing-core", |a, _| {
            let mut m = base();
            m.game_settings.game_id = "g03".into();
            m.game_settings.title = LONG_JA.into();
            m.game_settings.configuration_state = "missing-core".into();
            a.apply(&m);
            page(a, "game-settings");
        }),
        ("library/missing-core", |a, d| {
            let mut m = library_model(d);
            m.library.selected_id = "g03".into();
            m.selection.game = SelectedGameViewModel {
                id: "g03".into(),
                title: "Clockwork Garden".into(),
                engine: "SiglusEngine".into(),
                emulator: String::new(),
                emulator_id: String::new(),
                launchable: false,
                problem: "已识别引擎，缺少兼容核心".into(),
                developer: LONG_DEV_EN.into(),
                description: "合成的缺核心详情，用于检查启动按钮和安装入口。".into(),
                ..Default::default()
            };
            a.apply(&m);
            page(a, "library");
            a.window().global::<Library>().set_detail_open(true);
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
        ("settings/network-sync-failed", |a, _| {
            let mut m = base();
            m.network.sync_status = tr!("同步失败：{error}", error = tr!("请检查网络连接后重试。"));
            a.apply(&m);
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
        // 商店：登录、列表、详情、加载失败
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
    let opened_urls = Rc::new(RefCell::new(Vec::new()));
    slint::platform::set_platform(Box::new(SoftwarePlatform {
        window: software.clone(),
        start: Instant::now(),
        opened_urls: opened_urls.clone(),
    }))
    .expect("software platform");
    let adapter = Rc::new(SlintManagerAdapter::new().expect("manager window"));
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
    // A comma-separated language override draws only the narrow layout.
    let narrow = std::env::var("SHINKI_UI_NARROW_LOCALES").ok();
    let narrow_languages = narrow.as_deref().map(|value| value.split(',').collect::<Vec<_>>());
    let (languages, sizes): (&[&str], &[(u32, u32)]) = if let Some(langs) = &narrow_languages {
        (langs, &[(360, 780)])
    } else if output.is_some() {
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
            let save = |id: &str, rgba| {
                if let Some(dir) = &output {
                    let (group, state) = id.split_once('/').expect("page/state id");
                    let dir = dir.join(group);
                    std::fs::create_dir_all(&dir).expect("screenshot dir");
                    image::RgbaImage::from_raw(width, height, rgba)
                        .expect("frame size")
                        .save(dir.join(format!("{state}-{language}-{width}x{height}.png")))
                        .expect("write png");
                }
            };
            if only.is_empty() || only.iter().any(|p| "library-toolbar/interaction".starts_with(p)) {
                exercise_library(&adapter, &software, &data, (width, height), save);
            }
            if only.is_empty() || only.iter().any(|p| "library-import/interaction".starts_with(p)) {
                exercise_import_dialog(&adapter, &software, &data, (width, height), save);
            }
            if only.is_empty() || only.iter().any(|p| "settings/storage-access".starts_with(p)) {
                exercise_storage_access(&adapter, &software, (width, height), save);
            }
            if only.is_empty() || only.iter().any(|p| "onboarding/storage-access".starts_with(p)) {
                exercise_onboarding_storage(&adapter, &software, (width, height), save);
            }
            if only.is_empty() || only.iter().any(|p| "cores/cards".starts_with(p)) {
                exercise_core_cards(&adapter, &software, (width, height), save);
            }
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
                    || id.ends_with("/loading")
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
                save(id, rgba);
            }
            if only.is_empty() || only.iter().any(|p| "settings/about-credits".starts_with(p)) {
                exercise_about_credits(&adapter, &software, (width, height), &opened_urls, save);
            }
        }
    }
    reset(&adapter);
    adapter.window().hide().expect("hide");
}

// Use accessible names to locate real controls, then send pointer input through the window.
// The pinned Slint API gives their bounds without test-only geometry properties in the UI.
fn labeled_item(software: &MinimalSoftwareWindow, matches: impl Fn(&str) -> bool) -> Option<ItemRc> {
    matching_label(software, |label, _| matches(label))
}

fn matching_label(
    software: &MinimalSoftwareWindow,
    matches: impl Fn(&str, &ItemRc) -> bool,
) -> Option<ItemRc> {
    let window = WindowInner::from_pub(software.window());
    let mut stack = vec![window.window_item_rc()?];
    stack.extend(window.active_popups().iter().map(|popup| ItemRc::new_root(popup.component.clone())));
    while let Some(item) = stack.pop() {
        stack.extend(std::iter::successors(item.first_child(), ItemRc::next_sibling));
        if item.is_visible()
            && item.accessible_string_property(AccessibleStringProperty::Label)
                .is_some_and(|label| matches(label.as_str(), &item))
        {
            return Some(item);
        }
    }
    None
}

fn click_label(software: &MinimalSoftwareWindow, label: &str) -> LogicalPosition {
    let item = labeled_item(software, |text| text == label)
        .unwrap_or_else(|| panic!("visible control not found: {label}"));
    let center = item.map_to_native_window(item.geometry().center());
    click_at(software, LogicalPosition::new(center.x, center.y))
}

fn click_at(software: &MinimalSoftwareWindow, position: LogicalPosition) -> LogicalPosition {
    software.dispatch_event(WindowEvent::PointerMoved { position });
    software.dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
    software.dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
    software.dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(0.0, 0.0) });
    position
}

fn press(software: &MinimalSoftwareWindow, key: impl Into<SharedString>) {
    let text = key.into();
    software.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    software.dispatch_event(WindowEvent::KeyReleased { text });
}

fn ctrl_f(software: &MinimalSoftwareWindow) {
    let control: SharedString = Key::Control.into();
    software.dispatch_event(WindowEvent::KeyPressed { text: control.clone() });
    press(software, "f");
    software.dispatch_event(WindowEvent::KeyReleased { text: control });
}

fn touch(software: &MinimalSoftwareWindow, position: LogicalPosition, phase: TouchPhase) {
    software.dispatch_event(WindowEvent::internal(InternalEvent::Touch {
        id: 0,
        position: (position.x, position.y).into(),
        phase,
    }));
}

fn swipe(software: &MinimalSoftwareWindow, x: f32, y: f32, dx: f32, dy: f32) {
    touch(software, LogicalPosition::new(x, y), TouchPhase::Started);
    touch(software, LogicalPosition::new(x + dx, y + dy), TouchPhase::Moved);
    touch(software, LogicalPosition::new(x + dx, y + dy), TouchPhase::Ended);
}

fn exercise_library(
    a: &Rc<SlintManagerAdapter>,
    software: &MinimalSoftwareWindow,
    data: &Data,
    (width, height): (u32, u32),
    save: impl Fn(&str, Vec<u8>),
) {
    let frame = || render(software, width, height);
    let window = a.window();
    window.global::<shinki_emu_manager_ui_slint::MaterialWindowAdapter>().set_touch(width < 600);
    let app = window.global::<App>();
    let library = window.global::<Library>();
    let onboarding = window.global::<Onboarding>();
    let network = window.global::<shinki_emu_manager_ui_slint::Network>();
    let search_button = tr!("搜索");
    let download = tr!("下载");
    let add = tr!("添加文件夹");
    let search = tr!("搜索游戏标题");
    let filters = tr!("筛选选项");
    let clear = tr!("清除搜索和筛选");
    let refresh = tr!("刷新资料库");
    let visible = |label: &str| labeled_item(software, |text| text == label).is_some();
    let game_visible = |title: &str| labeled_item(software, |text| text.starts_with(title)).is_some();
    let check_toolbar = || {
        let mut last = width as f32 * 0.5;
        for label in [search_button, add, download] {
            let item = labeled_item(software, |text| text == label).expect("direct toolbar action");
            let bounds = item.geometry();
            let start = item.map_to_native_window(bounds.origin);
            assert_eq!((bounds.size.width, bounds.size.height), (48.0, 48.0));
            assert!(start.x >= last && start.y < 64.0);
            last = start.x + bounds.size.width;
        }
        assert!(last <= width as f32, "all three actions fit in the header");
    };

    reset(a);
    let mut model = base_empty();
    model.network.allowed = false;
    model.library.can_refresh = false;
    a.apply(&model);
    page(a, "onboarding");
    onboarding.set_step(0);
    frame();
    click_label(software, tr!("下一步"));
    assert_eq!(onboarding.get_step(), 1, "next works immediately on the first step");
    assert!(!network.get_allowed(), "advance does not grant network access");
    frame();
    click_label(software, tr!("下一步"));
    assert_eq!(onboarding.get_step(), 2);
    frame();
    let weak = window.as_weak();
    onboarding.on_finish(move || {
        if let Some(window) = weak.upgrade() {
            window.global::<App>().set_page("library".into());
        }
    });
    click_label(software, tr!("完成"));
    assert_eq!(app.get_page(), "library");
    let empty = frame();
    check_toolbar();
    assert!(!visible(search) && !visible(refresh));
    save("library-toolbar/empty", empty.clone());

    // The host owns the native picker and worker. Draw the next host models after
    // pressing the real picker button, including a failed permission request.
    let attempts = Rc::new(Cell::new(0));
    let weak = Rc::downgrade(a);
    let picked = attempts.clone();
    library.on_add_directory(move || {
        let Some(a) = weak.upgrade() else { return };
        picked.set(picked.get() + 1);
        if picked.get() == 1 {
            report_error(a.window(), "SHINKI_EMU_ANDROID_PICK_PERMISSION");
        } else {
            let mut model = base_empty();
            model.network.allowed = false;
            model.settings.roots = vec!["D:/Games/Visual Novels".into()];
            model.library.prepare_state = "scanning".into();
            model.library.prepare_progress = Some(0.4);
            model.library.can_cancel_prepare = true;
            model.library.can_refresh = false;
            a.apply(&model);
        }
    });
    click_label(software, add);
    assert_eq!(app.get_page(), "library", "add opens the picker callback without navigating");
    assert_eq!(attempts.get(), 1);
    save("library-toolbar/picker-denied", frame());
    assert!(visible(tr!("关闭")), "picker failure shows a dismissible notice");
    click_label(software, tr!("关闭"));
    click_label(software, add);
    assert_eq!(attempts.get(), 2);
    assert_eq!(app.get_page(), "library");
    save("library-toolbar/scanning", frame());
    assert!(visible(tr!("取消扫描")));
    assert!(!visible(refresh), "a running scan cannot be refreshed");

    let mut model = library_model(data);
    model.network.allowed = false;
    model.library.games[1].title = "Harbor Letters".into();
    a.apply(&model);
    let all_games = model.library.games.clone();
    let game_count = all_games.len();
    let state = Rc::new(RefCell::new(model));
    let project = {
        let state = state.clone();
        let weak = Rc::downgrade(a);
        let all_games = Rc::new(all_games);
        move |query: Option<SharedString>, filter: Option<SharedString>| {
            let Some(a) = weak.upgrade() else { return };
            let mut m = state.borrow_mut();
            if let Some(query) = query { m.library.query = query.to_string(); }
            if let Some(filter) = filter { m.library.compat_filter = filter.to_string(); }
            let query = m.library.query.to_ascii_lowercase();
            let filter = m.library.compat_filter.as_str();
            m.library.games = all_games.iter()
                .filter(|game| game.title.to_ascii_lowercase().contains(&query)
                    && (filter == "all" || game.compat == filter))
                .cloned()
                .collect();
            a.apply(&m);
        }
    };
    let update = project.clone();
    library.on_search(move |query| update(Some(query), None));
    library.on_set_compat_filter(move |filter| project(None, Some(filter)));
    let selected = state.clone();
    let weak = Rc::downgrade(a);
    library.on_select(move |id| {
        let Some(a) = weak.upgrade() else { return };
        let mut m = selected.borrow_mut();
        let game = m.library.games.iter().find(|g| g.id == id.as_str()).expect("selected result");
        m.selection.game = SelectedGameViewModel {
            id: game.id.clone(),
            title: game.title.clone(),
            engine: "KiriKiri Z".into(),
            emulator: "KiriKiri 2".into(),
            launchable: game.ready,
            ..Default::default()
        };
        m.library.selected_id = id.to_string();
        a.apply(&m);
    });
    let store_state = state.clone();
    let weak = Rc::downgrade(a);
    window.global::<Store>().on_open(move || {
        let Some(a) = weak.upgrade() else { return };
        let mut m = store_state.borrow_mut();
        m.store = store_model(StoreLoginState::SignedOut).store;
        a.apply(&m);
    });
    library.set_cover_loading(true);
    frame();
    check_toolbar();
    save("library-toolbar/populated", frame());
    click_label(software, download);
    assert_eq!(app.get_page(), "store");
    assert_eq!(window.global::<Store>().get_provider_name(), "Steam");
    save("library-toolbar/store-route", frame());
    click_label(software, tr!("返回游戏库"));
    assert_eq!(app.get_page(), "library");
    frame();
    click_label(software, download);
    press(software, Key::Back);
    assert_eq!(app.get_page(), "library");
    frame();
    click_label(software, search_button);
    assert_eq!(app.get_page(), "library-search");
    assert!(!visible(add) && !visible(refresh));
    save("library-toolbar/search", frame());
    for ch in "Harbor".chars() { press(software, ch.to_string()); }
    assert_eq!(state.borrow().library.query, "Harbor");
    assert_eq!(library.get_games().row_count(), 2);
    let mut results = frame();
    for _ in 0..10 {
        if library.get_games().row_data(0).unwrap().cover.size().width > 0 {
            break;
        }
        results = frame();
    }
    assert!(library.get_games().row_data(0).unwrap().cover.size().width > 0, "live search loads covers");
    save("library-toolbar/search-live-results", results);
    assert!(game_visible("Harbor Lights") && game_visible("Harbor Letters"));
    assert!(!game_visible("Snowfield Letters"));
    assert!(!visible(clear), "filter controls start collapsed");
    click_label(software, filters);
    frame();
    // The result card also displays "Perfect"; click the checkable filter chip, not its badge.
    let chip = matching_label(software, |label, item| label == tr!("完美")
        && item.accessible_string_property(AccessibleStringProperty::Checkable)
            .is_some_and(|value| value == "true"))
        .expect("compatibility filter chip is visible");
    let center = chip.map_to_native_window(chip.geometry().center());
    click_at(software, LogicalPosition::new(center.x, center.y));
    save("library-toolbar/search-with-filters", frame());
    assert_eq!(state.borrow().library.compat_filter, "perfect");
    assert_eq!(library.get_games().row_count(), 1);
    assert!(!game_visible("Harbor Letters"));
    click_label(software, filters);
    save("library-toolbar/search-filtered-result", frame());

    let result = labeled_item(software, |text| text.starts_with("Harbor Lights"))
        .expect("live result card is visible in search");
    let center = result.map_to_native_window(result.geometry().center());
    assert!(center.y > 56.0 && center.y < height as f32 - 80.0);
    click_at(software, LogicalPosition::new(center.x, center.y));
    assert!(library.get_detail_open(), "clicking the live result opens its normal detail");
    assert_eq!(state.borrow().library.selected_id, "g01");
    save("library-toolbar/search-result-detail", frame());
    assert!(visible(tr!("开始游戏")));
    let back_key = if width < 600 { Key::Back } else { Key::Escape };
    press(software, back_key);
    assert_eq!(app.get_page(), "library-search");
    assert!(!library.get_detail_open());
    frame();
    press(software, "x");
    assert!(state.borrow().library.query.contains('x'), "closing details refocuses search");
    assert_eq!(library.get_games().row_count(), 0);
    save("library-toolbar/search-no-match", frame());
    press(software, Key::Backspace);
    assert_eq!(state.borrow().library.query, "Harbor");
    frame();
    press(software, back_key);
    assert_eq!(app.get_page(), "library");
    let filtered = frame();
    assert_ne!(empty, filtered);
    assert_eq!(library.get_games().row_count(), 1);
    assert!(game_visible("Harbor Lights"));
    assert!(!game_visible("Harbor Letters"));
    check_toolbar();
    assert!(!visible(search) && !visible(refresh));
    save("library-toolbar/filtered-results", filtered);

    ctrl_f(software);
    assert_eq!(app.get_page(), "library-search");
    frame();
    press(software, "x");
    assert!(state.borrow().library.query.contains('x'), "Ctrl+F focuses the search field");
    frame();
    click_label(software, filters);
    frame();
    click_label(software, clear);
    assert!(state.borrow().library.query.is_empty());
    assert_eq!(state.borrow().library.compat_filter, "all");
    let back = click_label(software, tr!("返回游戏库"));
    assert!(back.y < 64.0, "the app bar stays above the scrollable controls");
    assert_eq!(app.get_page(), "library");
    frame();
    assert_eq!(library.get_games().row_count(), game_count);
    assert!(game_visible("Harbor Lights") && game_visible("Harbor Letters"));

    let refresh_count = Rc::new(Cell::new(0));
    let count = refresh_count.clone();
    let refreshing = state.clone();
    let weak = Rc::downgrade(a);
    library.on_prepare(move || {
        let Some(a) = weak.upgrade() else { return };
        count.set(count.get() + 1);
        let mut m = refreshing.borrow_mut();
        m.library.prepare_state = "scanning".into();
        m.library.prepare_detail = tr!("正在扫描游戏目录").into();
        m.library.prepare_progress = Some(0.35);
        m.library.can_cancel_prepare = true;
        m.library.can_refresh = false;
        a.apply(&m);
    });
    let origin = labeled_item(software, |text| text.starts_with("Harbor Lights"))
        .expect("first card in the list");
    let start = origin.map_to_native_window(origin.geometry().center());
    swipe(software, start.x, start.y, 0.0, 20.0);
    swipe(software, start.x, start.y, 85.0, 12.0);
    swipe(software, start.x, start.y, 95.0, 74.0);
    assert_eq!(refresh_count.get(), 0, "short, horizontal and diagonal drags do not refresh");
    if library.get_detail_open() {
        press(software, Key::Back);
        frame();
    }

    let position = LogicalPosition::new(start.x, start.y);
    touch(software, position, TouchPhase::Started);
    touch(software, position, TouchPhase::Ended);
    assert!(library.get_detail_open(), "ordinary tap opens the first card while refresh is available");
    press(software, back_key);
    frame();
    software.dispatch_event(WindowEvent::PointerScrolled { position, delta_x: 0.0, delta_y: -140.0 });
    frame();
    let offset_card = labeled_item(software, |text| text.starts_with("Harbor Lights"))
        .expect("first card still present after scrolling");
    let offset = offset_card.map_to_native_window(offset_card.geometry().center()).y;
    assert!(offset < start.y, "wheel scroll placed the viewport below the top");
    let mid = LogicalPosition::new(start.x, start.y);
    let top = LogicalPosition::new(start.x, start.y + 200.0);
    touch(software, mid, TouchPhase::Started);
    touch(software, top, TouchPhase::Moved);
    save("library-toolbar/pull-from-mid-list", frame());
    touch(software, top, TouchPhase::Ended);
    frame();
    assert_eq!(refresh_count.get(), 0, "reaching the top is not a refresh if the swipe began below it");
    let pos = LogicalPosition::new(start.x, start.y);
    let end = LogicalPosition::new(start.x, start.y + 92.0);
    touch(software, pos, TouchPhase::Started);
    touch(software, end, TouchPhase::Moved);
    assert_eq!(refresh_count.get(), 0);
    save("library-toolbar/pull-armed", frame());
    touch(software, end, TouchPhase::Ended);
    assert_eq!(refresh_count.get(), 1);
    assert!(!library.get_detail_open(), "a refresh pull does not open the first card");
    save("library-toolbar/pull-refreshing", frame());
    swipe(software, start.x, start.y + 50.0, 0.0, 95.0);
    assert_eq!(refresh_count.get(), 1, "busy refresh rejects another pull");

    let empty_library = base();
    a.apply(&empty_library);
    frame();
    swipe(software, width as f32 * 0.5, 140.0, 0.0, 100.0);
    assert_eq!(refresh_count.get(), 2, "a configured empty library can be refreshed");

    a.apply(&base());
    settings(a, "library-roots");
    save("library-toolbar/folder-settings", frame());
    assert!(visible(tr!("添加目录")));
    click_label(software, refresh);
    assert_eq!(refresh_count.get(), 3, "the Settings refresh still works");
    frame();
    assert!(visible(tr!("取消扫描")));
    assert!(!visible(refresh));
    window.global::<shinki_emu_manager_ui_slint::MaterialWindowAdapter>().set_touch(false);
}

fn exercise_storage_access(
    a: &SlintManagerAdapter,
    software: &MinimalSoftwareWindow,
    (width, height): (u32, u32),
    save: impl Fn(&str, Vec<u8>),
) {
    use shinki_emu_manager_ui_slint::Settings;

    let material = a.window().global::<shinki_emu_manager_ui_slint::MaterialWindowAdapter>();
    material.set_touch(width < 600);
    let app = a.window().global::<Settings>();
    let allow = tr!("允许访问共享存储");
    let granted = tr!("已允许访问共享存储");
    let visible = |label| labeled_item(software, |text| text == label).is_some();
    let frame = || render(software, width, height);
    reset(a);
    a.apply(&base());
    settings(a, "library-roots");
    frame();
    assert!(!visible(allow) && !visible(granted), "desktop has no Android prompt");
    app.set_android_storage_access_available(true);
    frame();
    assert!(visible(allow) && !visible(granted));

    let opens = Rc::new(Cell::new(0));
    let count = opens.clone();
    app.on_open_android_storage_access(move || count.set(count.get() + 1));
    click_label(software, allow);
    assert_eq!(opens.get(), 1);
    save("settings/storage-access-not-granted", frame());
    assert!(visible(allow) && !visible(granted), "opening settings does not grant anything");

    app.set_android_storage_access_granted(true);
    save("settings/storage-access-granted", frame());
    assert!(!visible(allow) && visible(granted), "only host-reported permission appears as granted");
    app.set_android_storage_access_granted(false);
    app.set_android_storage_access_available(false);
    material.set_touch(false);
}

fn exercise_import_dialog(
    a: &Rc<SlintManagerAdapter>,
    software: &MinimalSoftwareWindow,
    data: &Data,
    (width, height): (u32, u32),
    save: impl Fn(&str, Vec<u8>),
) {
    let window = a.window();
    let library = window.global::<Library>();
    let app = window.global::<App>();
    let frame = || render(software, width, height);
    let label = tr!("正在导入游戏目录…");
    let modal = || labeled_item(software, |text| text == label).is_some();
    let add = tr!("添加文件夹");
    let cancels = Rc::new(Cell::new(0));
    let count = cancels.clone();
    // Keep the host state importing until the simulated worker completes.
    library.on_cancel_prepare(move || count.set(count.get() + 1));
    reset(a);
    let mut ready = library_model(data);
    ready.library.selected_id = ready.library.games[0].id.clone();
    let mut import = ready.clone();
    import.library.prepare_state = "importing".into();
    import.library.prepare_detail = tr!("正在导入游戏目录：已检查 {directories} 个目录，识别 {games} 部作品…", directories = 18, games = 2);
    import.library.prepare_progress = Some(0.65);
    import.library.can_refresh = false;
    import.library.can_cancel_prepare = true;
    let adds = Rc::new(Cell::new(0));
    let count = adds.clone();
    let weak = Rc::downgrade(a);
    let pending = import.clone();
    library.on_add_directory(move || {
        if let Some(a) = weak.upgrade() {
            count.set(count.get() + 1);
            a.apply(&pending);
        }
    });

    for (i, key) in [None, Some(Key::Escape), Some(Key::Back)].into_iter().enumerate() {
        a.apply(&ready);
        frame();
        click_label(software, add);
        let progress = frame();
        assert!(modal());
        assert!(matching_label(software, |text, item| text == label
            && item.accessible_string_property(AccessibleStringProperty::Value).is_some_and(|v| v == "65%")).is_some());
        let add_item = labeled_item(software, |text| text == add).expect("background action");
        let center = add_item.map_to_native_window(add_item.geometry().center());
        click_at(software, LogicalPosition::new(center.x, center.y));
        assert_eq!(adds.get(), i + 1, "modal shields the library header");
        if let Some(key) = key {
            press(software, key);
        } else {
            save("library-import/progress", progress);
            click_label(software, tr!("取消"));
            save("library-import/cancel-pending", frame());
        }
        press(software, Key::Back);
        press(software, Key::Escape);
        assert_eq!(cancels.get(), i + 1, "cancel is sent only once before terminal state");
        assert!(modal());
        a.apply(&ready);
        frame();
        assert!(!modal());
        assert_eq!(app.get_page(), "library");
        assert_eq!(library.get_selected_id(), ready.library.selected_id.as_str());
        assert_eq!(library.get_games().row_count(), ready.library.games.len());
    }

    click_label(software, tr!("搜索"));
    let mut filtered = ready.clone();
    filtered.library.query = "Harbor".into();
    filtered.library.games.truncate(1);
    a.apply(&filtered);
    import.library.query = filtered.library.query.clone();
    import.library.games.truncate(1);
    a.apply(&import);
    frame();
    assert!(modal());
    a.apply(&filtered);
    save("library-import/search-restored", frame());
    assert!(!modal());
    assert_eq!(app.get_page(), "library-search");
    assert_eq!(library.get_query(), "Harbor");
    assert_eq!(library.get_games().row_count(), 1);

    for state in ["loading-cores", "scanning"] {
        let mut background = filtered.clone();
        background.library.prepare_state = state.into();
        a.apply(&background);
        frame();
        assert!(!modal(), "background scans must stay nonmodal");
    }

    page(a, "onboarding");
    a.apply(&import);
    frame();
    assert!(!modal(), "never interrupt onboarding with an import dialog");
    reset(a);
}

fn exercise_onboarding_storage(
    a: &SlintManagerAdapter,
    software: &MinimalSoftwareWindow,
    (width, height): (u32, u32),
    save: impl Fn(&str, Vec<u8>),
) {
    use shinki_emu_manager_ui_slint::{MaterialWindowAdapter, Settings};

    let window = a.window();
    let state = window.global::<Settings>();
    let material = window.global::<MaterialWindowAdapter>();
    material.set_touch(width < 600);
    let wizard = window.global::<Onboarding>();
    let frame = || render(software, width, height);
    let allow = tr!("允许访问所有文件");
    let granted = tr!("已允许访问共享存储");
    let visible = |label| labeled_item(software, |text| text == label).is_some();
    reset(a);
    page(a, "onboarding");
    wizard.set_step(0);
    frame();
    assert!(!visible(allow) && !visible(granted), "permission entry is Android-only");

    let opened = Rc::new(Cell::new(0));
    let count = opened.clone();
    state.on_open_android_storage_access(move || count.set(count.get() + 1));
    state.set_android_storage_access_available(true);
    save("onboarding/storage-available", frame());
    let action = click_label(software, allow);
    assert_eq!(opened.get(), 1);
    let add = labeled_item(software, |text| text == tr!("添加目录")).expect("folder picker stays available");
    assert!(action.y < add.map_to_native_window(add.geometry().center()).y);
    assert!(!visible(granted), "the click did not grant permission");
    click_label(software, tr!("下一步"));
    assert_eq!(wizard.get_step(), 1, "permission is optional");
    frame();
    click_label(software, tr!("上一步"));
    state.set_android_storage_access_granted(true);
    save("onboarding/storage-granted", frame());
    assert!(visible(granted) && !visible(allow));
    assert!(visible(tr!("添加目录")));
    state.set_android_storage_access_granted(false);
    state.set_android_storage_access_available(false);
    material.set_touch(false);
    reset(a);
}

fn exercise_core_cards(
    a: &SlintManagerAdapter,
    software: &MinimalSoftwareWindow,
    (width, height): (u32, u32),
    save: impl Fn(&str, Vec<u8>),
) {
    use shinki_emu_manager_ui_slint::{CoreDownloadViewModel, CoreViewModel, Cores, MaterialWindowAdapter};

    let window = a.window();
    let cores = window.global::<Cores>();
    let material = window.global::<MaterialWindowAdapter>();
    material.set_touch(width < 600);
    let frame = || render(software, width, height);
    reset(a);
    let core = |id: &str, title: &str, state: &str, version: &str| CoreViewModel {
        id: id.into(), name: title.into(), state: state.into(), version: version.into(),
        engine: "KiriKiri Z".into(), source: "bundled".into(),
        ..Default::default()
    };
    let version = "v0.6.1-beta.12@02c366a5 (adapter e83a23bf)";
    let long_version = "release/extended-runtime-and-scripting-support-2026.11@02c366a5 (adapter e83a23bf)";
    let update = "feature/android-native-video-and-scripting-runtime@79f5c82e (adapter c77383b8)";
    let mut model = base();
    model.cores.items = vec![
        core("next", "Prototype Core", "pending-install", "0.6.1-beta.12"),
        CoreViewModel {
            message: "Missing runtime archive; install the matching core package.".into(),
            removable: true,
            ..core("broken", "Legacy Core", "failed", "0.4.7")
        },
        CoreViewModel {
            update_version: update.into(),
            ..core("long", LONG_EN, "loaded", long_version)
        },
        core("other", LONG_JA, "installed", "feature/02c366a518d278eb@02c366a5 (adapter e83a23bf)"),
        CoreViewModel {
            source: "online".into(),
            ..core("online", "Aurora VN Runtime", "available", version)
        },
    ];
    a.apply(&model);
    page(a, "cores");
    save("cores/mixed-cards", frame());

    let reveal = |label: &str| {
        let position = LogicalPosition::new(if width < 600 { 160.0 } else { 480.0 }, 460.0);
        let bottom_nav = if width < 600 { 80.0 } else { 0.0 };
        for attempt in 0..16 {
            let mut delta_y = if attempt < 8 { -216.0 } else { 216.0 };
            if let Some(item) = labeled_item(software, |text| text == label) {
                let center = item.map_to_native_window(item.geometry().center());
                if center.y > 64.0 && center.y < height as f32 - bottom_nav - 8.0 {
                    return;
                }
                delta_y = if center.y <= 64.0 { 144.0 } else { -144.0 };
            }
            software.dispatch_event(WindowEvent::PointerScrolled { position, delta_x: 0.0, delta_y });
            frame();
        }
        panic!("could not reveal core action {label}");
    };
    reveal(tr!("默认设置"));
    let current = labeled_item(software, |text| text == long_version).expect("full current version is accessible");
    let update_display = format!("→ {update}");
    let next = labeled_item(software, |text| text == update_display).expect("full update version is accessible");
    assert_eq!(current.geometry().size.height, next.geometry().size.height);
    assert!(current.geometry().size.height < 32.0, "version text is a single line");
    save("cores/installed-cards", frame());
    click_label(software, tr!("默认设置"));
    assert_eq!(cores.get_default_plugin(), "long");
    frame();
    reveal(tr!("收起默认设置"));
    click_label(software, tr!("收起默认设置"));
    assert!(cores.get_default_plugin().is_empty());
    frame();

    let clicked = Rc::new(RefCell::new(Vec::new()));
    let calls = clicked.clone();
    cores.on_install_online(move |id| calls.borrow_mut().push(id));
    let update_label = tr!("更新核心");
    reveal(update_label);
    click_label(software, update_label);
    assert_eq!(clicked.borrow()[0], "long");
    model.cores.items[2].download = CoreDownloadViewModel {
        phase: "downloading".into(), received: 2 * 1024, total: 8 * 1024,
        ..Default::default()
    };
    a.apply(&model);
    frame();
    reveal(tr!("取消"));
    save("cores/download-progress", frame());
    let cancelled = Rc::new(RefCell::new(SharedString::default()));
    let response = cancelled.clone();
    cores.on_cancel_download(move |id| *response.borrow_mut() = id);
    click_label(software, tr!("取消"));
    assert_eq!(cancelled.borrow().as_str(), "long");
    reveal(tr!("安装"));
    save("cores/available-cards", frame());
    click_label(software, tr!("安装"));
    assert_eq!(clicked.borrow()[1], "online");
    model.network.allowed = false;
    a.apply(&model);
    reveal(tr!("安装"));
    save("cores/offline-install-disabled", frame());
    click_label(software, tr!("安装"));
    assert_eq!(clicked.borrow().len(), 2, "offline install remains disabled");
    material.set_touch(false);
    reset(a);
}

fn exercise_about_credits(
    a: &SlintManagerAdapter,
    software: &MinimalSoftwareWindow,
    (width, height): (u32, u32),
    opened_urls: &RefCell<Vec<String>>,
    save: impl Fn(&str, Vec<u8>),
) {
    const INVENTORY: &str = include_str!("../../../../build/cores.toml");
    const ABOUT: &str = include_str!("../ui/pages/settings/about.slint");
    const NOTICE: &str = include_str!("../../../../THIRD_PARTY_NOTICES.md");
    for line in INVENTORY.lines().filter(|line| line.starts_with("upstream_identity = ")) {
        let repo = line.split("repo = \"").nth(1).unwrap().split('"').next().unwrap();
        assert!(ABOUT.contains(&format!("url: \"{repo}\"")), "missing About credit for {repo}");
    }

    reset(a);
    a.apply(&base());
    settings(a, "about");
    let app = a.window().global::<App>();
    let frame = || render(software, width, height);
    let picked_path = Rc::new(RefCell::new(SharedString::default()));
    let record = picked_path.clone();
    app.on_open_path(move |key| *record.borrow_mut() = key);
    let weak = a.window().as_weak();
    app.on_load_about_licenses(move || {
        weak.upgrade().unwrap().global::<App>().set_about_license_files(slint::ModelRc::new(
            slint::VecModel::from(vec!["THIRD_PARTY_NOTICES.md".into()]),
        ));
    });
    let weak = a.window().as_weak();
    app.on_read_about_license(move |index| {
        assert_eq!(index, 0);
        weak.upgrade().unwrap().global::<App>().set_about_license_text(NOTICE.into());
    });
    let acknowledgment = tr!("致谢");
    let licenses = tr!("第三方许可");
    let back = tr!("返回");
    let site = |project: &str| tr!("{}：项目主页").replace("{}", project);
    let license = |project: &str| tr!("{}：许可证").replace("{}", project);
    opened_urls.borrow_mut().clear();
    frame();
    click_label(software, licenses);
    assert_eq!(*picked_path.borrow(), "third-party-notices");
    assert_eq!(app.get_settings_category(), "about");
    click_label(software, acknowledgment);
    save("settings/about-credits-start", frame());
    assert_eq!(app.get_settings_category(), "about-credits");
    assert!(opened_urls.borrow().is_empty(), "About and Acknowledgments are in-app pages");
    click_label(software, "Project Shinkirou");
    click_label(software, licenses);
    assert_eq!(app.get_settings_category(), "about-licenses");
    frame();
    click_label(software, tr!("第三方声明"));
    save("settings/about-credits-notice", frame());
    assert_eq!(app.get_settings_category(), "about-license-text");
    assert!(labeled_item(software, |label| label == tr!("许可证正文")).is_some());
    press(software, Key::Back);
    frame();
    assert_eq!(app.get_settings_category(), "about-licenses");
    click_label(software, back);
    frame();
    assert_eq!(app.get_settings_category(), "about-credits");

    let bottom = height as f32 - if width < 600 { 80.0 } else { 0.0 };
    let reveal = |label: &str| {
        for _ in 0..14 {
            if let Some(item) = labeled_item(software, |text| text == label) {
                let rect = item.geometry();
                let center = item.map_to_native_window(rect.center());
                if center.y - rect.size.height / 2.0 >= 64.0
                    && center.y + rect.size.height / 2.0 < bottom {
                    return;
                }
            }
            let position = LogicalPosition::new(if width < 600 { 180.0 } else { 540.0 }, 470.0);
            software.dispatch_event(WindowEvent::PointerScrolled { position, delta_x: 0.0, delta_y: -200.0 });
            frame();
        }
        panic!("credits action is not reachable: {label}");
    };
    reveal(&site("Art3m1s Core"));
    click_label(software, &site("Art3m1s Core"));
    click_label(software, &license("Art3m1s Core"));
    reveal(&site("RFVP"));
    save("settings/about-credits-middle", frame());
    click_label(software, &site("RFVP"));
    reveal(&site("SoftPAL RS (Sena)"));
    save("settings/about-credits-end", frame());
    click_label(software, &site("SoftPAL RS (Sena)"));
    assert_eq!(
        opened_urls.borrow().as_slice(),
        [
            "https://github.com/Project-Shinkirou/ShinkiEMU/graphs/contributors",
            "https://github.com/Alphaly2K/art3m1s-core",
            "https://github.com/Alphaly2K/art3m1s-core/blob/ad90a62cb192b119b7cf8589aa7f87a94bd747c8/LICENSE",
            "https://github.com/xmoezzz/rfvp",
            "https://github.com/xmoezzz/sena-rs",
        ]
    );
    press(software, Key::Escape);
    frame();
    assert_eq!(app.get_page(), "settings");
    assert_eq!(app.get_settings_category(), "about");

    app.set_android_about(true);
    save("settings/about-credits-android-about", frame());
    click_label(software, licenses);
    frame();
    assert_eq!(app.get_settings_category(), "about-licenses");
    click_label(software, back);
    assert_eq!(app.get_settings_category(), "about");
    app.set_android_about(false);
}
