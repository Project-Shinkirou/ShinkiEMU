---
title: Artemis / Art3m1s
description: Formats, native behavior and limits of Artemis / Art3m1s.
---

# Artemis / Art3m1s

`plugin_id: art3m1s-core` · Engine family: Artemis

Upstream: [Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core). Adaptation fork: [Project-Shinkirou/art3m1s-core, shinki-android-isolation branch](https://github.com/Project-Shinkirou/art3m1s-core/tree/shinki-android-isolation).

## Formats

Artemis loads `system.ini`, PFS archives (PF6/PF8) and native scenario/tag/Lua resources. Standard numeric patch volumes `.pfs.000` through `.pfs.999` overlay the base package; higher active numbers override lower ones. Backup suffixes are not automatically mounted as patches. Preserve native loose-file and archive precedence.

## Native behavior and limits

The core uses Luau on every platform, with an Artemis compatibility layer for legacy Lua APIs, alongside the native graphics path and FFmpeg. The standalone Android core binds its application context on explicit activation. The adapter owns scheduling and device audio, with bounded immutable CPU frame slots. Native modal dialogs use typed host services; exit ends the session rather than terminating Manager.

`extra_archive` explicitly selects an additional relative PFS after the normal active numeric chain. It does not scan backups or rewrite the base package. The descriptor does not offer adjustable render extent. Missing script targets/resources and unsupported native UI/media commands remain errors; a clickable title screen alone is not a complete-game result.

## Save paths

The adapter writes under `<game>/savedata/`. `SAVEPATH` in `system.ini` adds a relative subdirectory. It defaults to `save` if absent or empty after normalization. Slots such as `save0001.dat`, along with `saveg.dat` and `system.dat`, go under `<game>/savedata/save/` by default or `<game>/savedata/savedata/` when `SAVEPATH=savedata`. Absolute or CSIDL-style values are normalized to a logical prefix, not used as absolute destinations. Existing save paths are unchanged.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
