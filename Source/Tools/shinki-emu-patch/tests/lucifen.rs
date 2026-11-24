#[path = "../../../Families/shinki-emu-lucifen-formats/tests/support/mod.rs"]
mod fixture;

use sha2::{Digest, Sha256};
use shinki_emu_lucifen_formats::Archive;
use shinki_emu_patch::{PatchErrorKind, PatchMode, PatchRequest, apply};
use std::{fs, sync::{Arc, atomic::AtomicBool}};

#[test]
fn repacks_verified_archive_members_without_touching_the_source() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("game");
    let patch = root.path().join("patch");
    let output = root.path().join("output");
    let script = root.path().join("patch.luau");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&patch).unwrap();
    fs::write(source.join("SCRIPT.LPK"), fixture::SCRIPT_LPK).unwrap();
    let replacement = b"SOB2\0\0\0\0\0\0\0\0\0\0\0\0";
    fs::write(patch.join("new.sob"), replacement).unwrap();

    let recipe = |digest: &str| {
        let size = fixture::SCRIPT_LPK.len();
        format!(
            r#"return {{id='synthetic-lucifen',version=1,title='Lucifen',apply=function(ctx)
                ctx.lucifen_repack('SCRIPT.LPK','gameinit.sob',{{
                    {{archive='SCRIPT.LPK',size={size},sha256='{digest}',
                      replacements={{{{name='gameinit.sob',path='new.sob'}}}}}}
                }})
                return {{plugin_id='shinki.emu.lucifen',configuration={{text_encoding='gbk'}},required_capabilities=256}}
            end}}"#,
        )
    };
    let run = |digest: &str| {
        fs::write(&script, recipe(digest)).unwrap();
        apply(
            PatchRequest {
                script: script.clone(),
                source: source.clone(),
                patch: patch.clone(),
                mode: PatchMode::NewDirectory { output: output.clone() },
            },
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
    };

    let error = run(&"0".repeat(64)).err().unwrap();
    assert_eq!(error.kind(), PatchErrorKind::ReleaseMismatch);
    assert!(!output.exists());

    let digest = hex::encode(Sha256::digest(fixture::SCRIPT_LPK));
    run(&digest).unwrap();
    let bytes = fs::read(output.join("SCRIPT.LPK")).unwrap();
    let read = |offset: u64, output: &mut [u8]| {
        let offset = offset as usize;
        output.copy_from_slice(&bytes[offset..offset + output.len()]);
        Ok(())
    };
    let archive = Archive::open("SCRIPT.LPK", bytes.len() as u64, None, read).unwrap();
    let entry = archive.entry("gameinit.sob").unwrap().unwrap();
    assert_eq!(archive.read_member(entry, 64, read).unwrap(), replacement);
    assert_eq!(fs::read(source.join("SCRIPT.LPK")).unwrap(), fixture::SCRIPT_LPK);
}
