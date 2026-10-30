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
    vm.restore_eden_save(&save, |_| panic!("same script is already loaded"), 1)
        .unwrap();
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
