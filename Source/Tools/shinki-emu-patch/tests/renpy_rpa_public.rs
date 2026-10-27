//! Public pickle/zlib fixtures, including synthetic RPC2 containers.
//! No commercial source or payload bytes.
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use serde_pickle::{
    DeOptions, SerOptions,
    value::{HashableValue, Value},
};
use sha2::{Digest, Sha256};
use shinki_emu_patch::{PatchErrorKind, PatchRequest, apply};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

const KEY: u32 = 0x8f31a2c5;
const INDEX: usize = 256;
// CPython source:
// {'公开/file.txt': [(5^KEY,34^KEY)],
//  b'bytes/file.bin': [(4^KEY,45^KEY,b'PRE\0')],
//  'none.bin': [[3^KEY,49^KEY,None]],
//  'multi.bin': [(2^KEY,52^KEY),(2^KEY,54^KEY)]}
const VALID: &str = "789c6b609dba8c01026aa768f4f03f6d5df3744f837e5a664eaa5e4945c994d8295dac071619f63374b13e07516d53129df9922a4b528b216a9232f3c06a0e42d4bc0051ce2c0141ae0c53daa724f670e4e5e7c114c54ed1e8623d0651f70544f9a526f670e696e69464425500e58f43e43f42ec82f33f43f8a9a57a007e6b44ed";

fn unhex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn archive(index: &str) -> Vec<u8> {
    let mut bytes = format!("RPA-3.0 {INDEX:016x} {KEY:08x}\n").into_bytes();
    assert_eq!(bytes.len(), 34);
    bytes.extend((34..INDEX).map(|n| n as u8));
    bytes.extend(unhex(index));
    bytes
}

