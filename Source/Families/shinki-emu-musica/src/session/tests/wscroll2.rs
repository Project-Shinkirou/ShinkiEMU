use super::*;

#[test]
fn native_gpu_wscroll2_resumes_in_memory_state_and_suspends_animation() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".stage BG.png BG.png 0 0\r\n.effect WScroll2 sync:walk.txt 60 -8\r\n.wait 1000\r\n.effect end\r\n.end\r\n");
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_fn(1280, 720, |x, _| {
        image::Rgba([(x % 251) as u8, 30, 90, 255])
    })
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    fixture::asset(root.path(), "bg", "BG.png", png.get_ref());
    fixture::asset(root.path(), "st", "walk.txt", b"13\r\n16\r\n");
    let mut provider = fixture::provider();
    let (_, mut session) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    let mut frame = Capture(Vec::new());
    session.advance(16_666_667, &[]).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    assert_eq!(
        session.advance(40_000_000, &[]).unwrap().status,
        RuntimeStatus::Waiting
    );
    frame.read(session.acquire_frame(None).unwrap()).unwrap();
    let saved = frame.0.clone();
    let checkpoint = capture_transient_vm(&session);
    session.advance(40_000_000, &[]).unwrap();
    frame.read(session.acquire_frame(None).unwrap()).unwrap();
    let later = frame.0.clone();
    assert_ne!(later, saved);
    resume_vm_for_test(&mut session, &checkpoint);
    frame.read(session.acquire_frame(None).unwrap()).unwrap();
    assert_eq!(frame.0, saved);
    session.advance(40_000_000, &[]).unwrap();
    frame.read(session.acquire_frame(None).unwrap()).unwrap();
    assert_eq!(frame.0, later);
    session
        .advance(0, &[RuntimeEvent::WindowSuspended { suspended: true }])
        .unwrap();
    session.advance(40_000_000, &[]).unwrap();
    frame.read(session.acquire_frame(None).unwrap()).unwrap();
    assert_eq!(frame.0, later);
    session
        .advance(0, &[RuntimeEvent::WindowSuspended { suspended: false }])
        .unwrap();
    session.advance(40_000_000, &[]).unwrap();
    frame.read(session.acquire_frame(None).unwrap()).unwrap();
    assert_ne!(frame.0, later);
    Box::new(session).close().unwrap();
}
