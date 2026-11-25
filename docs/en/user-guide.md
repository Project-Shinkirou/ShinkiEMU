---
title: User Guide
description: Complete guide to ShinkiEMU Manager — library management, cores, store downloads, Luau patches, and saves.
---

# User Guide

This guide covers everyday use of ShinkiEMU Manager. If you are installing for the first time, start with the [3-Minute Quick Start](quick-start.md).

## Languages & Appearance

Manager ships with Simplified Chinese, English, and Japanese interfaces under **Settings → Appearance → Language**. "Follow system" matches your OS locale (Chinese → Simplified Chinese, Japanese → Japanese, all others → English). CJK fonts are bundled inside Manager so text renders cleanly even on systems without CJK fonts installed, using Japanese glyphs in Japanese mode and Simplified Chinese glyphs otherwise.

Game titles, descriptions, and patch notes come from community metadata and follow your chosen interface language (falling back from Japanese to English, then Simplified Chinese). Light mode, dark mode, and custom accent colors are supported.

## Manager Update Checks

With networking enabled, Manager checks for new Manager releases in the background on startup without blocking launch or popping up errors on offline failures. You can also trigger a manual check under **Settings → Updates**.

- Updates come from `Project-Shinkirou/ShinkiEMU` GitHub Releases (`manager-v<semver>`). Pre-release builds opt in to pre-release notifications by default.
- When an update is available, Manager shows release notes and opens the platform download link in your browser — it never auto-installs silently.
- "Ignore this version" persists locally; "Later" hides the prompt for the current session only.

## Game Library & Core Management

### Scanning Local Games

Add directories containing legally acquired games in **Settings** and scan your library. Selecting a game displays its detected core and core-specific configuration. If a title uses encrypted archives, supply its private key configuration locally (keys never leave your device). Deprecated settings are cleaned up automatically after core updates.

### Installing & Updating Desktop Cores

Desktop cores ship and update independently of Manager:

1. Enable networking in **Settings**.
2. Open the **Cores** page and refresh the catalog (which records version, ABI, download URL, and SHA-256 hash).
3. Install or update the cores for your platform.

If an update replaces an already-loaded core library, close any running game session and restart Manager when prompted. Keep each core's complete directory (`core.json`, dependencies, optional `Resources/`, and `licenses/`).

## Store Downloads (Steam / DLsite / DMM)

With networking enabled, open **Add Game → Store Download** to download owned PC games directly into your library. Cookies and refresh tokens are stored exclusively in your OS secret store (Windows Credential Manager, Linux Secret Service, macOS/iOS Keychain, or AndroidKeyStore) and cleared on sign-out.

### Steam

- **Sign-in**: Supports password + Steam Guard email/mobile confirmation, passwordless QR scan from the Steam mobile app, and cached desktop Steam client login import on PC.
- **Downloads & maintenance**: Pick content depots (Windows depots are selected by default even on mobile for cross-platform play) and branches. Finished downloads are added to your library and probed automatically. Supports **Check for updates** (incremental depot reuse) and **Verify and repair**; keep `.DepotDownloader` inside the game folder for incremental updates.

### DLsite & DMM GAMES / FANZA

- **Web sign-in**: Clicking "Open sign-in page" opens the store's official login page in a clean, ephemeral native webview (WebView2 on Windows, WKWebView on macOS/iOS, WebKitGTK on Linux, WebView on Android) so CAPTCHA and 2FA work naturally. Once signed in, Manager reads the session cookie automatically and discards the temporary webview storage.
- **DLsite downloads**: Lists PC games purchased on your account via DLsite Play and downloads original PC files only (never Play viewer media). ZIP archives are extracted after path-safety checks; RAR archives and installers are kept intact for manual extraction or installation.
- **DMM GAMES / FANZA**: Currently supports sign-in only; downloads are not yet available.

New store installs live under `<library-root>/.shinki/store/<store>/installs/`. Manager never removes or bypasses store DRM, launch tokens, or license checks.

## Compatibility & Metadata Sync

The default metadata endpoint is `https://project-shinkirou.github.io/data/shinkiemu/v1/`. You can sync manually from **Settings**; corrupt or invalid snapshots never overwrite a working local cache.

When reading compatibility reports, match the exact **VNDB release rID, core, platform, architecture, and desktop/device/simulator variant**. Share your own findings via the [Compatibility Report](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml) form.

## Luau Patches

The **Patches** page runs both reviewed **Online Patches** (bound to concrete VNDB releases and source SHA-256 fingerprints) and local `.luau`/ZIP scripts (**Load local patch…**, clearly marked unreviewed) inside the same host sandbox.

Scripts run only when you explicitly apply them. Allowed downloads must use HTTPS, match their declared size and SHA-256, and are cached under `<game>/.shinki/patch/`. Resource patches cannot add or modify EXE files.

### Output Modes & Restore

- **New directory (default)**: Writes the patched copy to `<chosen-parent>/localized-output` (`localized-output-1`, etc. if taken), leaving the original game untouched without copying saves.
- **In-place overwrite**: Stages output privately, backs up modified or removed original files to `<game>/.shinki/backup/<id>/` with a hash `manifest.json`, and supports **Restore last patch** without overwriting files you modified afterward.

> [!TIP]
> Some patches require newer core capabilities (for example, newer Musica patches require Musica core `beta.5` or later). Manager verifies loaded core capabilities before applying or launching and tells you if a core update is needed.

## Data Directories & Original Saves

All files written by ShinkiEMU inside a game folder stay under `<game>/.shinki/`. You can open the platform app-data directory directly from **Settings**, or override it on desktop with an absolute `SHINKI_EMU_DATA_DIR` path.

Save locations depend on the core:

- **External cores** (RFVP, Siglus, Artemis, KiriKiri2, Ren'Py, impacto, etc.): Keep upstream save locations and formats; see each core's page.
- **Shinki Musica** (`shinki.emu.musica`): Reads and writes byte-compatible original `.sav` and `.png` files under `<game>/.shinki/musica/minori/<title>/`, interchangeable with Windows `Documents\minori\<title>\` (see [Musica core notes](cores/shinki-emu-musica.md)).

Always close a running game before backing up or exchanging saves.
