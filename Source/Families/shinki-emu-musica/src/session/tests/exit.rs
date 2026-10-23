//! Game-initiated exits reach `SESSION_FINISHED` through the native worker and
//! leave `close` usable; the test process itself stays alive throughout.
use super::*;
use crate::native::harness::NativeSession;
use shinki_emu_abi as abi;

fn serve(request: OpenRequest) -> NativeSession {
    NativeSession::serve(request.session_epoch, move |provider| {
        provider.open_session(request).unwrap().1
    })
}

/// Host pause/resume and late input must not lift a finished game back to life.
fn assert_stays_finished(session: &NativeSession) {
    assert_eq!(session.control(abi::CONTROL_PAUSE), abi::OK);
    assert_eq!(session.control(abi::CONTROL_RESUME), abi::OK);
    // F9 quick-loads a slot and the title Enter would start a new story.
    assert_eq!(session.keys(&["F9", "ArrowDown", "Enter"]), abi::OK);
    std::thread::sleep(Duration::from_millis(50));
    let state = session.wait_for_state(abi::SESSION_FINISHED);
    assert_eq!(state.flags, 0);
    session.first_pixel();
}

#[test]
fn native_abi_script_end_reports_finished_and_closes() {
    let _lock = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".message 1   First\r\n.end\r\n");
    let session = serve(request(root.path(), Sink::default()));
    std::thread::sleep(Duration::from_millis(50));
    // The message waits for input; the script cannot end on its own.
    assert_eq!(session.state().state, abi::SESSION_RUNNING);
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.state().state != abi::SESSION_FINISHED {
        assert!(Instant::now() < deadline, "script end never finished");
        assert_eq!(session.keys(&["Enter"]), abi::OK);
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(session.state().flags, 0);
    assert_stays_finished(&session);
    assert_eq!(session.close(), abi::OK);
}

#[test]
fn native_abi_title_exit_reports_finished_and_closes() {
    let _lock = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    super::title::game(root.path());
    let session = serve(super::title::title_request(root.path()));
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(session.state().state, abi::SESSION_RUNNING);
    // Focus wraps from Start to the bottom Exit row.
    assert_eq!(session.keys(&["ArrowUp", "Enter"]), abi::OK);
    let state = session.wait_for_state(abi::SESSION_FINISHED);
    assert_eq!(state.flags, 0);
    assert_stays_finished(&session);
    assert_eq!(session.close(), abi::OK);
}

#[test]
fn native_abi_paused_session_still_reports_title_exit() {
    let _lock = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    super::title::game(root.path());
    let session = serve(super::title::title_request(root.path()));
    assert_eq!(session.keys(&["ArrowUp", "Enter"]), abi::OK);
    assert_eq!(session.control(abi::CONTROL_PAUSE), abi::OK);
    session.wait_for_state(abi::SESSION_FINISHED);
    assert_eq!(session.close(), abi::OK);
}

/// In title launch mode `.end` only returns to the title; the game keeps running.
#[test]
fn title_mode_script_end_returns_to_title_without_finishing() {
    let _lock = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    super::title::game(root.path());
    let mut provider = fixture::provider();
    let (_, mut session) = provider
        .open_session(super::title::title_request(root.path()))
        .unwrap();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "First");
    let response = session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(
        session.vm.state().system_ui.page,
        crate::MusicaSystemPage::Title
    );
    assert_ne!(response.status, RuntimeStatus::Finished);
    Box::new(session).close().unwrap();
}
