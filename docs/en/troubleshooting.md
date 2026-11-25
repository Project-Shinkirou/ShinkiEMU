---
title: Troubleshooting & FAQ
description: Solutions for Linux runtime libraries, Android emulator GPU settings, DLsite diagnostics, save migration, and core updates.
---

# Troubleshooting & FAQ

Use this guide to resolve common startup, runtime, store, or save issues. Also check the portal [FAQ](https://project-shinkirou.github.io/en/shinkiemu/faq/).

## Linux Runtime Libraries

Linux desktop bundles rely on distribution graphics, audio, and WebView libraries. On Ubuntu 24.04 / Debian-based systems, install:

```sh
# Manager UI and store web sign-in (WebKitGTK 4.1 + GTK 3)
sudo apt-get install -y libwebkit2gtk-4.1-0 libgtk-3-0t64 libfontconfig1 libasound2t64

# Common native core runtime libraries (OpenGL / X11 / Pango / OpenMP / DRM)
sudo apt-get install -y libgl1 libglx0 libopengl0 libx11-6 libgomp1 libdrm2 libpangocairo-1.0-0 libpangoft2-1.0-0

# GStreamer 1.x + plugins for native video playback in RFVP
sudo apt-get install -y libgstreamer1.0-0 libgstreamer-plugins-base1.0-0 libglib2.0-0 gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav
```

On other distributions, install the equivalent packages and run under a hardware-accelerated X11 or Wayland desktop session. Each core's `build.json` (`system_dependencies`) lists its exact system SONAME requirements.

## First-Launch Anti-Resale Checkbox Stays Disabled

The bilingual free-and-open-source notice on first launch requires **8 seconds of unobstructed visibility** before you can confirm and continue. If the timer pauses, the notice image is partially clipped or scrolled out of view:

- Increase the window height or scroll until the entire notice is visible inside the viewport.
- Switching light/dark theme re-renders the notice while preserving your accumulated reading time.

## Android Emulator Freezes on the First Frame

On Android emulators using software Vulkan/SwiftShader rendering, Slint 1.18.1 renders only the initial frame and stops updating.

- **Fix**: Start the emulator with host GPU acceleration (`-gpu host`), or run on a physical Android device.

## Store Download Issues

### DLsite or DMM Sign-In Page Fails to Open

- **Windows**: Requires the Microsoft Edge WebView2 Runtime (preinstalled on Windows 11; install the Evergreen WebView2 Runtime on Windows 10).
- **Linux**: Install `libwebkit2gtk-4.1-0` (which also pulls in GTK 3).

### "Downloaded archives need manual extraction or installer run"

The store distributed a RAR archive or self-extracting installer. ShinkiEMU never executes downloaded installers automatically: click **Open download folder**, extract or run the official installer yourself, and add the resulting game directory to Manager.

### "Update status unavailable"

Shown normally when a store API does not expose a reliable local build identifier (such as DLsite) or while offline. For Steam titles, refresh your library to re-query.

### Diagnosing a DLsite Title That Cannot Resolve a Download Plan

Reproduce the download in a signed-in browser and capture a HAR with response bodies starting from `https://play.dlsite.com/api/v3/download?workno=<work-id>` through redirects and `/home/download`, `/home/serial`, or split-volume requests. **Strip all cookies, serial numbers, and signed URL tokens before sharing — never commit or publicly post raw HAR files.**

## Patch Prompts for a Core Update

If applying a patch or launching a patched game reports missing core capabilities (for instance, newer Musica patches require Musica `beta.5`+, and native RFVP script patches require script save isolation and configured font priority):

1. Open **Cores**, refresh the catalog, and update the required core.
2. Close any active game session and restart Manager if prompted.
3. Retry applying the patch or launching the game.

## Saves & Legacy Data Notes

- **Original save exchange**: For `shinki.emu.musica`, copy files between Windows `Documents\minori\<title>\` and `<game>/.shinki/musica/minori/<title>/` while the game is closed.
- **Legacy beta `.asav` files**: Early Musica beta `.asav` files (`AMUSSV06` and older) are incompatible with original-format saves. ShinkiEMU leaves `.shinki-musica/saves/` and `.shinki/musica/saves/` untouched; back them up and start fresh original-format saves.
- **`Manager 0.1.0-beta.7` Windows version label**: Known historical issue — the official `beta.7` Windows binary self-reported `0.1.0-beta.6`. Fixed in `0.1.0-beta.8` via compile-time product version enforcement.

## Still Stuck?

- **Questions & community tips**: Post in [GitHub Discussions](https://github.com/Project-Shinkirou/ShinkiEMU/discussions).
- **Reproducible bugs**: File a [Bug Report](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=bug.yml).
- **Upstream engine bugs**: Each core page links its upstream project when an issue belongs in the underlying engine itself.
