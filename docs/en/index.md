---
title: ShinkiEMU Documentation
description: Complete ShinkiEMU documentation hub for players, community contributors, and developers.
---

# ShinkiEMU Documentation

ShinkiEMU pairs a cross-platform library **Manager** with modular **native engine cores** so visual novels run natively on modern computers and phones. The Manager handles library organization, core installation, metadata lookup, store downloads, and sandboxed patches, while each core preserves its engine's own VM, rendering, audio, menus, and save behavior. Shared runtime libraries come from our sister engine [RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine).

## Start Here (Players)

- [Quick Start](quick-start.md): Download Manager, add games, install cores, and start playing in 3 minutes.
- [User Guide](user-guide.md): Interface languages, update checks, core management, Steam/DLsite downloads, Luau patches, and original save compatibility.
- [Troubleshooting](troubleshooting.md): Linux desktop libraries, Android GPU settings, DLsite download diagnostics, and common issues.

## Community & Contributing

- [Contributing Guide](contributing.md): No-code contribution paths, architecture overview, UI translations (`.po`), AI/agent guidelines, and PR workflow.
- [Metadata & Patches](metadata.md): Submit game compatibility reports and Luau patches, plus maintainer intake and snapshot rules.
- [GitHub Discussions](https://github.com/Project-Shinkirou/ShinkiEMU/discussions): Ask questions, share setups, and discuss new ideas.

## Developers & Maintainers

- [Build from Source](build-and-release.md): Pinned toolchain, `cargo xtask` commands, and per-platform local build setup.
- [Core ABI & SDK](core-abi.md): C ABI v3 lifecycle, capability bits, async text replacement, and CPU frame leases.
- [Headless Runner](headless.md): `shinki-emu-headless` frame acceptance, input replay, and Manager light/dark UI screenshots.
- [Release Runbook](release-runbook.md): Pinned native dependencies, CI matrix, Apple/OHOS release handoff, corresponding-source & license bundling, and release delivery records.

## Core Concepts & Per-Core Notes

In ShinkiEMU, an **engine family** is the original engine or format family, a **core** is one implementation of that engine, and **`plugin_id`** is the case-sensitive identifier used for installation and metadata (for example, `shinki.emu.musica` and `ReMinori` belong to the Musica family but have independent runtimes, configs, and saves).

[RFVP](cores/rfvp.md) · [Siglus](cores/siglus-rs.md) · [Artemis](cores/art3m1s-core.md) · [Musica (in-repo)](cores/shinki-emu-musica.md) · [ReMinori](cores/reminori.md) · [CMVS (in-repo)](cores/shinki-emu-cmvs.md) · [KiriKiri2](cores/kirikiroid2-web.md) · [impacto](cores/impacto.md) · [Ren’Py](cores/renpy.md) · [Sena](cores/sena-rs.md) · [Ethornell](cores/ethornell-rs.md)

Live core releases appear in the [Core Catalog](https://project-shinkirou.github.io/en/shinkiemu/cores/), and per-game results live in the [Compatibility Catalog](https://project-shinkirou.github.io/en/shinkiemu/compatibility/).
