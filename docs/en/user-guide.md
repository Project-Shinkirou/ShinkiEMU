---
title: User guide
description: Install Manager, manage native cores and read platform-specific reports.
---

# User guide

## Install and start

Download a Manager asset matching your platform and architecture from [ShinkiEMU Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases). Extract the complete desktop bundle into a writable location and start `shinki-emu-manager` (`.exe` on Windows). Keep its bundled dependencies, licenses and resources together. `shinki-emu-manager --help` opens desktop CLI help; starting without a subcommand opens the GUI.

Only select assets actually attached to a release. Build availability is listed in the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/). Android debug-signed CI APKs are test builds, not release installers. iOS device and simulator outputs are distinct; a library is not an installable IPA.

## Add games and install cores

In Settings, add the directories containing your legally obtained game installations, then scan the library. Select the detected game and check the selected core and game-specific configuration before starting. An engine probe identifies a format; encrypted resources can still require a private profile. Keep all keys local.

Allow networking in Settings before synchronizing metadata or downloading cores. In the core management page, refresh the catalog and install or update the desired core for your platform. The catalog supplies the version, ABI, download URL and hash. Complete the session and restart Manager when an update requires replacing a loaded library. Keep the whole core directory, including `core.json`, dependencies, `Resources/` when present and licenses; a DLL copied alone is not an installation.

## Steam downloads

Open **Add game → Steam downloads** after allowing network access in Settings. Sign in with an account and password, a Steam Guard email/mobile code or mobile confirmation, or scan the locally rendered QR code with Steam's mobile app. Desktop platforms can also import a cached Steam client login; mobile platforms use password or QR login. Passwords are never persisted. Refresh tokens live in the platform secret store: Windows Credential Manager, Linux secret service, Apple Keychain (macOS/iOS), or AndroidKeyStore-backed encrypted app storage. Sign out removes the saved login.

**QR login** requests a fresh Steam challenge without an account name or password. The code appears once Steam responds; approve it in the mobile app to finish signing in. Cancel stops the pending request without completing login.

Search your owned games, select a game, choose depots and a branch, then select a writable library folder. Windows depots are selected by default even on mobile; choose language depots separately. A downloaded game is added to Manager and probed for compatible cores. Download availability does not imply that a core supports the game's engine.

**Check updates** compares installed depot manifests with Steam. Download again to apply a delta update; **Verify and repair** checks and repairs installed files. Cancellation preserves the interrupted-install marker and waits for outstanding writes; retrying resumes verified local chunks. Keep each game's `.DepotDownloader` directory: it records installed manifests and the metadata needed for delta reuse.

## Compatibility and metadata

The default metadata source is:

`https://project-shinkirou.github.io/data/shinkiemu/v1/`

Settings controls network permission and manual synchronization. A rejected or damaged snapshot does not replace a valid cached snapshot. Match your VNDB release rID, core, platform, architecture and variant (desktop/device/simulator) when reading a report. A result on another platform or architecture is not your result. Submit reproducible observations through the [issue forms](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new/choose).

## Patches

Reviewed **Luau recipes** describe offline file transformations and run only when explicitly selected. Review their scope and keep a separate output/backup before applying a recipe; submitting an issue never runs its script. **External** and **hosted** patches are presented as links or downloads, never executed by Manager. Check the license, authorization, applicable releases and artifact SHA256. An empty release list means “release not restricted,” not that every edition was tested. See [patch contributions](metadata.md).

## Data and saves

ShinkiEMU uses a new per-platform application data directory. Open its location from Settings; desktop deployments can set `SHINKI_EMU_DATA_DIR` to an absolute path. Android uses the application's private data directory. Pre-rename application databases, caches and product-owned saves are not imported. Existing game installations and original commercial saves remain untouched by that cutover.

Save behavior is core-specific: Musica uses `.shinki-musica`, while ReMinori requires its own separate save root. Do not exchange saves merely because two cores share a family. Close the active session before backing up data, changing a core or moving a save root.
