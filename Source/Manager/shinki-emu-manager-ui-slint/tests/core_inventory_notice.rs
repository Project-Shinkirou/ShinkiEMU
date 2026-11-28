use std::{path::PathBuf, rc::Rc};

use shinki_emu_manager_ui_slint::{
    ManagerViewModel, NoticeViewModel, Notices, SlintManagerAdapter, describe_error, i18n,
    report_error,
};
use slint::{
    ComponentHandle,
    platform::{
        Platform, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
};

struct SoftwarePlatform(Rc<MinimalSoftwareWindow>);

impl Platform for SoftwarePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn render(window: &MinimalSoftwareWindow, width: u32, height: u32) -> Vec<u8> {
    window.set_size(slint::PhysicalSize::new(width, height));
    window.dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(width as f32, height as f32),
    });
    let mut pixels = vec![slint::Rgb8Pixel { r: 0, g: 0, b: 0 }; width as usize * height as usize];
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        assert!(window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        }));
    }
    pixels
        .iter()
        .flat_map(|pixel| [pixel.r, pixel.g, pixel.b])
        .collect()
}

#[test]
fn inventory_detail_renders_with_three_host_notices_and_clears_after_repair() {
    let software = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(SoftwarePlatform(software.clone()))).unwrap();
    let adapter = SlintManagerAdapter::new().unwrap();
    let window = adapter.window();
    window.show().unwrap();
    const ID: &str = "inventory-test";
    const CODE: &str = "SHINKI_EMU_CORE_PENDING_APPLY";
    let mut model = ManagerViewModel::default();
    model.app.appearance.reduce_motion = true;
    model.app.appearance.theme_mode = "dark".into();
    model.library.prepare_state = "ready".into();
    let output = std::env::var_os("SHINKI_UI_SCREENSHOT_DIR").map(PathBuf::from);
    if let Some(dir) = &output {
        std::fs::create_dir_all(dir).unwrap();
    }
    let languages: &[&str] = if output.is_some() {
        &["zh-CN", "en", "ja"]
    } else {
        &["en"]
    };
    let sizes: &[(u32, u32)] = if output.is_some() {
        &[(1280, 800), (360, 780)]
    } else {
        &[(360, 780)]
    };
    for &language in languages {
        model.app.appearance.language = language.into();
        i18n::apply(language);
        for raw in [
            "SHINKI_EMU_LOG_READ",
            "SHINKI_EMU_METADATA_NETWORK",
            "SHINKI_EMU_OPTICAL_NO_MEDIA",
        ] {
            report_error(window, raw);
        }
        let text = describe_error(CODE);
        for &(width, height) in sizes {
            model.notices.priority = None;
            adapter.apply(&model);
            let repaired = render(&software, width, height);

            model.notices.priority = Some(NoticeViewModel {
                id: ID.into(),
                severity: "error".into(),
                title: text.title.clone(),
                detail: format!("{}\ncores/pending: NotADirectory", text.detail),
                code: CODE.into(),
                ..NoticeViewModel::default()
            });
            adapter.apply(&model);
            let visible = render(&software, width, height);
            model.notices.priority.as_mut().unwrap().detail =
                format!("{}\ncores/pending: PermissionDenied", text.detail);
            adapter.apply(&model);
            assert!(
                visible != render(&software, width, height),
                "changing the raw cause must change the rendered card"
            );
            if let Some(dir) = &output {
                for (state, pixels) in [("blocked", &visible), ("repaired", &repaired)] {
                    image::save_buffer(
                        dir.join(format!("inventory-{state}-{language}-{width}x{height}.png")),
                        pixels,
                        width,
                        height,
                        image::ColorType::Rgb8,
                    )
                    .unwrap();
                }
            }

            window.global::<Notices>().invoke_dismiss(ID.into());
            adapter.apply(&model);
            assert!(
                repaired == render(&software, width, height),
                "dismissed active notice reappeared on refresh"
            );
            model.notices.priority = None;
            adapter.apply(&model);
            assert!(
                repaired == render(&software, width, height),
                "repair left a stale notice"
            );
        }
    }
    window.hide().unwrap();
}
