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

Musica resolves save identity from original files, not executable or title names. It examines the game root, `save/`, `savedata/` and existing `.shinki/musica/minori/*/` folders. A decodable `system.dat` establishes the RC4 or Blowfish envelope; a valid numbered `.sav` supplies its actual directory, filename prefix/digit width, NUL-terminated container signature, prefix and basic/extended checkpoint fields. Both naming namespaces must exist to infer an append namespace. A signature or SerialCode alone cannot establish the other format parameters.

| File | Contents |
| --- | --- |
| `system.dat` | Global flags and clear state |
| `sysparams.txt` | Settings, quick save and auto save cursors (tab-separated, CRLF) |
| `<script>.tbr` | Read-message bitmap of one script |
| `cgv.dat` | Viewed CG list |
| `<prefix><number>.sav` | Message checkpoint and history, using the parsed/configured naming template |
| Matching `.png` | Opaque RGB box-average thumbnail: basic layout 128×80, extended layout 106×60 |

When source files do not establish the format, a release-bound Metadata preset (`Metadata/engines/presets/<rid>/<plugin_id>.json`, `shinki.emu.presets.v1`) supplies `configuration.native_save` as a JSON **string**. The decoded descriptor has this shape:

```json
{
  "version": 1,
  "directory": "save",
  "slot_prefix": "slot_",
  "append_slot_prefix": null,
  "slot_digits": 4,
  "signature": "engine-release 1.00",
  "system_format": "Rc4",
  "extended": false,
  "serial_key": null,
  "prefix": 0,
  "auto_save_cycle": 200,
  "voice_keys": null
}
```

`directory` is game-relative or an explicit absolute original save directory (including Documents). There is no automatic Documents lookup. `system_format` is `Rc4` or `Blowfish`; `extended` selects the recovered extended checkpoint layout and quick/auto slot banks, independently of title names. `append_slot_prefix` supplies a namespace absent from the source files. `voice_keys`, when supplied, maps the existing five frontend voice groups to native settings keys; frontend resource specialization is unchanged in this slice. Unsupported descriptor versions fail explicitly.

The original files take precedence for properties they establish; the descriptor fills registry, append, settings-key and command-cycle information unavailable from them. The resolved descriptor is retained as `.shinki/musica/save-identity.json`, so later starts retain the same configuration even before any slots exist. No existing configuration field was removed. Old portable minori folders are retained in place; if their files cannot establish a complete identity, startup reports `SHINKI_EMU_MUSICA_SAVE_CONFIG` and asks for `native_save`, rather than silently hiding the old progress.

On Windows, `serial_key` names the HKLM registry key whose ANSI `SerialCode` bytes (up to the first NUL) are CRC32-hashed. Without that value, the explicit `prefix`, a validated existing compressed-file prefix, or finally a non-strict zero is used. The four-byte prefix cannot reveal the registry key or the save directory. For example, an eden-style descriptor uses the original `eden` / `edenPM` names and `eden_en 1.00` signature with RC4 and basic slots; a Perseus-style descriptor supplies `perseus_`, `perseus 1.00`, Blowfish, extended slots and its registry key. These are release configuration, not core lookup entries.

For a supported original layout, globals, settings, CGs, read flags and slots keep the native byte format. A configured original directory is written directly; old game-local `.shinki/musica/minori/<folder>/` directories remain at their existing locations. You can copy compatible files between that folder and the original installation. `system.dat`, `sysparams.txt` and prefixes retain their original representation. Zlib streams may differ byte-for-byte while decoding to the same contents. Unmapped settings bytes survive rewrites. Frontend-only settings remain in `.shinki/musica/musica-settings.txt`.

**Without a recoverable native identity or descriptor**, Musica still creates a working backend and persists all state under `<game>/.shinki/musica/generic-v1/`. This path is isolated by the game directory. Its `slot_NNNN.sav` files use the distinct `shinki.musica.save.v1` signature and the recovered basic checkpoint body; auxiliary files use RC4 system properties, tab-separated settings and zero-prefixed zlib CG/read tables. This is a ShinkiEMU format, not a claim that an unknown original executable can read it. A familiar EXE filename does not change this choice. Switching the descriptor selects another backend without deleting or merging the previous files; back up and retain them.

Earlier JSON settings, quick-save cursors and global progress in `.shinki/musica/saves/` have a one-time, no-clobber import into **either** backend. Only after a successful write is each source renamed to `.imported`. Existing destination files win. Old beta `.asav` files (`AMUSSV06` and earlier) remain incompatible and untouched in both legacy locations; back them up and start a new slot.

Checkpoints restore script variables, background, composite foreground, stand positions/offsets, BGM, message history and novel-mode (`CP1=2`) page lines with `L4_n` positions. Loading restarts the recorded message and voice; playback clocks, translations and menu focus are not stored. Unsupported effect/secondary-layer state is rejected, with no snapshot fallback.

Basic slots use auto slot 0 and quick slots 1–9; `AppendPlaying` selects the configured append namespace. Extended slots use quick slots 0–9, auto slots 10–19 and manual slots 20 onward. These preserve eden/Perseus rotation when given their corresponding source files/descriptors. Autosaves run at the configured command cycle, before selections of up to four choices and before movies; `exitFromEvent`, `prohibitAutoSave` and `prohibitSaveLoad` retain their native prohibitions. Thumbnails precede slot writes, each by atomic replacement.

[ReMinori](reminori.md) is a separate implementation with different execution, configuration and saves.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
