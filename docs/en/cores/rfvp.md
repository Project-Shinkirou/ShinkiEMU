---
title: RFVP
description: Formats, native behavior and limits of RFVP.
---

# RFVP

`plugin_id: rfvp` · Engine family: FVP

Upstream: [xmoezzz/rfvp](https://github.com/xmoezzz/rfvp). Adaptation fork: [Project-Shinkirou/rfvp, shinki-android-isolation branch](https://github.com/Project-Shinkirou/rfvp/tree/shinki-android-isolation).

## Formats

FVP uses HCB bytecode. The first little-endian u32 is the system-descriptor offset; code begins at byte 4 rather than after a magic string. The descriptor supplies the entry point, globals and syscall table. Length-prefixed C strings include their terminating NUL; encoding is an explicit core setting.

## Native behavior and limits

The fork retains its App, VM, GPU renderer, device audio and saves. The adapter delivers the final scene through immutable CPU leases; Manager does not reproduce syscalls or drive a host tick. Native configuration can select UTF-8 translation tables indexed by original HCB offsets and movie/BGM ASS mappings with explicit fonts and original media clocks.

The Android static Manager graph shares wgpu 30 with Siglus and Manager. Native WMV decoding uses the Rust API; its optional `c-abi` exports stay disabled in Manager.

WMV/ASF/WMA playback uses the fork's native movie/decoder path. Requested native fonts and game-specific resources remain real inputs. Recognizing an HCB file does not establish syscall coverage or edition compatibility.

## Save paths and format

The original RFVP engine stores numbered saves in `<game>/save/sNNN.bin` and global HCB data in `<game>/save/save.bin` (`crates/rfvp/src/script/global.rs:134-136` and the `setsumei/` reverse-engineering notes). The fork's `SaveItem` appends its own runtime-state block after the original header, and its global file wraps `GlobalSaveDataV1` in an `RFVG` envelope, so neither file is a byte-level implementation of the original format. The fork keeps its own names, `<game>/save/rfvp_sNNN.bin` and `<game>/save/rfvp_global.bin`, and still reads older `<game>/save/sNNN.bin` files. These saves belong to the game directory and are not moved into `.shinki`.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
