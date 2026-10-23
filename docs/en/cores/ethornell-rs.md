---
title: Ethornell / BGI
description: Formats, native behavior and limits of Ethornell / BGI.
---

# Ethornell / BGI

`plugin_id: ethornell-rs` · Engine family: Ethornell / BGI

Upstream: [xmoezzz/ethornell-rs](https://github.com/xmoezzz/ethornell-rs). Adaptation fork: [Project-Shinkirou/ethornell-rs, shinki branch](https://github.com/Project-Shinkirou/ethornell-rs/tree/shinki).

## Formats

BGI script detection examines decoded bytes, not only extensions. Archive entries may first be wrapped in `DSC FORMAT 1.00`. BCS begins with `BurikoCompiledScriptVer1.00`; BP system programs often use `._bp`. Headerless scenarios are a distinct variant. BCS body-relative addresses and BP file/code offsets are different coordinate systems.

## Native behavior and limits

The fork owns its VM dispatch, graphics, text, audio and save state; the native adapter publishes final CPU frames. System, graph, sound and user dispatch groups retain their native argument-stack semantics. CP932 source text and localized encodings require the actual edition's configuration, not an assumed global conversion.

Recognized archive/script headers do not imply every dispatch or early-engine variant is implemented. Keep error reports to numeric command/location data and reproduction steps, without dumping commercial strings. The fork carries the full MPL-2.0 text and modification attribution needed for distribution.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
