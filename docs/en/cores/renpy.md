---
title: Ren’Py
description: Formats, native behavior and limits of Ren’Py.
---

# Ren’Py

`plugin_id: renpy` · Engine family: Ren’Py

Upstream: [renpy/renpy](https://github.com/renpy/renpy). Adaptation fork: [Project-Shinkirou/renpy, shinki branch](https://github.com/Project-Shinkirou/renpy/tree/shinki).

## Formats and execution

This core embeds the official Ren’Py runtime rather than translating its scripts to a new VM. Script execution, Screen Language, ATL, GL2 composition, media and saves keep their official behavior. The integration pins the official SDK, CPython 3.12.7, Cython 3.2.4 and SDL 2.0.20; DLL checksums and sources are recorded in `shinki/sdk.json`.

## Storage

The adapter preserves Ren’Py 8.5.3 save-location behavior. With a
`config.save_directory`, the runtime synchronizes the game-local
`<game>/game/saves` directory with `%APPDATA%/RenPy/<config.save_directory>`;
when it is unset, saves use `<game>/game/saves`. Save tokens and backups follow
the same upstream resolver. No Shinki-specific data directory is required.

Beta.3 imports the former default `%LOCALAPPDATA%/ShinkiEMU/renpy/<game-hash>/saves`
once, before save tokens and persistent data load. Missing save files are copied
byte-for-byte into the native save directories; an existing file in either native
directory wins. Tokens and backups go through the upstream resolvers. The old
files remain, and a completion marker prevents deleted saves from being imported
again. A failed import leaves no completion marker and reports a warning; the
runtime still uses its normal locations, with no writable-directory fallback.

If you chose a custom `data_directory` in beta.1/beta.2, close the game and copy
missing files from `<old-directory>/<game-hash>/saves` into `<game>/game/saves`.
Copy its `tokens` and `backups` subdirectories into the corresponding upstream
Ren’Py directories, not into the game save directory. Do not replace existing
files. Existing native security keys are retained; Ren’Py may ask you to trust
an imported save signed by a different key. The Manager shows the retired path
once so you can locate it. Logs and generated caches use upstream locations;
Python interpreter bytecode writes remain disabled.

Storage comparison base: upstream `39895c1e017f0b36ffea2447d97eccd69d76ee1c`
(the fork's merge base). `renpy.py`, `savelocation.py`, `loader.py`, `script.py`
and translation output paths are unchanged. The five added lines in `main.py`
only invoke the one-time import before the native token/persistent loaders.

## Native behavior and limits

Windows builds generate MSVC import libraries from the official SDK DLLs in the
Rust build helper, then link those SDK libraries into both the display extension
and the core. The package includes the SDK-generated `renpy/vc_version.py`.

The runtime bridge remains part of the embedded interpreter. Native build output includes the display extension and real shared core, while the package also needs matching SDK DLLs and `Resources/`. Session close and cross-game reopening retain interpreter/native cleanup responsibilities; copying only the core DLL is insufficient.

The current embedding requires Windows x64/MSVC. Upstream Ren’Py platform support does not imply the adapter is ported to those platforms. Official standard archive readers remain unchanged; edition-specific offline transformations belong in reviewed recipes. No Steam, Cubism, Bink, nvdrs or redistributable D3DCompiler DLL is bundled.

The runtime includes multiple licenses, including LGPL dependencies with corresponding-source and relinking obligations. Preserve official notices rather than labeling the whole runtime MIT.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
