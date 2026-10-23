//! Data-only pickle bridge. serde-pickle owns value decoding/encoding; the
//! borrowed repugnant-pickle opcode parser supplies an allocation admission pass.
//!
//! Accepted: binary protocols 2..=5, finite data values, scalar memo references.
//! Container memo references, memo replacement, bytearray/out-of-band buffers,
//! object construction and textual protocol 0/1 forms are rejected. The only
//! GLOBAL/REDUCE pair admitted is the mature library's hard-coded data-only
//! builtins set/frozenset(list) conversion; no module or callable is executed.
//! Float keys (including nested tuple/frozenset keys) are rejected because the
//! mature reader's mixed float/integer ordering is not exact Python equality.
//! Memo IDs must be fresh and sequential (as emitted by CPython).
//!
//! serde-pickle 1.2.0 has no allocation/depth limits. Its memoize/resolve paths
//! clone values, and convert_value recursively expands references. Therefore
//! admission happens BEFORE invoking it: at most 500k value occurrences, 250k
//! memo entries, depth 64, 2M operations, 1MiB atoms and 256-byte binary integers.
//! A cumulative 256MiB envelope charges 512 bytes per value/memo/mark and eight
//! times scalar storage, covering both internal/public trees, geometric Vec
//! growth, BTree nodes, memo clones, bigint and bridge scratch. A fixed 1MiB
//! admission charge covers the reader and the mature parser's 128-element
//! stacks for all live marks. No container can be cloned through GET or
//! re-memoization. Scalar GET is charged as a full new occurrence. Input/output
//! separately use the native 64MiB buffer ceiling.
//! The safety pass retains only size/type summaries, never decoded payloads.
//!
//! `strings(Buffer) -> {string, ...}` is a separate, non-evaluating wire scan:
//! it returns inline binary Unicode literals in opcode order, including repeats
//! but not memo references, byte strings or GLOBAL names. Opaque construction
//! opcodes are parsed without resolving names, imports, persistent IDs or calls.
//! It accepts protocols 2..=5 and enforces framing, termination, opcode/atom
//! budgets and MARK nesting (64), without constructing an object graph. Textual
//! UNICODE is rejected because the borrowed parser exposes its escaped spelling.
use crate::{
    buffer::{self, BoundedWriter},
    error::check_cancelled,
    PatchError, PatchErrorKind,
};
use mlua::{Buffer, Lua, Table, Value};
use repugnant_pickle::{ops::PickleOp as Op, parsers::parse_op};
use serde_pickle::{value::Value as PyValue, DeOptions, SerOptions};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    io::{self, Read, Write},
    sync::{atomic::AtomicBool, Arc},
};

const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 500_000;
const MAX_MEMO: usize = 250_000;
const MAX_OPS: usize = 2_000_000;
const MAX_ATOM: usize = 1024 * 1024;
const MAX_INT_BYTES: usize = 256;
const MAX_INT_DIGITS: usize = 620;
const NATIVE_ENVELOPE: usize = 256 * 1024 * 1024;
const IO_CHUNK: usize = 32 * 1024;

fn codec(message: &'static str) -> PatchError {
    PatchError::new(PatchErrorKind::Codec, message)
}
fn bounds() -> PatchError {
    PatchError::new(PatchErrorKind::Bounds, "pickle resource budget exceeded")
}
fn invalid() -> mlua::Error {
    codec("invalid tagged pickle tree").into()
}

struct Budget {
    nodes: usize,
    native: usize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            nodes: 0,
            native: 1024 * 1024,
        }
    }
}
impl Budget {
    fn storage(&mut self, bytes: usize) -> Result<(), PatchError> {
        self.native = self.native.checked_add(bytes).ok_or_else(bounds)?;
        if self.native > NATIVE_ENVELOPE {
            return Err(bounds());
        }
        Ok(())
    }
    fn node(&mut self, bytes: usize) -> Result<(), PatchError> {
        if bytes > MAX_ATOM {
            return Err(bounds());
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(bounds());
        }
        self.storage(512 + bytes * 8)
    }
}

