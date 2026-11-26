#[path = "support/musica.rs"]
mod native;

use shinki_emu_patch::{PatchErrorKind, PatchMode, PatchRequest, apply};
use std::{
    fs,
    sync::{Arc, atomic::AtomicBool},
};

#[test]
fn recall_overlay_validates_existing_archives_without_mutating_source() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("game-copy");
    fs::create_dir(&source).unwrap();
    let key = format!(
        "schema='shinki.emu.musica.keys.v2'\npaz_version=0\nindex_size_xor=0\nrc4_skip_crc=true\n{}",
        ["bg", "scr", "sys"].map(native::key_role).concat(),
    );
    fs::write(source.join("key.toml"), &key).unwrap();
    let png = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x02\
                \x00\x00\x00\x01\x08\x02\x00\x00\x00\x7b\x40\xe8\xdd\x00\x00\x00\x0f\x49\x44\x41\
                \x54\x78\x9c\x63\xf8\xcf\xc0\xc0\xf0\x9f\x01\x00\x07\xff\x01\xff\x01\x7f\x89\xa7\
                \x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";
    let sys = native::archive("sys", "gallery.png", png);
    let bg = native::archive("bg", "gallery.png", png);
    let scripts = native::archive_entries(
        "scr",
        &[
            ("test.sc", b".end\r\n"),
            (
                "recall.sc",
                b".label Again\r\n.message 1   Public recollection\r\n.end\r\n",
            ),
        ],
    );
    for (name, data) in [("sys.paz", &sys), ("bg.paz", &bg), ("scr.paz", &scripts)] {
        fs::write(source.join(name), data).unwrap();
    }
    let recipe = root.path().join("recall.luau");
    let run = |label: &str, member: &str, mode| {
        fs::write(&recipe, format!(r#"
            return {{id='recall-test', title='Recall fixture', version=1, apply=function(ctx)
                local image='musica:/sys/gallery.png'
                local found=ctx.musica_gallery_source_resources('key.toml', {{
                    {{candidates={{'musica:/bg/{member}', 'musica:/sys/gallery.png'}}, png_size=true}},
                    {{candidates={{'musica:/bg/missing.png', 'musica:/sys/{member}'}}}}
                }})
                assert(found[1].resource == 'musica:/bg/gallery.png')
                assert(found[1].size[1] == 2 and found[1].size[2] == 1)
                assert(found[2].resource == image)
                local output=ctx.create('recall-layout.json')
                output:write(0, buffer.fromstring(ctx.json.encode({{screens={{{{
                    id='recall', layers={{{{resource=image}}}}, buttons={{{{
                        rect={{80,90,100,40}}, preview={{resource=image,at={{20,30}}}},
                        action={{kind='script', resource='musica:/scr/recall.sc', label='{label}'}}
                    }}}}
                }}}}}})))
                local frontend=ctx.json.encode({{gallery_layout='recall-layout.json', titles={{{{
                    background=image, buttons={{{{rect={{20,30,100,40}}, action='memories'}}}}
                }}}}}})
                ctx.musica_validate_title_frontend('key.toml', frontend, 'shift_jis', 640, 360, 'source')
                return {{plugin_id='shinki.emu.musica', configuration={{launch_mode='title',frontend=frontend}}}}
            end}}
        "#)).unwrap();
        apply(
            PatchRequest {
                script: recipe.clone(),
                source: source.clone(),
                patch: source.clone(),
                mode,
            },
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
    };

    let output = root.path().join("recall-overlay");
    let rejected = run("Again", "gallery.png", PatchMode::InPlace)
        .err()
        .unwrap();
    assert_eq!(rejected.kind(), PatchErrorKind::InvalidRequest);
    assert!(!source.join(".shinki").exists());
    assert!(!source.join("recall-layout.json").exists());

    let new_directory = || PatchMode::NewDirectory {
        output: output.clone(),
    };
    let rejected = run("Missing", "gallery.png", new_directory())
        .err()
        .unwrap();
    assert_eq!(rejected.kind(), PatchErrorKind::Integrity);
    assert!(!output.exists());

    let rejected = run("Again", "missing.png", new_directory()).err().unwrap();
    assert_eq!(rejected.kind(), PatchErrorKind::Integrity);
    assert!(!output.exists());

    let installed = run("Again", "gallery.png", new_directory()).unwrap();
    assert_eq!(installed.profile.configuration["launch_mode"], "title");
    assert!(output.join("recall-layout.json").is_file());
    assert!(output.join("shinki-patch-result.json").is_file());
    assert!(!output.join("scr.paz").exists());
    assert!(!source.join("recall-layout.json").exists());
    assert!(!source.join("shinki-patch-result.json").exists());
    assert_eq!(fs::read(source.join("scr.paz")).unwrap(), scripts);
    assert_eq!(fs::read_to_string(source.join("key.toml")).unwrap(), key);
}
