---
title: Quick Start
description: Download ShinkiEMU Manager, add your games, install native cores, and launch in 3 minutes.
---

# Quick Start

Get your first visual novel running in ShinkiEMU in three minutes.

> [!IMPORTANT]
> **Free & Open Source**: ShinkiEMU is 100% free, open-source software. Download it only from our [official portal](https://project-shinkirou.github.io/en/shinkiemu/#download) or [GitHub Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases). If you paid someone for it, request a refund and report the seller.

## 1. Download Manager for Your Platform

Download the latest release (currently **Manager 0.1.0-beta.8**) from the [Portal](https://project-shinkirou.github.io/en/shinkiemu/#download) or [GitHub Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases):

| Platform | Package | Cores |
|---|---|---|
| **Windows x64** | Extract the ZIP to a writable folder and run `shinki-emu-manager.exe` | Installed on demand in-app |
| **Linux x64** | Install [runtime libraries](troubleshooting.md#linux-runtime-libraries), extract, and run `./shinki-emu-manager` | Installed on demand in-app |
| **macOS (Apple Silicon / Intel)** | Extract the ZIP and open `ShinkiEMU.app` | Installed on demand in-app |
| **Android** | Install the release-signed universal APK (`arm64-v8a` + `x86_64`) | 8 built-in cores |
| **iOS** | Ad-hoc device/simulator test packages (bring your own signing) | 6 built-in cores |
| **OpenHarmony** | Experimental SDK test-signed HAP (device launch unverified) | Musica built in |

When extracting desktop bundles, keep all bundled dependencies, resources, and licenses together with the executable.

## 2. First-Launch Confirmation

On first launch, Manager displays a bilingual **free & open-source anti-resale notice**. Read through it until the confirmation timer completes, check "我已阅读 / I have read this", and continue. You can reopen the notice anytime under **Settings → About**.

- Manager follows your OS language automatically (Simplified Chinese, English, or Japanese) and can be switched immediately under **Settings → Appearance → Language**.
- Enable network access under **Settings → Network** to sync the core catalog, fetch metadata, or download from stores.

## 3. Add Games & Install Cores

Add games you legally own in either way:

1. **Local folders**: Add your game directories under **Settings → Library Roots** and scan your library.
2. **Store downloads**: With networking enabled, open **Add Game → Store Download** and sign in to Steam or DLsite to download owned PC titles.

On desktop, open the **Cores** page, refresh the catalog, and install the cores your games need. Mobile builds use their built-in cores directly.

## 4. Launch and Play

Return to **Library**, select a game to confirm its detected core and optional per-game settings, and click **Launch**.

- Check per-game, per-platform results in the [Compatibility Catalog](https://project-shinkirou.github.io/en/shinkiemu/compatibility/).
- Explore store downloads, Luau patches, updates, and original save compatibility in the [User Guide](user-guide.md).
- Run into a snag? Check [Troubleshooting](troubleshooting.md) or ask in [GitHub Discussions](https://github.com/Project-Shinkirou/ShinkiEMU/discussions).
