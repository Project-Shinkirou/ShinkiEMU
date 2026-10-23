//! A core that ends the game itself (SESSION_FINISHED) must close the session
//! as finished and hand the Manager back to the library.
use super::family_config::tests::{configured_controller, launch_fixture, pump_until};
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

fn opened(controller: &ShinkiEmuManagerController) -> bool {
    controller
        .active
        .as_ref()
        .is_some_and(|active| active.opened)
}

/// Global South plays "space"; the game override binds it to "enter".
fn with_game_mapping(controller: &mut ShinkiEmuManagerController) {
    controller.set_gamepad_binding("south", "space").unwrap();
    controller.set_game_input_override("game", true).unwrap();
    controller
        .set_game_gamepad_binding("game", "south", "enter")
        .unwrap();
}

fn history(controller: &ShinkiEmuManagerController) -> Vec<shinki_emu_manager_core::PlaySessionRecord> {
    controller.library.session_history("game").unwrap()
}

fn assert_returned_to_library(controller: &mut ShinkiEmuManagerController) {
    assert!(controller.active.is_none());
    assert!(controller.runtime_deadline().is_none());
    assert!(controller.physical_keys.is_empty());
    assert!(controller.held_controls.is_empty());
    assert_eq!(
        controller.key_modifiers,
        KeyModifiers {
            shift: false,
            control: false,
            alt: false,
            super_key: false,
        }
    );
    assert!(controller.pending_events.is_empty());
    // The gamepad pump falls back to the global mapping.
    assert_eq!(
        controller.input_mapping().gamepad[&GamepadInput::South],
        "space"
    );
    controller.ensure_no_session().unwrap();
    controller.ensure_owner_idle().unwrap();
    controller
        .set_game_config("game", "launch_mode", "title")
        .unwrap();
}

fn assert_finished_record(controller: &ShinkiEmuManagerController) {
    let record = history(controller).remove(0);
    assert_eq!(record.ended_by, Some(PlaySessionEndReason::Finished));
    let end = record.end_unix_ms.expect("finished session has an end time");
    assert_eq!(record.duration_ms, end - record.start_unix_ms);
    let model = controller.model().unwrap();
    assert_eq!(model.notices.snackbar.text, "游戏已结束");
    assert_eq!(model.selection.history[0].ended_by, "游戏已结束");
}

#[test]
fn a_game_the_core_ends_is_recorded_as_finished_and_returns_to_the_library() {
    let root = tempfile::tempdir().unwrap();
    let finish = Arc::new(AtomicBool::new(false));
    let trigger = finish.clone();
    let mut controller = configured_controller(root.path(), move |fixture| {
        fixture.finish = trigger;
    });
    with_game_mapping(&mut controller);
    let woken = launch_fixture(&mut controller);
    pump_until(&mut controller, &woken, opened);
    assert_eq!(
        controller.input_mapping().gamepad[&GamepadInput::South],
        "enter"
    );
    // A key and a modifier are held when the game ends itself.
    controller.game_input("shift", true, 1.0).unwrap();
    controller.game_input("enter", true, 1.0).unwrap();
    controller.advance_runtime().unwrap();
    assert!(!controller.held_controls.is_empty());
    std::thread::sleep(Duration::from_millis(20));
    finish.store(true, Ordering::Release);
    pump_until(&mut controller, &woken, |controller| {
        controller.active.is_none()
    });
    assert_finished_record(&controller);
    assert!(history(&controller)[0].duration_ms >= 20);
    assert_returned_to_library(&mut controller);

    // The next launch opens a fresh session normally.
    finish.store(false, Ordering::Release);
    let woken = launch_fixture(&mut controller);
    pump_until(&mut controller, &woken, opened);
    controller.leave_game().unwrap();
    pump_until(&mut controller, &woken, |controller| {
        controller.active.is_none()
    });
    let records = history(&controller);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].ended_by, Some(PlaySessionEndReason::Leave));
    assert_eq!(records[1].ended_by, Some(PlaySessionEndReason::Finished));
}

