use super::*;
use shinki_emu_manager::resale_notice::{slot_is_readable, NoticeRasterizer, NoticeTheme};
use shinki_emu_manager_ui_slint::{
    ManagerWindow, NoticeClipRect, NoticeSlotRect, ResaleNoticeState, ResaleNoticeViewModel,
};
use std::{cell::RefCell, rc::Rc, time::Duration};

struct CachedImage {
    key: (u32, u32, NoticeTheme),
    minimum: (f32, f32),
    image: slint::Image,
}

pub(super) struct NoticeHost {
    timer: slint::Timer,
    onboarding: RefCell<Option<CachedImage>>,
    about: RefCell<Option<CachedImage>>,
}

fn current_theme(window: &ManagerWindow) -> NoticeTheme {
    let notice = window.global::<ResaleNoticeState>();
    let surface = notice.get_surface_color();
    let on_surface = notice.get_on_surface_color();
    NoticeTheme {
        dark: notice.get_dark(),
        surface: [surface.red(), surface.green(), surface.blue()],
        on_surface: [on_surface.red(), on_surface.green(), on_surface.blue()],
    }
}

impl NoticeHost {
    pub(super) fn stop(&self) {
        self.timer.stop();
    }

    pub(super) fn presented<C: ManagerController>(
        &self,
        window: &ManagerWindow,
        controller: &RefCell<C>,
    ) -> Result<(), String> {
        let notice = window.global::<ResaleNoticeState>();
        let slot = NoticeSlotRect {
            x: notice.get_onboarding_x(),
            y: notice.get_onboarding_y(),
            width: notice.get_onboarding_width(),
            height: notice.get_onboarding_height(),
            clip: Some(NoticeClipRect {
                x: notice.get_onboarding_clip_x(),
                y: notice.get_onboarding_clip_y(),
                width: notice.get_onboarding_clip_width(),
                height: notice.get_onboarding_clip_height(),
            }),
        };
        let scale = window.window().scale_factor();
        let size = window.window().size();
        let readable = self.onboarding.borrow().as_ref().is_some_and(|cached| {
            cached.key
                == (
                    (slot.width * scale).floor() as u32,
                    scale.to_bits(),
                    current_theme(window),
                )
                && slot_is_readable(
                    notice.get_onboarding_visible()
                        && window.window().is_visible()
                        && !window.window().is_minimized(),
                    slot,
                    cached.minimum,
                    (size.width as f32 / scale, size.height as f32 / scale),
                )
        });
        controller
            .borrow_mut()
            .notice_presented(Instant::now(), readable)
    }
}

fn update_image(
    rasterizer: &NoticeRasterizer,
    cache: &RefCell<Option<CachedImage>>,
    width: f32,
    scale: f32,
    theme: NoticeTheme,
) -> Result<bool, String> {
    if !width.is_finite() || width < 96.0 {
        return Ok(false);
    }
    let key = ((width * scale).floor() as u32, scale.to_bits(), theme);
    if cache
        .borrow()
        .as_ref()
        .is_some_and(|image| image.key == key)
    {
        return Ok(false);
    }
    let raster = rasterizer.rasterize(key.0, scale, theme)?;
    let minimum = (raster.width as f32 / scale, raster.height as f32 / scale);
    let pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        &raster.rgba,
        raster.width,
        raster.height,
    );
    *cache.borrow_mut() = Some(CachedImage {
        key,
        minimum,
        image: slint::Image::from_rgba8(pixels),
    });
    Ok(true)
}

fn apply_state(window: &ManagerWindow, state: &ResaleNoticeViewModel) {
    let notice = window.global::<ResaleNoticeState>();
    notice.set_checked(state.checked);
    notice.set_remaining_seconds(state.remaining_seconds);
    notice.set_acknowledged(state.acknowledged);
}

pub(super) fn install<C: ManagerController>(
    adapter: &Rc<SlintManagerAdapter>,
    controller: &Rc<RefCell<C>>,
) -> Result<Rc<NoticeHost>, String> {
    let rasterizer = NoticeRasterizer::new()?;
    let host = Rc::new(NoticeHost {
        timer: slint::Timer::default(),
        onboarding: RefCell::default(),
        about: RefCell::default(),
    });
    let weak = adapter.window().as_weak();
    let callback_window = weak.clone();
    let callback_controller = controller.clone();
    adapter
        .window()
        .global::<ResaleNoticeState>()
        .on_acknowledge(move |checked| {
            let result = callback_controller.borrow_mut().acknowledge_notice(checked);
            if let Some(window) = callback_window.upgrade() {
                match result {
                    Ok(model) => apply_state(&window, &model.resale_notice),
                    Err(error) => report_error(&window, &error),
                }
            }
        });
    let controller = controller.clone();
    let weak_host = Rc::downgrade(&host);
    host.timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(100),
        move || {
            let (Some(window), Some(host)) = (weak.upgrade(), weak_host.upgrade()) else {
                return;
            };
            let notice = window.global::<ResaleNoticeState>();
            let scale = window.window().scale_factor();
            let theme = current_theme(&window);
            let result = (|| {
                if update_image(
                    &rasterizer,
                    &host.onboarding,
                    notice.get_onboarding_width(),
                    scale,
                    theme,
                )? {
                    let cache = host.onboarding.borrow();
                    let cached = cache.as_ref().unwrap();
                    notice.set_onboarding_content_height(cached.minimum.1);
                    notice.set_onboarding_image(cached.image.clone());
                }
                if update_image(
                    &rasterizer,
                    &host.about,
                    notice.get_about_width(),
                    scale,
                    theme,
                )? {
                    let cache = host.about.borrow();
                    let cached = cache.as_ref().unwrap();
                    notice.set_about_content_height(cached.minimum.1);
                    notice.set_about_image(cached.image.clone());
                }
                let state = controller.borrow().notice_state();
                apply_state(&window, &state);
                if !controller.borrow().onboarding_completed()? {
                    window.global::<App>().set_page("onboarding".into());
                    if !state.acknowledged {
                        window.global::<Onboarding>().set_step(0);
                    }
                }
                // Presented frames, not this timer, advance dwell. Redraw also
                // covers static pages; minimized windows cannot accumulate time.
                if !state.acknowledged {
                    window.window().request_redraw();
                }
                Ok::<_, String>(())
            })();
            if let Err(error) = result {
                report_error(&window, &error);
            }
        },
    );
    Ok(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_refreshes_on_theme_palette_width_and_scale_changes() {
        let rasterizer = NoticeRasterizer::new().unwrap();
        let cache = RefCell::default();
        let light = NoticeTheme {
            dark: false,
            surface: [235, 242, 250],
            on_surface: [20, 28, 35],
        };
        assert!(update_image(&rasterizer, &cache, 600.0, 1.0, light).unwrap());
        assert!(!update_image(&rasterizer, &cache, 600.0, 1.0, light).unwrap());
        let dark = NoticeTheme {
            dark: true,
            surface: [20, 28, 35],
            on_surface: [235, 242, 250],
        };
        assert!(update_image(&rasterizer, &cache, 600.0, 1.0, dark).unwrap());
        assert!(update_image(
            &rasterizer,
            &cache,
            600.0,
            1.0,
            NoticeTheme {
                surface: [22, 30, 37],
                ..dark
            }
        )
        .unwrap());
        assert!(update_image(&rasterizer, &cache, 320.0, 1.0, dark).unwrap());
        assert!(update_image(&rasterizer, &cache, 320.0, 2.0, dark).unwrap());
        assert_eq!(cache.borrow().as_ref().unwrap().minimum.0, 320.0);
    }
}
