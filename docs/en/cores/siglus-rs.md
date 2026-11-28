---
title: Siglus
description: Formats, native behavior and limits of Siglus.
---

# Siglus

`plugin_id: siglus_rs` · Engine family: SiglusEngine

Upstream: [xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs). `Cargo.lock` pins the exact [ShinkiEMU adaptation](https://github.com/Project-Shinkirou/siglus_rs/tree/575075a678a17d4e21ac81c24adbef124ab2027e).

## Formats

`Scene.pck` groups scene chunks and package-level include properties/commands. Package offsets are relative to the package, while offsets in each decoded `.ss` chunk are relative to that chunk. Indexed string lengths and offsets count UTF-16 code units, not bytes; the stored strings have an index-dependent word transform. Scene, label and command-entry tables are separate inputs to VM control flow.

## Native behavior and limits

The fork retains the Siglus VM, renderer, text/keyboard input and native saves. Its adapter owns execution and publishes CPU frames. Native configuration supports paired original/chs resources and G01/movie selection.

Scene restarts retain the session's image and movie resource language. CHS mode selects native G01 and `mov_chs` replacements without overwriting original G00 files.

Android Manager loads Siglus from a separate core package when a game starts. Directory scanning uses package metadata and does not load the native library. At activation, Manager passes the Android context through the ABI extension; Siglus still supplies CPU frames.

The standalone Android core uses the NDK's `libc++_shared.so` for Oboe audio. Packaging includes the target ABI's library and the NDK notices.

## Saves and keys

The core writes to a managed save directory. The default starts at `%LOCALAPPDATA%` on Windows, `~/Library/Application Support` on macOS, `$XDG_DATA_HOME` or `~/.local/share` on Linux, and the app’s private files directory on Android. The core appends `siglus_rs/saves/<canonical-game-path-hash>/`.

An explicit `save_root` replaces that whole path. It must be absolute and separate from the game and original save directories. Either way, the core adds `siglus-<original|chs>-<resource-pair-identity>/`. Native slots and settings go in its `savedata/`, with captures and exports relative to the same namespace. Original and CHS resources do not automatically share saves.

The core does not import or rename old saves. To try a compatible save from another layout, close the game, retain the original, and copy it into the managed namespace for the same resource pair without overwriting files. Verify a cold load with the copy. Keep recovered keys local.

The core does not apply localized launchers’ machine-code patches. Its CJK fallback can fill missing glyphs but cannot recreate a missing game font. Check resource and runtime compatibility for the exact edition.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
