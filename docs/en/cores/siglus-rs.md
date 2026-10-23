---
title: Siglus
description: Formats, native behavior and limits of Siglus.
---

# Siglus

`plugin_id: siglus_rs` · Engine family: SiglusEngine

Upstream: [xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs). Adaptation fork: [Project-Shinkirou/siglus_rs, shinki branch](https://github.com/Project-Shinkirou/siglus_rs/tree/shinki).

## Formats

`Scene.pck` groups scene chunks and package-level include properties/commands. Package offsets are relative to the package, while offsets in each decoded `.ss` chunk are relative to that chunk. Indexed string lengths and offsets count UTF-16 code units, not bytes; the stored strings have an index-dependent word transform. Scene, label and command-entry tables are separate inputs to VM control flow.

## Native behavior and limits

The fork retains the Siglus VM, renderer, text/keyboard input and native saves. Its adapter owns execution and publishes CPU frames. Native configuration supports paired original/chs resources, G01/movie selection and a separate save root.

A localized edition's wrapper is not part of the embedded runtime: the core does not apply wrapper machine-code patches or invent missing fonts. Package decoding, resource configuration and runtime behavior must be established for the exact edition.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
