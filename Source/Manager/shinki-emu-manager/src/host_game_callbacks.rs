use super::*;
use std::rc::Rc;

fn install_input(
    window: &shinki_emu_manager_ui_slint::ManagerWindow,
    enqueue: impl Fn(&str, bool, f32) -> Result<(), String> + 'static,
    flush: impl Fn() -> Result<(), String> + 'static,
    on_flush_error: impl Fn(&shinki_emu_manager_ui_slint::ManagerWindow, String) + 'static,
) {
    struct Dispatch<F, E> {
        scheduled: Cell<bool>,
        flush: F,
        on_error: E,
    }

    let weak = window.as_weak();
    let dispatch = Rc::new(Dispatch {
        scheduled: Cell::new(false),
        flush,
        on_error: on_flush_error,
    });
    window.global::<Session>().on_input(move |control, pressed, value| {
        if let Err(error) = enqueue(control.as_str(), pressed, value) {
            if let Some(window) = weak.upgrade() {
                report_input_error(&window, &error);
            }
            return;
        }
        if dispatch.scheduled.replace(true) {
            return;
        }

        // A single Slint pointer event sends X, Y, and possibly a button edge.
        // Drain after that input turn, independently of presentation timers.
        let pending = dispatch.clone();
        let window = weak.clone();
        if let Err(error) = slint::spawn_local(async move {
            pending.scheduled.set(false);
            if let Some(window) = window.upgrade()
                && let Err(error) = (pending.flush)()
            {
                (pending.on_error)(&window, error);
            }
        }) {
            dispatch.scheduled.set(false);
            if let Some(window) = weak.upgrade() {
                (dispatch.on_error)(&window, format!("SHINKI_EMU_INPUT_WAKE_FAILED: {error}"));
            }
        }
    });
}

