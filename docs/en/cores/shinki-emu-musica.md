---
title: Musica
description: Formats, native behavior and limits of Musica.
---

# Musica

`plugin_id: shinki.emu.musica` · Engine family: Musica

This is an in-tree ShinkiEMU core, with no separate external fork. See [Source/Families/shinki-emu-musica](https://github.com/Project-Shinkirou/ShinkiEMU/tree/main/Source/Families/shinki-emu-musica) and [ORIGINS](../../../ORIGINS.md) for source revisions.

## Formats

This in-tree core reads PAZ archives and native SC scripts, including split archives, archive-role keys, transforms and bounded decompression. The private `key.toml` uses `shinki.emu.musica.keys.v2`; algorithm parameters and keys are explicit. Probe recognition does not install keys. Standard loose SC and configured PNG replacements remain confined to the game root.

## Native behavior and limits

A core owner runs the SC VM, hardware offscreen composition and native audio. Title launch precedes story execution by default. Input and frame polling never advance the VM. The current presentation includes a 1280×720 layout and Noto Sans JP binding; host resizing does not rewrite game coordinates. Movie ASS subtitles use actual movie clocks and explicit fonts; BGM subtitle mappings are rejected on this path.

Unknown commands, unsupported effects and device failures remain explicit. Audio currently decodes whole bounded resources rather than arbitrary-length streaming.

## Saves

When the game directory holds `eden_en.exe` or `perseus.exe`, Musica reads and writes the original minori files byte-compatibly under `<game>/.shinki/musica/minori/<title>/`. The original keeps the same files in `Documents\minori\<title>\`; Musica never writes to Documents. The title folder is `eden (English edition)` for eden and `夏空のペルセウス` for Perseus. The original Perseus passes that name to ANSI APIs, so on a Windows system whose code page is not CP932 its Documents folder appears under a garbled name (for example `壞嬻偺儁儖僙僂僗` with GBK).

| File | Contents |
| --- | --- |
| `system.dat` | Global flags and clear state |
| `sysparams.txt` | Settings, quick save and auto save cursors (tab-separated, CRLF) |
| `<script>.tbr` | Read-message bitmap of one script |
| `cgv.dat` | Viewed CG list |

To use an original installation's progress or settings, copy the files from `Documents\minori\<title>\` into `.shinki/musica/minori/<title>/`; copy them back the same way to continue in the original executable. `sysparams.txt` keys Musica does not use, such as the font name or window size, are kept as they are. Settings the original has no key for are stored in `.shinki/musica/musica-settings.txt` in the same layout; for eden this holds the five per-character voice toggles.

Compressed files start with a prefix derived from the Perseus `SerialCode`. On Windows, Musica reads the CRC32 of `HKLM\Software\minori\perseus` `SerialCode`. Without that value it reuses the prefix of an existing original file in the folder, otherwise it writes 0. eden always uses 0.

Known difference: `system.dat`, `sysparams.txt` and the file prefixes are byte-identical to the original. For the zlib-compressed `.tbr` and `cgv.dat`, the decompressed contents are identical but the compressed bytes usually differ, because Musica's deflate encoder makes different choices than the original's zlib. The original executable reads these files normally. Musica rewrites a file only when its contents change, so copied files stay byte-for-byte as they were until a new read flag or CG is recorded.

Settings, quick save cursor and global progress that earlier Musica builds stored as JSON in `.shinki/musica/saves/` are imported once into the native files. An existing native file is never overwritten, and an imported JSON file is renamed to `*.imported`. A foreign or damaged JSON file stops the import without changing any file. With an unrecognised executable Musica writes no native files: it still reads the old JSON files and `.asav` slots, logs `SHINKI_EMU_MUSICA_SAVE_TITLE` once, and keeps setting and progress changes only for the session.

Product-owned slots live under `.shinki/musica/saves/` and bind archive/profile/localization/font identities. At first use, existing `.shinki-musica/saves/` files move there without overwriting newer files; subsequent reads only use the new path. Invalid candidates leave live state and existing files intact. Original eden save interchange is an explicit strict subset of known message checkpoints, not general native-save compatibility.

[ReMinori](reminori.md) is a separate implementation with different execution, configuration and saves.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
