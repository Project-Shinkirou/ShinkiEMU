---
title: cmvs-rs
description: External CMVS/CVNS adapter, upstream coverage and configuration.
---

# cmvs-rs

`plugin_id: cmvs-rs` · Engine family: CMVS / CVNS

This external core adapts [Shizuku-in/cmvs-rs](https://github.com/Shizuku-in/cmvs-rs), through the [Project-Shinkirou fork](https://github.com/Project-Shinkirou/cmvs-rs/tree/shinki). It coexists with ShinkiEMU's own [`shinki.emu.cmvs`](shinki-emu-cmvs.md); selecting one does not replace or configure the other. Their VM, renderer, compatibility and save policies are independent.

Publication is blocked while real-game frame acceptance lacks an upstream-registered local release. Local HAPYMAHER_RE has a different startup-script hash; HAPYMAHERFD_RE has no upstream registry entry. No registry changes or profile overrides are used to bypass this boundary.

## Coverage

Only upstream-registered CMVS/CVNS releases are supported. The [upstream game registry](https://github.com/Shizuku-in/cmvs-rs/blob/b9fed68ba05f80444338331bb3999f14c8f4f976/crates/cmvs-core/profiles/games.txt) and [status report](https://github.com/Shizuku-in/cmvs-rs/blob/b9fed68ba05f80444338331bb3999f14c8f4f976/docs/status.md) describe the actual formats and per-release limits. A PS2A probe match identifies an engine installation, not a supported or playable release. Upstream rejects unregistered data or executables during open; the adapter keeps this check and adds no title whitelist.

Unregistered releases return ABI `NO_MATCH` (8). Only upstream `SelectionError::UnknownGame` and `UnknownBinary` are translated this way; rejection conditions and messages are unchanged. The initial investigation surfaced `UnknownGame` as status 6 (`game data matches no recorded game`); the adapter now classifies it as a normal non-match. I/O, missing-resource and decode failures remain `CORE_ERROR`. Real-game open, screenshots and frame-hash acceptance are deferred pending a registered sample.

The worker uses upstream's `Startup`, CPU/font scene composition, CMV/MGV movie decoders, native Vorbis audio and snapshot/save implementations. Frame leases own immutable complete engine canvases and remain valid after close. Dynamic builds export C ABI v3 query; static builds provide the same Rust-path query without exporting a conflicting symbol.

## Configuration and saves

- `encoding`: upstream `auto`, `shift-jis`, `gbk` or `utf-8`; ambiguous automatic detection fails explicitly.
- `profile`: optional upstream engine profile override, for research; empty uses upstream detection.
- `font` / `font-face`: an explicitly supplied OpenType/TTC file and face index. Upstream's font renderer requires Shift-JIS; an empty font preserves its text-output barrier. Fonts are not redistributed.
- `user-data-dir`: optional upstream user data directory, outside the game directory. Default is empty. It is passed unchanged to upstream; no managed save root or `.shinki` redirection is introduced. Without it, upstream write/save barriers remain active. Existing saves are not migrated or overwritten by the adapter.
- `block-unreviewed` / `block-presumed`: upstream command gates, both false by default. Allowing a command does not establish compatibility.

## Host ABI limits

C ABI v3 has no custom cursor bitmap, monitor enumeration/selection or aspect/filter policy fields. Cosmetic cursor shape requests retain the Host default cursor and emit one diagnostic. Other unavailable requests return `UNSUPPORTED`, with the missing capability in the error. Host dialogs use the window service and can be cancelled; a headless host never invents an answer. No native game window is created by the core.

The core retains upstream's save/key/config formats and paths. The original MIT/Apache-2.0 licenses, third-party notices and modification statement accompany the binary and corresponding-source bundle. Exact pins and target-level build restrictions are recorded in [`build/cores.toml`](../../../build/cores.toml).
