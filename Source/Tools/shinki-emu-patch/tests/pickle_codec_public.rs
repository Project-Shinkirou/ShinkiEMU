use serde_pickle::{
    DeOptions,
    value::{HashableValue as Key, Value},
};
use shinki_emu_patch::{PatchError, PatchErrorKind, PatchRequest, apply};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

// Generated independently with CPython pickle.dumps(..., protocol=5):
// {'tuple': (b'\x00\xffdata', '雪🌿', 2**80+123, -(2**100)+7),
//  'list': [None, True, False, -2147483649, 9007199254740993, 1.25],
//  'bytes-key': {b'\x00key': [(4294967295, 17, b'')]},
//  'set': {1, 3}, 'frozen': frozenset({b'a', b'b'})}
const CPYTHON: &str = "800595af000000000000007d94288c057475706c659428430600ff64617461948c07e99baaf09f8cbf948a0b7b000000000000000000018a0d070000000000000000000000f074948c046c697374945d94284e88898a05ffffff7fff8a0701000000000020473ff4000000000000658c0962797465732d6b6579947d944304006b6579945d948a05ffffffff004b11430094879461738c03736574948f94284b014b03908c0666726f7a656e942843016194430162949194752e";
// CPython zlib.compress(b'public zlib bytes\x00\xff' * 9), and compress(b'').
const ZLIB: &str = "789c2b284dcac94c56a8cac94c5248aa2c492d66f85f30488500dd1e4447";
const ZLIB_EMPTY: &str = "789c030000000001";

// Public CPython fixtures, generated with pickle.dumps([...], protocol=3/5).
// PublicObject is a plain class with message="public object 雪🌿" and
// shared="same literal"; PublicReduce.__reduce__ returns
// (eval, ("(_ for _ in ()).throw(RuntimeError('must not execute'))",)).
// The list also contains "same literal", b"not unicode\x00\xff", "", and
// "line\nnext\x00". Unpickling requires __main__.PublicObject and would execute
// the deliberately failing reduction. Scanning needs neither.
const CPYTHON_OBJECT3: &str = concat!(
    "80035d710028635f5f6d61696e5f5f0a5075626c69634f626a6563740a710129817102",
    "7d71032858070000006d657373616765710458150000007075626c6963206f626a65637420",
    "e99baaf09f8cbf710558060000007368617265647106580c00000073616d65206c697465",
    "72616c71077562636275696c74696e730a6576616c0a71085837000000285f20666f7220",
    "5f20696e202829292e7468726f772852756e74696d654572726f7228276d757374206e6f",
    "742065786563757465272929710985710a52710b580c00000073616d65206c69746572616c",
    "710c430d6e6f7420756e69636f646500ff710d5800000000710e580a0000006c696e650a",
    "6e65787400710f652e",
);
const CPYTHON_OBJECT5: &str = concat!(
    "800595e4000000000000005d94288c085f5f6d61696e5f5f948c0c5075626c69634f626a",
    "6563749493942981947d94288c076d657373616765948c157075626c6963206f626a656374",
    "20e99baaf09f8cbf948c06736861726564948c0c73616d65206c69746572616c9475628c",
    "086275696c74696e73948c046576616c9493948c37285f20666f72205f20696e20282929",
    "2e7468726f772852756e74696d654572726f7228276d757374206e6f742065786563757465",
    "27292994859452948c0c73616d65206c69746572616c94430d6e6f7420756e69636f646500",
    "ff948c00948c0a6c696e650a6e6578740094652e",
);

