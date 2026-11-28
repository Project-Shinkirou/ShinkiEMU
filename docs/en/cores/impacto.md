---
title: impacto
description: Formats, native behavior and limits of impacto.
---

# impacto

`plugin_id: impacto` · Engine family: MAGES.

Upstream: [CommitteeOfZero/impacto](https://github.com/CommitteeOfZero/impacto). Adaptation fork: [Project-Shinkirou/impacto, shinki branch](https://github.com/Project-Shinkirou/impacto/tree/shinki).

## Formats and execution

impacto retains its MAGES SC3 VM, native resource readers and game-specific configuration. The C++ integration remains under `src/shinki`, with Cargo/CMake build entry under `shinki`. Original OpenGL composition, OpenAL audio, FFmpeg movies, game UI and save behavior remain in the native engine.

### Android core packages

In Manager, download Impacto from the official catalog or install a compatible local ZIP for `arm64-v8a` or `x86_64`. Android accepts release sequence 4 or later. Profiles, shaders, resources, runtime libraries and notices live in the core ZIP. An empty `resources_root` resolves to the `Resources/` directory beside the installed library; an explicit path takes precedence.

The core uses its own SDL offscreen event queue and an EGL pbuffer on the session worker. Manager keeps ownership of the Android activity, input and lifecycle; the core does not require an SDL activity or the Android context extension. The Android library keeps its private Lua and native dependency symbols out of the process-wide symbol namespace.

## Storage

Set `save_root` to a separate writable directory outside the game data and the core’s `Resources/`. The core uses `<save_root>/userconfig.toml` on desktop and Android. If you already used an isolated root, keep pointing at that directory. The core does not move old configuration automatically.

The current Chaos;Child profile sets `SaveDataType.None` and has no native slot save files. A configuration write or relaunch does not verify native save/load. On Linux, also set `resources_root` to the installed core’s `Resources/` directory.

## Native behavior and limits

The adapter exposes C ABI v3 and CPU frame ownership rather than reimplementing MAGES scripts in Manager. Native DDS parallel-read state and movie-audio release are ownership boundaries preserved by the fork. Steam font resources are read from the user's actual game DDS files, not distributed as replacement assets.

The open-source dependency path does not bundle or load Bink DLLs. Game-specific system-data failures and unexercised routes remain separate compatibility concerns; a successful query does not establish that a title's system page, movies or saves work. ISC and component-specific licenses both apply; binary distribution requires a complete native dependency and corresponding-source review.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
