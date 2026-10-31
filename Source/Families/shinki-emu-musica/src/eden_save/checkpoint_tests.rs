use super::*;
use crate::{MusicaVm, ScOpcodeCatalog, ScriptEncoding};

fn field(name: &str, value: &str) -> EdenSaveField {
    EdenSaveField {
        name: name.into(),
        value: value.into(),
    }
}

pub(super) fn checkpoint_fixture() -> EdenSave {
    let mut save = tests::fixture();
    save.route = [0; 4];
    save.variables.clear();
    for name in ["angBG", "angFG"] {
        save.variables.push(field(name, "0000000000000000"));
    }
    for name in [
        "bgH",
        "bgS",
        "bgV",
        "stH",
        "stS",
        "stV",
        "stage_FGX",
        "stage_FGY",
        "rotBG_dest.x",
        "rotBG_dest.y",
        "rotBG_source.x",
        "rotBG_source.y",
        "rotFG_dest.x",
        "rotFG_dest.y",
        "rotFG_source.x",
        "rotFG_source.y",
        "tr_Method",
        "stage_BGX",
        "stage_BGY",
    ] {
        save.variables.push(field(name, "0"));
    }
    for name in ["eff_Count", "eff_Param", "eff_Speed", "frameParam"] {
        save.variables.push(field(name, "-1"));
    }
    for (name, value) in [
        ("font_Color", "16777215"),
        ("font_FontSize", "22"),
        ("font_RubySize", "10"),
        ("script_Filename", "fixture.sc"),
        ("script_Pointer", "2"),
        ("script_ID", "7"),
        ("stage_BGFilename", "background.png"),
        ("panel_Mode", "1"),
        ("bgm_Filename", ""),
        ("bgm_Volume", "100"),
        ("tr_Param", "30"),
    ] {
        save.variables.push(field(name, value));
    }
    let mut record = Vec::new();
    for name in [
        "LC", "CT3", "CT4", "CT6", "CT7", "bgH", "bgS", "bgV", "stH", "stS", "stV", "Ct1",
    ] {
        record.push(field(name, "0"));
    }
    for name in [
        "CT1", "Frm", "CO0", "CO1", "CO2", "Cs0", "Cs1", "Cs2", "Ce1", "Ce2", "Ct2", "CP2", "CB1",
        "CE1", "CE2", "L1",
    ] {
        record.push(field(name, ""));
    }
    for name in ["FrP", "Ce3", "Ce4", "Ce5"] {
        record.push(field(name, "-1"));
    }
    for (name, value) in [
        ("CF1", "16777215"),
        ("CF2", "22"),
        ("CF3", "10"),
        ("CT5", "10"),
        ("L0", "7"),
        ("CS3", "7"),
        ("CS1", "fixture.sc"),
        ("CS2", "2"),
        ("L2", "speaker"),
        ("L3", "saved"),
        ("CT2", "background.png"),
        ("CP1", "1"),
        ("CB1v", "100"),
        ("Ct3", "30"),
    ] {
        record.push(field(name, value));
    }
    save.backlog = vec![record];
    save
}

fn vm_for(bytes: &[u8]) -> MusicaVm {
    let script = crate::parse_sc_with_encoding(
        bytes,
        &ScOpcodeCatalog::observed_musica(),
        ScriptEncoding::Gbk,
    )
    .unwrap();
    MusicaVm::new(
        "musica:/scr/fixture.sc".into(),
        roukaku_core::Hash256::from_sha256(bytes),
        script,
        42,
    )
    .unwrap()
}

#[test]
fn decoded_native_slot_restores_script_position_variables_and_stage() {
    let source =
        b".set before = 99\n.message 7  speaker saved\n.message 8  speaker continued\n.end\n";
    let bytes = checkpoint_fixture().encode().unwrap();
    let save = EdenSave::decode(&bytes, EdenEdition::English, EdenSaveEncoding::Gbk).unwrap();
    let mut vm = vm_for(source);
    vm.restore_eden_save(&save, 1).unwrap();
    assert_eq!(vm.state().pc_line, 2);
    assert!(!vm.state().variables.contains_key("before"));
    let background = vm
        .state()
        .stage
        .as_ref()
        .unwrap()
        .background
        .as_ref()
        .unwrap();
    assert_eq!(background.resource_uri, "musica:/bg/background.png");
    assert_eq!([background.x, background.y], [0, 0]);
}

#[test]
fn live_writer_restores_variables_foreground_stands_and_novel_page() {
    let source = b".set score = 17\n.stage front.png 12 34 back.png 5 6 stand.png 2,7\n.panel 2 -\n.message 1  speaker First\\nline\n.message 1  speaker Second\n.end\n";
    for edition in [EdenEdition::English, EdenEdition::Perseus] {
        let mut vm = vm_for(source);
        vm.set_native_edition(edition);
        vm.step(1).unwrap();
        vm.step(2).unwrap();
        vm.step(3).unwrap();
        let token = match vm.state().wait.as_ref().unwrap() {
            crate::MusicaWaitState::Input { token_id } => token_id.clone(),
            _ => panic!("not a message"),
        };
        vm.resolve_wait(&token).unwrap();
        vm.step(4).unwrap();
        let checkpoint = vm.native_checkpoint().unwrap();
        assert_eq!(checkpoint.history.len(), 1);
        assert_eq!(checkpoint.history[0].page.len(), 2);
        let layout = edition.novel_layout();
        assert_eq!(
            checkpoint.history[0].page[1].top_y,
            layout.top + layout.line_height as i32 * 2
        );
        let save = EdenSave::from_checkpoint(
            &checkpoint,
            edition,
            EdenSaveEncoding::Gbk,
            0,
            String::new(),
        )
        .unwrap();
        let save =
            EdenSave::decode(&save.encode().unwrap(), edition, EdenSaveEncoding::Gbk).unwrap();
        let mut restored = vm_for(source);
        restored.restore_eden_save(&save, 1).unwrap();
        assert_eq!(restored.state().variables.get("score"), Some(&17));
        assert_eq!(restored.state().stage, vm.state().stage);
        assert_eq!(
            restored.state().native_novel_page,
            vm.state().native_novel_page
        );
        assert_eq!(restored.state().panel, vm.state().panel);
        assert!(restored.native_checkpoint().unwrap() == checkpoint);
    }
}