pub(crate) fn install(lua: &Lua, ctx: &Table, cancelled: Arc<AtomicBool>) -> mlua::Result<()> {
    let pickle = lua.create_table()?;
    let flag = cancelled.clone();
    pickle.set(
        "decode",
        lua.create_function(move |lua, bytes: Buffer| {
            let (value, expected_nodes) = buffer::input(lua, bytes, |bytes| decode(bytes, &flag))?;
            let mut nodes = 0;
            let result = to_lua(lua, value, &flag, &mut nodes)?;
            if nodes != expected_nodes {
                // The mature Value reader canonicalizes maps/sets. Reject duplicate
                // keys/elements instead of silently accepting that lossy conversion.
                return Err(codec("duplicate pickle keys or set elements").into());
            }
            check_cancelled(&flag)?;
            Ok(result)
        })?,
    )?;
    let flag = cancelled.clone();
    pickle.set(
        "strings",
        lua.create_function(move |lua, bytes: Buffer| {
            // Never enter Lua while buffer::input holds the borrowed buffer lock.
            let strings = buffer::input(lua, bytes, |bytes| scan_strings(bytes, &flag))?;
            let result = lua.create_table_with_capacity(strings.len(), 0)?;
            for (index, text) in strings.into_iter().enumerate() {
                check_cancelled(&flag)?;
                result.raw_set(index + 1, lua.create_string(text.as_bytes())?)?;
            }
            check_cancelled(&flag)?;
            Ok(result)
        })?,
    )?;
    pickle.set(
        "encode",
        lua.create_function(move |lua, node: Value| {
            let mut tree = LuaTree {
                lua,
                cancelled: &cancelled,
                budget: Budget::default(),
                seen: HashSet::new(),
            };
            let value = tree.node(node, 1)?;
            let mut writer = CheckedWriter {
                inner: BoundedWriter::new(),
                cancelled: &cancelled,
                failure: None,
            };
            if serde_pickle::value_to_writer(&mut writer, &value, SerOptions::new()).is_err() {
                return Err(PatchError::new(
                    writer.failure.unwrap_or(PatchErrorKind::Codec),
                    "pickle encoding failed",
                )
                .into());
            }
            check_cancelled(&cancelled)?;
            buffer::output(lua, &writer.inner.finish())
        })?,
    )?;
    ctx.set("pickle", pickle)
}

struct CheckedReader<'a> {
    remaining: &'a [u8],
    cancelled: &'a AtomicBool,
}
impl Read for CheckedReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        check_cancelled(self.cancelled).map_err(io::Error::other)?;
        let count = output.len().min(self.remaining.len()).min(IO_CHUNK);
        output[..count].copy_from_slice(&self.remaining[..count]);
        self.remaining = &self.remaining[count..];
        Ok(count)
    }
}
struct CheckedWriter<'a> {
    inner: BoundedWriter,
    cancelled: &'a AtomicBool,
    failure: Option<PatchErrorKind>,
}
impl Write for CheckedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Err(error) = check_cancelled(self.cancelled) {
            self.failure = Some(PatchErrorKind::Cancelled);
            return Err(io::Error::other(error));
        }
        let result = self.inner.write(&bytes[..bytes.len().min(IO_CHUNK)]);
        if let Err(cause) = &result {
            self.failure = Some(if cause.kind() == io::ErrorKind::OutOfMemory {
                PatchErrorKind::Memory
            } else {
                PatchErrorKind::Bounds
            });
        }
        result
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn decode(bytes: &[u8], cancelled: &AtomicBool) -> Result<(PyValue, usize), PatchError> {
    check_cancelled(cancelled)?;
    buffer::bounded(bytes.len())?;
    let nodes = admit(bytes, cancelled)?;
    let result = serde_pickle::value_from_reader(
        CheckedReader {
            remaining: bytes,
            cancelled,
        },
        DeOptions::new(),
    );
    check_cancelled(cancelled)?;
    result
        .map(|value| (value, nodes))
        .map_err(|_| codec("invalid pickle data"))
}