fn hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn run(input: &[u8], body: &str) -> Result<Vec<u8>, PatchError> {
    run_with_cancel(input, body, false)
}
fn run_with_cancel(input: &[u8], body: &str, cancel: bool) -> Result<Vec<u8>, PatchError> {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let patch = root.path().join("patch");
    let script = root.path().join("codec.luau");
    let output = root.path().join("output");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&patch).unwrap();
    fs::write(source.join("public.bin"), input).unwrap();
    fs::write(
        &script,
        format!(
            r#"
return {{ id="public-generic-codec", version=1, title="Public generic codec",
apply=function(ctx)
    local source = ctx.open("source", "public.bin")
    local input = source:read(0, source:size())
    {body}
    return {{plugin_id="public.codec", configuration={{}}}}
end }}
"#
        ),
    )
    .unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let result = apply(
        PatchRequest {
            script,
            source: source.clone(),
            patch,
            mode: shinki_emu_patch::PatchMode::NewDirectory { output: output.clone() },
        },
        cancelled,
        move |event| {
            if cancel && event.phase == "cancel pickle strings" {
                flag.store(true, Ordering::Release);
            }
        },
    );
    assert_eq!(fs::read(source.join("public.bin")).unwrap(), input);
    match result {
        Ok(result) => Ok(fs::read(result.output.join("result.bin")).unwrap()),
        Err(error) => {
            assert!(!output.exists());
            Err(error)
        }
    }
}
const WRITE: &str = r#"
local out = ctx.create("result.bin")
out:write(0, result)
out:close()
"#;
fn fail(input: &[u8], body: &str, kind: PatchErrorKind) {
    let error = run(input, body).unwrap_err();
    assert_eq!(error.kind(), kind, "{error}");
    // Diagnostics must not carry arbitrary payload bytes from either parser.
    assert!(!error.to_string().contains("private-marker"));
}

