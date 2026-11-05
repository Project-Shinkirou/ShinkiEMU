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

A core owner runs the SC VM, hardware offscreen composition and native audio. The default `launch_mode=direct` executes the configured original entry script; `title` requires release frontend data. Input and frame polling never advance the VM. Logical canvas dimensions are configured independently of host resizing. The native settings and save-card pages retain their recovered layout; the title hit regions and gallery list do not require a 1280-wide canvas. Movie ASS subtitles use actual movie clocks and explicit fonts; BGM subtitle mappings are rejected on this path.

Unknown commands, unsupported effects and device failures remain explicit. Audio currently decodes whole bounded resources rather than arbitrary-length streaming.

## Frontend data and archive discovery

Data comes from original resources first, then release-bound Metadata configuration, then Luau resource correction for nonstandard files. No game-name dispatch or built-in clear flags, replay scripts, movie filenames or character roster is used.

| Data | Original source and implemented coverage | Preset when unavailable |
| --- | --- | --- |
| Title/menu actions, rectangles, variants and unlocks | A script-owned menu runs as the original SC entry. The recovered PAZ directory contains image names, not action/rectangle/flag semantics; native executable menu tables are not decoded. Image-name patterns are not interpreted as menus. | `frontend.titles`: actual background/hover URIs, buttons and global-variable requirements |
| Recollections | An explicitly identified original SC index, `frontend.replay_index`, contributes its `.chain file.sc` entries in source order. Arbitrary story filenames are not classified as replays. Index targets with `#label` are not supported by this frontend reader. | `frontend.replays`: ordered labels, script URIs and requirements |
| Movies | Original SC directory scan finds standalone `.movie` wrappers containing only movie/wait/end commands; the referenced movie must exist. Alternatively `movie_index` reads original `.chain file.sc` entries. Normal story scripts containing movies are excluded. | `frontend.movies`: script entries, including releases whose movies require additional setup |
| Music / CG | Actual `bgm` archive `.ogg` entries form the music list. General image directories do not distinguish CGs from backgrounds. | `frontend.bgm` when no original tracks are found; `frontend.cg` for the CG index |
| Characters | Actual `voice` archive `.ogg` basenames: segment before `-`/`_`, or basename with trailing digits removed. No role names are assigned by the core. | `voice_characters`: JSON array of `{prefix,label,enabled}`; configured order/labels/initial flags supplement discovery |
| Archive roles | `key.toml` declares the actual `archive_keys` map; PAZ 0/1/2 structure, decryption and index/entry bounds are validated by the same mount path in probe and open. | `archive_keys.<role>.required`, default **true**, false for optional archives; an absent optional file is skipped, a present malformed file still fails |

Automatic probe reads the game's existing default `key.toml` and verifies a parseable SC command in the mounted script archive. Encrypted PAZ has no universal magic, so archive names alone are insufficient. No default key means no automatic match; an explicitly selected external `profile_file` still opens through the same mount validation. Probe does not install keys. Legacy CLI imports default every declared role to `required=true`; `required=false` in keys.v1/profile.v1 input is preserved. GARbro has no optional-role flag: edit the resulting private key.toml explicitly. Missing required archives report the role and expected path.

The descriptor declares `frontend` (JSON string, default empty), `voice_characters` (JSON string, default `[]`) and the existing canvas/startup fields, with Chinese presentation labels. Put release values in `Metadata/engines/presets/<rid>/shinki.emu.musica.json` using `shinki.emu.presets.v1`; its `configuration.frontend` is the string encoding of this example:

```json
{
  "titles": [{
    "background": "musica:/sys/welcome.png",
    "hover": "musica:/sys/highlight.png",
    "requires": {},
    "buttons": [
      {"rect": [10, 20, 120, 40], "action": "new_game"},
      {"rect": [10, 80, 120, 40], "action": "memories"},
      {"rect": [10, 140, 120, 40], "action": "exit"}
    ]
  }],
  "replay_index": "musica:/scr/memory-index.sc",
  "replays": [],
  "movies": [{"label": "Credits", "resource": "musica:/scr/credits.sc", "requires": {"finished": 1}}],
  "cg": [{"label": "Memory", "resource": "musica:/bg/memory.png"}]
}
```

Titles are ordered: the last variant whose `requires` map matches wins; absent globals are zero. Buttons use logical `[x,y,width,height]` rectangles and actions `new_game`, `load`, `config`, `memories`, `exit`. Gallery entries use `label`, `resource` and optional `requires`. Empty categories are omitted. Arrow keys select; Enter/Space activates; Escape returns; Backspace stops gallery music. Gallery text and pointer rows share canvas-relative pagination (12 entries per page). Music and CG resources are listed only through the documented sources; there is no hardcoded track-number table or CG page count.

Each JSON field is bounded by the ABI's 4096-byte configuration limit. Large catalogs should use original SC indexes. Automatic movie scanning is bounded to 4096 scripts and 64 MiB of expanded source; supply `movie_index` for larger directories. Where discovery succeeds, native membership/order wins while preset labels and unlock conditions for matching resource URIs are retained. The reader does not reconstruct conditional SC control flow into unlock formulas; describe those rules in the preset.

