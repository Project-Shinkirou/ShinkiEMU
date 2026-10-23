---
title: Artemis / Art3m1s
description: Formats, native behavior and limits of Artemis / Art3m1s.
---

# Artemis / Art3m1s

`plugin_id: art3m1s-core` · Engine family: Artemis

Upstream: [Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core). Adaptation fork: [Project-Shinkirou/art3m1s-core, shinki branch](https://github.com/Project-Shinkirou/art3m1s-core/tree/shinki).

## Formats

Artemis loads `system.ini`, PFS archives (PF6/PF8) and native scenario/tag/Lua resources. Standard numeric patch volumes `.pfs.000` through `.pfs.999` overlay the base package; higher active numbers override lower ones. Backup suffixes are not automatically mounted as patches. Preserve native loose-file and archive precedence.

## Native behavior and limits

The core keeps Lua 5.1, the native graphics path and FFmpeg. This Lua graph is separate from Manager's Luau recipes. The adapter owns scheduling and device audio, with bounded immutable CPU frame slots. Native modal dialogs use typed host services; exit ends the session rather than terminating Manager.

`extra_archive` explicitly selects an additional relative PFS after the normal active numeric chain. It does not scan backups or rewrite the base package. The descriptor does not offer adjustable render extent. Missing script targets/resources and unsupported native UI/media commands remain errors; a clickable title screen alone is not a complete-game result.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