/// The core finishes on the Enter press and rejects the rest of the batch,
/// which carries the releases the host sends when the game stops taking input.
fn finished_while_input_is_withdrawn(withdraw: impl FnOnce(&mut ShinkiEmuManagerController)) {
    let root = tempfile::tempdir().unwrap();
    let mut controller = configured_controller(root.path(), |fixture| {
        fixture.finish_on_key = Some("Enter");
        fixture.reject_input_after_finish = true;
    });
    with_game_mapping(&mut controller);
    let woken = launch_fixture(&mut controller);
    pump_until(&mut controller, &woken, opened);
    controller.game_input("space", true, 1.0).unwrap();
    controller.game_input("enter", true, 1.0).unwrap();
    withdraw(&mut controller);
    // pump_until unwraps every owner update: a crash would fail here.
    pump_until(&mut controller, &woken, |controller| {
        controller.active.is_none()
    });
    assert_finished_record(&controller);
    assert_returned_to_library(&mut controller);
}

#[test]
fn a_game_that_finishes_while_the_menu_is_open_closes_as_finished() {
    // Opening the session menu blocks game input: the host releases held keys.
    finished_while_input_is_withdrawn(|controller| controller.release_inputs().unwrap());
}

#[test]
fn a_game_that_finishes_while_paused_closes_as_finished() {
    // Minimising suspends the session; the lifecycle event flushes at once.
    finished_while_input_is_withdrawn(|controller| {
        controller
            .set_window_state(WindowState {
                width: 1280,
                height: 720,
                scale_factor: 1.0,
                focused: false,
                visible: true,
                suspended: true,
            })
            .unwrap();
    });
}

#[test]
fn an_explicit_close_wins_over_a_finish_observed_while_closing() {
    let root = tempfile::tempdir().unwrap();
    let finish = Arc::new(AtomicBool::new(false));
    let trigger = finish.clone();
    let mut controller = configured_controller(root.path(), move |fixture| {
        fixture.finish = trigger;
    });
    with_game_mapping(&mut controller);
    let woken = launch_fixture(&mut controller);
    pump_until(&mut controller, &woken, opened);
    let epoch = controller.active.as_ref().unwrap().epoch;
    let serial = controller.snackbar.view().serial;
    finish.store(true, Ordering::Release);
    // The owner closes the finished game before the user's leave arrives.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !controller.registry.closed_unconsumed(epoch) {
        assert!(Instant::now() < deadline, "owner did not close");
        let _ = woken.recv_timeout(Duration::from_millis(8));
    }
    controller.leave_game().unwrap();
    assert_eq!(
        controller.active.as_ref().unwrap().closing,
        Some(PlaySessionEndReason::Leave)
    );
    controller.advance_runtime().unwrap();
    assert_eq!(
        history(&controller)[0].ended_by,
        Some(PlaySessionEndReason::Leave)
    );
    assert_eq!(controller.snackbar.view().serial, serial);
    assert_returned_to_library(&mut controller);
}

#[test]
fn a_finished_game_whose_core_fails_to_close_is_still_recorded_as_finished() {
    let root = tempfile::tempdir().unwrap();
    let finish = Arc::new(AtomicBool::new(false));
    let trigger = finish.clone();
    let mut controller = configured_controller(root.path(), move |fixture| {
        fixture.finish = trigger;
        fixture.close_error = true;
    });
    with_game_mapping(&mut controller);
    let woken = launch_fixture(&mut controller);
    pump_until(&mut controller, &woken, opened);
    let epoch = controller.active.as_ref().unwrap().epoch;
    finish.store(true, Ordering::Release);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !controller.registry.closed_unconsumed(epoch) {
        assert!(Instant::now() < deadline, "owner did not close");
        let _ = woken.recv_timeout(Duration::from_millis(8));
    }
    // The cleanup error is still reported to the host.
    let error = controller.advance_runtime().unwrap_err();
    assert!(error.contains("SHINKI_EMU_TEST_CLOSE"), "{error}");
    assert_finished_record(&controller);
    assert_returned_to_library(&mut controller);
}
