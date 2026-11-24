---
title: Shinki ADV32
description: Development status and local execution of the CIRCUS ADV32 core.
---

# Shinki ADV32

`shinki.emu.adv32` opens a CIRCUS ADV32 game's original `AdvData` directory. It reads MES instructions, CRX graphics, CRM/PCK archives and native audio, and publishes CPU frames through C ABI v3. The desktop core is a standalone dynamic library.

The core runs story scripts, choices, menus and movies. It reads and writes original saves in the ADV32 dialects it implements. Keep the original executable in your game directory: the core reads save defaults, slot filename prefixes, music lists and supported gallery data from it. An unsupported instruction reports the script name and byte offset. No original game files are bundled.

CPU compositing is the default. On Windows, Linux, macOS, Android and iOS, `hardware_compositing` selects GPU composition for scene layers and text, with at most one frame in flight during continuous animation. C ABI v3 still requires CPU readback. Video decoding and per-pixel masks run on the CPU. OpenHarmony uses CPU composition and OHAudio.

The core uses the executable's requested typeface when it is installed. Set `font_file` to select a font from the game directory, such as a font supplied by a translation patch.

When a script requests startup authorization, the Windows core checks mounted optical media for the volume name in the original executable. Diskless registry authorization is unsupported. To run installed files without the startup check, explicitly set `skip_disc_check` (off by default). For headless runs, place `[{"id":"skip_disc_check","value":{"Bool":true}}]` in the file passed to `--configuration`.

Local Manager builds can opt in with `--features static-adv32` on desktop, Android, iOS and OpenHarmony. The integration targets are not build or playability results. Published Manager builds do not embed ADV32, and standalone publication remains blocked pending platform and playtest acceptance.

Run the desktop library with the standalone headless host.

```sh
cargo build --locked -p shinki-emu-core-shinki-emu-adv32
cargo build --locked -p shinki-emu-headless --no-default-features
./target/debug/shinki-emu-headless run --audio-device nullaudiodevice --core "$core_library" --game-dir "$game_directory" --output "$temporary_output"
```

For save tests, `--copy-out AdvData/SAVEDATA` exports the runtime copy after a successful close. The exported files go under `game-copy-out` in the output directory.
