use super::*;

/// Micro programs owned by this cluster, indexed by the local
/// `table_index` of contract arms.
pub(super) const OPS: &[&[NativeMicroOp]] = &[
    // Case 932 (`sub_487270`): builds `<COMPUTERNAME>_<id>.dvg` under the
    // video subsystem base path and loads its descriptor lines into the
    // per-machine video playlist (`sub_4153C0` -> `sub_4156D0` list
    // records).  The engine writes these files itself on machines that
    // played the movies; on a fresh machine `sub_419BD0`
    // (FindFirstFileA + CreateFileA) returns null and the handler is a
    // pure no-op.  The headless session has no such file, so the
    // observable contract is the no-op path.  Returns 0x4000.
    &[],
    // Case 934 (`sub_47A270`): same path build, then
    // `*(this + 20305) = sub_4786F0(path) != 0`.  `sub_4786F0` builds a
    // 0x3F40-byte scratch object, drives one virtual call and frees it;
    // it returns 1 unconditionally, so the result word is always one.
    // Returns 0x4000.
    &[NativeMicroOp::SetResult(NativeMicroSrc::Const(1))],
];

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        932 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 6,
                table_index: 0,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        934 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 6,
                table_index: 1,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    })
}
