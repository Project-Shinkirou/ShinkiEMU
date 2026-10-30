//! Synthetic Perseus slots; no real Perseus slot exists. Layout follows the
//! original writer: `sub_470C50` header, `sub_45D2D0` body, `sub_46B930` map
//! dump and `sub_43C980` -> `sub_43B040` + `sub_461C80` backlog records.
use super::perseus::{decode_slot, encode_slot, slot_file_name};
use super::*;
use crate::minori_data::FilePrefix;

const SERIAL: u32 = 0x1234_abcd;
const STRICT: FilePrefix = FilePrefix {
    value: SERIAL,
    strict: true,
};
/// The engine+6280 map in key byte order: non-empty `sub_45FB10` fields (an
/// empty value is erased by `sub_46B2D0`) including the Perseus-only
/// `se3`/`se4`/`eff2_*`, then `.set` variables (`sub_44F9D0`).
const VARIABLES: &str = "<begin variables>\n\
!bgm_Filename\tbgm01.ogg\n!eff2_Count\t4\n!eff2_Filename\tsnow.png\n!eff2_Method\t2\n\
!eff2_Param\t1\n!eff2_Speed\t3\n!script_Filename\tfixture.sc\n!script_ID\t7\n\
!script_Pointer\t12\n!se3_Filename\twind.ogg\n!se4_Filename\train.ogg\n\
!stage_BGFilename\tbg01.png\n!stage_StandFilename1\tst01.png\n\
Flag2\t9\nflagX\t1\n<end variables>\n";
/// One record per writer statement order. `sub_43B040` multi-page branch:
/// `LC`, `L0`, then `L1_i`..`L4_i`. `sub_461C80`: stand groups only for named
/// stands (stand 0 empty, so only `_1`), `aBG` group because `angBG != 0`,
/// no `aFG` group, then `chL_%d\t%d\t%s` choices.
const RECORD: [(&str, &str); 61] = [
    ("LC", "1"),
    ("L0", "7"),
    ("L1_0", "v01.ogg"),
    ("L2_0", "name"),
    ("L3_0", "text"),
    ("L4_0", "0"),
    ("CS1", "fixture.sc"),
    ("CS2", "12"),
    ("CS3", "7"),
    ("CF1", "0"),
    ("CF2", "0"),
    ("CF3", "0"),
    ("CP1", "0"),
    ("CP2", ""),
    ("CT1", "bg01.png"),
    ("CT2", ""),
    ("CT3", "0"),
    ("CT4", "0"),
    ("CT5", "2"),
    ("CT6_1", "st01.png"),
    ("CT7_1", "3"),
    ("CT7Y_1", "0"),
    ("CT6", "0"),
    ("CT7", "0"),
    ("Frm", ""),
    ("FrP", "0"),
    ("CO0", ""),
    ("CO1", ""),
    ("CO2", ""),
    ("Cs0", ""),
    ("Cs1", ""),
    ("Cs2", ""),
    ("bgH", "0"),
    ("bgS", "0"),
    ("bgV", "0"),
    ("stH", "0"),
    ("stS", "0"),
    ("stV", "0"),
    ("CB1", "bgm01.ogg"),
    ("CB1v", "100"),
    ("CE1", ""),
    ("CE2", ""),
    ("CE3", "wind.ogg"),
    ("CE4", "rain.ogg"),
    ("Ce1", "0"),
    ("Ce2", ""),
    ("Ce3", "0"),
    ("Ce4", "0"),
    ("Ce5", "0"),
    ("Ce21", "2"),
    ("Ce22", "snow.png"),
    ("Ce23", "4"),
    ("Ce24", "3"),
    ("Ce25", "1"),
    ("Ct1", "0"),
    ("Ct2", ""),
    ("Ct3", "0"),
    ("aBG", "1.5"),
    ("rBGdx", "0"),
    ("rBGdy", "0"),
    ("rBGsx", "0"),
];
fn body() -> String {
    let mut text = VARIABLES.to_owned();
    text.push_str("<<begin backlog>>\n");
    for (name, value) in RECORD.iter().chain(&[("rBGsy", "0"), ("chL_0", "2\tleft")]) {
        text.push_str(&format!("{name}\t{value}\n"));
    }
    text.push_str("ZZ\n<<end backlog>>\n");
    text
}
fn slot(body: &str) -> Vec<u8> {
    let mut bytes = SERIAL.to_le_bytes().to_vec();
    bytes.extend_from_slice(b"perseus 1.00\0test\0\0\0\0\0");
    let mut z = ZlibEncoder::new(bytes, Compression::default());
    z.write_all(body.as_bytes()).unwrap();
    z.finish().unwrap()
}
#[test]
fn perseus_slot_with_script_variables_survives_decode_encode_byte_identical() {
    let bytes = slot(&body());
    let save = decode_slot(&bytes, STRICT, EdenSaveEncoding::ShiftJis).unwrap();
    assert_eq!(save.prefix, SERIAL);
    assert_eq!(
        save.script_variables,
        [("Flag2".into(), "9".into()), ("flagX".into(), "1".into())]
    );
    assert_eq!(save.variable("se4_Filename"), Some("rain.ogg"));
    assert_eq!(body::format(&save).unwrap(), body());
    assert_eq!(encode_slot(&save, STRICT).unwrap(), bytes);
}
#[test]
fn backlog_record_keeps_the_writer_field_order() {
    let save = EdenSave::decode(
        &slot(&body()),
        EdenEdition::Perseus,
        EdenSaveEncoding::ShiftJis,
    )
    .unwrap();
    let [record] = &save.backlog[..] else {
        panic!("one record expected");
    };
    let names: Vec<_> = record.iter().map(|field| field.name.as_str()).collect();
    let expected: Vec<_> = RECORD
        .iter()
        .map(|(name, _)| *name)
        .chain(["rBGsy", "chL_0"])
        .collect();
    assert_eq!(names, expected);
    assert_eq!(record.last().unwrap().value, "2\tleft");
}
#[test]
fn header_prefix_mismatch_is_rejected_like_the_original() {
    let bytes = slot(&body());
    for prefix in [SERIAL ^ 1, 0] {
        let strict = FilePrefix {
            value: prefix,
            strict: true,
        };
        let error = decode_slot(&bytes, strict, EdenSaveEncoding::ShiftJis)
            .err()
            .unwrap();
        assert_eq!(error.code(), "SHINKI_EMU_EDEN_SAVE_SIGNATURE");
    }
    // No SerialCode input: the original has nothing to compare against here.
    let loose = FilePrefix {
        value: 0,
        strict: false,
    };
    let save = decode_slot(&bytes, loose, EdenSaveEncoding::ShiftJis).unwrap();
    assert!(decode_slot(&bytes[..3], STRICT, EdenSaveEncoding::ShiftJis).is_err());
    // The writer binds the slot to the resolved prefix, never another one.
    let error = encode_slot(&save, loose).err().unwrap();
    assert_eq!(error.code(), "SHINKI_EMU_EDEN_SAVE_SIGNATURE");
    // The right header with the eden signature is still not a Perseus slot.
    let mut eden = bytes.clone();
    eden.splice(4..16, *b"eden_en 1.00");
    assert!(decode_slot(&eden, STRICT, EdenSaveEncoding::ShiftJis).is_err());
}
#[test]
fn slot_file_names_stop_at_the_original_bound() {
    assert_eq!(slot_file_name(7).unwrap(), "perseus_0007.sav");
    assert_eq!(slot_file_name(9999).unwrap(), "perseus_9999.sav");
    let error = slot_file_name(10_000).err().unwrap();
    assert_eq!(error.code(), "SHINKI_EMU_EDEN_SAVE_BOUND");
}
#[test]
fn script_variables_are_written_in_map_byte_order() {
    let mut save = super::tests::fixture();
    save.script_variables = vec![
        ("b".into(), "2".into()),
        ("aZ".into(), "3".into()),
        ("a".into(), "1".into()),
        ("Z".into(), "4".into()),
    ];
    let text = body::format(&save).unwrap();
    assert!(text.contains("!script_Pointer\t12\nZ\t4\na\t1\naZ\t3\nb\t2\n<end variables>\n"));
    save.script_variables.push(("a".into(), "5".into()));
    assert!(body::format(&save).is_err());
}
#[test]
fn plain_lines_out_of_map_order_are_unsupported() {
    let unsupported = checkpoint::unsupported();
    for (from, to) in [
        // A plain line before a `!` field: the sorted map never writes this.
        (
            "!stage_StandFilename1\tst01.png\nFlag2\t9\n",
            "Flag2\t9\n!stage_StandFilename1\tst01.png\n",
        ),
        ("flagX\t1\n", "flagX\t1\nflagW\t1\n"),
        ("flagX\t1\n", "flagX\t1\nflagX\t2\n"),
        ("flagX\t1\n", "flagX\t1\nflagY\n"),
        ("flagX\t1\n", "flagX\t1\nflagY\t\n"),
    ] {
        let body = body().replacen(from, to, 1);
        assert_ne!(body, self::body());
        let error = body::parse(&body, EdenSaveEncoding::ShiftJis)
            .err()
            .unwrap();
        assert_eq!(error.code(), unsupported.code(), "{to:?}");
    }
}