fn invoke(
    root: &Path,
    script: &Path,
) -> Result<shinki_emu_patch::PatchResult, shinki_emu_patch::PatchError> {
    apply(
        PatchRequest {
            script: script.to_owned(),
            source: root.join("source"),
            patch: root.join("source"),
            mode: shinki_emu_patch::PatchMode::NewDirectory {
                output: root.join("output"),
            },
        },
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
}

fn normalize(
    root: &Path,
    input: &[u8],
    wrong_hash: bool,
    abort_after: bool,
) -> Result<shinki_emu_patch::PatchResult, shinki_emu_patch::PatchError> {
    fs::create_dir_all(root.join("source/game")).unwrap();
    fs::create_dir_all(root.join("scripts/common")).unwrap();
    fs::write(root.join("source/game/public.rpa"), input).unwrap();
    fs::write(
        root.join("scripts/common/renpy_rpa.luau"),
        include_str!("../../../../Metadata/patches/common/renpy_rpa.luau"),
    )
    .unwrap();
    let hash = if wrong_hash {
        "0".repeat(64)
    } else {
        hex::encode(Sha256::digest(input))
    };
    let abort = if abort_after {
        "ctx.fail('INTEGRITY', 'public transaction abort')"
    } else {
        ""
    };
    let script = root.join("scripts/public.luau");
    fs::write(
        &script,
        format!(
            r#"
return {{id='public-renpy-rpa',version=1,title='Public RPA conversion',apply=function(ctx)
    ctx.load('common/renpy_rpa.luau').normalize(ctx, {{
        source='game/public.rpa', destination='game/public.rpa',
        fingerprint={{size={},sha256='{}'}},
    }})
    {}
    return {{plugin_id='renpy',configuration={{}}}}
end}}
"#,
            input.len(),
            hash,
            abort
        ),
    )
    .unwrap();
    invoke(root, &script)
}

fn coordinates(offset: u32, length: u32, prefix: Option<Value>, list: bool) -> Value {
    let mut fields = vec![
        Value::I64((offset ^ KEY) as i64),
        Value::I64((length ^ KEY) as i64),
    ];
    if let Some(prefix) = prefix {
        fields.push(prefix);
    }
    if list {
        Value::List(fields)
    } else {
        Value::Tuple(fields)
    }
}

fn decode_standard(bytes: &[u8]) -> BTreeMap<HashableValue, Value> {
    let offset = usize::from_str_radix(std::str::from_utf8(&bytes[8..24]).unwrap(), 16).unwrap();
    let mut plain = Vec::new();
    let mut decoder = ZlibDecoder::new(&bytes[offset..]);
    decoder.read_to_end(&mut plain).unwrap();
    assert_eq!(decoder.total_in() as usize, bytes.len() - offset);
    match serde_pickle::value_from_slice(&plain, DeOptions::new()).unwrap() {
        Value::Dict(entries) => entries,
        _ => panic!("standard RPA index is not a dictionary"),
    }
}

// Standard offset-first interpretation: prefix bytes are prepended to the
// complete dlen-byte extent, not subtracted from it. This independent consumer
// checks the format; it does not claim SDK execution of the rare prefix branch.
fn standard_read(bytes: &[u8], chunks: &Value) -> Vec<u8> {
    let Value::List(chunks) = chunks else {
        panic!("chunks must be a list")
    };
    let mut result = Vec::new();
    for chunk in chunks {
        let fields = match chunk {
            Value::Tuple(fields) | Value::List(fields) => fields,
            _ => panic!("invalid chunk type"),
        };
        let Value::I64(encoded_offset) = fields[0] else {
            panic!("invalid offset")
        };
        let Value::I64(encoded_length) = fields[1] else {
            panic!("invalid length")
        };
        let offset = (encoded_offset as u32 ^ KEY) as usize;
        let length = (encoded_length as u32 ^ KEY) as usize;
        if let Some(prefix) = fields.get(2) {
            match prefix {
                Value::Bytes(prefix) => result.extend_from_slice(prefix),
                Value::None => {}
                _ => panic!("invalid prefix type"),
            }
        }
        result.extend_from_slice(&bytes[offset..offset + length]);
    }
    result
}

#[test]
fn cpython_index_becomes_standard_rpa_preserving_header_payload_and_python_types() {
    let root = tempfile::tempdir().unwrap();
    let input = archive(VALID);
    let result = normalize(root.path(), &input, false, false).unwrap();
    assert_eq!(result.profile.plugin_id, "renpy");
    assert!(result.profile.configuration.is_empty());
    let output = fs::read(result.output.join("game/public.rpa")).unwrap();
    assert_eq!(&output[..INDEX], &input[..INDEX]);
    assert_eq!(
        fs::read(root.path().join("source/game/public.rpa")).unwrap(),
        input
    );
    let index = decode_standard(&output);
    let text = HashableValue::String("公开/file.txt".into());
    let binary = HashableValue::Bytes(b"bytes/file.bin".to_vec());
    let none = HashableValue::String("none.bin".into());
    let multi = HashableValue::String("multi.bin".into());
    let expected = BTreeMap::from([
        (
            text.clone(),
            Value::List(vec![coordinates(34, 5, None, false)]),
        ),
        (
            binary.clone(),
            Value::List(vec![coordinates(
                45,
                4,
                Some(Value::Bytes(b"PRE\0".to_vec())),
                false,
            )]),
        ),
        (
            none.clone(),
            Value::List(vec![coordinates(49, 3, Some(Value::None), true)]),
        ),
        (
            multi.clone(),
            Value::List(vec![
                coordinates(52, 2, None, false),
                coordinates(54, 2, None, false),
            ]),
        ),
    ]);
    assert_eq!(index, expected);
    assert_eq!(standard_read(&output, &index[&text]), input[34..39]);
    let mut prefixed = b"PRE\0".to_vec();
    prefixed.extend_from_slice(&input[45..49]);
    assert_eq!(standard_read(&output, &index[&binary]), prefixed);
    assert_eq!(standard_read(&output, &index[&none]), input[49..52]);
    assert_eq!(standard_read(&output, &index[&multi]), input[52..56]);
}

fn rejects(input: Vec<u8>, expected: PatchErrorKind, wrong_hash: bool, abort_after: bool) {
    let root = tempfile::tempdir().unwrap();
    let error = normalize(root.path(), &input, wrong_hash, abort_after)
        .err()
        .unwrap();
    assert_eq!(error.kind(), expected, "{error}");
    assert!(!root.path().join("output").exists());
    assert_eq!(
        fs::read(root.path().join("source/game/public.rpa")).unwrap(),
        input
    );
    assert!(!fs::read_dir(root.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".shinki-patch-staging-")
    }));
}