#[test]
#[ignore = "requires private original slots and extracted scripts"]
fn private_native_slots_semantic_round_trip() {
    let slots = std::path::PathBuf::from(std::env::var_os("SHINKI_MUSICA_NATIVE_SLOTS").unwrap());
    let scripts =
        std::path::PathBuf::from(std::env::var_os("SHINKI_MUSICA_NATIVE_SCRIPTS").unwrap());
    let output = tempfile::tempdir().unwrap();
    let load_script = |name: &str| {
        let bytes = std::fs::read(scripts.join(name)).unwrap();
        crate::parse_sc_with_encoding(
            &bytes,
            &ScOpcodeCatalog::observed_musica(),
            ScriptEncoding::Gbk,
        )
        .unwrap()
    };
    let mut count = 0;
    for entry in std::fs::read_dir(slots).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "sav") {
            continue;
        }
        let save = EdenSave::decode(
            &std::fs::read(&path).unwrap(),
            EdenEdition::English,
            EdenSaveEncoding::Gbk,
        )
        .unwrap();
        let checkpoint = save.checkpoint().unwrap_or_else(|cause| {
            panic!("{}: {cause:?}", path.file_name().unwrap().to_string_lossy(),)
        });
        let script = load_script(&checkpoint.script);
        let mut vm = MusicaVm::new(
            format!("musica:/scr/{}", checkpoint.script),
            roukaku_core::Hash256::from_sha256(b"private-script"),
            script,
            0,
        )
        .unwrap();
        vm.restore_eden_save(&save, 1).unwrap_or_else(|cause| {
            panic!("{}: {cause:?}", path.file_name().unwrap().to_string_lossy())
        });
        let written = EdenSave::from_checkpoint(
            &vm.native_checkpoint().unwrap(),
            save.edition,
            save.encoding,
            save.prefix,
            save.comment.clone(),
        )
        .unwrap();
        let target = output.path().join(path.file_name().unwrap());
        std::fs::write(&target, written.encode().unwrap()).unwrap();
        let decoded =
            EdenSave::decode(&std::fs::read(target).unwrap(), save.edition, save.encoding).unwrap();
        assert!(decoded.checkpoint().unwrap() == checkpoint);
        count += 1;
    }
    assert!(count > 0);
    eprintln!("private native decode/restore/write semantic checks: {count} slots");
}

#[test]
fn native_history_keeps_recorded_text_effects_and_positions_without_replaying_old_scripts() {
    let source = b".message 1   Earlier\n.message 2   Current\n.end\n";
    let mut vm = vm_for(source);
    vm.step(1).unwrap();
    let crate::MusicaWaitState::Input { token_id } = vm.state().wait.clone().unwrap() else {
        panic!("not a message")
    };
    vm.resolve_wait(&token_id).unwrap();
    vm.step(2).unwrap();
    let mut save = EdenSave::from_checkpoint(
        &vm.native_checkpoint().unwrap(),
        EdenEdition::English,
        EdenSaveEncoding::Gbk,
        0,
        String::new(),
    )
    .unwrap();
    for (name, value) in [
        ("CS1", "retired.sc"),
        ("CS2", "999999"),
        ("L3", "Recorded history"),
        ("Ce1", "Firefly"),
        ("Ce2", "Firefly"),
        ("Ce3", "30"),
        ("Ce4", "6000"),
        ("Ct1", "1"),
        ("Ct2", "mask.png"),
    ] {
        save.backlog[0]
            .iter_mut()
            .find(|field| field.name == name)
            .unwrap()
            .value = value.into();
    }
    let save = EdenSave::decode(&save.encode().unwrap(), save.edition, save.encoding).unwrap();
    let expected = save.checkpoint().unwrap();
    let mut restored = vm_for(source);
    restored.restore_eden_save(&save, 1).unwrap();
    assert_eq!(restored.state().backlog[0].text, "Recorded history");
    assert_eq!(restored.state().backlog[0].source, None);
    assert!(restored.state().backlog[1].source.is_some());
    assert!(restored.native_checkpoint().unwrap() == expected);
}

#[test]
fn native_bgm_round_trip_preserves_authored_gain_and_pan() {
    let source = b".playbgm tone.ogg[70,-30]\n.message 1   Current\n.end\n";
    let mut vm = vm_for(source);
    vm.step(1).unwrap();
    vm.step(2).unwrap();
    let save = EdenSave::from_checkpoint(
        &vm.native_checkpoint().unwrap(),
        EdenEdition::English,
        EdenSaveEncoding::Gbk,
        0,
        String::new(),
    )
    .unwrap();
    let mut restored = vm_for(source);
    restored.restore_eden_save(&save, 1).unwrap();
    assert_eq!(restored.state().audio[&0], vm.state().audio[&0]);
}
