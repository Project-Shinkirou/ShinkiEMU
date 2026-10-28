use super::*;
use crate::session::test_support::{command, game, open, picture, pixel};
use crate::system_save::slot::decode_native_slot;
use crate::system_save::slot_image::SavedProviderSource;

#[test]
fn native_slot_teardown_retains_font_managers_and_their_first_mode_cache() {
    let mut vm = CmvsPs2aVmState::new(0);
    crate::vm::native_text::font_owner::ensure_owner(&mut vm, 762, 0).unwrap();
    crate::vm::native_text::font_owner::ensure_owner(&mut vm, 783, 0).unwrap();
    vm.native_font_owners
        .get_mut(&762)
        .unwrap()
        .set_bold(true)
        .unwrap();
    vm.native_text_fonts.insert(0, String::new());
    let cached = vm.native_text_raster(0, 0x41, 27).unwrap();
    vm.process_global_words.insert(0x004f_0580, 1);
    vm.slot_objects.insert(750 << 8, 91);
    vm.slot_objects.insert(775 << 8, 92);
    vm.native_stage_size = [1280, 720];
    vm.native_render_size = [1920, 1080];
    vm.process_string_slots.insert(0, b"Example Font".to_vec());
    assert!(matches!(
        crate::scene::parent_load_tests::command(&mut vm, 896, &[0, 0x8000_0000, 20, 64]),
        Some(CmvsPs2aVmAction::NativeText(
            CmvsNativeTextAction::GlyphCreate { slot: 0 }
        ))
    ));
    vm.native_glyphs.get_mut(&0).unwrap().atlas.occupancy[31] = 1;
    assert!(vm.native_glyph_raster(0, 65, 30).is_ok());
    let owners = vm.native_font_owners.clone();
    clear_native_objects(&mut vm);
    assert_eq!(vm.native_font_owners, owners);
    assert!(vm.native_text_fonts.contains_key(&0));
    assert!(vm.slot_objects.contains_key(&(762 << 8)));
    assert!(vm.slot_objects.contains_key(&(783 << 8)));
    assert!(!vm.slot_objects.contains_key(&(750 << 8)));
    assert!(!vm.slot_objects.contains_key(&(775 << 8)));
    assert!(vm.native_glyphs.is_empty());
    assert_eq!(vm.native_text_raster(0, 0x41, 27).unwrap(), cached);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_file_selects_its_own_program_and_media_across_fresh_session() {
    let (root, archive) = game();
    let mut session = open(root.path(), archive.clone());
    picture(&mut session, "red.pb3", 0x1122_3344);
    command(&mut session, 320, &[0, 0, 0, 0, 16, 16]);
    session
        .vm
        .interpreter_words
        .extend([(10312, 17), (10316, 18), (10320, 19)]);
    let original_code = session.scripts[&0].bytecode.clone();
    command(&mut session, 690, &[]);
    assert!(!session.vm.process_flag_bits.contains(&0));
    command(&mut session, 694, &[987]);
    let red_bytes = std::fs::read(root.path().join("save/save987.dat")).unwrap();
    // Literal bytes of the 4712F0 immediate844518211, not the old port magic.
    assert_eq!(&red_bytes[..4], &[67, 83, 86, 50]);
    session.load(0, "other.ps3".into()).unwrap();
    picture(&mut session, "blue.pb3", 0x5566_7788);
    command(&mut session, 690, &[]);
    command(&mut session, 694, &[988]);
    command(&mut session, 320, &[1, 1, 0, 0, 16, 16]);
    assert_eq!(pixel(&session, 4, 4), [0, 0, 240, 255]);
    session.vm.interpreter_words.extend([
        (10312, 71),
        (10316, 72),
        (10320, 73),
        (1464, 1),
        (1500, 1),
        (10572, 1),
        (10708, 1),
    ]);
    assert_ne!(session.scripts[&0].bytecode, original_code);
    let before_vm = session.vm.clone();
    let before_frame = session.frame.clone();
    let mut missing = decode_native_slot(&red_bytes).unwrap();
    match &mut missing
        .images
        .get_mut(&0)
        .unwrap()
        .primary
        .as_mut()
        .unwrap()
        .source
    {
        SavedProviderSource::File { resource, .. } => *resource = b"missing.pb3".to_vec(),
        _ => panic!("file fixture did not capture a real provider"),
    }
    assert!(session.apply_native_slot(missing).is_err());
    assert_eq!(session.vm, before_vm);
    assert_eq!(session.frame, before_frame);
    session
        .action(CmvsPs2aVmAction::LoadNativeSaveSlot { slot: 987 })
        .unwrap();
    let selected = session.pending_load.take().unwrap();
    session.apply_native_slot(*selected).unwrap();
    assert_eq!(
        (
            session.vm.interpreter_words[&10312],
            session.vm.interpreter_words[&10316],
            session.vm.interpreter_words[&10320]
        ),
        (17, 0, 19)
    );
    for field in [1464, 1500, 10572, 10708] {
        assert_eq!(session.vm.interpreter_words[&field], 0);
    }
    assert!(session.vm.process_flag_bits.contains(&0));
    let continuation = session.vm.program_counter;
    session.tick().unwrap();
    assert_eq!(session.vm.program_counter, continuation + 2);
    command(&mut session, 334, &[0, 1]);
    assert_eq!(pixel(&session, 4, 4), [240, 0, 0, 255]);
    assert_eq!(session.vm.script_data_segment_words[&0][&0], 0x1122_3344);
    assert_eq!(session.scripts[&0].bytecode, original_code);
    drop(session);
    let mut fresh = open(root.path(), archive);
    assert!(fresh.vm.texture_parents.is_empty());
    // 471F50 never compares the header identity, so a save written under
    // another build identity still loads.
    fresh.vm.interpreter_words.insert(10556, 0xBAD0_1D1D);
    fresh
        .action(CmvsPs2aVmAction::LoadNativeSaveSlot { slot: 987 })
        .unwrap();
    let selected = fresh.pending_load.take().unwrap();
    fresh.apply_native_slot(*selected).unwrap();
    let continuation = fresh.vm.program_counter;
    fresh.tick().unwrap();
    assert_eq!(fresh.vm.program_counter, continuation + 2);
    command(&mut fresh, 334, &[0, 1]);
    assert_eq!(pixel(&fresh, 4, 4), [240, 0, 0, 255]);
    assert_eq!(fresh.vm.script_data_segment_words[&0][&0], 0x1122_3344);
    assert_eq!(fresh.scripts[&0].bytecode, original_code);
    fresh.vm.scene_words.insert(289, 1);
    fresh
        .action(CmvsPs2aVmAction::ReloadRootScript {
            name: "other.ps3".into(),
        })
        .unwrap();
    assert_ne!(fresh.scripts[&0].bytecode, original_code);
    assert!(fresh.native_slot_bytes().is_err());
}
