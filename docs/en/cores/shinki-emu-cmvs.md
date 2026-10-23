---
title: CMVS
description: Formats, native behavior and limits of CMVS.
---

# CMVS

`plugin_id: shinki.emu.cmvs` · Engine family: CMVS

This is an in-tree ShinkiEMU core, with no separate external fork. See [Source/Families/shinki-emu-cmvs](https://github.com/Project-Shinkirou/ShinkiEMU/tree/main/Source/Families/shinki-emu-cmvs) and [ORIGINS](../../../ORIGINS.md) for source revisions.

## Formats

The in-tree core reads CPZ5 archives and executes PS2A through its native scheduler. `cmvs.profile.json` uses `shinki.emu.cmvs.profile.v2`; ordered archive roles and role-specific loose files are explicit. The default entry is `start.ps3`, with Script slot 0 initialized from `cmvs.cfg`. Script, texture and native search domains remain distinct; a selected damaged loose resource is not silently replaced by an archive match.

## Native behavior and limits

The native owner controls VM, Scene and media. It preserves PS2A return-code stack behavior and native commands 693/694 for saving/loading. Menu entry suspends the story context; restoration keeps slot objects, texture relationships and placements rather than adopting the menu's temporary scheduler. Corrupt save candidates cannot partly replace live state or overwrite original files.

Core-owned audio mixes BGM/crossfades/effects and converts to the real output device. MGV keeps its slot-2 audio clock and existing Theora rules. Cancellation wakes queues, and close joins VM, audio and decoders; CPU leases remain returnable afterward.

Only one CMVS session may be open at a time. The current reader rejects unsupported CPZ versions, unsafe paths, unknown commands and unrecovered string domains. Font lookup requires the exact native font rather than a generic substitute. Documents/AppData save-root behavior is not reconstructed.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
