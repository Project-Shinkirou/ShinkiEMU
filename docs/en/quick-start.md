---
title: Quick Start
description: Download ShinkiEMU Manager, add your games, install native cores, and launch in 3 minutes.
---

# Quick Start

Get your first visual novel running in ShinkiEMU in three minutes.

> [!IMPORTANT]
> **Free & Open Source**: ShinkiEMU is 100% free, open-source software. Download it only from our [official portal](https://project-shinkirou.github.io/en/shinkiemu/#download) or [GitHub Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases). If you paid someone for it, request a refund and report the seller.

## 1. Download Manager for Your Platform

Download the published Manager package for your platform from the [Portal](https://project-shinkirou.github.io/en/shinkiemu/#download) or [GitHub Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases). Pick an asset listed on that release.

| Platform | Package | Cores |
|---|---|---|
| **Windows x64** | Extract the ZIP to a writable folder and run `shinki-emu-manager.exe` | Installed on demand in-app |
| **Linux x64** | Install [runtime libraries](troubleshooting.md#linux-runtime-libraries), extract, and run `./shinki-emu-manager` | Installed on demand in-app |
| **macOS (Apple Silicon / Intel)** | Extract the ZIP and open `ShinkiEMU.app` | Installed on demand in-app |
| **Android** | Install the release-signed APK for your ABI (the universal APK includes `arm64-v8a` and `x86_64`) | Bundled or installed from the verified catalog, depending on release |
| **iOS** | Use the test package for your device or simulator; devices require appropriate provisioning | Bundled |
| **OpenHarmony** | Experimental SDK test-signed HAP (device launch unverified) | Musica built in |

When extracting desktop bundles, keep all bundled dependencies, resources, and licenses together with the executable.

For earlier package inventories, see the [release history](release-runbook.md#release-history). The current Android source requires Android 9 (API 28) or later.

## 2. First-Launch Confirmation

On first launch, Manager displays a bilingual **free & open-source anti-resale notice**. Read through it until the confirmation timer completes, check "我已阅读 / I have read this", and continue. You can reopen the notice anytime under **Settings → About**.

- Manager follows your OS language automatically (Simplified Chinese, English, or Japanese) and can be switched immediately under **Settings → Appearance → Language**.
- Enable network access under **Settings → Network** to sync the core catalog, fetch metadata, or download from stores.

## 3. Add Games & Install Cores

Add games you legally own in either way:

1. **Local folders**: Use **Add folder** in the library header and select the original game folder.
2. **Store downloads**: With networking enabled, use **Download** in the library header and sign in to Steam or DLsite to download owned PC titles.

On desktop and Android releases with a core catalog, open **Cores** and install the required core if it is not bundled. iOS and OpenHarmony use their bundled cores.

## 4. Launch and Play

Select a game in **Library** and choose **Play**. Review the selected core and any per-game settings first.

- Check per-game, per-platform results in the [Compatibility Catalog](https://project-shinkirou.github.io/en/shinkiemu/compatibility/).
- Explore store downloads, Luau patches, updates, and original save compatibility in the [User Guide](user-guide.md).
- Run into a snag? Check [Troubleshooting](troubleshooting.md) or ask in [GitHub Discussions](https://github.com/Project-Shinkirou/ShinkiEMU/discussions).
