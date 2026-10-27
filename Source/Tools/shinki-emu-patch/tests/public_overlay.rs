mod fixtures;
use fixtures::PublicOverlay;
use shinki_emu_patch::{PatchErrorKind, PatchRequest, apply};
use std::{
    fs,
    sync::{Arc, atomic::AtomicBool},
};

fn request(fixture: &PublicOverlay) -> PatchRequest {
    PatchRequest {
        script: fixture.script.clone(),
        source: fixture.source.clone(),
        patch: fixture.patch.clone(),
        mode: shinki_emu_patch::PatchMode::NewDirectory {
            output: fixture.output.clone(),
        },
    }
}

#[test]
fn pe_overlay_patches_both_text_slots_and_images_without_changing_originals() {
    let root = tempfile::tempdir().unwrap();
    let fixture = PublicOverlay::new(root.path());
    let patch_before = fs::read(fixture.patch.join("overlay.exe")).unwrap();
    let result = apply(request(&fixture), Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    assert_eq!(
        fs::read(result.output.join("assets.bin")).unwrap(),
        fixture.expected
    );
    assert_eq!(
        fs::read(result.output.join("unchanged.dat")).unwrap(),
        b"complete independent public fixture"
    );
    assert_eq!(
        fs::metadata(result.output.join("zero.dat")).unwrap().len(),
        0
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(result.output.join("shinki-patch-result.json")).unwrap())
            .unwrap();
    assert_eq!(receipt["recipe"]["id"], "public-pe-overlay");
    assert_eq!(
        receipt["profile"]["configuration"]["script_encoding"],
        "utf8"
    );
    fixture.assert_inputs();
    assert_eq!(
        fs::read(fixture.patch.join("overlay.exe")).unwrap(),
        patch_before
    );
}

#[test]
fn corrupt_overlay_blocks_fail_closed_without_publishing_or_changing_inputs() {
    for (case, expected) in [
        ("crc", PatchErrorKind::Integrity),
        ("truncated zlib", PatchErrorKind::Codec),
        ("declared length", PatchErrorKind::Codec),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut fixture = PublicOverlay::new(root.path());
        match case {
            "crc" => fixture.blocks[1].2 ^= 1,
            "truncated zlib" => {
                let new_length = fixture.blocks[2].3.len() - 2;
                fixture.blocks[2].3.truncate(new_length);
            }
            _ => fixture.blocks[0].1 -= 1,
        }
        fixture.write_patch();
        fixture.write_script();
        let error = apply(request(&fixture), Arc::new(AtomicBool::new(false)), |_| {})
            .err()
            .unwrap();
        assert_eq!(error.kind(), expected, "{case}");
        assert!(!fixture.output.exists(), "{case}");
        fixture.assert_inputs();
    }
}
