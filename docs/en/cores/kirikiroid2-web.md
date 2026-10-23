---
title: KiriKiri / Kirikiroid2-web
description: Formats, native behavior and limits of KiriKiri / Kirikiroid2-web.
---

# KiriKiri / Kirikiroid2-web

`plugin_id: kirikiroid2-web` · Engine family: KiriKiri

Upstream: [fenghengzhi/kirikiroid2-web](https://github.com/fenghengzhi/kirikiroid2-web). Adaptation fork: [Project-Shinkirou/kirikiroid2-web, shinki branch](https://github.com/Project-Shinkirou/kirikiroid2-web/tree/shinki).

## Formats

KiriKiri storage combines XP3 archives, loose files and native KAG/TJS resources; the retained native engine also handles its PSB/KBAD paths. Mount order follows native boot/mount rules. Sorting archive filenames is not a substitute for those rules. A `TJS2100` patch script is bytecode, not an XP3 archive or plain text.

## Native behavior and limits

The C++ core exports the common ABI directly and retains native VM, rendering, fonts, menus and save behavior. Cargo's query binding and build script drive the real native shared target. WindowEx overlay ownership and reentrant cleanup remain native lifecycle responsibilities; no child-process/IPC game host is used.

The full bundle needs actual native dependencies and any required resources, not just a Rust binding library. FFmpeg 3.3.9 is dynamically linked, with its corresponding-source/relinking obligations. Third-party native plugins and edition-specific archive filters are not made compatible merely by loading XP3.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
