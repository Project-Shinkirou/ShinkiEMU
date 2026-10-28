---
title: Sena
description: Formats, native behavior and limits of Sena.
---

# Sena

`plugin_id: sena-rs` · Engine family: SoftPAL

Upstream: [xmoezzz/sena-rs](https://github.com/xmoezzz/sena-rs). Adaptation fork: [Project-Shinkirou/sena-rs, shinki branch](https://github.com/Project-Shinkirou/sena-rs/tree/shinki).

## Formats

SoftPAL combines `SCRIPT.SRC` bytecode with `POINT.DAT` branch/call targets, `FILE.DAT` resource names, `TEXT.DAT` text and `MEM.DAT` initial data. The `Sv20` script header is 12 bytes and supplies an explicit entry PC. Instructions/data use little-endian words; extcall arguments must follow native pop order, not a documentation display order.

## Native behavior and limits

The fork owns the SoftPAL VM, wait semantics, native rendering/audio and saves. A successful native restore must leave subsequent saving enabled. The adapter exposes `text_language=jp/cn/tc`; native multilingual record selection preserves the original single-field Japanese behavior. Fonts come from native game paths or explicit native configuration, and render extent is requested through ABI v3.

The adapter's game scope remains the native and Steam Koikake paths. It does not establish support for every SoftPAL title. Unknown external calls and indirect-memory paths must be assessed against the actual runtime; table recognition alone is insufficient.

## Save paths

Numbered native slots remain `<game>/save/saveNNN.dat`. Reverse-engineering notes
identify the metadata `SaveFolder` as the directory used by
`VmTask_WriteSystemDat`; for the shipped layout this writes
`<game>/save/system.dat` (`reverse/GAME_VM_RE.md:489-492`,
`reverse/IDB_CHANGELOG.md:782-801`). The fork's portable companions
`system.ini`, `system_mem.bin`, `read_lines.bin`, and `history/` remain under
`<game>/save/sena_rs/` because they are fork state, not files written by the
original engine.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