**Migration:** remove retired Manager fields `voice_ren`, `voice_sui`, `voice_aya`, `voice_tou`, `voice_mot` from stored configuration and express them in `voice_characters` with the release's actual prefixes. There are no aliases. Existing explicit `launch_mode=title` requires `frontend`; select `direct` for a native script-owned menu, or supply the release menu description. Existing eden/Perseus installations need their actual menu art, clear rules, replay/movie lists and character order in a release preset, not core exceptions. Synthetic fixtures cover both title-image casing layouts and a different 640×360 release with missing optional archives; these do not establish commercial-release compatibility. The settings codec's five native voice keys remain format parameters: declared characters map in configured order, additional characters use `voice_<prefix>` in frontend-only settings. Regression tests compare encoded native settings byte-for-byte with the former mapping.

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

`directory` is game-relative or an explicit absolute original save directory (including Documents). There is no automatic Documents lookup. `system_format` is `Rc4` or `Blowfish`; `extended` selects the recovered extended checkpoint layout and quick/auto slot banks, independently of title names. `append_slot_prefix` supplies a namespace absent from the source files. `voice_keys`, when supplied, maps the first five declared characters in configuration order to native settings keys; additional characters use frontend-only keys. Unsupported descriptor versions fail explicitly.

The original files take precedence for properties they establish; the descriptor fills registry, append, settings-key and command-cycle information unavailable from them. The resolved descriptor is retained as `.shinki/musica/save-identity.json`, so later starts retain the same configuration even before any slots exist. The frontend field migration is described above. Old portable minori folders are retained in place; if their files cannot establish a complete identity, startup reports `SHINKI_EMU_MUSICA_SAVE_CONFIG` and asks for `native_save`, rather than silently hiding the old progress.

On Windows, `serial_key` names the HKLM registry key whose ANSI `SerialCode` bytes (up to the first NUL) are CRC32-hashed. Without that value, the explicit `prefix`, a validated existing compressed-file prefix, or finally a non-strict zero is used. The four-byte prefix cannot reveal the registry key or the save directory. For example, an eden-style descriptor uses the original `eden` / `edenPM` names and `eden_en 1.00` signature with RC4 and basic slots; a Perseus-style descriptor supplies `perseus_`, `perseus 1.00`, Blowfish, extended slots and its registry key. These are release configuration, not core lookup entries.

For a supported original layout, globals, settings, CGs, read flags and slots keep the native byte format. A configured original directory is written directly; old game-local `.shinki/musica/minori/<folder>/` directories remain at their existing locations. You can copy compatible files between that folder and the original installation. `system.dat`, `sysparams.txt` and prefixes retain their original representation. Zlib streams may differ byte-for-byte while decoding to the same contents. Unmapped settings bytes survive rewrites. Frontend-only settings remain in `.shinki/musica/musica-settings.txt`.

**Without a recoverable native identity or descriptor**, Musica still creates a working backend and persists all state under `<game>/.shinki/musica/generic-v1/`. This path is isolated by the game directory. Its `slot_NNNN.sav` files use the distinct `shinki.musica.save.v1` signature and the recovered basic checkpoint body; auxiliary files use RC4 system properties, tab-separated settings and zero-prefixed zlib CG/read tables. This is a ShinkiEMU format, not a claim that an unknown original executable can read it. A familiar EXE filename does not change this choice. Switching the descriptor selects another backend without deleting or merging the previous files; back up and retain them.

Earlier JSON settings, quick-save cursors and global progress in `.shinki/musica/saves/` have a one-time, no-clobber import into **either** backend. Only after a successful write is each source renamed to `.imported`. Existing destination files win. Old beta `.asav` files (`AMUSSV06` and earlier) remain incompatible and untouched in both legacy locations; back them up and start a new slot.

Legacy progress hashes are resolved in this order: original entry-script `.setglobal` names, `frontend` requirement keys, then the preset's `legacy_progress_flags` JSON string array (default `[]`). No variable-name whitelist is embedded. Unresolved hashes do not prevent opening: mapped records migrate without replacing existing globals, the original JSON remains untouched, and `.shinki/musica/saves/global-progress-unmapped.json` lists the missing hashes and game directory. Complete the declarations and start again; the pending import resumes, preserves intervening global changes, and renames the original to `.imported` only when all records are resolved. A pre-existing native destination with no pending diagnostic still takes precedence. The runtime records actual `.setglobal` assignments whose value is one, regardless of variable spelling; menu conditions continue to evaluate their configured integer values.

Old `configuration.json` files containing `character_voice_enabled` need explicit conversion to `character_voices` objects using the release's declared character order. Retain the original as a backup before conversion; the strict importer does not guess a five-character roster.

Checkpoints restore script variables, background, composite foreground, stand positions/offsets, BGM, message history and novel-mode (`CP1=2`) page lines with `L4_n` positions. Loading restarts the recorded message and voice; playback clocks, translations and menu focus are not stored. Unsupported effect/secondary-layer state is rejected, with no snapshot fallback.

Basic slots use auto slot 0 and quick slots 1–9; `AppendPlaying` selects the configured append namespace. Extended slots use quick slots 0–9, auto slots 10–19 and manual slots 20 onward. These preserve eden/Perseus rotation when given their corresponding source files/descriptors. Autosaves run at the configured command cycle, before selections of up to four choices and before movies; `exitFromEvent`, `prohibitAutoSave` and `prohibitSaveLoad` retain their native prohibitions. Thumbnails precede slot writes, each by atomic replacement.

[ReMinori](reminori.md) is a separate implementation with different execution, configuration and saves.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
