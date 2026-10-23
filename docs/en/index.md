---
title: ShinkiEMU
description: Manager and native cores for legacy visual novel engines.
---

# ShinkiEMU

ShinkiEMU combines a game-library Manager with native cores for legacy visual novel engines. The Manager discovers games, installs cores, presents their frames and connects metadata. Each core owns its engine's VM, rendering, audio, menus and save behavior. [RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine) is the separate creation engine and supplies shared low-level libraries.

## Names and responsibilities

- A **family** identifies the original engine or format family.
- A **core** is a particular implementation of that engine.
- **plugin_id** is the exact, case-sensitive installation and metadata identity. External cores retain their upstream repository names, including `ReMinori`.

`shinki.emu.musica` and `ReMinori` are different cores in the Musica family. They have separate execution implementations, configuration and save identities. Choosing one does not install or configure the other.

## Guides

- [User guide](user-guide.md): install, games, core updates and patches.
- [Build and release](build-and-release.md): the actual Cargo entry points and blocked targets.
- [Core ABI and SDK](core-abi.md): native ownership and frame lifetimes.
- [Metadata contributions](metadata.md): compatibility reports, patches and review.

## Core references

[RFVP](cores/rfvp.md) · [Siglus](cores/siglus-rs.md) · [Artemis](cores/art3m1s-core.md) · [Musica](cores/shinki-emu-musica.md) · [Sena](cores/sena-rs.md) · [KiriKiri](cores/kirikiroid2-web.md) · [Ethornell](cores/ethornell-rs.md) · [CMVS](cores/shinki-emu-cmvs.md) · [ReMinori](cores/reminori.md) · [impacto](cores/impacto.md) · [Ren’Py](cores/renpy.md).

Current build/release information comes from the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/); game results come from [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/). A build or successful probe is not a playability report. No matching report means unreported.
