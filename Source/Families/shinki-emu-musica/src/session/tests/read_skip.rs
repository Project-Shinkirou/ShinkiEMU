use super::*;
fn read_flags(root: &std::path::Path) -> Option<crate::runtime::ReadBitmap> {
    let path = root.join(".shinki/musica/minori/夏空のペルセウス/test.tbr");
    std::fs::read(path)
        .ok()
        .map(|bytes| crate::minori_data::decode_read_flags(&bytes, 0).unwrap())
}

#[test]
fn native_gpu_read_skip_follows_tbr_across_load_and_restart() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".label repeat\r\n.message 1  speaker First\r\n.message 2  speaker Second\r\n.goto repeat\r\n",
    );
    let mut provider = fixture::provider();
    let (_, mut session) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    session.advance(16_666_667, &[]).unwrap();
    session.advance(0, &[key(KeyCode::S)]).unwrap();
    session.advance(100_000_000, &[]).unwrap();
    // Skip stops on unread messages.
    assert_eq!(session.message.as_ref().unwrap().0, "First");
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    session.advance(100_000_000, &[]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "Second");
    session.save(0).unwrap();
    // Only teardown, `.chain` and loads write `.tbr`.
    assert_eq!(read_flags(root.path()), None);
    let before = session.vm.state().backlog.len();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    session.advance(100_000_000, &[]).unwrap();
    assert!(session.vm.state().backlog.len() > before + 2);
    session.advance(0, &[key(KeyCode::S)]).unwrap();
    session.load(0).unwrap();
    assert_eq!(read_flags(root.path()).unwrap()[0], 0x60);
    assert_eq!(
        session.vm.state().system_ui.play_mode,
        crate::MusicaPlayMode::Skip
    );
    assert!(session.vm.active_message_is_read().unwrap());
    assert!(session.vm.state().read_message_identities.is_empty());
    Box::new(session).close().unwrap();
    let (_, mut session) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    session.advance(16_666_667, &[]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "First");
    assert!(session.vm.active_message_is_read().unwrap());
    Box::new(session).close().unwrap();
}

#[test]
fn native_gpu_globals_persist_in_system_dat_across_old_slot_and_cold_restart() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".message 1  speaker Before\r\n.setglobal REN_CLEAR = 1\r\n.message 2  speaker After\r\n.end\r\n");
    let mut provider = fixture::provider();
    let (_, mut session) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    session.advance(16_666_667, &[]).unwrap();
    session.save(20).unwrap();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.vm.state().gallery_unlocks.len(), 1);
    let system = root
        .path()
        .join(".shinki/musica/minori/夏空のペルセウス/system.dat");
    let decoded = crate::minori_data::decode_system(
        &std::fs::read(&system).unwrap(),
        crate::minori_data::SystemFormat::Blowfish,
    )
    .unwrap();
    assert_eq!(decoded.get(b"REN_CLEAR".as_slice()).unwrap(), b"1");
    session.load(20).unwrap();
    assert_eq!(session.vm.state().global_variables["REN_CLEAR"], 1);
    assert_eq!(session.vm.state().gallery_unlocks.len(), 1);
    Box::new(session).close().unwrap();
    let (_, session) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    assert_eq!(session.vm.state().global_variables["REN_CLEAR"], 1);
    assert_eq!(session.vm.state().gallery_unlocks.len(), 1);
    Box::new(session).close().unwrap();
    std::fs::write(&system, b"damaged").unwrap();
    assert!(
        provider
            .open_session(request(root.path(), Sink::default()))
            .is_err()
    );
    assert_eq!(std::fs::read(system).unwrap(), b"damaged");
}
