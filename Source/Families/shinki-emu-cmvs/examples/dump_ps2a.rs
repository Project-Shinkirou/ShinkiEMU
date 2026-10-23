//! Dumps the decoded PS2A instruction stream of one script.
//!
//! Usage: `cargo run --release --example dump_ps2a -- <game_root> <profile> <script> [search_source] [start] [end]`
//!
//! The dump prints one line per decoded instruction as
//! `<offset> len=<bytes> <instruction debug>`, matching the navigation dumps
//! recorded in the porting log. It always decodes with the current crate so
//! the offsets stay in step with the VM. `search_source` names a game-root
//! relative CPZ archive (or an empty string for loose root files) used as
//! the script search domain; it defaults to `data\pack\script.cpz` with a
//! loose-root fallback.

use std::{collections::BTreeMap, path::PathBuf};

use shinki_emu_cmvs::{
    frame_ps2a_instruction, mount_cmvs, parse_ps2a, CmvsPs2aControlInstruction,
    CmvsPs2aInstructionFrame, CmvsPs2aVmState, CmvsResourceSearchBank,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("missing game root")?);
    let profile = PathBuf::from(args.next().ok_or("missing profile path")?);
    let script_name = args.next().ok_or("missing script name")?;
    let search_source = args
        .next()
        .unwrap_or_else(|| "data\\pack\\script.cpz".to_string());
    let start: u32 = args
        .next()
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(0);
    let end: Option<u32> = args.next().map(|value| value.parse()).transpose()?;

    let archive = mount_cmvs(&root, &profile)?;
    let mut vm = CmvsPs2aVmState::new(0);
    let mut script_table: BTreeMap<u8, String> = BTreeMap::new();
    script_table.insert(0, search_source.replace('/', "\\"));
    script_table.insert(1, String::new());
    vm.resource_search_paths
        .insert(CmvsResourceSearchBank::Script, script_table);
    let uri = archive.resolve_native_resource(&vm, CmvsResourceSearchBank::Script, &script_name)?;
    let size = archive.stat(&uri)?.size;
    let source = archive.read_range(&uri, 0, size)?.bytes;
    let script = parse_ps2a(&source)?;
    println!(
        "# {script_name} ({uri}) bytes={} initial_pc={}",
        source.len(),
        script.header.initial_pc
    );

    if let Ok(out) = std::env::var("DUMP_BIN") {
        std::fs::write(out, source.as_slice())?;
    }
    if let Ok(out) = std::env::var("DUMP_DECODED") {
        std::fs::write(out, script.bytecode.as_slice())?;
    }
    if let Ok(out) = std::env::var("DUMP_NAME_INDEX") {
        use std::fmt::Write as _;
        let mut text = String::new();
        for (index, target) in script.name_index.iter().enumerate() {
            writeln!(text, "{index} {target}")?;
        }
        std::fs::write(out, text)?;
    }
    if std::env::var("DUMP_RAW").is_ok() {
        use std::io::Write;
        std::io::stdout().write_all(source.as_slice())?;
    }
    // Per-pc decode mode: decode exactly the requested pcs (one decimal
    // offset per line) so an execution trace can be reconciled against the
    // instruction stream.
    if let Ok(list) = std::env::var("DUMP_PCS") {
        for line in std::fs::read_to_string(list)?.lines() {
            let Ok(offset) = line.trim().parse::<u32>() else {
                continue;
            };
            match frame_ps2a_instruction(&script, offset) {
                Ok(frame) => {
                    println!("{offset} len={} {frame:?}", frame.span().byte_length);
                }
                Err(_) => println!("{offset} <undecoded>"),
            }
        }
        return Ok(());
    }
    let limit = end.unwrap_or(source.len() as u32);
    // Follow control flow instead of walking every byte: PS2A scripts embed
    // data tables between basic blocks, and a linear walk decodes those bytes
    // and then drifts out of alignment for the rest of the function.
    let mut pending = std::collections::BTreeSet::from([start]);
    let mut visited = std::collections::BTreeSet::new();
    while let Some(offset) = pending.pop_first() {
        if offset >= limit || !visited.insert(offset) {
            continue;
        }
        let Ok(frame) = frame_ps2a_instruction(&script, offset) else {
            println!("{offset} <undecoded>");
            pending.insert(offset + 2);
            continue;
        };
        let length = frame.span().byte_length;
        println!("{offset} len={length} {frame:?}");
        if length == 0 {
            continue;
        }
        let next = offset + u32::from(length);
        match &frame {
            CmvsPs2aInstructionFrame::Control(control) => match control {
                CmvsPs2aControlInstruction::AbsoluteJump { target, .. }
                | CmvsPs2aControlInstruction::NameIndexJump { target, .. } => {
                    pending.insert(*target);
                }
                CmvsPs2aControlInstruction::JumpWhenFlagClear { target, .. }
                | CmvsPs2aControlInstruction::JumpWhenFlagSet { target, .. }
                | CmvsPs2aControlInstruction::JumpWhenValueEquals { target, .. }
                | CmvsPs2aControlInstruction::JumpWithOpaqueField { target, .. } => {
                    pending.insert(*target);
                    pending.insert(next);
                }
                CmvsPs2aControlInstruction::StackReturn { .. }
                | CmvsPs2aControlInstruction::FrameReturn { .. }
                | CmvsPs2aControlInstruction::ScriptReturn { .. } => {}
                _ => {
                    pending.insert(next);
                }
            },
            _ => {
                pending.insert(next);
            }
        }
    }
    Ok(())
}
