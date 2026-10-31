use super::*;

#[test]
fn native_gpu_particle_in_memory_resume_and_pause() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".effect Firefly glow 20 1000\r\n.effect2 Snow\r\n.wait 1000\r\n.effect end\r\n.effect2 fadeout\r\n.end\r\n");
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(32, 32, image::Rgba([220, 230, 250, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let entries = [
        "glowS.png",
        "glowM.png",
        "glowL.png",
        "snowS.png",
        "snowM.png",
        "snowL.png",
    ]
    .map(|name| (name, png.get_ref().as_slice()));
    fixture::assets(root.path(), "sys", &entries);
    let mut provider = fixture::provider();
    let (_, mut session) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    let mut capture = Capture(Vec::new());
    session.advance(16_666_667, &[]).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    session.advance(64_000_000, &[]).unwrap();
    capture.read(session.acquire_frame(None).unwrap()).unwrap();
    let saved = capture.0.clone();
    assert!(
        saved
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] != [0, 0, 0])
    );
    let checkpoint = capture_transient_vm(&session);
    session.advance(64_000_000, &[]).unwrap();
    capture.read(session.acquire_frame(None).unwrap()).unwrap();
    let later = capture.0.clone();
    assert_ne!(saved, later);
    resume_vm_for_test(&mut session, &checkpoint);
    capture.read(session.acquire_frame(None).unwrap()).unwrap();
    assert_eq!(saved, capture.0);
    session.advance(64_000_000, &[]).unwrap();
    capture.read(session.acquire_frame(None).unwrap()).unwrap();
    assert_eq!(later, capture.0);
    session
        .advance(0, &[RuntimeEvent::WindowSuspended { suspended: true }])
        .unwrap();
    session.advance(64_000_000, &[]).unwrap();
    capture.read(session.acquire_frame(None).unwrap()).unwrap();
    assert_eq!(later, capture.0);
    Box::new(session).close().unwrap();
}