#[test]
fn rejects_wrong_root_duplicate_paths_unsafe_paths_and_chunk_types_transactionally() {
    // Each compressed pickle is independently serialized by CPython. The labels
    // describe semantic errors, rather than relying on host codec roundtrips.
    let cases = [
        (
            "root-list",
            "789c6b608d9da20700050601a5",
            PatchErrorKind::UnsupportedInput,
        ),
        (
            "str-bytes-duplicate",
            "789c6b609d6ac90001b553347a588a137353a7c44ee9623db2c8b09fa18bf52d886a9b92e88c21f3062a53aa0700fa2f1878",
            PatchErrorKind::Integrity,
        ),
        (
            "traversal",
            "789c6b609daacc0001b5537a38f5f4f4538b93130b52a7c44ee9623db2c8b09fa18bf52d886a9b9258ac07002ca50f14",
            PatchErrorKind::UnsupportedInput,
        ),
        (
            "absolute",
            "789c6b609daac80001b5537ad8f5538b93130b52a7c44ee9623db2c8b09fa18bf52d886a9b9258ac07001b590eb4",
            PatchErrorKind::UnsupportedInput,
        ),
        (
            "backslash",
            "789c6b609daac20001b5537ab852328b62528b93130b52a7c44ee9623db2c8b09fa18bf52d886a9b9258ac0700508c1026",
            PatchErrorKind::UnsupportedInput,
        ),
        (
            "invalid-utf8",
            "789c6b609d2acd0001b5539c19ff4f899dd2c57a6491613f4317eb5b10d53625b1580f00d4d60cbe",
            PatchErrorKind::UnsupportedInput,
        ),
        (
            "integer-key",
            "789c6b609d2ac90001b553bc9962a774b11e5964d8cfd0c5fa1644b54d492cd60300acab0b32",
            PatchErrorKind::UnsupportedInput,
        ),
        (
            "chunks-tuple",
            "789c6b609d2acb0001b5537a58d2327352a774b11e5964d8cfd0c5fa1644b54d699d52ac0700e7130d74",
            PatchErrorKind::UnsupportedInput,
        ),
        (
            "chunk-int",
            "789c6b609d2ac00001b5537a58d2327352a7c44ef1664f2cd6030055ff0745",
            PatchErrorKind::UnsupportedInput,
        ),
        (
            "arity-four",
            "789c6b609daac40001b5537a58d2327352a7c44ed1e8623db2c8b09fa18bf52d88f2f36628999258ac070027440e61",
            PatchErrorKind::UnsupportedInput,
        ),
        (
            "bool-coordinate",
            "789c6b609d2ac10001b5537a58d2327352a7c44ee9e8627dbbc8b09fa16d4a62b11e00abde0b7b",
            PatchErrorKind::UnsupportedInput,
        ),
        (
            "negative-coordinate",
            "789c6b609d2ac30001b5537a58d2327352a7c44ef1fa0f045dac6f1719f633b44d492cd60300fc1a0f3d",
            PatchErrorKind::UnsupportedInput,
        ),
        (
            "coordinate-overflow",
            "789c6b609d2ac70001b5537a58d2327352a7c44ee96205093076b1be5d64d8cfd03625b1580f00d4550b89",
            PatchErrorKind::Bounds,
        ),
        (
            "string-prefix",
            "789c6b609daace0001b5537a58d2327352a7c44ee9623db2c8b09fa18bf52d88ea612b284a4dcbac98d23e25b1580f0083a6116c",
            PatchErrorKind::UnsupportedInput,
        ),
        (
            "header-overlap",
            "789c6b609d2ac70001b5537a58d2327352a7c44ee9623db2c8b09fa18bf509886a9b9258ac0700f5c30da5",
            PatchErrorKind::Bounds,
        ),
        (
            "index-overlap",
            "789c6b609d2ac70001b5537a58d2327352a7c44ee9623dbec8b09fa18bd50a44b54d492cd60300ef520cfe",
            PatchErrorKind::Bounds,
        ),
    ];
    for (_label, compressed, kind) in cases {
        rejects(archive(compressed), kind, false, false);
    }
}

#[test]
fn rejects_invalid_hex_offsets_truncation_and_zlib_trailing_data() {
    let mut invalid_hex = archive(VALID);
    invalid_hex[8] = b'g';
    rejects(invalid_hex, PatchErrorKind::UnsupportedInput, false, false);
    let mut overflow = archive(VALID);
    overflow[8..24].fill(b'f');
    rejects(overflow, PatchErrorKind::Bounds, false, false);
    for offset in [33, 1_000_000] {
        let mut invalid_offset = archive(VALID);
        invalid_offset[8..24].copy_from_slice(format!("{offset:016x}").as_bytes());
        rejects(invalid_offset, PatchErrorKind::Bounds, false, false);
    }
    let mut invalid_key = archive(VALID);
    invalid_key[26] = b'-';
    rejects(invalid_key, PatchErrorKind::UnsupportedInput, false, false);
    rejects(b"RPA-3.0".to_vec(), PatchErrorKind::Bounds, false, false);
    let mut truncated = archive(VALID);
    truncated.pop();
    rejects(truncated, PatchErrorKind::Codec, false, false);
    let mut trailing = archive(VALID);
    trailing.push(0);
    rejects(trailing, PatchErrorKind::Codec, false, false);
}

