---
title: Shinki ADV32
description: Development status and local execution of the CIRCUS ADV32 core.
---

# Shinki ADV32

`shinki.emu.adv32` is a desktop development preview for the CIRCUS ADV32 engine. It opens a game's original `AdvData` directory, reads MES instructions, CRX images, CRM/PCK archives and native audio, and exposes CPU frames through C ABI v3. It is not available as a public download or as a mobile Manager core.

The preview implements parts of story playback, choices, menus, movies and native saves. Full VM coverage and interoperability with original saves are release gates. Supply your own lawful game installation, including the original executable: the core reads save defaults, slot filename prefixes, music lists and supported gallery data from it. No original game files are bundled. An unsupported instruction reports the script name and byte offset.

CPU compositing is the default. Set the core's `hardware_compositing` option to use the GPU for scene layers and text, with one frame in flight during continuous animation. C ABI v3 still requires CPU readback; video decoding and per-pixel masks stay on the CPU. No measured speedup is claimed. The separate shared-renderer work to combine GPU submission and readback is not part of this preview.

Run the desktop library with the standalone headless host.

```sh
cargo build --locked -p shinki-emu-core-shinki-emu-adv32
cargo build --locked -p shinki-emu-headless --no-default-features
./target/debug/shinki-emu-headless run --audio-device nullaudiodevice --core "$core_library" --game-dir "$game_directory" --output "$temporary_output"
```

For save tests, `--copy-out AdvData/SAVEDATA` exports the runtime copy after a successful close. The exported files go under `game-copy-out` in the output directory.