// This scans wire syntax only. In particular, it has no value stack, memo
// resolver or constructor dispatch; those would turn a literal scan into a VM.
fn scan_strings(bytes: &[u8], cancelled: &AtomicBool) -> Result<Vec<String>, PatchError> {
    check_cancelled(cancelled)?;
    buffer::bounded(bytes.len())?;
    let protocol = match bytes.get(..2) {
        Some([0x80, protocol @ 2..=5]) => *protocol,
        _ => return Err(codec("pickle requires binary protocol 2 through 5")),
    };
    let mut budget = Budget::default();
    let mut strings = Vec::new();
    let mut input = bytes;
    let mut frame_end = None;
    let mut marks = 0_usize;
    for step in 0..MAX_OPS {
        check_cancelled(cancelled)?;
        let position = bytes.len() - input.len();
        if frame_end == Some(position) {
            frame_end = None;
        }
        // The mature parser borrows all arguments and validates Unicode UTF-8.
        // Bound even its UTF-8/newline scan before inspecting any opcode.
        let window = &input[..input.len().min(MAX_ATOM + 16)];
        let (rest, op) = parse_op::<nom::error::Error<&[u8]>>(window)
            .map_err(|_| codec("invalid or oversized pickle opcode"))?;
        let consumed = window.len() - rest.len();
        let next = position + consumed;
        if frame_end.is_some_and(|end| next > end) {
            return Err(codec("pickle opcode crosses frame boundary"));
        }
        input = &input[consumed..];
        let minimum_protocol = match op {
            Op::PROTO(_)
            | Op::NEWOBJ
            | Op::EXT1(_)
            | Op::EXT2(_)
            | Op::EXT4(_)
            | Op::TUPLE1
            | Op::TUPLE2
            | Op::TUPLE3
            | Op::NEWTRUE
            | Op::NEWFALSE
            | Op::LONG1(_)
            | Op::LONG4(_) => 2,
            Op::BINBYTES(_) | Op::SHORT_BINBYTES(_) => 3,
            Op::SHORT_BINUNICODE(_)
            | Op::BINUNICODE8(_)
            | Op::BINBYTES8(_)
            | Op::EMPTY_SET
            | Op::ADDITEMS
            | Op::FROZENSET
            | Op::NEWOBJ_EX
            | Op::STACK_GLOBAL
            | Op::MEMOIZE
            | Op::FRAME(_) => 4,
            Op::BYTEARRAY8(_) | Op::NEXT_BUFFER | Op::READONLY_BUFFER => 5,
            _ => 0,
        };
        if minimum_protocol > protocol {
            return Err(codec("pickle opcode requires a newer protocol"));
        }
        match op {
            Op::PROTO(_) if step == 0 => {}
            Op::PROTO(_) => return Err(codec("unexpected pickle protocol header")),
            Op::FRAME(length) => {
                if frame_end.is_some() {
                    return Err(codec("nested pickle frame"));
                }
                let end = next
                    .checked_add(usize::try_from(length).map_err(|_| bounds())?)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(bounds)?;
                frame_end = Some(end);
            }
            Op::STOP => {
                if !input.is_empty() || marks != 0 || frame_end.is_some_and(|end| end != next) {
                    return Err(codec("invalid pickle termination"));
                }
                check_cancelled(cancelled)?;
                return Ok(strings);
            }
            Op::MARK => {
                marks += 1;
                if marks > MAX_DEPTH {
                    return Err(bounds());
                }
            }
            Op::POP_MARK
            | Op::LIST
            | Op::TUPLE
            | Op::DICT
            | Op::APPENDS
            | Op::SETITEMS
            | Op::ADDITEMS
            | Op::FROZENSET
            | Op::OBJ
            | Op::INST(_, _) => {
                marks = marks
                    .checked_sub(1)
                    .ok_or_else(|| codec("missing pickle mark"))?;
                if matches!(op, Op::INST(module, name)
                    if module.len() > MAX_ATOM || name.len() > MAX_ATOM)
                {
                    return Err(bounds());
                }
            }
            Op::BINUNICODE(text) | Op::SHORT_BINUNICODE(text) | Op::BINUNICODE8(text) => {
                budget.node(text.len())?;
                strings.push(text.to_owned());
            }
            Op::UNICODE(_) => return Err(codec("textual pickle Unicode is unsupported")),
            Op::BINSTRING(data)
            | Op::SHORT_BINSTRING(data)
            | Op::BINBYTES(data)
            | Op::SHORT_BINBYTES(data)
            | Op::BINBYTES8(data)
            | Op::BYTEARRAY8(data)
            | Op::LONG1(data)
            | Op::LONG4(data) => {
                if data.len() > MAX_ATOM {
                    return Err(bounds());
                }
            }
            Op::FLOAT(text)
            | Op::INT(text)
            | Op::LONG(text)
            | Op::PERSID(text)
            | Op::STRING(text)
            | Op::GET(text)
            | Op::PUT(text) => {
                if text.len() > MAX_ATOM {
                    return Err(bounds());
                }
            }
            Op::GLOBAL(module, name) if module.len() > MAX_ATOM || name.len() > MAX_ATOM => {
                return Err(bounds());
            }
            _ => {}
        }
    }
    Err(bounds())
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Kind {
    Scalar,
    List,
    Tuple,
    Dict,
    Set,
    FrozenSet,
    SetConstructor,
    FrozenConstructor,
}
#[derive(Clone, Copy)]
struct Shape {
    kind: Kind,
    nodes: usize,
    bytes: usize,
    depth: usize,
    memoized: bool,
    // A list's depth/nodes when wrapped in a singleton tuple, for the only
    // permitted data constructor. This is metadata, not a decoded value.
    list_argument: bool,
}
impl Shape {
    fn new(kind: Kind, bytes: usize) -> Self {
        Self {
            kind,
            nodes: 1,
            bytes,
            depth: 1,
            memoized: false,
            list_argument: false,
        }
    }
    fn extend(&mut self, items: &[Shape]) -> Result<(), PatchError> {
        for item in items {
            if matches!(item.kind, Kind::SetConstructor | Kind::FrozenConstructor) {
                return Err(codec("pickle callable is not a data value"));
            }
            self.nodes = self.nodes.checked_add(item.nodes).ok_or_else(bounds)?;
            self.depth = self.depth.max(item.depth + 1);
        }
        if self.nodes > MAX_NODES || self.depth > MAX_DEPTH {
            return Err(bounds());
        }
        Ok(())
    }
}

// Admission is deliberately conservative: no object values or pickle evaluator
// live here. The mature borrowed parser decodes all opcode arguments, and the
// mature value reader below remains responsible for actual pickle semantics.
fn admit(bytes: &[u8], cancelled: &AtomicBool) -> Result<usize, PatchError> {
    if !matches!(bytes.get(..2), Some([0x80, 2..=5])) {
        return Err(codec("pickle requires binary protocol 2 through 5"));
    }
    let mut budget = Budget::default();
    let mut stack: Vec<Shape> = Vec::new();
    let mut marks: Vec<Vec<Shape>> = Vec::new();
    let mut memo: Vec<Shape> = Vec::new();
    let mut input = bytes;
    let mut frame_end = None;
    for step in 0..MAX_OPS {
        check_cancelled(cancelled)?;
        let position = bytes.len() - input.len();
        if frame_end == Some(position) {
            frame_end = None;
        }
        // parse_op is nonrecursive and borrows counted strings/bytes. Cap its
        // slice too: even its UTF-8 validation cannot scan an unbounded atom.
        let window = &input[..input.len().min(MAX_ATOM + 16)];
        let (rest, op) = parse_op::<nom::error::Error<&[u8]>>(window)
            .map_err(|_| codec("invalid or oversized pickle opcode"))?;
        let consumed = window.len() - rest.len();
        let next = position + consumed;
        if frame_end.is_some_and(|end| next > end) {
            return Err(codec("pickle opcode crosses frame boundary"));
        }
        input = &input[consumed..];
        match op {
            Op::PROTO(2..=5) if step == 0 => {}
            Op::FRAME(length) if frame_end.is_none() => {
                let length = usize::try_from(length).map_err(|_| bounds())?;
                let end = next
                    .checked_add(length)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(bounds)?;
                frame_end = Some(end);
            }
            Op::STOP => {
                if !input.is_empty()
                    || !marks.is_empty()
                    || stack.len() != 1
                    || frame_end.is_some_and(|end| end != next)
                {
                    return Err(codec("invalid pickle termination"));
                }
                let root = stack.pop().ok_or_else(|| codec("empty pickle stack"))?;
                if matches!(root.kind, Kind::SetConstructor | Kind::FrozenConstructor) {
                    return Err(codec("pickle callable is not a data value"));
                }
                return Ok(root.nodes);
            }
            Op::MARK => {
                if marks.len() >= MAX_DEPTH {
                    return Err(bounds());
                }
                budget.storage(512)?;
                marks.push(std::mem::take(&mut stack));
            }
            Op::BINPUT(id) => save_memo(usize::from(id), &mut stack, &mut memo, &mut budget)?,
            Op::LONG_BINPUT(id) => save_memo(id as usize, &mut stack, &mut memo, &mut budget)?,
            Op::MEMOIZE => save_memo(memo.len(), &mut stack, &mut memo, &mut budget)?,
            Op::BINGET(id) => load_memo(usize::from(id), &mut stack, &memo, &mut budget)?,
            Op::LONG_BINGET(id) => load_memo(id as usize, &mut stack, &memo, &mut budget)?,
            Op::NONE
            | Op::NEWTRUE
            | Op::NEWFALSE
            | Op::BININT(_)
            | Op::BININT1(_)
            | Op::BININT2(_) => {
                push_scalar(&mut stack, 0, &mut budget)?;
            }
            Op::BINFLOAT(_) => {
                push_scalar(&mut stack, 0, &mut budget)?;
            }
            Op::LONG1(data) | Op::LONG4(data) => {
                if data.len() > MAX_INT_BYTES {
                    return Err(bounds());
                }
                push_scalar(&mut stack, data.len() * 4, &mut budget)?;
            }
            Op::BINSTRING(data)
            | Op::SHORT_BINSTRING(data)
            | Op::BINBYTES(data)
            | Op::SHORT_BINBYTES(data)
            | Op::BINBYTES8(data) => {
                push_scalar(&mut stack, data.len(), &mut budget)?;
            }
            Op::BINUNICODE(text) | Op::SHORT_BINUNICODE(text) | Op::BINUNICODE8(text) => {
                push_scalar(&mut stack, text.len(), &mut budget)?;
            }
            Op::EMPTY_LIST => push_container(Kind::List, &mut stack, &mut budget)?,
            Op::EMPTY_TUPLE => push_container(Kind::Tuple, &mut stack, &mut budget)?,
            Op::EMPTY_DICT => push_container(Kind::Dict, &mut stack, &mut budget)?,
            Op::EMPTY_SET => push_container(Kind::Set, &mut stack, &mut budget)?,
            Op::TUPLE1 | Op::TUPLE2 | Op::TUPLE3 => {
                let count = match op {
                    Op::TUPLE1 => 1,
                    Op::TUPLE2 => 2,
                    _ => 3,
                };
                if stack.len() < count {
                    return Err(codec("invalid pickle stack"));
                }
                let start = stack.len() - count;
                let mut shape = Shape::new(Kind::Tuple, 0);
                shape.extend(&stack[start..])?;
                shape.list_argument = count == 1 && stack[start].kind == Kind::List;
                budget.node(0)?;
                stack.truncate(start);
                stack.push(shape);
            }
            Op::LIST | Op::TUPLE | Op::DICT | Op::FROZENSET => {
                let kind = match op {
                    Op::LIST => Kind::List,
                    Op::TUPLE => Kind::Tuple,
                    Op::DICT => Kind::Dict,
                    _ => Kind::FrozenSet,
                };
                if kind == Kind::Dict && !stack.len().is_multiple_of(2) {
                    return Err(codec("odd pickle dictionary entries"));
                }
                let mut shape = Shape::new(kind, 0);
                shape.extend(&stack)?;
                shape.list_argument =
                    kind == Kind::Tuple && stack.len() == 1 && stack[0].kind == Kind::List;
                stack = marks.pop().ok_or_else(|| codec("missing pickle mark"))?;
                budget.node(0)?;
                stack.push(shape);
            }
            Op::APPEND | Op::SETITEM => {
                let count = if matches!(op, Op::APPEND) { 1 } else { 2 };
                if stack.len() <= count {
                    return Err(codec("invalid pickle stack"));
                }
                let start = stack.len() - count;
                let mut target = stack[start - 1];
                let expected = if count == 1 { Kind::List } else { Kind::Dict };
                if target.kind != expected {
                    return Err(codec("invalid pickle container"));
                }
                target.extend(&stack[start..])?;
                stack.truncate(start);
                stack[start - 1] = target;
            }
            Op::APPENDS | Op::SETITEMS | Op::ADDITEMS => {
                let kind = match op {
                    Op::APPENDS => Kind::List,
                    Op::SETITEMS => Kind::Dict,
                    _ => Kind::Set,
                };
                if kind == Kind::Dict && !stack.len().is_multiple_of(2) {
                    return Err(codec("odd pickle dictionary entries"));
                }
                let mut outer = marks.pop().ok_or_else(|| codec("missing pickle mark"))?;
                let target = outer
                    .last_mut()
                    .ok_or_else(|| codec("invalid pickle stack"))?;
                if target.kind != kind {
                    return Err(codec("invalid pickle container"));
                }
                target.extend(&stack)?;
                stack = outer;
            }
            Op::GLOBAL("builtins" | "__builtin__", "set" | "frozenset") => {
                let kind = if matches!(op, Op::GLOBAL(_, "set")) {
                    Kind::SetConstructor
                } else {
                    Kind::FrozenConstructor
                };
                budget.node(0)?;
                stack.push(Shape::new(kind, 0));
            }
            Op::REDUCE => {
                let args = stack
                    .pop()
                    .ok_or_else(|| codec("invalid pickle constructor"))?;
                let constructor = stack
                    .pop()
                    .ok_or_else(|| codec("invalid pickle constructor"))?;
                if !args.list_argument || args.memoized {
                    return Err(codec("unsupported pickle constructor arguments"));
                }
                let kind = match constructor.kind {
                    Kind::SetConstructor => Kind::Set,
                    Kind::FrozenConstructor => Kind::FrozenSet,
                    _ => return Err(codec("unsupported pickle callable")),
                };
                // Replace tuple(list(items)) with set(items), preserving the
                // admitted item occurrences without constructing their values.
                let mut shape = Shape::new(kind, 0);
                shape.nodes = args.nodes - 1;
                shape.depth = args.depth - 1;
                budget.node(0)?;
                stack.push(shape);
            }
            _ => return Err(codec("unsupported pickle opcode")),
        }
    }
    Err(bounds())
}
fn push_scalar(
    stack: &mut Vec<Shape>,
    bytes: usize,
    budget: &mut Budget,
) -> Result<(), PatchError> {
    budget.node(bytes)?;
    stack.push(Shape::new(Kind::Scalar, bytes));
    Ok(())
}
fn push_container(
    kind: Kind,
    stack: &mut Vec<Shape>,
    budget: &mut Budget,
) -> Result<(), PatchError> {
    budget.node(0)?;
    stack.push(Shape::new(kind, 0));
    Ok(())
}
fn save_memo(
    id: usize,
    stack: &mut [Shape],
    memo: &mut Vec<Shape>,
    budget: &mut Budget,
) -> Result<(), PatchError> {
    if id != memo.len() || id >= MAX_MEMO {
        return Err(bounds());
    }
    let item = stack
        .last_mut()
        .ok_or_else(|| codec("invalid pickle memo"))?;
    if item.memoized {
        if item.kind != Kind::Scalar {
            return Err(codec("container memo aliases are unsupported"));
        }
        budget.node(item.bytes)?;
    }
    budget.storage(512)?;
    item.memoized = true;
    memo.push(*item);
    Ok(())
}
fn load_memo(
    id: usize,
    stack: &mut Vec<Shape>,
    memo: &[Shape],
    budget: &mut Budget,
) -> Result<(), PatchError> {
    let item = *memo.get(id).ok_or_else(|| codec("missing pickle memo"))?;
    if item.kind != Kind::Scalar {
        return Err(codec("container memo references are unsupported"));
    }
    budget.node(item.bytes)?;
    stack.push(item);
    Ok(())
}

fn to_lua(
    lua: &Lua,
    value: PyValue,
    cancelled: &AtomicBool,
    nodes: &mut usize,
) -> mlua::Result<Table> {
    check_cancelled(cancelled)?;
    *nodes += 1;
    let result = lua.create_table()?;
    let collection_kind = match &value {
        PyValue::List(_) => "list",
        PyValue::Tuple(_) => "tuple",
        PyValue::Set(_) => "set",
        PyValue::FrozenSet(_) => "frozenset",
        _ => "",
    };
    let kind = match value {
        PyValue::None => "none",
        PyValue::Bool(value) => {
            result.raw_set("value", value)?;
            "bool"
        }
        PyValue::I64(value) => {
            result.raw_set("value", value.to_string())?;
            "int"
        }
        PyValue::Int(value) => {
            result.raw_set("value", value.to_string())?;
            "int"
        }
        PyValue::F64(value) => {
            if !value.is_finite() {
                return Err(codec("nonfinite pickle float is unsupported").into());
            }
            result.raw_set("value", value)?;
            "float"
        }
        PyValue::Bytes(value) => {
            result.raw_set("value", buffer::output(lua, &value)?)?;
            "bytes"
        }
        PyValue::String(value) => {
            result.raw_set("value", lua.create_string(value.as_bytes())?)?;
            "string"
        }
        PyValue::List(items) | PyValue::Tuple(items) => {
            let kind = collection_kind;
            let array = lua.create_table()?;
            for (index, item) in items.into_iter().enumerate() {
                array.raw_set(index + 1, to_lua(lua, item, cancelled, nodes)?)?;
            }
            result.raw_set("items", array)?;
            kind
        }
        PyValue::Set(items) | PyValue::FrozenSet(items) => {
            let kind = collection_kind;
            let array = lua.create_table()?;
            for (index, item) in items.into_iter().enumerate() {
                check_key(&item)?;
                array.raw_set(index + 1, to_lua(lua, item.into_value(), cancelled, nodes)?)?;
            }
            result.raw_set("items", array)?;
            kind
        }
        PyValue::Dict(entries) => {
            let array = lua.create_table()?;
            for (index, (key, value)) in entries.into_iter().enumerate() {
                check_key(&key)?;
                let entry = lua.create_table()?;
                entry.raw_set("key", to_lua(lua, key.into_value(), cancelled, nodes)?)?;
                entry.raw_set("value", to_lua(lua, value, cancelled, nodes)?)?;
                array.raw_set(index + 1, entry)?;
            }
            result.raw_set("entries", array)?;
            "dict"
        }
    };
    result.raw_set("kind", kind)?;
    Ok(result)
}

struct LuaTree<'a> {
    lua: &'a Lua,
    cancelled: &'a AtomicBool,
    budget: Budget,
    seen: HashSet<usize>,
}
impl LuaTree<'_> {
    fn table(&mut self, value: Value) -> mlua::Result<Table> {
        let Value::Table(table) = value else {
            return Err(invalid());
        };
        if table.metatable().is_some() || !self.seen.insert(table.to_pointer() as usize) {
            return Err(invalid());
        }
        self.budget.storage(512)?;
        Ok(table)
    }
    fn fields(&self, table: &Table, expected: &[&str]) -> mlua::Result<()> {
        let mut count = 0;
        for pair in table.clone().pairs::<Value, Value>() {
            check_cancelled(self.cancelled)?;
            let (Value::String(key), _) = pair? else {
                return Err(invalid());
            };
            if !expected
                .iter()
                .any(|name| key.as_bytes().as_ref() == name.as_bytes())
            {
                return Err(invalid());
            }
            count += 1;
            if count > expected.len() {
                return Err(invalid());
            }
        }
        if count != expected.len() {
            return Err(invalid());
        }
        Ok(())
    }
    fn array(&mut self, value: Value) -> mlua::Result<(Table, usize)> {
        let table = self.table(value)?;
        let len = table.raw_len();
        if len > MAX_NODES {
            return Err(bounds().into());
        }
        let mut count = 0;
        for pair in table.clone().pairs::<Value, Value>() {
            check_cancelled(self.cancelled)?;
            let (key, _) = pair?;
            let index = buffer::integer(key)?;
            if index == 0 || index > len as u64 {
                return Err(invalid());
            }
            count += 1;
            if count > len {
                return Err(invalid());
            }
        }
        if count != len {
            return Err(invalid());
        }
        Ok((table, len))
    }
    fn node(&mut self, value: Value, depth: usize) -> mlua::Result<PyValue> {
        check_cancelled(self.cancelled)?;
        if depth > MAX_DEPTH {
            return Err(bounds().into());
        }
        self.budget.node(0)?;
        let table = self.table(value)?;
        let Value::String(kind) = table.raw_get::<Value>("kind")? else {
            return Err(invalid());
        };
        let kind_bytes = kind.as_bytes();
        match kind_bytes.as_ref() {
            b"none" => {
                self.fields(&table, &["kind"])?;
                Ok(PyValue::None)
            }
            b"bool" => {
                self.fields(&table, &["kind", "value"])?;
                let Value::Boolean(value) = table.raw_get::<Value>("value")? else {
                    return Err(invalid());
                };
                Ok(PyValue::Bool(value))
            }
            b"int" => {
                self.fields(&table, &["kind", "value"])?;
                let Value::String(value) = table.raw_get::<Value>("value")? else {
                    return Err(invalid());
                };
                let bytes = value.as_bytes();
                if bytes.len() > MAX_INT_DIGITS + 1 {
                    return Err(bounds().into());
                }
                let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
                let digits = text.strip_prefix('-').unwrap_or(text);
                if digits.is_empty()
                    || !digits.bytes().all(|byte| byte.is_ascii_digit())
                    || (digits.len() > 1 && digits.starts_with('0'))
                    || text == "-0"
                {
                    return Err(invalid());
                }
                let value = PyValue::Int(text.parse().map_err(|_| invalid())?);
                if let PyValue::Int(integer) = &value {
                    if integer.to_signed_bytes_le().len() > MAX_INT_BYTES {
                        return Err(bounds().into());
                    }
                }
                self.budget.storage(bytes.len() * 8)?;
                Ok(value)
            }
            b"float" => {
                self.fields(&table, &["kind", "value"])?;
                let value = match table.raw_get::<Value>("value")? {
                    Value::Number(value) => value,
                    Value::Integer(value) => value as f64,
                    _ => return Err(invalid()),
                };
                if !value.is_finite() {
                    return Err(invalid());
                }
                Ok(PyValue::F64(value))
            }
            b"string" => {
                self.fields(&table, &["kind", "value"])?;
                let Value::String(value) = table.raw_get::<Value>("value")? else {
                    return Err(invalid());
                };
                let bytes = value.as_bytes();
                if bytes.len() > MAX_ATOM {
                    return Err(bounds().into());
                }
                self.budget.storage(bytes.len() * 8)?;
                let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
                Ok(PyValue::String(text.to_owned()))
            }
            b"bytes" => {
                self.fields(&table, &["kind", "value"])?;
                let Value::Buffer(value) = table.raw_get::<Value>("value")? else {
                    return Err(invalid());
                };
                let bytes = buffer::input(self.lua, value, |bytes| {
                    check_cancelled(self.cancelled)?;
                    if bytes.len() > MAX_ATOM {
                        return Err(bounds());
                    }
                    self.budget.storage(bytes.len() * 8)?;
                    Ok(bytes.to_vec())
                })?;
                Ok(PyValue::Bytes(bytes))
            }
            b"list" | b"tuple" | b"set" | b"frozenset" => {
                self.fields(&table, &["kind", "items"])?;
                let (array, len) = self.array(table.raw_get("items")?)?;
                let mut values = Vec::new();
                let mut set = BTreeSet::new();
                for index in 1..=len {
                    let value = self.node(array.raw_get(index)?, depth + 1)?;
                    if matches!(kind_bytes.as_ref(), b"set" | b"frozenset") {
                        let key = hashable(value)?;
                        if !set.insert(key) {
                            return Err(codec("duplicate pickle set element").into());
                        }
                    } else {
                        values.push(value);
                    }
                }
                Ok(match kind_bytes.as_ref() {
                    b"list" => PyValue::List(values),
                    b"tuple" => PyValue::Tuple(values),
                    b"set" => PyValue::Set(set),
                    _ => PyValue::FrozenSet(set),
                })
            }
            b"dict" => {
                self.fields(&table, &["kind", "entries"])?;
                let (array, len) = self.array(table.raw_get("entries")?)?;
                let mut entries = BTreeMap::new();
                for index in 1..=len {
                    let entry = self.table(array.raw_get(index)?)?;
                    self.fields(&entry, &["key", "value"])?;
                    let key = hashable(self.node(entry.raw_get("key")?, depth + 1)?)?;
                    let value = self.node(entry.raw_get("value")?, depth + 1)?;
                    if entries.insert(key, value).is_some() {
                        return Err(codec("duplicate pickle dictionary key").into());
                    }
                }
                Ok(PyValue::Dict(entries))
            }
            _ => Err(invalid()),
        }
    }
}
fn check_key(value: &serde_pickle::value::HashableValue) -> Result<(), PatchError> {
    use serde_pickle::value::HashableValue as Key;
    match value {
        Key::F64(_) => return Err(codec("floating pickle key is unsupported")),
        Key::Tuple(items) => {
            for item in items {
                check_key(item)?;
            }
        }
        Key::FrozenSet(items) => {
            for item in items {
                check_key(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn hashable(value: PyValue) -> mlua::Result<serde_pickle::value::HashableValue> {
    let key = value
        .into_hashable()
        .map_err(|_| mlua::Error::from(codec("unhashable pickle key or set element")))?;
    check_key(&key)?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn cancellation_interrupts_native_reader_writer_and_tree_conversion() {
        let cancelled = AtomicBool::new(false);
        let input = vec![1; IO_CHUNK * 2];
        let mut reader = CheckedReader {
            remaining: &input,
            cancelled: &cancelled,
        };
        let mut output = vec![0; input.len()];
        assert_eq!(reader.read(&mut output).unwrap(), IO_CHUNK);
        cancelled.store(true, Ordering::Release);
        assert!(reader.read(&mut output).is_err());
        assert_eq!(
            decode(b"\x80\x03N.", &cancelled).unwrap_err().kind(),
            PatchErrorKind::Cancelled
        );
        assert!(to_lua(&Lua::new(), PyValue::None, &cancelled, &mut 0).is_err());

        cancelled.store(false, Ordering::Release);
        let mut writer = CheckedWriter {
            inner: BoundedWriter::new(),
            cancelled: &cancelled,
            failure: None,
        };
        assert_eq!(writer.write(&input).unwrap(), IO_CHUNK);
        cancelled.store(true, Ordering::Release);
        assert!(writer.write(&input).is_err());
        assert_eq!(writer.failure, Some(PatchErrorKind::Cancelled));
        assert_eq!(writer.inner.finish().len(), IO_CHUNK);
    }

    #[test]
    fn memo_budget_and_replacement_are_rejected_before_mature_allocation() {
        let flag = AtomicBool::new(false);
        let mut input = b"\x80\x03".to_vec();
        for id in 0..=MAX_MEMO {
            input.extend(b"Nr");
            input.extend((id as u32).to_le_bytes());
        }
        input.push(b'.');
        assert_eq!(
            admit(&input, &flag).unwrap_err().kind(),
            PatchErrorKind::Bounds
        );
        assert!(admit(b"\x80\x03Nq\x00q\x00.", &flag).is_err());
        assert!(admit(b"\x80\x03]q\x00q\x01.", &flag).is_err());
    }

    #[test]
    fn writer_enforces_output_budget_without_payload_diagnostics() {
        let flag = AtomicBool::new(false);
        let mut writer = CheckedWriter {
            inner: BoundedWriter::new(),
            cancelled: &flag,
            failure: None,
        };
        let chunk = vec![0; IO_CHUNK];
        for _ in 0..buffer::MAX_NATIVE_BYTES / IO_CHUNK {
            writer.write_all(&chunk).unwrap();
        }
        assert!(writer.write_all(b"private-marker").is_err());
        assert_eq!(writer.failure, Some(PatchErrorKind::Bounds));
    }
}
