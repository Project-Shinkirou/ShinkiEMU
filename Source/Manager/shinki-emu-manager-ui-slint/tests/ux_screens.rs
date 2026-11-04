//! Renders the real `ManagerWindow` through the software renderer for the
//! core download, 全部更新 and translation provider screens.
//!
//! Set `SHINKI_UI_SCREENSHOT_DIR` to keep PNGs of every state in zh-CN, en
//! and ja at 1280x800 and 360x780; without it zh-CN is rendered at 360x780
//! only. All data is synthetic.

use std::{
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use shinki_emu_manager_ui_slint::{
    App, CoreBatchViewModel, CoreDownloadViewModel, CoreViewModel, ManagerViewModel,
    ProviderEditor, Settings, SlintManagerAdapter, TranslationFormViewModel, TranslationProvider,
    TranslationProviderViewModel, i18n, tr,
};
use slint::{
    ComponentHandle, Model,
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
    // Indeterminate progress never stops animating, so it ends at the cap.
    for frame in 0..60 {
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

fn core(
    id: &str,
    name: &str,
    state: &str,
    update: &str,
    download: CoreDownloadViewModel,
) -> CoreViewModel {
    CoreViewModel {
        id: id.into(),
        name: name.into(),
        engine: name.into(),
        version: "0.1.0-beta.8".into(),
        state: state.into(),
        source: if state == "available" {
            "online"
        } else {
            "user"
        }
        .into(),
        removable: state != "available",
        update_version: update.into(),
        download,
        ..Default::default()
    }
}

fn downloading(received: u64, total: u64, speed: u64) -> CoreDownloadViewModel {
    CoreDownloadViewModel {
        phase: "downloading".into(),
        received,
        total,
        bytes_per_second: speed,
        error: String::new(),
    }
}

fn phase(name: &str) -> CoreDownloadViewModel {
    CoreDownloadViewModel {
        phase: name.into(),
        ..Default::default()
    }
}

/// (file id, cores view)
fn core_states() -> Vec<(&'static str, ManagerViewModel)> {
    let mut idle = ManagerViewModel::default();
    idle.cores.items = vec![
        core(
            "shinki.emu.alpha",
            "Alpha",
            "loaded",
            "0.1.0-beta.9",
            Default::default(),
        ),
        core(
            "shinki.emu.beta",
            "Beta",
            "loaded",
            "0.1.0-beta.9",
            Default::default(),
        ),
        core(
            "shinki.emu.gamma",
            "Gamma",
            "loaded",
            "",
            Default::default(),
        ),
        core(
            "shinki.emu.delta",
            "Delta",
            "available",
            "",
            Default::default(),
        ),
    ];
    idle.cores.updates_available = 2;
    idle.cores.update_all_count = 2;

    let mut running = idle.clone();
    running.cores.items = vec![
        core(
            "shinki.emu.alpha",
            "Alpha",
            "loaded",
            "0.1.0-beta.9",
            downloading(3_355_443, 8_808_038, 1_258_291),
        ),
        core(
            "shinki.emu.beta",
            "Beta",
            "loaded",
            "0.1.0-beta.9",
            phase("queued"),
        ),
        core(
            "shinki.emu.gamma",
            "Gamma",
            "loaded",
            "",
            Default::default(),
        ),
        core(
            "shinki.emu.delta",
            "Delta",
            "available",
            "",
            phase("verifying"),
        ),
        core(
            "shinki.emu.epsilon",
            "Epsilon",
            "available",
            "",
            downloading(524_288, 0, 0),
        ),
    ];
    running.cores.update_all_count = 0;
    running.cores.batch = CoreBatchViewModel {
        state: "running".into(),
        total: 2,
        progress: 0.19,
        ..Default::default()
    };

    let mut finished = idle.clone();
    finished.cores.restart_required = true;
    finished.cores.items = vec![
        core(
            "shinki.emu.alpha",
            "Alpha",
            "pending-update",
            "",
            Default::default(),
        ),
        core(
            "shinki.emu.beta",
            "Beta",
            "loaded",
            "0.1.0-beta.9",
            CoreDownloadViewModel {
                phase: "failed".into(),
                error: shinki_emu_manager_ui_slint::describe_error(
                    "SHINKI_EMU_CORE_DOWNLOAD_HASH_MISMATCH",
                )
                .title,
                ..Default::default()
            },
        ),
        core(
            "shinki.emu.gamma",
            "Gamma",
            "loaded",
            "",
            Default::default(),
        ),
        core(
            "shinki.emu.delta",
            "Delta",
            "available",
            "",
            Default::default(),
        ),
    ];
    finished.cores.updates_available = 1;
    finished.cores.update_all_count = 1;
    finished.cores.batch = CoreBatchViewModel {
        state: "finished".into(),
        total: 2,
        succeeded: 1,
        failed: 1,
        progress: 1.0,
        ..Default::default()
    };
    vec![
        ("cores-update-all", idle),
        ("cores-downloading", running),
        ("cores-batch-finished", finished),
    ]
}

fn provider(
    id: &str,
    name: &str,
    preset: &str,
    active: bool,
    url: &str,
    model: &str,
) -> TranslationProviderViewModel {
    TranslationProviderViewModel {
        id: id.into(),
        name: name.into(),
        preset: preset.into(),
        active,
        form: TranslationFormViewModel {
            provider: "openai".into(),
            base_url: url.into(),
            openai_api: "chat_completions".into(),
            model: model.into(),
            language: "zh-CN".into(),
            timeout_s: 15,
            custom_prompt: String::new(),
            context_segments: 8,
            context_chars: 6000,
        },
    }
}

fn translation_model() -> ManagerViewModel {
    let mut model = ManagerViewModel::default();
    model.settings.translation_enabled = true;
    model.settings.translation_providers = vec![
        provider(
            "provider-1",
            "OpenAI",
            "openai",
            false,
            "https://api.openai.com/v1",
            "gpt-4.1-mini",
        ),
        provider(
            "provider-2",
            "DeepSeek",
            "deepseek",
            true,
            "https://api.deepseek.com/v1",
            "deepseek-chat",
        ),
        provider(
            "provider-3",
            "Ollama",
            "ollama",
            false,
            "http://localhost:11434/v1",
            "qwen2.5:14b",
        ),
    ];
    model
}

fn base(model: &mut ManagerViewModel, language: &str) {
    model.app.version = "0.1.0-beta.8".into();
    model.app.appearance.language = language.into();
    model.app.appearance.theme_mode = "dark".into();
    model.app.appearance.reduce_motion = true;
    model.resale_notice.acknowledged = true;
    model.library.prepare_state = "ready".into();
    model.network.allowed = true;
}

fn reset_editor(adapter: &SlintManagerAdapter) {
    let editor = adapter.window().global::<ProviderEditor>();
    editor.invoke_close();
    editor.set_confirm_delete(TranslationProvider::default());
}

#[test]
fn core_and_translation_provider_screens_render() {
    let software = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(SoftwarePlatform {
        window: software.clone(),
        start: Instant::now(),
    }))
    .expect("software platform");
    let adapter = SlintManagerAdapter::new().expect("manager window");

    let output = std::env::var_os("SHINKI_UI_SCREENSHOT_DIR").map(PathBuf::from);
    let (languages, sizes): (&[&str], &[(u32, u32)]) = if output.is_some() {
        (&["zh-CN", "en", "ja"], &[(1280, 800), (360, 780)])
    } else {
        (&["zh-CN"], &[(360, 780)])
    };
    if let Some(dir) = &output {
        std::fs::create_dir_all(dir).expect("screenshot dir");
    }
    adapter.window().show().expect("show");
    let save = |id: &str, language: &str, (width, height): (u32, u32), rgba: &[u8]| {
        if let Some(dir) = &output {
            image::RgbaImage::from_raw(width, height, rgba.to_vec())
                .expect("frame size")
                .save(dir.join(format!("{id}-{language}-{width}x{height}.png")))
                .expect("write png");
        }
    };
    for &language in languages {
        i18n::apply(language);
        for &size in sizes {
            let mut frames = Vec::new();
            for (id, mut model) in core_states() {
                base(&mut model, language);
                adapter.apply(&model);
                let app = adapter.window().global::<App>();
                app.set_page("cores".into());
                let rgba = render(&software, size.0, size.1);
                save(id, language, size, &rgba);
                frames.push(rgba);
                app.set_page("library".into());
            }
            assert!(frames[0] != frames[1] && frames[1] != frames[2] && frames[0] != frames[2]);

            let mut model = translation_model();
            base(&mut model, language);
            adapter.apply(&model);
            let app = adapter.window().global::<App>();
            let editor = adapter.window().global::<ProviderEditor>();
            let card = |index: usize| {
                adapter
                    .window()
                    .global::<Settings>()
                    .get_translation_providers()
                    .row_data(index)
                    .expect("card")
            };
            let shots: [(&str, &dyn Fn()); 5] = [
                ("translation-cards", &|| {}),
                ("translation-presets", &|| editor.invoke_add()),
                ("translation-edit", &|| editor.invoke_edit(card(1))),
                ("translation-discard", &|| {
                    editor.invoke_edit(card(0));
                    editor.set_dirty(true);
                    editor.invoke_request_close();
                }),
                ("translation-delete", &|| editor.set_confirm_delete(card(2))),
            ];
            let mut translation_frames = Vec::new();
            for (id, setup) in shots {
                app.set_settings_category("translation".into());
                app.set_page("settings".into());
                setup();
                let rgba = render(&software, size.0, size.1);
                save(id, language, size, &rgba);
                translation_frames.push(rgba);
                reset_editor(&adapter);
                app.set_page("library".into());
            }
            for (index, left) in translation_frames.iter().enumerate() {
                for right in &translation_frames[index + 1..] {
                    assert!(
                        left != right,
                        "{language} {size:?}: two translation states drew the same frame"
                    );
                }
            }

            // A failed model fetch is shown inside the editor of its card.
            let mut failed = translation_model();
            base(&mut failed, language);
            failed.settings.translation_test_for = "provider-2".into();
            failed.settings.translation_status = tr!(
                "连接失败：{error}",
                error = tr!(
                    "API Key 无效或没有权限（HTTP {code}），请检查密钥",
                    code = 401
                )
            );
            adapter.apply(&failed);
            app.set_settings_category("translation".into());
            app.set_page("settings".into());
            editor.invoke_edit(card(1));
            let rgba = render(&software, size.0, size.1);
            save("translation-fetch-failed", language, size, &rgba);
            reset_editor(&adapter);
            app.set_page("library".into());
        }
    }
    adapter.window().hide().expect("hide");
}
