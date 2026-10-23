---
title: ReMinori
description: Formats, native behavior and limits of ReMinori.
---

# ReMinori

`plugin_id: ReMinori` · Engine family: Musica

Upstream: [luoyily/ReMinori](https://github.com/luoyily/ReMinori). Adaptation fork: [Project-Shinkirou/ReMinori, shinki branch](https://github.com/Project-Shinkirou/ReMinori/tree/shinki).

## Formats

ReMinori is an independent Musica implementation. Its native PAZ reader supports versions 0/1/2 with per-role keys, transforms and zlib entries. An explicit private `shinki.emu.musica.keys.v2` profile may select archive parameters; without one, upstream retail CD-ROM parameters apply. Script decoding strictly compares CP932/GBK validity and rejects remaining invalid bytes.

## Native behavior and limits

The adapter retains the original VM/IR, GPU composition, CPAL mixer, Media Foundation video, menus and save implementation. It publishes top-down BGRA8 CPU leases at the native 1024×640 raster; render extent is not adjustable. Three reusable CPU slots protect retained frames while native execution continues.

`save_directory` must be an absolute directory outside the game tree. Pure CP932 saves keep the native format; GBK script sessions use the existing UTF-8 port save container to preserve dialogue/comments. Failed audio devices do not silently select diagnostic NullAudio. Native modals require functioning typed host services.

The eden-oriented implementation does not establish support for every minori title or edition. It is not a replacement executor for [the in-tree Musica core](shinki-emu-musica.md), and their save/configuration identities remain separate. ReMinori retains upstream AGPL-3.0 obligations.

See the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/) for build/release status and [compatibility reports](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for specific releases. See also [Core ABI](../core-abi.md) and [User guide](../user-guide.md).