#[test]
fn sha_gate_and_failure_after_copy_never_publish_or_modify_source() {
    rejects(
        archive(VALID),
        PatchErrorKind::UnsupportedInput,
        true,
        false,
    );
    rejects(archive(VALID), PatchErrorKind::Integrity, false, true);
}

#[test]
fn empty_dictionary_and_zero_length_bytes_prefix_remain_valid() {
    for (compressed, empty) in [
        ("789c6b60ad9da20700056601c5", true),
        (
            "789c6b609daaca0001b5537a38aa528bf2f59232f3a6c44ee9623dbac8b09fa18bf53988726698d23e25b1580f005c0f1013",
            false,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let input = archive(compressed);
        let result = normalize(root.path(), &input, false, false).unwrap();
        let output = fs::read(result.output.join("game/public.rpa")).unwrap();
        assert_eq!(&output[..INDEX], &input[..INDEX]);
        let index = decode_standard(&output);
        if empty {
            assert!(index.is_empty());
        } else {
            let key = HashableValue::String("zero.bin".into());
            assert_eq!(
                index[&key],
                Value::List(vec![coordinates(
                    34,
                    0,
                    Some(Value::Bytes(Vec::new())),
                    false
                ),])
            );
            assert!(standard_read(&output, &index[&key]).is_empty());
        }
    }
}

#[test]
fn compressed_index_read_is_bounded_before_inflate() {
    let mut input = archive(VALID);
    input.resize(INDEX + 16 * 1024 * 1024 + 1, 0);
    rejects(input, PatchErrorKind::Bounds, false, false);
}

const RESOLUTION_MEMBER: &str = "kagami_games/code/system/force_720p.rpyc";
const UNTOUCHED: &[u8] = b"public unrelated script payload";

fn public_python() -> String {
    let mut source = "public_trace.append('init100')\n#".to_owned();
    source.push_str(&"x".repeat(114 - source.len()));
    source.push('\n');
    assert_eq!(source.len(), 115);
    source
}

fn compressed(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn public_rpc2(strings: &[String]) -> Vec<u8> {
    // A protocol-5 list of literal strings, not an emulated Ren'Py AST.
    // Emit each literal separately so duplicate-source scanning is exercised.
    let mut pickle = vec![0x80, 5, b']', b'('];
    for text in strings {
        pickle.push(b'X');
        pickle.extend_from_slice(&(text.len() as u32).to_le_bytes());
        pickle.extend_from_slice(text.as_bytes());
    }
    pickle.extend_from_slice(b"e.");
    let slot = compressed(&pickle);
    let mut bytes = b"RENPY RPC2".to_vec();
    for field in [1, 46, 4, 2, 50, slot.len() as u32, 0, 0, 0] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes.extend_from_slice(b"pub1");
    bytes.extend(slot);
    assert!(bytes.len() <= 1386);
    bytes.resize(1386, 0);
    bytes
}

fn conversion_archive(member: &[u8], target: &str) -> Vec<u8> {
    let midpoint = member.len() / 2;
    let index_offset = 34 + member.len() - 10 + UNTOUCHED.len();
    let mut bytes = format!("RPA-3.0 {index_offset:016x} {KEY:08x}\n").into_bytes();
    bytes.extend_from_slice(&member[10..]);
    let untouched_offset = bytes.len();
    bytes.extend_from_slice(UNTOUCHED);
    let chunks = Value::List(vec![
        coordinates(
            (midpoint - 10) as u32,
            34,
            Some(Value::Bytes(member[..10].to_vec())),
            false,
        ),
        coordinates(
            (member.len() - midpoint) as u32,
            (34 + midpoint - 10) as u32,
            None,
            true,
        ),
    ]);
    let mut entries = BTreeMap::from([(
        HashableValue::String("public/untouched.rpyc".into()),
        Value::List(vec![coordinates(
            UNTOUCHED.len() as u32,
            untouched_offset as u32,
            None,
            false,
        )]),
    )]);
    if target != "missing" {
        entries.insert(
            HashableValue::String(RESOLUTION_MEMBER.into()),
            chunks.clone(),
        );
    }
    if target == "duplicate" {
        entries.insert(
            HashableValue::Bytes(RESOLUTION_MEMBER.as_bytes().to_vec()),
            chunks,
        );
    }
    let pickle = serde_pickle::value_to_vec(&Value::Dict(entries), SerOptions::new()).unwrap();
    bytes.extend(compressed(&pickle));
    bytes
}

fn seed_conversion_recipe(root: &Path, input: &[u8], member: &[u8], wrong_member_hash: bool) {
    fs::create_dir_all(root.join("source/game/saves")).unwrap();
    fs::create_dir_all(root.join("source/game/cache")).unwrap();
    fs::create_dir_all(root.join("scripts/common")).unwrap();
    fs::write(root.join("source/game/script_version.txt"), b"(8, 5, 2)\n").unwrap();
    for name in ["audio", "code", "font", "gui", "image", "movie", "patch"] {
        fs::write(root.join(format!("source/game/{name}.rpa")), input).unwrap();
    }
    for path in [
        "game/saves/public.save",
        "game/cache/public.cache",
        "public.exe",
        "public.dll",
    ] {
        fs::write(root.join("source").join(path), b"public excluded file").unwrap();
    }
    let archive_hash = hex::encode(Sha256::digest(input));
    // Run the shipped recipe with public edition identities substituted only
    // inside this isolated fixture. Production pins and conversion code stay intact.
    let recipe = include_str!("../../../../Metadata/patches/renpy-himanatsu.luau")
        .lines()
        .map(|line| {
            if let Some(rest) = line.trim().strip_prefix("{ name = \"") {
                let name = rest.split('"').next().unwrap();
                format!(
                    "    {{ name = \"{name}\", size = {}, sha256 = \"{archive_hash}\" }},",
                    input.len()
                )
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let member_hash = if wrong_member_hash {
        "0".repeat(64)
    } else {
        hex::encode(Sha256::digest(member))
    };
    let recipe = recipe
        .replace(
            "95e32e5243470f3dc25013b557696b7f84c12b7caf9847c087bb2fc4dcc76ceb",
            &member_hash,
        )
        .replace(
            "2c75b53935137167151018c0f2245d03b262f5faa66e9d46be24d0de0f3ddf7f",
            &hex::encode(Sha256::digest(public_python().as_bytes())),
        );
    fs::write(root.join("scripts/renpy-himanatsu.luau"), recipe).unwrap();
    fs::write(
        root.join("scripts/common/renpy_rpa.luau"),
        include_str!("../../../../Metadata/patches/common/renpy_rpa.luau"),
    )
    .unwrap();
}

#[test]
fn recipe_replaces_only_selected_compiled_member_with_standard_source_and_retains_raw_payload() {
    let root = tempfile::tempdir().unwrap();
    let source = public_python();
    let member = public_rpc2(std::slice::from_ref(&source));
    let input = conversion_archive(&member, "present");
    seed_conversion_recipe(root.path(), &input, &member, false);
    let result = invoke(
        root.path(),
        &root.path().join("scripts/renpy-himanatsu.luau"),
    )
    .unwrap();
    assert_eq!(result.profile.plugin_id, "renpy");
    assert!(result.profile.configuration.is_empty());
    let index_offset =
        usize::from_str_radix(std::str::from_utf8(&input[8..24]).unwrap(), 16).unwrap();
    for name in ["audio", "code", "font", "gui", "image", "movie", "patch"] {
        let output = fs::read(result.output.join(format!("game/{name}.rpa"))).unwrap();
        assert_eq!(&output[..index_offset], &input[..index_offset]);
        let index = decode_standard(&output);
        let untouched = HashableValue::String("public/untouched.rpyc".into());
        assert_eq!(standard_read(&output, &index[&untouched]), UNTOUCHED);
        let selected = HashableValue::String(RESOLUTION_MEMBER.into());
        if name == "code" {
            assert_eq!(index.len(), 1);
            assert!(!index.contains_key(&selected));
        } else {
            assert_eq!(index.len(), 2);
            assert_eq!(standard_read(&output, &index[&selected]), member);
        }
        assert_eq!(
            fs::read(root.path().join(format!("source/game/{name}.rpa"))).unwrap(),
            input
        );
    }
    let expected = format!(
        "define config.physical_width = 1280\n\
define config.physical_height = 720\n\
define kagami_hd_text_enabled = True\n\n\
init 100 python:\n    {}\n",
        source.replace('\n', "\n    ")
    );
    assert_eq!(
        fs::read_to_string(
            result
                .output
                .join("game/kagami_games/code/system/force_720p.rpy")
        )
        .unwrap(),
        expected
    );
    for path in [
        "game/kagami_games/code/system/force_720p.rpyc",
        "game/saves",
        "game/cache",
        "public.exe",
        "public.dll",
    ] {
        assert!(!result.output.join(path).exists(), "{path}");
    }
}

fn rejects_conversion(member: &[u8], target: &str, wrong_member_hash: bool, kind: PatchErrorKind) {
    let root = tempfile::tempdir().unwrap();
    let input = conversion_archive(member, target);
    seed_conversion_recipe(root.path(), &input, member, wrong_member_hash);
    let error = invoke(
        root.path(),
        &root.path().join("scripts/renpy-himanatsu.luau"),
    )
    .err()
    .unwrap();
    assert_eq!(error.kind(), kind, "{error}");
    assert!(!root.path().join("output").exists());
    for name in ["audio", "code", "font", "gui", "image", "movie", "patch"] {
        assert_eq!(
            fs::read(root.path().join(format!("source/game/{name}.rpa"))).unwrap(),
            input
        );
    }
    assert!(!fs::read_dir(root.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".shinki-patch-staging-")
    }));
}

#[test]
fn required_member_and_source_fingerprints_fail_transactionally() {
    let source = public_python();
    let member = public_rpc2(std::slice::from_ref(&source));
    for target in ["missing", "duplicate"] {
        rejects_conversion(&member, target, false, PatchErrorKind::Integrity);
    }
    rejects_conversion(&member, "present", true, PatchErrorKind::Integrity);
    let wrong_source = source.replacen("public_trace", "public_other", 1);
    assert_eq!(wrong_source.len(), source.len());
    for strings in [
        vec![wrong_source],
        vec!["public_short_source()\n".to_owned()],
        vec![source.clone(), source],
        Vec::new(),
    ] {
        rejects_conversion(
            &public_rpc2(&strings),
            "present",
            false,
            PatchErrorKind::Integrity,
        );
    }
}

#[test]
fn rpc2_slot_ranges_and_bounded_inflate_fail_transactionally() {
    let member = public_rpc2(&[public_python()]);
    for (field_offset, value) in [
        (26, 1386_u32), // Slot 2 begins outside the member.
        (30, u32::MAX), // Slot 2 extends outside the member.
        (26, 10),       // Slot 2 overlaps the table.
        (26, 46),       // Slot 2 overlaps slot 1.
    ] {
        let mut invalid = member.clone();
        invalid[field_offset..field_offset + 4].copy_from_slice(&value.to_le_bytes());
        rejects_conversion(&invalid, "present", false, PatchErrorKind::Bounds);
    }
    let mut missing = member.clone();
    missing[22..26].copy_from_slice(&3_u32.to_le_bytes());
    rejects_conversion(&missing, "present", false, PatchErrorKind::Integrity);
    let mut duplicate = member.clone();
    duplicate[22..26].copy_from_slice(&1_u32.to_le_bytes());
    rejects_conversion(&duplicate, "present", false, PatchErrorKind::Integrity);
    let mut corrupt = member.clone();
    corrupt[50] = 0;
    rejects_conversion(&corrupt, "present", false, PatchErrorKind::Codec);
    let bomb = compressed(&vec![b'x'; 64 * 1024 * 1024 + 1]);
    let mut bounded = member;
    // A separate member-size identity is deliberately allowed in this fixture;
    // the production recipe remains pinned to the 1386-byte member.
    bounded.truncate(50);
    bounded[30..34].copy_from_slice(&(bomb.len() as u32).to_le_bytes());
    bounded.extend(bomb);
    let root = tempfile::tempdir().unwrap();
    let input = conversion_archive(&bounded, "present");
    seed_conversion_recipe(root.path(), &input, &bounded, false);
    let script = root.path().join("scripts/renpy-himanatsu.luau");
    let recipe = fs::read_to_string(&script)
        .unwrap()
        .replace("size = 1386,", &format!("size = {},", bounded.len()));
    fs::write(&script, recipe).unwrap();
    let error = invoke(root.path(), &script).err().unwrap();
    assert_eq!(error.kind(), PatchErrorKind::Bounds, "{error}");
    assert!(!root.path().join("output").exists());
}
