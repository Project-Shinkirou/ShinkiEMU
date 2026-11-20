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
            if let Some(window) = w.window.upgrade() {
                window.window().set_fullscreen(fullscreen);
                window.global::<Session>().set_fullscreen(fullscreen);
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

    struct TestPlatform(Jobs);

    impl Platform for TestPlatform {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
        }

        fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
            Some(Box::new(Proxy(self.0.clone())))
        }
    }

    #[test]
    fn first_pointer_batch_dispatches_without_a_frame_or_timer() {
        let jobs = Jobs::default();
        slint::platform::set_platform(Box::new(TestPlatform(jobs.clone()))).unwrap();
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
}
