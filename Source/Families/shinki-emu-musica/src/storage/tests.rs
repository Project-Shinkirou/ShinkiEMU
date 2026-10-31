use super::*;
use crate::{
    ScriptEncoding,
    eden_save::{EdenEdition, EdenSave, EdenSaveEncoding},
    native_directory::NativeFiles,
};
use roukaku_media_core::Extent2D;
use shinki_emu_sdk::StageCanvas;

fn save() -> EdenSave {
    let source = b".set score = 17\r\n.message 1  speaker Original\r\n.end\r\n";
    let mut vm = crate::MusicaVm::new(
        "musica:/scr/test.sc".into(),
        roukaku_core::Hash256::from_sha256(source),
        crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
        0,
    )
    .unwrap();
    vm.step(1).unwrap();
    EdenSave::from_checkpoint(
        &vm.native_checkpoint().unwrap(),
        EdenEdition::Perseus,
        EdenSaveEncoding::ShiftJis,
        0,
        String::new(),
    )
    .unwrap()
}
fn thumbnail() -> Vec<u8> {
    SaveCard::capture_thumbnail(
        EdenEdition::Perseus,
        1280,
        720,
        1280,
        720,
        &vec![255; 1280 * 720 * 4],
    )
    .unwrap()
}
#[test]
fn native_slots_are_independent_and_reject_out_of_range() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("perseus.exe"), []).unwrap();
    let files = NativeFiles::open(root.path(), ScriptEncoding::ShiftJis).unwrap();
    let save = save();
    let png = thumbnail();
    assert_eq!(
        files.write_slot(100, &save, &png).unwrap_err().code(),
        "SHINKI_EMU_MUSICA_SAVE_SLOT"
    );
    for slot in [0, 10, 19, 20, 99] {
        files.write_slot(slot, &save, &png).unwrap();
    }
    let reopened = NativeFiles::open(root.path(), ScriptEncoding::ShiftJis).unwrap();
    for slot in [0, 10, 19, 20, 99] {
        let restored = reopened.slot(slot).unwrap().unwrap().checkpoint().unwrap();
        assert_eq!(restored.script_variables.get("score"), Some(&17));
        assert_eq!(restored.history.last().unwrap().text, "Original");
        assert!(reopened.card(slot).unwrap().is_some());
    }
    let path = root
        .path()
        .join(".shinki/musica/minori")
        .join(crate::minori_data::MinoriTitle::Perseus.directory())
        .join("perseus_0020.sav");
    std::fs::write(&path, b"damaged native slot").unwrap();
    assert!(reopened.write_slot(20, &save, &png).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"damaged native slot");
}
#[test]
fn old_beta_slots_are_untouched_in_both_namespaces() {
    let root = tempfile::tempdir().unwrap();
    for folder in [".shinki-musica/saves", ".shinki/musica/saves"] {
        std::fs::create_dir_all(root.path().join(folder)).unwrap();
        std::fs::write(
            root.path().join(folder).join("slot-020.asav"),
            b"private old bytes",
        )
        .unwrap();
    }
    std::fs::write(root.path().join("perseus.exe"), []).unwrap();
    let files = NativeFiles::open(root.path(), ScriptEncoding::ShiftJis).unwrap();
    assert!(files.slot(20).unwrap().is_none());
    files.write_slot(20, &save(), &thumbnail()).unwrap();
    for folder in [".shinki-musica/saves", ".shinki/musica/saves"] {
        assert_eq!(
            std::fs::read(root.path().join(folder).join("slot-020.asav")).unwrap(),
            b"private old bytes"
        );
    }
}
#[test]
fn save_card_crops_content_before_box_averaging() {
    let (width, height) = (721, 1280);
    let viewport = StageCanvas::new(Extent2D::new(1280, 720), Extent2D::new(width, height))
        .unwrap()
        .viewport();
    let mut rgba = vec![0; width as usize * height as usize * 4];
    for y in viewport.y..viewport.y + viewport.height {
        for x in viewport.x..viewport.x + viewport.width {
            let offset = ((y * width + x) * 4) as usize;
            rgba[offset..offset + 4].copy_from_slice(&[255, 32, 16, 255]);
        }
    }
    let png =
        SaveCard::capture_thumbnail(EdenEdition::Perseus, 1280, 720, width, height, &rgba).unwrap();
    let image = image::load_from_memory(&png).unwrap().into_rgba8();
    assert!(
        image
            .as_raw()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [255, 32, 16, 255])
    );
}
#[test]
fn save_cards_use_native_dimensions_and_reject_invalid_frames() {
    for (edition, expected) in [
        (EdenEdition::English, (128, 80)),
        (EdenEdition::Perseus, (106, 60)),
    ] {
        for (width, height) in [(1280, 720), (721, 1280), (1279, 719), (641, 359), (17, 1)] {
            let png = SaveCard::capture_thumbnail(
                edition,
                1280,
                720,
                width,
                height,
                &vec![255; width as usize * height as usize * 4],
            )
            .unwrap();
            let frame = image::load_from_memory(&png).unwrap();
            assert_eq!((frame.width(), frame.height()), expected);
        }
    }
    for (width, height, rgba) in [
        (0, 720, vec![]),
        (720, 0, vec![]),
        (1920, 1080, vec![255; 1920 * 1080 * 4 - 1]),
    ] {
        assert!(
            SaveCard::capture_thumbnail(EdenEdition::Perseus, 1280, 720, width, height, &rgba)
                .is_err()
        );
    }
}

