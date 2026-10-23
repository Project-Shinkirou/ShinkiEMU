use super::super::{CmvsPs2aCommandEffectKind, CmvsPs2aCommandStackWord};
use super::*;

/// Micro programs owned by this cluster, indexed by the local
/// `table_index` of contract arms.
pub(super) const OPS: &[&[NativeMicroOp]] = &[
    // table_index 0 (cmd 290): `sub_427870` with a2 = 0 detaches the
    // native linked list at interpreter dword 2625 (byte 10500).
    &[NativeMicroOp::SetField {
        byte_offset: 10_500,
        src: NativeMicroSrc::Const(0),
    }],
    // table_index 1 (cmd 291): `sub_427870` with a2 = 1 detaches the
    // native linked list at interpreter dword 2626 (byte 10504).
    &[NativeMicroOp::SetField {
        byte_offset: 10_504,
        src: NativeMicroSrc::Const(0),
    }],
];

pub(super) fn lookup(command_id: u16) -> Option<CommandFields> {
    Some(match command_id {
        // Handler `sub_427870` (case 290, a2 = 0): frees the native
        // LocalAlloc linked list headed at interpreter dword 2625 (byte
        // 10500, the first `native_system_lists` head) and zeroes the
        // head field. The node walk (`LocalFree` over `node[1]` chains)
        // is native heap management and is omitted; the head clear is
        // the observable interpreter write. Returns 0x4000.
        290 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 3,
                table_index: 0,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_427870` (case 291, a2 = 1): same free walk on the
        // list headed at interpreter dword 2626 (byte 10504, the second
        // `native_system_lists` head); only the head clear is kept.
        // Returns 0x4000.
        291 => (
            0,
            CmvsPs2aCommandEffectKind::NativeMicroProgram {
                cluster: 3,
                table_index: 1,
            },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4612F0` (case 413): the dispatch arm hands the
        // scene-manager object (`[esi+0xCC8]`, byte 3272) to the callee,
        // which zeroes its dwords 316 and 318. Returns 0x4000.
        413 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 316 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4612D0` (case 415): zeroes scene-manager (`[esi+
        // 0xCC8]`) dwords 319 and 321. Returns 0x4000.
        415 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 319 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_45C7F0` (case 419): zeroes scene-manager (`[esi+
        // 0xCC8]`) dwords 274 and 276. Returns 0x4000.
        419 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 274 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4613B0` (case 435): zeroes scene-manager (`[esi+
        // 0xCC8]`) dwords 310 and 312. Returns 0x4000.
        435 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 310 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_461390` (case 437): zeroes scene-manager (`[esi+
        // 0xCC8]`) dwords 313 and 315. Returns 0x4000.
        437 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 313 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_45CB60` (case 439): zeroes scene-manager (`[esi+
        // 0xCC8]`) dwords 277 and 279. Returns 0x4000.
        439 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 277 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_45CB00` (case 441): zeroes scene-manager (`[esi+
        // 0xCC8]`) dwords 280 and 282. Returns 0x4000.
        441 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 280 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_45CB20` (case 443): zeroes scene-manager (`[esi+
        // 0xCC8]`) dwords 283 and 285. Returns 0x4000.
        443 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 283 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_45CB40` (case 445): zeroes scene-manager (`[esi+
        // 0xCC8]`) dwords 286 and 288. Returns 0x4000.
        445 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 286 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4616A0` (case 479): zeroes scene-manager (`[esi+
        // 0xCC8]`) dwords 343 and 345. Returns 0x4000.
        479 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 343 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_424AC0` (case 642): the dispatch arm passes the
        // layer-worker object (`[esi+0xCF0]`, byte 3312); when the
        // worker's latch pair `obj[0] == 0 && obj[1] != 0` holds it
        // stores the `sub_41A000` millisecond timestamp at `obj[3]`,
        // zeroes `obj[2]`/`obj[4]` and sets `obj[0] = 1`. Every write is
        // object-side; the callee is omitted (no interpreter-word,
        // result-slot or return-path effect; the arm ignores the
        // callee's return 1). Returns 0x4000.
        642 => (
            0,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_424B10` (case 643): on the same layer-worker
        // object (`[esi+0xCF0]`), when `(*obj)[0] != 0 && (*obj)[1] != 0`
        // clears `(*obj)[0]` — a stop latch. Object-side only, callee
        // omitted like case 642. Returns 0x4000.
        643 => (
            0,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_461350` (case 753): zeroes scene-manager (`[esi+
        // 0xCC8]`) dwords 322 and 324. Returns 0x4000.
        753 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 322 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_4614A0` (case 755): zeroes scene-manager (`[esi+
        // 0xCC8]`) dwords 325 and 327. Returns 0x4000.
        755 => (
            0,
            CmvsPs2aCommandEffectKind::ClearSceneLayer { base_word: 325 },
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_440910` (case 826): the dispatch arm passes the
        // engine object at interpreter dword 831 (`[esi+0xCFC]`, byte
        // 3324, the texture resource-search owner). The callee spins on
        // the object's flag at byte 24756, then under its critical
        // section resets the name buffer at byte 24764 to "Default",
        // frees the payload at byte 24828 and zeroes the words at bytes
        // 24760/24756. All writes are object-side; the callee is
        // omitted (case 63 is the precedent for this object).
        // Returns 0x4000.
        826 => (
            0,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_45AE00` (case 852): the dispatch arm passes the
        // worker object at `[esi+0xCCC]` (byte 3276); the callee resets
        // fourteen of its dwords (12, 13, 11 = -1, 18, 19, 20, 22 = -1,
        // 23, 27, 21, 30, 43 = -1, 45, 46) to zero or the -1 sentinel.
        // Object-side only, callee omitted; no interpreter state is
        // recovered for this object. Returns 0x4000.
        852 => (
            0,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        // Handler `sub_45B250` (case 871): sets dword 43 of the
        // `[esi+0xCCC]` worker object (byte 3276) to -1. Object-side
        // only, callee omitted like case 852. Returns 0x4000.
        871 => (
            0,
            CmvsPs2aCommandEffectKind::NoOpCommand,
            &[] as &[CmvsPs2aCommandStackWord],
        ),
        _ => return None,
    })
}
