---
title: Shinki ADV32
description: Development status and local execution of the CIRCUS ADV32 core.
---

# Shinki ADV32

`shinki.emu.adv32` opens a CIRCUS ADV32 game's original `AdvData` directory. It reads MES instructions, CRX graphics, CRM/PCK archives and native audio, and publishes CPU frames through C ABI v3. The desktop core is a standalone dynamic library.

Recognition checks for `AdvData/MES/start.mes` and parses its MES dialect. The parser admits `0x9e97`, `0x9f9a`, `0xa6b9`, `0xaab6` and `0xabb6`; other scripts must match the bootstrap dialect. Recognizing a folder does not establish VM coverage across its routes, menus, movies or authorization paths. An unsupported instruction reports the script name and byte offset. Bring your own game files and keep the original executable: the core can read save defaults, slot prefixes, music lists and supported gallery data from it.

CPU compositing is the default. On Windows, Linux, macOS, Android and iOS, `hardware_compositing` selects GPU composition for scene layers and text, with at most one frame in flight during continuous animation. C ABI v3 still requires CPU readback. Video decoding and per-pixel masks run on the CPU. OpenHarmony uses CPU composition and OHAudio.

The core uses the executable's requested typeface when it is installed. Set `font_file` to select a font from the game directory, such as a font supplied by a translation patch.

When a script requests startup authorization, the Windows core checks mounted optical media for the volume name in the original executable. Diskless registry authorization is unsupported. To run installed files without the startup check, explicitly set `skip_disc_check` (off by default). For headless runs, place `[{"id":"skip_disc_check","value":{"Bool":true}}]` in the file passed to `--configuration`.

Public Manager builds do not embed ADV32. The proposed Windows x64 release installs the standalone core into Manager. Publication stays blocked pending edition-specific VM and native save validation, an exact-package playtest, and [source, licensing and DLL replacement review](../../licenses/shinki-emu-adv32/WINDOWS-FFMPEG.md). Local Manager builds can opt in with `--features static-adv32` on desktop, Android, iOS and OpenHarmony. Those targets are integration candidates; an embedded build is not an acceptance result for a standalone package.

## Save formats

The core uses the game's `AdvData/SAVEDATA` directory. It reads `system.dat` and native `<prefix><slot>.dat` checkpoints; modern formats also write RGB PNG thumbnails. The layout is selected by the MES bootstrap dialect, not by the game-folder name:

| MES dialect | System bytes | Slot bytes | Written thumbnails |
| --- | ---: | ---: | --- |
| `0x9e97` | 17,692 | 55,296 | No |
| `0x9f9a` | 19,736 | 72,440 | No |
| `0xa6b9`, `0xaab6`, `0xabb6` | 21,368 | 356,324 | 128 × 72 RGB PNG |

The system writer preserves unmodeled bytes in an existing file. The slot writer reconstructs the checkpoint from VM state; it does not preserve every original field. A new slot cannot overwrite an unrecognized file at its native filename. Back up saves before moving them. A Shinki-to-Shinki reopen checks the core's own resume path. Native interoperability also requires loading an original save in Shinki and a Shinki save in the original executable for the same edition.

Dal Segno (`0x9f9a`, `DS` namespace) is the Windows acceptance candidate. No edition has yet passed the standalone product gate. A matching dialect or save size does not extend this candidate to other Dal Segno editions, patches or titles.

## Local execution

Run the desktop library with the standalone headless host.

```sh
cargo build --locked -p shinki-emu-core-shinki-emu-adv32
cargo build --locked -p shinki-emu-headless --no-default-features
./target/debug/shinki-emu-headless run --audio-device nullaudiodevice --core "$core_library" --game-dir "$game_directory" --output "$temporary_output"
```

For save tests, `--copy-out AdvData/SAVEDATA` exports the runtime copy after a successful close. The exported files go under `game-copy-out` in the output directory.
