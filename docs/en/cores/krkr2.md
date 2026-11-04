---
title: KrKr2
description: Independent native KrKr2 core for the mobile Manager.
---

# KrKr2

`plugin_id: krkr2` · Engine family: KiriKiri

Upstream: [2468785842/krkr2](https://github.com/2468785842/krkr2). Adapter: [Project-Shinkirou/krkr2, shinki](https://github.com/Project-Shinkirou/krkr2/tree/shinki).

## Formats and selection

Probe recognizes loose `startup.tjs` at confidence 9400 and `data.xp3` at 9300. The existing `kirikiroid2-web` core has a different ID and remains the default at 9500 when both match. Select `krkr2` explicitly to use this implementation. Probe confidence identifies a format, not game compatibility.

## Native behavior

The adapter runs this upstream's own modular C++ TJS VM, KAG scripts, renderer, plugins, fonts and save/preferences behavior. It does not forward to kirikiroid2-web. Android and iOS link a private static native closure; Android also uses private Java/JNI packages. Frames are immutable CPU leases from the original game texture. Pause/resume and input reach the native owner thread; cancellation and close join its workers.

This branch integrates the Android/iOS Manager paths. Standalone desktop/mobile core packages are not declared. Building requires the target SDK, Bison and the core-specific prepared vcpkg closure. Release acceptance and actual game compatibility are recorded separately from this integration. Licenses and changes are in the fork's `LICENSE` and `MODIFICATIONS.md`.

Linux build hosts also need Yasm (`apt-get install yasm` on Ubuntu); the pinned FFmpeg x86/x64 recipe requires this assembler tool during dependency preparation.

macOS hosts also need Yasm for x86/x64 inputs: `brew install yasm`.