#[test]
fn append_playing_selects_independent_eden_slot_and_thumbnail_namespaces() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("eden_en.exe"), []).unwrap();
    let mut files = NativeFiles::open(root.path(), ScriptEncoding::ShiftJis).unwrap();
    let checkpoint = save().checkpoint().unwrap();
    let normal = EdenSave::from_checkpoint(
        &checkpoint,
        EdenEdition::English,
        EdenSaveEncoding::ShiftJis,
        0,
        "normal".into(),
    )
    .unwrap();
    let append = EdenSave::from_checkpoint(
        &checkpoint,
        EdenEdition::English,
        EdenSaveEncoding::ShiftJis,
        0,
        "append".into(),
    )
    .unwrap();
    let png = SaveCard::capture_thumbnail(
        EdenEdition::English,
        1024,
        640,
        1024,
        640,
        &vec![255; 1024 * 640 * 4],
    )
    .unwrap();
    files.write_slot(1, &normal, &png).unwrap();
    files
        .store_globals(&[("AppendPlaying".into(), 1)].into())
        .unwrap();
    assert!(files.slot(1).unwrap().is_none());
    files.write_slot(1, &append, &png).unwrap();
    assert_eq!(files.slot(1).unwrap().unwrap().comment, "append");
    assert_eq!(files.card(1).unwrap().unwrap().thumbnail.width, 128);
    files
        .store_globals(&[("AppendPlaying".into(), 0)].into())
        .unwrap();
    assert_eq!(files.slot(1).unwrap().unwrap().comment, "normal");
    let directory = root
        .path()
        .join(".shinki/musica/minori")
        .join(crate::minori_data::MinoriTitle::EdenEnglish.directory());
    for name in [
        "eden0001.sav",
        "eden0001.png",
        "edenPM0001.sav",
        "edenPM0001.png",
    ] {
        assert!(directory.join(name).is_file());
    }
}

#[test]
fn eden_thumbnail_averages_each_eight_pixel_block_and_forces_opaque_alpha() {
    let image = image::RgbaImage::from_fn(1024, 640, |x, y| {
        image::Rgba([(x % 8 * 32) as u8, (y % 8 * 32) as u8, 17, 0])
    });
    let png =
        SaveCard::capture_thumbnail(EdenEdition::English, 1024, 640, 1024, 640, image.as_raw())
            .unwrap();
    let thumbnail = image::load_from_memory(&png).unwrap().into_rgba8();
    assert!(
        thumbnail
            .as_raw()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [112, 112, 17, 255])
    );
}
