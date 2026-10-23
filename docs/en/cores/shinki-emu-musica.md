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

Product-owned slots are under `.shinki-musica/saves/` and bind archive/profile/localization/font identities. Invalid candidates leave live state and existing files intact. Original eden save interchange is an explicit strict subset of known message checkpoints, not general native-save compatibility. Unknown commands, unsupported effects and device failures remain explicit. Audio currently decodes whole bounded resources rather than arbitrary-length streaming.

[ReMinori](reminori.md) is a separate implementation with different execution, configuration and saves.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
