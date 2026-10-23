---
title: Ren’Py
description: Formats, native behavior and limits of Ren’Py.
---

# Ren’Py

`plugin_id: renpy` · Engine family: Ren’Py

Upstream: [renpy/renpy](https://github.com/renpy/renpy). Adaptation fork: [Project-Shinkirou/renpy, shinki branch](https://github.com/Project-Shinkirou/renpy/tree/shinki).

## Formats and execution

This core embeds the official Ren’Py runtime rather than translating its scripts to a new VM. Script execution, Screen Language, ATL, GL2 composition, media and saves keep their official behavior. The integration pins the official SDK, CPython 3.12.7, Cython 3.2.4 and SDL 2.0.20; DLL checksums and sources are recorded in `shinki/sdk.json`.

## Native behavior and limits

The runtime bridge remains part of the embedded interpreter. Native build output includes the display extension and real shared core, while the package also needs matching SDK DLLs and `Resources/`. Session close and cross-game reopening retain interpreter/native cleanup responsibilities; copying only the core DLL is insufficient.

The current embedding requires Windows x64/MSVC. Upstream Ren’Py platform support does not imply the adapter is ported to those platforms. Official standard archive readers remain unchanged; edition-specific offline transformations belong in reviewed recipes. No Steam, Cubism, Bink, nvdrs or redistributable D3DCompiler DLL is bundled.

The runtime includes multiple licenses, including LGPL dependencies with corresponding-source and relinking obligations. Preserve official notices rather than labeling the whole runtime MIT.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
