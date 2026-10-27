---
title: User guide
description: Install Manager, manage native cores and read platform-specific reports.
---

# User guide

## Install and start

Download a Manager asset matching your platform and architecture from [ShinkiEMU Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases). Extract the complete desktop bundle into a writable location and start `shinki-emu-manager` (`.exe` on Windows). Keep its bundled dependencies, licenses and resources together. `shinki-emu-manager --help` opens desktop CLI help; starting without a subcommand opens the GUI.

Only select assets actually attached to a release. Build availability is listed in the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/). Android debug-signed CI APKs are test builds, not release installers. iOS device and simulator outputs are distinct; a library is not an installable IPA.

On first launch, read the bilingual anti-resale notice and tick “我已阅读 / I have read this”. Continue becomes available after the notice has been visible for eight seconds. The acknowledgement is stored locally with the notice's SHA-256 digest; a revised notice requires acknowledgement again. ShinkiEMU is free, open-source software: use the official website and GitHub Releases, and request a refund and report the seller if you paid for the software. The notice is also available in Settings → About.

The bilingual image is rasterized in Rust and displayed in the onboarding and About image slots. Reading time advances only while the complete image fits at its readable size inside the visible scroll viewport; hidden, undersized, clipped or minimized views pause it. Resize a narrow window vertically or scroll the notice fully into view before reading. Removing the UI image does not bypass the Rust completion gate: onboarding stays locked.

The image follows the current light/dark theme and its surface/text colors, including custom theme colors. Changing the theme refreshes the raster without resetting accumulated reading time.

## Add games and install cores

In Settings, add the directories containing your legally obtained game installations, then scan the library. Select the detected game and check the selected core and game-specific configuration before starting. An engine probe identifies a format; encrypted resources can still require a private profile. Keep all keys local.

Allow networking in Settings before synchronizing metadata or downloading cores. In the core management page, refresh the catalog and install or update the desired core for your platform. The catalog supplies the version, ABI, download URL and hash. Complete the session and restart Manager when an update requires replacing a loaded library. Keep the whole core directory, including `core.json`, dependencies, `Resources/` when present and licenses; a DLL copied alone is not an installation.

## Store downloads

Open **Add game → Store downloads** after allowing network access in Settings. When several providers are compiled in, select a store first. The login options come from that provider: Steam offers QR, account/password with Steam Guard, and desktop client import; DLsite and DMM/FANZA use their official browser sign-in pages and a cookie handoff. Open the official page, sign in to your own account, then paste that site’s cookies into the password-style Cookie field. The field is cleared on submission. Cookies and refresh tokens are stored only in the platform secret store, never in installation records or logs. Logging out removes that provider’s stored login.

**Steam sign-in:** Sign in with an account and password, a Steam Guard email/mobile code or mobile confirmation, or scan the locally rendered QR code with Steam's mobile app. Desktop platforms can also import a cached Steam client login; mobile platforms use password or QR login. Passwords are never persisted. Refresh tokens live in the platform secret store: Windows Credential Manager, Linux secret service, Apple Keychain (macOS/iOS), or AndroidKeyStore-backed encrypted app storage. Sign out removes the saved login.

**QR login** requests a fresh Steam challenge without an account name or password. The code appears once Steam responds; approve it in the mobile app to finish signing in. Cancel stops the pending request without completing login.

Search your owned games, select a game, choose depots and a branch, then select a writable library folder. Windows depots are selected by default even on mobile; choose language depots separately. A downloaded game is added to Manager and probed for compatible cores. Download availability does not imply that a core supports the game's engine.

**Check updates** compares installed depot manifests with Steam. Download again to apply a delta update; **Verify and repair** checks and repairs installed files. Cancellation preserves the interrupted-install marker and waits for outstanding writes; retrying resumes verified local chunks. Keep each game's `.DepotDownloader` directory: it records installed manifests and the metadata needed for delta reuse.

Only PC downloads owned by the signed-in account are eligible. Play-only, streaming and browser titles show why they cannot be downloaded. DRM, launch tokens and licence checks remain the store’s responsibility. Branch/depot controls and verification appear only when supported by the selected provider. New installs live below `<library folder>/.shinki/store/<provider>/installs/`; staging and caches live below `<Manager data root>/.shinki/store/<provider>/`. Existing Steam installation records and secret-store references are migrated once without moving games.

When the store cannot determine the installed version, Manager shows **Update status unavailable** as a neutral status and hides update actions. Refreshing the library asks the provider again. Some downloads are archives or installers: after completion, **Downloaded archives need manual extraction or installer run** appears with **Open folder**. Extract or run the official installer yourself, then add the playable game directory to Manager; Manager does not bypass installers or DRM.


## DLsite downloads

DLsite sign-in uses a browser cookie handoff. Open the sign-in page, complete any CAPTCHA or verification in the browser, then paste the session cookies into Manager. The provider stores the session only in the platform secret store. It lists owned works from DLsite Play and retrieves original PC files only when DLsite exposes an authenticated PC download plan; Play-optimized reader/media files are not installed as games. ZIP files are extracted with path-safety checks. RAR and self-extracting installer parts are kept unchanged and require manual extraction or installation; Manager never runs downloaded executables. DLsite does not provide a reliable installed-build identifier through the researched endpoints, so update state is reported as unknown.

If a work's download plan cannot be resolved, reproduce it in a signed-in browser and export a HAR with response content, beginning at `https://play.dlsite.com/api/v3/download?workno=<owned work number>` through redirects and any `/home/download`, `/home/serial`, or split listing request. Remove cookies, serial numbers and signed URL tokens before sharing. Never commit the HAR.

## Compatibility and metadata

The default metadata source is:

`https://project-shinkirou.github.io/data/shinkiemu/v1/`

Settings controls network permission and manual synchronization. A rejected or damaged snapshot does not replace a valid cached snapshot. Match your VNDB release rID, core, platform, architecture and variant (desktop/device/simulator) when reading a report. A result on another platform or architecture is not your result. Submit reproducible observations through the [issue forms](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new/choose).

## Patches

Reviewed **Luau recipes** run only when explicitly selected. The patch page defaults to **New directory**, which keeps the source game unchanged. **In-place overwrite** writes through private staging, moves changed and removed original files to `<game>/.shinki/backup/<id>/`, records their hashes in `manifest.json`, and re-probes the same game. **Restore latest patch** checks that patched files still match the manifest before restoring; if you edited a patched file, restoration refuses to overwrite it. The receipt is `<game>/.shinki/patch-result.json` in in-place mode. External and hosted catalog patches with artifacts offer an HTTPS download action: Manager verifies size and SHA256 before publishing the file under `<game>/.shinki/patch/`, then selects that directory as patch input. Downloads never execute on their own; only a separately selected trusted Luau recipe can run. Check licenses, authorizations and applicable releases; an empty release list means “release not restricted,” not “every edition tested.” See [patch contributions](metadata.md).

## Data and saves

ShinkiEMU uses a new per-platform application data directory. Open its location from Settings; desktop deployments can set `SHINKI_EMU_DATA_DIR` to an absolute path. Android uses the application's private data directory. Pre-rename application databases, caches and product-owned saves are not imported. Existing game installations and original commercial saves remain untouched by that cutover.

Save behavior is core-specific: Musica keeps its own slots at `<game>/.shinki/musica/saves/`. On first use, existing `.shinki-musica/saves/` files are moved there without overwriting newer files; later reads use only the new path. ReMinori uses a separate save root. Do not exchange saves just because two cores share a family. Close the active session before backing up data or changing cores.
