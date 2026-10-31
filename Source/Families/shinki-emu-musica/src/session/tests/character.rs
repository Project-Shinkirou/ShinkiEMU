use super::*;
#[test]
fn native_gpu_character_mirror_transition_wait_and_in_memory_resume() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".char load -1 Stand.png\r\n.char pos 1 16 0\r\n.char trans 1 100 0\r\n.wait 1000\r\n.end\r\n");
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_fn(16, 16, |x, _| {
        image::Rgba(if x < 8 {
            [255, 0, 0, 255]
        } else {
            [0, 255, 0, 255]
        })
    })
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    fixture::stand(root.path(), png.get_ref());
    let mut provider = fixture::provider();
    let (_, mut session) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    let mut capture = Capture(Vec::new());
    session.advance(16_666_667, &[]).unwrap();
    session.advance(16_666_667, &[]).unwrap();
    capture.read(session.acquire_frame(None).unwrap()).unwrap();
    let index = (704 * 1280 + 8) * 4;
    assert_eq!(&capture.0[index..index + 4], &[0, 255, 0, 255]);
    session.advance(16_666_667, &[]).unwrap();
    assert_eq!(
        session.advance(40_000_000, &[]).unwrap().status,
        RuntimeStatus::Waiting
    );
    capture.read(session.acquire_frame(None).unwrap()).unwrap();
    let saved = capture.0.clone();
    assert!(saved[index + 1] > 0 && saved[index + 1] < 255);
    let checkpoint = capture_transient_vm(&session);
    session.advance(40_000_000, &[]).unwrap();
    capture.read(session.acquire_frame(None).unwrap()).unwrap();
    let later = capture.0.clone();
    assert!(later[index + 1] < saved[index + 1]);
    resume_vm_for_test(&mut session, &checkpoint);
    capture.read(session.acquire_frame(None).unwrap()).unwrap();
    assert_eq!(capture.0, saved);
    session.advance(40_000_000, &[]).unwrap();
    capture.read(session.acquire_frame(None).unwrap()).unwrap();
    assert_eq!(capture.0, later);
    session.advance(40_000_000, &[]).unwrap();
    capture.read(session.acquire_frame(None).unwrap()).unwrap();
    assert_eq!(&capture.0[index..index + 4], &[0, 0, 0, 255]);
    Box::new(session).close().unwrap();
}