#[test]
fn cpython_object_literals_scan_without_imports_or_object_construction() {
    for (fixture, modern) in [(CPYTHON_OBJECT3, false), (CPYTHON_OBJECT5, true)] {
        let bytes = hex(fixture);
        // A data-only reader still rejects exactly the object payload that the
        // non-evaluating scanner accepts.
        fail(&bytes, "ctx.pickle.decode(input)", PatchErrorKind::Codec);
        let body = format!(
            r#"
local values = ctx.pickle.strings(input)
local expected = {{
    "message", "public object 雪🌿", "shared", "same literal",
    "(_ for _ in ()).throw(RuntimeError('must not execute'))",
    "same literal", "", "line\nnext\0"
}}
if {modern} then
    table.insert(expected, 1, "__main__")
    table.insert(expected, 2, "PublicObject")
    table.insert(expected, 7, "builtins")
    table.insert(expected, 8, "eval")
end
assert(#values == #expected)
for index, text in expected do
    assert(values[index] == text)
end
local result = buffer.fromstring(values[if {modern} then 4 else 2])
{WRITE}
"#
        );
        assert_eq!(run(&bytes, &body).unwrap(), "public object 雪🌿".as_bytes());
    }
}

#[test]
fn string_scanner_returns_inline_occurrences_not_memo_or_bytes() {
    // CPython's data fixture contains a Unicode leaf and several bytes leaves.
    let body = format!(
        r#"
local values = ctx.pickle.strings(input)
local expected = {{"tuple", "雪🌿", "list", "bytes-key", "set", "frozen"}}
assert(#values == #expected)
for index, text in expected do assert(values[index] == text) end
local result = buffer.fromstring(values[2])
{WRITE}
"#
    );
    assert_eq!(run(&hex(CPYTHON), &body).unwrap(), "雪🌿".as_bytes());

    // A repeated memo reference adds no literal; repeated inline literals do.
    let body = format!(
        r#"
local values = ctx.pickle.strings(input)
assert(#values == 2 and values[1] == "one" and values[2] == "one")
local result = buffer.fromstring(table.concat(values, "|"))
{WRITE}
"#
    );
    assert_eq!(
        run(b"\x80\x05]\x94(\x8c\x03one\x94h\x01\x8c\x03onee.", &body).unwrap(),
        b"one|one"
    );
}

#[test]
fn scanner_supports_binary_unicode_protocols_without_evaluating_buffer_opcodes() {
    let body = format!(
        r#"
local values = ctx.pickle.strings(input)
assert(#values == 1 and values[1] == "雪")
local result = buffer.fromstring(values[1])
{WRITE}
"#
    );
    for protocol in 2..=5 {
        let mut bytes = vec![0x80, protocol, b'X', 3, 0, 0, 0];
        bytes.extend("雪".as_bytes());
        bytes.push(b'.');
        assert_eq!(run(&bytes, &body).unwrap(), "雪".as_bytes());
    }
    let mut wide = b"\x80\x04\x8d".to_vec();
    wide.extend(3_u64.to_le_bytes());
    wide.extend("雪".as_bytes());
    wide.push(b'.');
    assert_eq!(run(&wide, &body).unwrap(), "雪".as_bytes());
    let mut protocol4 = hex(CPYTHON_OBJECT5);
    protocol4[1] = 4;
    let body = format!(
        r#"
local values = ctx.pickle.strings(input)
assert(#values == 12)
local result = buffer.fromstring(values[4])
{WRITE}
"#
    );
    assert_eq!(
        run(&protocol4, &body).unwrap(),
        "public object 雪🌿".as_bytes()
    );

    // Opaque protocol-5 buffers are wire syntax, not a request to resolve or
    // materialize an external buffer. Their bytes must not become Unicode.
    let body = format!(
        r#"
assert(#ctx.pickle.strings(input) == 0)
local result = buffer.create(0)
{WRITE}
"#
    );
    assert!(run(b"\x80\x05\x97\x98.", &body).unwrap().is_empty());
    assert!(
        run(b"\x80\x05\x96\x02\0\0\0\0\0\0\0\xff\0.", &body)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn scanner_rejects_corrupt_framing_protocols_unicode_and_trailing_bytes() {
    let mut framed = hex(CPYTHON_OBJECT5);
    framed[3..11].copy_from_slice(&4_u64.to_le_bytes());
    let mut nested = b"\x80\x05\x95".to_vec();
    nested.extend(10_u64.to_le_bytes());
    nested.push(0x95);
    nested.extend(1_u64.to_le_bytes());
    nested.push(b'.');
    for bytes in [
        Vec::new(),
        b"N.".to_vec(),
        b"\x80\x01N.".to_vec(),
        b"\x80\x06N.".to_vec(),
        b"\x80\x05".to_vec(),
        b"\x80\x05N".to_vec(),
        b"\x80\x05N..".to_vec(),
        b"\x80\x05N.\x80\x05N.".to_vec(),
        b"\x80\x05\x80\x05N.".to_vec(),
        b"\x80\x03\x8c\x01a.".to_vec(),
        b"\x80\x04\x97.".to_vec(),
        b"\x80\x05X\x02\0\0\0a.".to_vec(),
        b"\x80\x05X\x01\0\0\0\xff.".to_vec(),
        b"\x80\x05\x8c\x03\xed\xa0\x80.".to_vec(),
        b"\x80\x05\x8d\xff\xff\xff\xff\xff\xff\xff\xff.".to_vec(),
        b"\x80\x05Vraw\\u96ea\n.".to_vec(),
        b"\x80\x05cprivate-marker\n".to_vec(),
        b"\x80\x05(N.".to_vec(),
        b"\x80\x05t.".to_vec(),
        framed,
        nested,
    ] {
        fail(&bytes, "ctx.pickle.strings(input)", PatchErrorKind::Codec);
    }
    let mut oversized_frame = b"\x80\x05\x95".to_vec();
    oversized_frame.extend(u64::MAX.to_le_bytes());
    oversized_frame.extend(b"N.");
    fail(
        &oversized_frame,
        "ctx.pickle.strings(input)",
        PatchErrorKind::Bounds,
    );
}

#[test]
fn scanner_applies_native_atom_operation_depth_and_cumulative_budgets() {
    fail(
        b"",
        "ctx.pickle.strings(buffer.create(67108865))",
        PatchErrorKind::Bounds,
    );
    for opcode in *b"XB" {
        let mut bytes = vec![0x80, 5, opcode];
        bytes.extend(1_048_577_u32.to_le_bytes());
        bytes.extend(vec![b'x'; 1_048_577]);
        bytes.push(b'.');
        fail(&bytes, "ctx.pickle.strings(input)", PatchErrorKind::Bounds);
    }
    let mut deep = b"\x80\x05".to_vec();
    deep.extend([b'('; 65]);
    deep.push(b'.');
    fail(&deep, "ctx.pickle.strings(input)", PatchErrorKind::Bounds);
    let mut boundary = b"\x80\x05".to_vec();
    boundary.extend([b'('; 64]);
    boundary.push(b'N');
    boundary.extend([b't'; 64]);
    boundary.push(b'.');
    let body = format!(
        "assert(#ctx.pickle.strings(input) == 0); local result = buffer.create(0); {WRITE}"
    );
    assert!(run(&boundary, &body).unwrap().is_empty());
    let mut operations = b"\x80\x05".to_vec();
    operations.extend(vec![b'N'; 2_000_000]);
    operations.push(b'.');
    fail(
        &operations,
        "ctx.pickle.strings(input)",
        PatchErrorKind::Bounds,
    );
    let mut literals = b"\x80\x05".to_vec();
    for _ in 0..500_001 {
        literals.extend(b"\x8c\0");
    }
    literals.push(b'.');
    fail(
        &literals,
        "ctx.pickle.strings(input)",
        PatchErrorKind::Bounds,
    );

    // Each literal is small, but all native copies + Lua result storage are
    // charged cumulatively before allocating the result array.
    let mut cumulative = b"\x80\x05".to_vec();
    for _ in 0..33 {
        cumulative.push(b'X');
        cumulative.extend(1_048_576_u32.to_le_bytes());
        cumulative.extend(vec![b'x'; 1_048_576]);
    }
    cumulative.push(b'.');
    fail(
        &cumulative,
        "ctx.pickle.strings(input)",
        PatchErrorKind::Bounds,
    );
}

#[test]
fn scanner_observes_public_transaction_cancellation() {
    let error = run_with_cancel(
        &hex(CPYTHON_OBJECT5),
        r#"ctx.progress("cancel pickle strings",0,1); ctx.pickle.strings(input)"#,
        true,
    )
    .unwrap_err();
    assert_eq!(error.kind(), PatchErrorKind::Cancelled);
}

#[test]
fn cpython_types_survive_luau_edits_and_mature_standard_reader() {
    let result = run(&hex(CPYTHON), &format!(r#"
local node = ctx.pickle.decode(input)
assert(node.kind == "dict")
local function field(name)
    for _, entry in ipairs(node.entries) do
        if entry.key.kind == "string" and entry.key.value == name then return entry.value end
    end
    error("missing public key")
end
local tuple = field("tuple")
assert(tuple.kind == "tuple" and #tuple.items == 4)
assert(tuple.items[1].kind == "bytes" and buffer.tostring(tuple.items[1].value) == string.char(0,255).."data")
assert(tuple.items[2].kind == "string" and tuple.items[2].value == "雪🌿")
assert(tuple.items[3].kind == "int" and tuple.items[3].value == "1208925819614629174706299")
assert(tuple.items[4].kind == "int" and tuple.items[4].value == "-1267650600228229401496703205369")
local list = field("list")
assert(list.kind == "list" and list.items[1].kind == "none")
assert(list.items[2].kind == "bool" and list.items[2].value == true)
assert(list.items[3].kind == "bool" and list.items[3].value == false)
assert(list.items[4].value == "-2147483649" and list.items[5].value == "9007199254740993")
assert(list.items[6].kind == "float" and list.items[6].value == 1.25)
local nested = field("bytes-key")
assert(nested.entries[1].key.kind == "bytes")
assert(buffer.tostring(nested.entries[1].key.value) == string.char(0).."key")
assert(nested.entries[1].value.items[1].kind == "tuple")
assert(field("set").kind == "set" and field("frozen").kind == "frozenset")
tuple.items[3].value = "18446744073709551617"
tuple.items[1].value = buffer.fromstring("changed"..string.char(0,255))
list.items[4].value = "-9223372036854775809"
local result = ctx.pickle.encode(node)
-- The writer's safe builtin set constructors must also roundtrip in this API.
local again = ctx.pickle.decode(result)
assert(again.kind == "dict" and #again.entries == #node.entries)
{WRITE}
"#)).unwrap();
    assert_eq!(&result[..2], &[0x80, 3]);
    let Value::Dict(map) = serde_pickle::value_from_slice(&result, DeOptions::new()).unwrap()
    else {
        panic!("dict expected")
    };
    let Value::Tuple(tuple) = &map[&Key::String("tuple".into())] else {
        panic!("tuple expected")
    };
    assert_eq!(tuple[0], Value::Bytes(b"changed\x00\xff".to_vec()));
    assert_eq!(tuple[1], Value::String("雪🌿".into()));
    assert_eq!(
        tuple[2],
        Value::Int("18446744073709551617".parse().unwrap())
    );
    assert_eq!(
        tuple[3],
        Value::Int("-1267650600228229401496703205369".parse().unwrap())
    );
    let Value::List(list) = &map[&Key::String("list".into())] else {
        panic!("list expected")
    };
    assert_eq!(list[3], Value::Int("-9223372036854775809".parse().unwrap()));
    assert!(matches!(map[&Key::String("set".into())], Value::Set(_)));
    assert!(matches!(
        map[&Key::String("frozen".into())],
        Value::FrozenSet(_)
    ));
}

#[test]
fn scalar_memo_aliases_preserve_bytes_without_container_aliases() {
    // Independent protocol3 form of [b'private-marker', b'private-marker'].
    let input = b"\x80\x03](C\x0eprivate-markerq\x00h\x00e.";
    let bytes = run(
        input,
        &format!(
            r#"
local node = ctx.pickle.decode(input)
assert(node.kind == "list" and #node.items == 2)
assert(node.items[1].kind == "bytes" and node.items[2].kind == "bytes")
assert(buffer.tostring(node.items[1].value) == buffer.tostring(node.items[2].value))
local result = ctx.pickle.encode(node)
{WRITE}
"#
        ),
    )
    .unwrap();
    assert_eq!(
        serde_pickle::value_from_slice(&bytes, DeOptions::new()).unwrap(),
        Value::List(vec![Value::Bytes(b"private-marker".to_vec()); 2])
    );
}

#[test]
fn corrupt_unsafe_recursive_and_duplicate_pickle_inputs_fail_closed() {
    let mut trailing = hex(CPYTHON);
    trailing.push(0);
    let mut truncated = hex(CPYTHON);
    truncated.pop();
    for bytes in [
        trailing,
        truncated,
        b"\x80\x05]q\x00h\x00a.".to_vec(),
        b"\x80\x03cos\nsystem\nC\x0eprivate-marker\x85R.".to_vec(),
        b"\x80\x05h\x00.".to_vec(),
        b"\x80\x05}(K\x01K\x02K\x01K\x03u.".to_vec(),
        b"\x80\x05}(K\x01u.".to_vec(),
        b"\x80\x05\x8e\xff\xff\xff\xff\xff\xff\xff\xff.".to_vec(),
        b"\x80\x03G\x7f\xf0\x00\x00\x00\x00\x00\x00.".to_vec(),
        b"\x80\x03}G\x3f\xf0\x00\x00\x00\x00\x00\x00Ns.".to_vec(),
    ] {
        let error = run(&bytes, "ctx.pickle.decode(input)").unwrap_err();
        assert!(matches!(
            error.kind(),
            PatchErrorKind::Codec | PatchErrorKind::Bounds
        ));
        assert!(!error.to_string().contains("private-marker"));
    }
}

#[test]
fn invalid_lua_graphs_decimal_ranges_and_sparse_arrays_are_rejected() {
    for body in [
        r#"local n={kind="list",items={}}; n.items[1]=n; ctx.pickle.encode(n)"#,
        r#"local n={kind="none"}; ctx.pickle.encode({kind="tuple",items={n,n}})"#,
        r#"ctx.pickle.encode({kind="list",items={[1]={kind="none"},[3]={kind="none"}}})"#,
        r#"ctx.pickle.encode({kind="none",value=false})"#,
        r#"ctx.pickle.encode({kind="int",value="01"})"#,
        r#"ctx.pickle.encode({kind="int",value="-0"})"#,
        r#"ctx.pickle.encode({kind="int",value="1e3"})"#,
        r#"ctx.pickle.encode({kind="int",value=9007199254740992})"#,
        r#"ctx.pickle.encode({kind="bytes",value="not a buffer"})"#,
        r#"ctx.pickle.encode({kind="string",value=string.char(255)})"#,
        r#"ctx.pickle.encode(setmetatable({kind="none"}, {__index=function() error("must not execute") end}))"#,
        r#"ctx.pickle.encode({kind="float",value=0/0})"#,
        r#"ctx.pickle.encode({kind="dict",entries={{key={kind="list",items={}},value={kind="none"}}}})"#,
        r#"ctx.pickle.encode({kind="dict",entries={{key={kind="int",value="1"},value={kind="none"}},{key={kind="bool",value=true},value={kind="none"}}}})"#,
        r#"ctx.pickle.encode({kind="set",items={{kind="float",value=1.5}}})"#,
    ] {
        fail(b"", body, PatchErrorKind::Codec);
    }
    fail(
        b"",
        r#"ctx.pickle.encode({kind="int",value=string.rep("9",621)})"#,
        PatchErrorKind::Bounds,
    );
    fail(
        b"",
        r#"ctx.pickle.encode({kind="bytes",value=buffer.create(1048577)})"#,
        PatchErrorKind::Bounds,
    );
    fail(
        b"",
        r#"local n={kind="none"}; for i=1,65 do n={kind="tuple",items={n}} end; ctx.pickle.encode(n)"#,
        PatchErrorKind::Bounds,
    );
}

#[test]
fn declared_pickle_lengths_depth_nodes_and_scalar_amplification_are_bounded() {
    let mut deep = b"\x80\x03N".to_vec();
    deep.extend([0x85; 65]);
    deep.push(b'.');
    fail(&deep, "ctx.pickle.decode(input)", PatchErrorKind::Bounds);
    let mut nodes = b"\x80\x03](".to_vec();
    nodes.extend(vec![b'N'; 500_001]);
    nodes.extend(b"e.");
    fail(&nodes, "ctx.pickle.decode(input)", PatchErrorKind::Bounds);
    // A tiny number of scalar memo GETs can request hundreds of MiB of copies.
    let mut amplification = b"\x80\x03](B".to_vec();
    amplification.extend((1024_u32 * 1024).to_le_bytes());
    amplification.extend(vec![b'x'; 1024 * 1024]);
    amplification.extend(b"q\x00");
    for _ in 0..64 {
        amplification.extend(b"h\x00");
    }
    amplification.extend(b"e.");
    fail(
        &amplification,
        "ctx.pickle.decode(input)",
        PatchErrorKind::Bounds,
    );
    let mut integer = b"\x80\x03\x8b".to_vec();
    integer.extend(257_u32.to_le_bytes());
    integer.extend([0; 257]);
    integer.push(b'.');
    fail(&integer, "ctx.pickle.decode(input)", PatchErrorKind::Bounds);
}

#[test]
fn bounded_zlib_accepts_actual_sizes_below_or_at_limit_including_zero() {
    let body = format!(
        r#"
local expected = string.rep("public zlib bytes"..string.char(0,255),9)
local exact = ctx.codec.inflate_bounded(input,#expected)
local result = ctx.codec.inflate_bounded(input,#expected+1024)
assert(buffer.tostring(result) == expected and buffer.tostring(exact) == expected)
{WRITE}
"#
    );
    assert_eq!(
        run(&hex(ZLIB), &body).unwrap(),
        b"public zlib bytes\x00\xff".repeat(9)
    );
    assert_eq!(
        run(
            &hex(ZLIB_EMPTY),
            &format!("local result=ctx.codec.inflate_bounded(input,0); {WRITE}")
        )
        .unwrap(),
        b""
    );
    fail(
        &hex(ZLIB),
        "ctx.codec.inflate_bounded(input,0)",
        PatchErrorKind::Bounds,
    );
    fail(
        &hex(ZLIB),
        "ctx.codec.inflate_bounded(input,170)",
        PatchErrorKind::Bounds,
    );
    fail(
        &hex(ZLIB),
        "ctx.codec.inflate(input,172)",
        PatchErrorKind::Codec,
    );
    for limit in ["-1", "0.5", "67108865", "\"12\""] {
        fail(
            &hex(ZLIB),
            &format!("ctx.codec.inflate_bounded(input,{limit})"),
            PatchErrorKind::Bounds,
        );
    }
}

#[test]
fn bounded_zlib_rejects_truncation_junk_and_concatenated_streams() {
    let bytes = hex(ZLIB);
    let mut junk = bytes.clone();
    junk.push(0);
    let mut concatenated = bytes.clone();
    concatenated.extend(hex(ZLIB_EMPTY));
    for input in [
        b"".as_slice(),
        &bytes[..bytes.len() - 1],
        junk.as_slice(),
        concatenated.as_slice(),
    ] {
        fail(
            input,
            "ctx.codec.inflate_bounded(input,1024)",
            PatchErrorKind::Codec,
        );
    }
}
