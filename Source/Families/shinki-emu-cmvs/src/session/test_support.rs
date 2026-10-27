use super::*;
use std::path::Path;

pub(super) fn game() -> (tempfile::TempDir, Arc<CmvsArchive>) {
    let (root, archive, _) = crate::scene::parent_load_tests::fixture();
    std::fs::write(root.path().join("cmvs.cfg"),
        "[CMVS_SYSTEM_MAIN]\nSCRIPT_INIT_PATH=\nWINDOW_SIZE=9\n[RESOLVING_POWER_INFO]\nWINDOW_WIDTH=16\nWINDOW_HEIGHT=16\nFULL_SCREEN_WIDTH=16\nFULL_SCREEN_HEIGHT=16\n[WINDOW_STATUS]\nWINDOW_MODE=1\n").unwrap();
    write_program(root.path(), "start.ps3", 0);
    write_program(root.path(), "other.ps3", 42);
    crate::scene::parent_load_tests::write_image(root.path(), "red.pb3", 16, 16, [240, 0, 0, 255]);
    crate::scene::parent_load_tests::write_image(root.path(), "blue.pb3", 16, 16, [0, 0, 240, 255]);
    std::fs::create_dir(root.path().join("save")).unwrap();
    (root, Arc::new(archive))
}

pub(super) fn write_program(root: &Path, name: &str, last_word: u16) {
    let mut body = Vec::with_capacity(518);
    body.extend_from_slice(&0x440u16.to_le_bytes());
    body.extend_from_slice(&4u16.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    for _ in (8..512).step_by(2) {
        body.extend_from_slice(&0x2000u16.to_le_bytes());
    }
    body[510..512].copy_from_slice(&(0x2000 | last_word).to_le_bytes());
    body.extend_from_slice(&[1, 2, 3, 4]);
    body.extend_from_slice(&[0, 0]);
    let mut packed = crate::system_save::lzss::compress(&body);
    for byte in &mut packed {
        *byte = byte.rotate_left(1).wrapping_add(0x7c);
    }
    let mut source = b"PS2A".to_vec();
    for word in [
        48u32,
        0,
        0,
        0,
        512,
        4,
        0,
        0,
        packed.len() as u32,
        body.len() as u32,
        0,
    ] {
        source.extend_from_slice(&word.to_le_bytes());
    }
    source.extend(packed);
    std::fs::write(root.join(name), source).unwrap();
}

/// The CMVS lease admits one live session per process, so a fixture session
/// holds the test lock until it drops; the session field drops first.
pub(super) struct TestSession {
    session: CmvsSession,
    _single: parking_lot::MutexGuard<'static, ()>,
}

impl std::ops::Deref for TestSession {
    type Target = CmvsSession;
    fn deref(&self) -> &CmvsSession {
        &self.session
    }
}

impl std::ops::DerefMut for TestSession {
    fn deref_mut(&mut self) -> &mut CmvsSession {
        &mut self.session
    }
}

pub(super) fn open(root: &Path, archive: Arc<CmvsArchive>) -> TestSession {
    let single = crate::family::TEST_SESSION_LOCK.lock();
    let mut session = CmvsSession::new_with_audio(
        archive,
        root.to_path_buf(),
        "start.ps3",
        WindowState {
            width: 16,
            height: 16,
            focused: true,
            visible: true,
        },
        Lease::acquire().unwrap(),
        crate::scene::tests::gpu_instance(),
        || Ok(audio::test_output()),
    )
    .unwrap();
    session.vm.resource_search_paths.insert(
        CmvsResourceSearchBank::Texture,
        BTreeMap::from([(0, String::new())]),
    );
    TestSession {
        session,
        _single: single,
    }
}

pub(super) fn command(session: &mut CmvsSession, id: u16, words: &[u32]) {
    // A helper call begins a new native dispatch batch after any prior yield.
    // Instruction-failure gating remains enforced by the VM itself.
    session.vm.dispatch_stopped = false;
    if let Some(action) = crate::scene::parent_load_tests::command(&mut session.vm, id, words) {
        session.action(action).unwrap();
    }
}

pub(super) fn picture(session: &mut CmvsSession, name: &str, marker: u32) {
    session
        .vm
        .process_string_slots
        .insert(0, name.as_bytes().to_vec());
    command(session, 32, &[0]);
    command(session, 64, &[0]);
    command(session, 48, &[0, 0x8000_0000]);
    command(session, 68, &[0, u32::MAX, 0, 0, 16, 16]);
    session
        .vm
        .script_data_segment_words
        .entry(0)
        .or_default()
        .insert(0, marker);
    render(session);
}

pub(super) fn render(session: &mut CmvsSession) {
    session.frame = session
        .scene
        .render(
            &mut session.vm,
            CmvsTextureSlot::Parent(0),
            &session.archive,
            (16, 16),
            None,
        )
        .unwrap()
        .unwrap();
}

pub(super) fn pixel(session: &CmvsSession, x: usize, y: usize) -> [u8; 4] {
    session.frame.rgba8[(y * 16 + x) * 4..(y * 16 + x + 1) * 4]
        .try_into()
        .unwrap()
}
