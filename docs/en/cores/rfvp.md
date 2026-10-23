---
title: RFVP
description: Formats, native behavior and limits of RFVP.
---

# RFVP

`plugin_id: rfvp` · Engine family: FVP

Upstream: [xmoezzz/rfvp](https://github.com/xmoezzz/rfvp). Adaptation fork: [Project-Shinkirou/rfvp, shinki branch](https://github.com/Project-Shinkirou/rfvp/tree/shinki).

## Formats

FVP uses HCB bytecode. The first little-endian u32 is the system-descriptor offset; code begins at byte 4 rather than after a magic string. The descriptor supplies the entry point, globals and syscall table. Length-prefixed C strings include their terminating NUL; encoding is an explicit core setting.

## Native behavior and limits

The fork retains its App, VM, GPU renderer, device audio and saves. The adapter delivers the final scene through immutable CPU leases; Manager does not reproduce syscalls or drive a host tick. Native configuration can select UTF-8 translation tables indexed by original HCB offsets and movie/BGM ASS mappings with explicit fonts and original media clocks.

WMV/ASF/WMA playback uses the fork's native movie/decoder path. Requested native fonts and game-specific resources remain real inputs. Recognizing an HCB file does not establish syscall coverage or edition compatibility.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