pub(super) fn install<C: ManagerController, R: ShinkiUnderlayRenderer>(
    wiring: &Wiring<C>,
    renderer: &std::rc::Rc<std::cell::RefCell<R>>,
    runtime_schedule: &HostCallbackSlot,
) {
    let Some(window) = wiring.window.upgrade() else {
        return;
    };
    let w = wiring.clone();
    let retry_schedule = runtime_schedule.clone();
    window.global::<Dialogs>().on_retry(move || {
        let Some(window) = w.window.upgrade() else {
            return;
        };
        let result = w.controller.borrow_mut().retry_native();
        let dialogs = window.global::<Dialogs>();
        match result {
            Ok(()) => {
                #[cfg(target_os = "android")]
                if window.global::<Session>().get_phase().as_str() == "opening" {
                    let fullscreen = window.global::<Session>().get_fullscreen();
                    if let Err(error) = window_commands::set_game_fullscreen(&window, fullscreen) {
                        terminate_game(&w.controller, &w.adapter, &window, error);
                        return;
                    }
                }
                dialogs.set_fatal_active(false);
                dialogs.set_fatal_retryable(false);
                fire_host_callback(&retry_schedule);
            }
            Err(error) => {
                let text = describe_error(&error);
                dialogs.set_fatal_title(text.title.into());
                dialogs.set_fatal_detail(text.detail.into());
                dialogs.set_fatal_code(text.code.into());
            }
        }
    });
    let w = wiring.clone();
    let launch_schedule = runtime_schedule.clone();
    window.global::<Library>().on_launch(move |game_id| {
        tracing::info!(event = "shinki.emu.manager.launch.requested");
        let Some(window) = w.window.upgrade() else {
            return;
        };
        let result = window_events::current_window_state(window.window())
            .and_then(|state| w.controller.borrow_mut().set_window_state(state))
            .and_then(|()| w.controller.borrow_mut().launch(game_id.as_str()));
        match result {
            Ok(model) => {
                let mapping = w.controller.borrow().input_mapping();
                if let Err(error) = w.gamepad.borrow_mut().set_mapping(mapping) {
                    terminate_game(&w.controller, &w.adapter, &window, error);
                    return;
                }
                #[cfg(target_os = "android")]
                if let Err(error) = window_commands::set_game_fullscreen(&window, true) {
                    terminate_game(&w.controller, &w.adapter, &window, error);
                    return;
                }
                apply_model(&w.adapter, &model);
                let session = window.global::<Session>();
                session.set_active(true);
                session.set_title("".into());
                let dialogs = window.global::<Dialogs>();
                dialogs.set_fatal_active(false);
                dialogs.set_fatal_retryable(false);
                fire_host_callback(&launch_schedule);
            }
            Err(error) => {
                tracing::error!(event = "shinki.emu.manager.launch.failed", diagnostic_code = %error);
                report_error(&window, &error);
            }
        }
    });
    let w = wiring.clone();
    let leave_schedule = runtime_schedule.clone();
    window.global::<Session>().on_leave(move || {
        let Some(window) = w.window.upgrade() else {
            return;
        };
        let result = w.controller.borrow_mut().leave_game();
        finish_leave(&window, &w.adapter, result);
        fire_host_callback(&leave_schedule);
    });
    let input = wiring.controller.clone();
    let flush = wiring.controller.clone();
    let w = wiring.clone();
    install_input(
        &window,
        move |control, pressed, value| input.borrow_mut().game_input(control, pressed, value),
        move || flush.borrow_mut().flush_game_input(),
        move |window, error| terminate_game(&w.controller, &w.adapter, window, error),
    );
    let w = wiring.clone();
    window.global::<Session>().on_engine_action(move |control| {
        let Some(window) = w.window.upgrade() else {
            return;
        };
        let Ok(control) = u32::try_from(control) else {
            return report_error(&window, "SHINKI_EMU_ENGINE_ACTION_UNAVAILABLE");
        };
        if let Err(error) = w.controller.borrow_mut().engine_action(control) {
            let session = window.global::<Session>();
            session.set_action_message(describe_error(&error).title.into());
            session.set_action_serial(session.get_action_serial().saturating_add(1));
        }
    });
    let w = wiring.clone();
    window
        .global::<Session>()
        .on_set_fullscreen(move |fullscreen| {
            if let Some(window) = w.window.upgrade()
                && let Err(error) = window_commands::set_game_fullscreen(&window, fullscreen)
            {
                report_error(&window, &error);
            }
        });
    filters::install(wiring, renderer);
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::platform::{
        EventLoopProxy, Platform, WindowAdapter,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    };
    use std::{cell::RefCell, sync::Mutex};

    type Jobs = Arc<Mutex<Vec<Box<dyn FnOnce() + Send>>>>;

    struct Proxy(Jobs);

    impl EventLoopProxy for Proxy {
        fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
            Ok(())
        }

        fn invoke_from_event_loop(
            &self,
            job: Box<dyn FnOnce() + Send>,
        ) -> Result<(), slint::EventLoopError> {
            self.0.lock().unwrap().push(job);
            Ok(())
        }
    }

    struct TestPlatform {
        jobs: Option<Jobs>,
        window: Rc<MinimalSoftwareWindow>,
    }

    impl Platform for TestPlatform {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(self.window.clone())
        }

        fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
            self.jobs.as_ref().map(|jobs| Box::new(Proxy(jobs.clone())) as Box<dyn EventLoopProxy>)
        }
    }

    #[test]
    fn first_pointer_batch_dispatches_without_a_frame_or_timer() {
        let jobs = Jobs::default();
        slint::platform::set_platform(Box::new(TestPlatform {
            jobs: Some(jobs.clone()),
            window: MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer),
        }))
        .unwrap();
        let window = shinki_emu_manager_ui_slint::ManagerWindow::new().unwrap();
        let pending = Rc::new(RefCell::new(Vec::new()));
        let delivered = Rc::new(RefCell::new(Vec::new()));
        let submit = pending.clone();
        let flush_pending = pending.clone();
        let flush_delivered = delivered.clone();
        install_input(
            &window,
            move |control, pressed, value| {
                submit.borrow_mut().push((control.to_owned(), pressed, value));
                Ok(())
            },
            move || {
                let batch = std::mem::take(&mut *flush_pending.borrow_mut());
                if !batch.is_empty() {
                    flush_delivered.borrow_mut().push(batch);
                }
                Ok(())
            },
            |_, error| panic!("{error}"),
        );
        let session = window.global::<Session>();
        for (previous_batches, edge) in [true, false].into_iter().enumerate() {
            session.invoke_input("pointer.x".into(), true, 569.0);
            session.invoke_input("pointer.y".into(), true, 646.0);
            session.invoke_input("pointer.primary".into(), edge, 1.0);

            // A UI input turn must never dispatch the intermediate X/Y state.
            assert_eq!(delivered.borrow().len(), previous_batches);
            let scheduled = std::mem::take(&mut *jobs.lock().unwrap());
            assert_eq!(scheduled.len(), 1, "input must post one event-loop wake");
            for job in scheduled {
                job();
            }
            assert!(jobs.lock().unwrap().is_empty(), "flush must not self-reschedule");
            assert_eq!(
                delivered.borrow().last().unwrap().as_slice(),
                [
                    ("pointer.x".to_owned(), true, 569.0),
                    ("pointer.y".to_owned(), true, 646.0),
                    ("pointer.primary".to_owned(), edge, 1.0),
                ],
            );
        }
    }

    #[test]
    fn session_finish_background_poll_restores_the_library_surface() {
        use slint::platform::WindowEvent;

        const WIDTH: usize = 360;
        const HEIGHT: usize = 780;
        let software = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(TestPlatform {
            jobs: None,
            window: software.clone(),
        }))
        .unwrap();
        let adapter = Rc::new(SlintManagerAdapter::new().unwrap());
        let weak = Rc::downgrade(&adapter);
        let window = adapter.window();
        window.global::<App>().set_page("library-search".into());
        software.set_size(slint::PhysicalSize::new(WIDTH as u32, HEIGHT as u32));
        software.dispatch_event(WindowEvent::Resized {
            size: slint::LogicalSize::new(WIDTH as f32, HEIGHT as f32),
        });

        let capture = |name: &str| {
            let mut pixels = vec![slint::Rgb8Pixel::default(); WIDTH * HEIGHT];
            software.request_redraw();
            assert!(software.draw_if_needed(|renderer| {
                renderer.render(&mut pixels, WIDTH);
            }));
            if let Some(dir) = std::env::var_os("SHINKI_UI_SCREENSHOT_DIR") {
                use std::io::Write;
                let name = std::path::PathBuf::from(dir).join(format!("session-finish-{name}.ppm"));
                let mut file = std::io::BufWriter::new(std::fs::File::create(name).unwrap());
                write!(file, "P6\n{WIDTH} {HEIGHT}\n255\n").unwrap();
                for pixel in &pixels {
                    file.write_all(&[pixel.r, pixel.g, pixel.b]).unwrap();
                }
                file.flush().unwrap();
            }
            pixels
        };
        let mut model = ManagerViewModel::default();
        model.app.appearance.reduce_motion = true;
        adapter.apply(&model);
        let library = capture("library");

        model.session.launch_title = "Rewrite+".into();
        adapter.set_session_active(true);
        apply_session_progress(window, &weak, Some(model.clone()), true);
        assert!(window.global::<Session>().get_active());
        let opening = capture("opening");
        let deltas = |seen: &[slint::Rgb8Pixel]| {
            seen.iter()
                .zip(&library)
                .take(WIDTH * 340)
                .filter(|(a, b)| a != b)
                .count()
        };
        let opening_delta = deltas(&opening);
        assert!(opening_delta > 1_000, "opening must cover the library");

        model.session.phase = "running".into();
        apply_session_progress(window, &weak, Some(model.clone()), true);
        let mut stage = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(1, 1);
        stage.make_mut_slice()[0] = slint::Rgb8Pixel { r: 242, g: 39, b: 51 };
        apply_stage_image(
            window,
            slint::Image::from_rgb8(stage),
            (1, 1),
            (1, 1),
            Viewport2D { x: 0, y: 0, width: 1, height: 1 },
        );
        let running = capture("running");
        assert_eq!(
            running[(HEIGHT / 2) * WIDTH + WIDTH / 2],
            slint::Rgb8Pixel { r: 242, g: 39, b: 51 },
        );

        // The background poll consumes the terminal model before any runtime
        // timer. Inactive session models still carry the default opening phase.
        model.session = Default::default();
        model.notices.snackbar.text = "Game ended".into();
        model.notices.snackbar.serial += 1;
        apply_session_progress(window, &weak, Some(model), false);
        let returned = capture("returned");
        // The toast and refocused controls can alter pixels even if the
        // underlying library is the same. A stale opening view is full-screen.
        let returned_delta = deltas(&returned);
        assert!(
            returned_delta * 4 < opening_delta,
            "library pixel delta {returned_delta}, opening delta {opening_delta}",
        );
        assert!(!window.global::<Session>().get_active());
        assert_eq!(window.global::<Session>().get_frame().size().width, 0);
        assert_eq!(window.global::<App>().get_page().as_str(), "library-search");
        assert_eq!(window.global::<Notices>().get_snackbar_text().as_str(), "Game ended");

        // A nonterminal update never tears down a running or fatal session.
        adapter.set_session_active(true);
        apply_fatal(window, "SHINKI_EMU_TEST_FATAL", true);
        apply_session_progress(window, &weak, None, true);
        assert!(window.global::<Dialogs>().get_fatal_active());
        assert!(window.global::<Dialogs>().get_fatal_retryable());
        assert!(window.global::<Session>().get_active());

        // Explicit Leave remains the same route and reveals the same page.
        finish_leave(window, &weak, Ok(ManagerViewModel::default()));
        assert!(!window.global::<Dialogs>().get_fatal_active());
        assert!(!window.global::<Session>().get_active());
        assert_eq!(window.global::<App>().get_page().as_str(), "library-search");
    }

    #[test]
    fn android_game_window_centers_a_wide_frame_without_stretch() {
        use slint::platform::WindowEvent;

        const WIDTH: usize = 800;
        const HEIGHT: usize = 360;
        const GAME: slint::Rgb8Pixel = slint::Rgb8Pixel { r: 242, g: 39, b: 51 };
        const BLACK: slint::Rgb8Pixel = slint::Rgb8Pixel { r: 0, g: 0, b: 0 };
        let software = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(TestPlatform {
            jobs: None,
            window: software.clone(),
        }))
        .unwrap();
        let adapter = SlintManagerAdapter::new().unwrap();
        let window = adapter.window();
        software.set_size(slint::PhysicalSize::new(WIDTH as u32, HEIGHT as u32));
        software.dispatch_event(WindowEvent::Resized {
            size: slint::LogicalSize::new(WIDTH as f32, HEIGHT as f32),
        });
        let session = window.global::<Session>();
        session.set_android_game_window(true);
        session.set_active(true);
        session.set_phase("running".into());
        let mut stage = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(16, 9);
        stage.make_mut_slice().fill(GAME);
        apply_stage_image(
            window,
            slint::Image::from_rgb8(stage),
            (16, 9),
            (16, 9),
            Viewport2D { x: 0, y: 0, width: 16, height: 9 },
        );
        let mut pixels = vec![slint::Rgb8Pixel::default(); WIDTH * HEIGHT];
        software.request_redraw();
        assert!(software.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, WIDTH);
        }));

        // 16:9 at height 360 is exactly 640 pixels wide. Keep the whole
        // source visible, centered with 80 black pixels on each side.
        for line in pixels.chunks_exact(WIDTH) {
            assert!(line[..80].iter().all(|pixel| *pixel == BLACK));
            assert!(line[80..720].iter().all(|pixel| *pixel == GAME));
            assert!(line[720..].iter().all(|pixel| *pixel == BLACK));
        }
    }
}
