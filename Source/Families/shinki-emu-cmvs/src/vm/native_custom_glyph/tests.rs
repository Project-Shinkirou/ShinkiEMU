use super::*;

fn sheet(manager: &mut CmvsNativeCustomGlyphManager, size: u32, value: u8) {
    manager
        .load_sheet(
            format!("{size}.msk"),
            size,
            size,
            size,
            vec![value; size as usize * size as usize],
        )
        .unwrap();
}

#[test]
fn exact_lookup_and_sheet_index_use_native_coordinates_and_first_duplicate() {
    let mut manager = CmvsNativeCustomGlyphManager::default();
    manager
        .load_sheet(
            "sheet.msk".into(),
            2,
            4,
            4,
            vec![1, 2, 11, 12, 3, 4, 13, 14, 21, 22, 31, 32, 23, 24, 33, 34],
        )
        .unwrap();
    manager.register(3, 0x82a0).unwrap();
    let bitmap = manager.lookup(0x82a0, 2).unwrap().unwrap();
    assert_eq!(bitmap.coverage, [31, 32, 33, 34]);
    assert_eq!(bitmap.descriptor, 2);
    let identity = bitmap.identity;
    manager.register(0, 0x82a0).unwrap();
    assert_eq!(
        manager.lookup(0x82a0, 2).unwrap().unwrap().identity,
        identity
    );
    assert!(manager.lookup(0x82a2, 2).unwrap().is_none());
}

#[test]
fn nearest_larger_scaling_truncates_native_fixedpoint_and_retains_cache() {
    let mut manager = CmvsNativeCustomGlyphManager::default();
    sheet(&mut manager, 6, 255);
    manager
        .load_sheet(
            "small.msk".into(),
            3,
            3,
            3,
            vec![0, 10, 20, 30, 40, 50, 60, 70, 80],
        )
        .unwrap();
    manager.register(0, 65).unwrap();
    manager.set_descriptor(65, 3, 2);
    let bitmap = manager.lookup(65, 2).unwrap().unwrap();
    assert_eq!(bitmap.coverage, [0, 15, 45, 60]);
    assert_eq!(bitmap.descriptor, 1); // floor(2*2/3)
    let identity = bitmap.identity;
    // 962 does not propagate into derived sizes, and later sheets do not
    // retroactively modify a registered glyph or invalidate its cache.
    manager.set_descriptor(65, 3, 1);
    sheet(&mut manager, 2, 99);
    assert_eq!(manager.lookup(65, 2).unwrap().unwrap().identity, identity);
    assert_eq!(manager.bitmap_by_identity(identity).unwrap().descriptor, 1);
    assert!(manager.lookup(65, 7).unwrap().is_none());
}

#[test]
fn scaler_uses_ten_bit_step_not_float_interpolation() {
    let source: Vec<_> = (0..16).map(|n| n * 16).collect();
    assert_eq!(
        scale(&source, 4, 3),
        [0, 21, 42, 85, 106, 127, 170, 191, 213]
    );
}

#[test]
fn descriptor_update_changes_only_first_exact_record_and_preserves_pixels() {
    let mut manager = CmvsNativeCustomGlyphManager::default();
    sheet(&mut manager, 4, 77);
    manager.register(0, 0x8140).unwrap();
    manager.register(0, 0x8140).unwrap();
    let identity = manager.lookup(0x8140, 4).unwrap().unwrap().identity;
    assert!(!manager.set_descriptor(0x8140, 3, 1));
    assert!(!manager.set_descriptor(0x8141, 4, 1));
    assert!(manager.set_descriptor(0x8140, 4, 0xffff));
    let bitmap = manager.bitmap_by_identity(identity).unwrap();
    assert_eq!(bitmap.descriptor, 0xffff);
    assert_eq!(bitmap.coverage, [77; 16]);
    assert_eq!(manager.glyphs[1].bitmaps[0].descriptor, 4);
}

#[test]
fn native_sixteen_slot_limit_and_strict_1024_scaling_ceiling() {
    let mut manager = CmvsNativeCustomGlyphManager::default();
    sheet(&mut manager, 32, 100);
    manager.register(0, 65).unwrap();
    for size in 17..32 {
        assert!(manager.lookup(65, size).unwrap().is_some());
    }
    assert!(manager.lookup(65, 16).unwrap().is_none());
    assert!(manager.lookup(65, 17).unwrap().is_some()); // exact allowed at full capacity
    let mut manager = CmvsNativeCustomGlyphManager::default();
    sheet(&mut manager, 1024, 200);
    manager.register(0, 65).unwrap();
    assert!(manager.lookup(65, 1023).unwrap().is_none());
    assert!(manager.lookup(65, 1024).unwrap().is_some());
}

#[test]
fn invalid_native_memory_access_is_reported_only_when_used() {
    let mut manager = CmvsNativeCustomGlyphManager::default();
    sheet(&mut manager, 2, 100);
    assert!(manager.register(1, 65).is_err());
    assert!(manager.glyphs.is_empty());
    manager.register(0, 65).unwrap();
    assert!(manager.lookup(65, 0).is_err());
    let mut zero = CmvsNativeCustomGlyphManager::default();
    zero.load_sheet("zero.msk".into(), 0, 1, 1, vec![255])
        .unwrap();
    assert!(zero.register(0, 65).is_err());
    let mut many = CmvsNativeCustomGlyphManager::default();
    for _ in 0..17 {
        sheet(&mut many, 1, 100);
    }
    assert!(many.register(0, 65).is_err());
}

#[test]
fn private_snapshot_preserves_source_and_derived_allocation_identities() {
    let mut vm = CmvsPs2aVmState::new(0);
    sheet(&mut vm.native_custom_glyphs, 4, 201);
    vm.native_custom_glyphs.register(0, 65).unwrap();
    let identity = vm
        .native_custom_glyphs
        .lookup(65, 3)
        .unwrap()
        .unwrap()
        .identity;
    let bytes = postcard::to_allocvec(&vm).unwrap();
    let mut restored: CmvsPs2aVmState = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(restored.native_custom_glyphs, vm.native_custom_glyphs);
    assert_eq!(
        restored
            .native_custom_glyphs
            .lookup(65, 3)
            .unwrap()
            .unwrap()
            .identity,
        identity
    );
}

#[test]
fn commands_use_actual_operand_order_and_truncate_native_words() {
    let mut vm = CmvsPs2aVmState::new(0);
    sheet(&mut vm.native_custom_glyphs, 4, 211);
    crate::scene::parent_load_tests::command(&mut vm, 961, &[0, 0x182a0]);
    let original = vm
        .native_custom_glyphs
        .lookup(0x82a0, 4)
        .unwrap()
        .unwrap()
        .identity;
    crate::scene::parent_load_tests::command(&mut vm, 962, &[0x182a0, 4, 0x10002]);
    assert_eq!(
        vm.native_custom_glyphs
            .bitmap_by_identity(original)
            .unwrap()
            .descriptor,
        2
    );
    assert_eq!(
        vm.native_custom_glyphs
            .bitmap_by_identity(original)
            .unwrap()
            .coverage,
        [211; 16]
    );
}
