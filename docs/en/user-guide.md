---
title: User guide
description: Install Manager, manage native cores and read platform-specific reports.
---

# User guide

## Install and start

Download a Manager asset matching your platform and architecture from [ShinkiEMU Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases). Extract the complete desktop bundle into a writable location and start `shinki-emu-manager` (`.exe` on Windows). Keep its bundled dependencies, licenses and resources together. `shinki-emu-manager --help` opens desktop CLI help; starting without a subcommand opens the GUI.

Only select assets actually attached to a release. Build availability is listed in the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/). Android debug-signed CI APKs are test builds, not release installers. iOS device and simulator outputs are distinct; a library is not an installable IPA.

On first launch, you can add a game folder and choose your network and appearance settings, or skip setup. Networking is off by default. ShinkiEMU is free and open source; download it from the official site or GitHub Releases. See **Settings → About** for licensing information.

## Language

Manager is available in Simplified Chinese, English and Japanese. Choose **Settings → Appearance → Language**. **Follow system** uses the operating-system language: Chinese locales select Simplified Chinese, Japanese locales select Japanese, and every other locale selects English. The choice applies immediately and is saved with the other settings. Chinese and Japanese fonts are bundled, so text displays correctly on systems without CJK fonts. Han characters use Japanese glyph forms when the UI is Japanese and Simplified Chinese forms otherwise.

Game titles, descriptions and patch notes come from metadata. Manager shows the entry for the current language, falls back to English when a Japanese entry is missing, and falls back to Simplified Chinese when English is missing too.

Metadata request failures belong to the selected installation. Starting a retry clears its previous failure state while the request is pending; changing the interface language does not change whether a request failed.

## Manager updates

When networking is allowed, Manager checks its own version once in the background at startup. This never blocks startup or opens an error dialog. **Settings → Updates → Check now** performs a manual check and displays failures. Releases come from Project-Shinkirou/ShinkiEMU's GitHub Releases and are compared using `manager-v<semver>` tags. Core and store-game updates keep their separate controls.

The prerelease channel is selectable and defaults to enabled when running a prerelease Manager. An available update shows its version, release notes and a download link for the current platform, opened in the system browser. Manager never downloads or installs itself. Ignored versions are saved locally and are not prompted again; **Later** hides the prompt for this session. Offline mode sends no update requests.

## Add games and install cores

Open the **⋯** menu in the library header and choose **Manage library → Add folder**. Manager finds games automatically, even offline and before you install a core. The management page shows scan progress and offers a refresh when available. If a folder cannot be read, check its permissions or choose another. On Android, grant access in the system folder picker when prompted.

Choose **Search and filters** from the same menu, or press **Ctrl+F**, to search by title, filter by compatibility, or change the sort order and layout. Select **View games** or go back to see the filtered list. **Clear search and filters** restores all games.

Install a compatible core if a game cannot start because its core is missing. Restart when prompted. If several engines or cores match, choose the one you want to use; Manager keeps your selection even if the core is temporarily missing.

The library and core list use the static catalog and validated package manifests. Native capabilities and settings remain unverified until you explicitly prepare advanced configuration, diagnose one core, or start the selected game. These actions run on the owner thread and touch only the selected implementation. Recognition is not a runtime guarantee; encrypted resources can still need a private profile. Keep keys local.

Settings removed by a core update are removed from saved configuration automatically.

Allow networking in Settings before synchronizing metadata or downloading cores. In the core management page, refresh the catalog and install or update the desired core for your platform. The catalog supplies the version, ABI, download URL and hash. Complete the session and restart Manager when an update requires replacing a loaded library. Keep the whole core directory, including `core.json`, dependencies, `Resources/` when present and licenses; a DLL copied alone is not an installation.

## Store downloads

Open **Manage library → Store downloads** after allowing network access in Settings. When several providers are compiled in, select a store first. The login options come from that provider: Steam offers QR, account/password with Steam Guard, and desktop client import; DLsite and DMM/FANZA sign in on their own pages inside Manager. Cookies and refresh tokens are stored only in the platform secret store, never in installation records or logs. Logging out removes that provider’s stored login.

**Web sign-in (DLsite, DMM/FANZA):** choose **Open sign-in page**. Manager shows the store’s official sign-in page in the system web view — WebView2 on Windows, WKWebView on macOS and iOS, a WebKitGTK window on Linux, and Android’s WebView over the app. Sign in there, including any CAPTCHA or two-step verification. Once the store reaches its signed-in page with the account session, Manager reads that store’s cookies (HttpOnly ones included) directly from the web view, closes it and finishes the login; there is nothing to copy or paste. The web view starts with an empty private cookie store and keeps nothing after it closes; only the provider’s login is saved, in the platform secret store. **Cancel**, closing the window, Android’s back gesture or leaving the Store page abandons the sign-in. Windows needs the Microsoft Edge WebView2 Runtime (included with Windows 11) and Linux needs WebKitGTK 4.1 (`libwebkit2gtk-4.1`).

**Steam sign-in:** Sign in with an account and password, a Steam Guard email/mobile code or mobile confirmation, or scan the locally rendered QR code with Steam's mobile app. Desktop platforms can also import a cached Steam client login; mobile platforms use password or QR login. Passwords are never persisted. Refresh tokens live in the platform secret store: Windows Credential Manager, Linux secret service, Apple Keychain (macOS/iOS), or AndroidKeyStore-backed encrypted app storage. Sign out removes the saved login.

**QR login** requests a fresh Steam challenge without an account name or password. The code appears once Steam responds; approve it in the mobile app to finish signing in. Cancel stops the pending request without completing login.

Search your owned games, select a game, choose depots and a branch, then select a writable library folder. Windows depots are selected by default even on mobile; choose language depots separately. A downloaded game is scanned with the same native-free recognizer. Only a recognized directory enters the game library; downloading it does not establish runtime compatibility.

**Check updates** compares installed depot manifests with Steam. Download again to apply a delta update; **Verify and repair** checks and repairs installed files. Cancellation preserves the interrupted-install marker and waits for outstanding writes; retrying resumes verified local chunks. Keep each game's `.DepotDownloader` directory: it records installed manifests and the metadata needed for delta reuse.

Only PC downloads owned by the signed-in account are eligible. Play-only, streaming and browser titles show why they cannot be downloaded. DRM, launch tokens and licence checks remain the store’s responsibility. Branch/depot controls and verification appear only when supported by the selected provider. New installs live below `<library folder>/.shinki/store/<provider>/installs/`; staging and caches live below `<Manager data root>/.shinki/store/<provider>/`. Existing Steam installation records and secret-store references are migrated once without moving games.

When the store cannot determine the installed version, Manager shows **Update status unavailable** as a neutral status and hides update actions. Refreshing the library asks the provider again. Some downloads are archives or installers: after completion, **Downloaded archives need manual extraction or installer run** appears with **Open folder**. Extract or run the official installer yourself, then add the playable game directory to Manager; Manager does not bypass installers or DRM.


## DLsite downloads

DLsite sign-in uses the web sign-in above, starting at DLsite Play’s login page; it completes once DLsite returns to Play with the `__DLsite_SID` account session. The provider stores the session only in the platform secret store. It lists owned works from DLsite Play and retrieves original PC files only when DLsite exposes an authenticated PC download plan; Play-optimized reader/media files are not installed as games. ZIP files are extracted with path-safety checks. RAR and self-extracting installer parts are kept unchanged and require manual extraction or installation; Manager never runs downloaded executables. DLsite does not provide a reliable installed-build identifier through the researched endpoints, so update state is reported as unknown.

If a work's download plan cannot be resolved, reproduce it in a signed-in browser and export a HAR with response content, beginning at `https://play.dlsite.com/api/v3/download?workno=<owned work number>` through redirects and any `/home/download`, `/home/serial`, or split listing request. Remove cookies, serial numbers and signed URL tokens before sharing. Never commit the HAR.

## Compatibility and metadata

The default metadata source is:

`https://project-shinkirou.github.io/data/shinkiemu/v1/`

Settings controls network permission and manual synchronization. A rejected or damaged snapshot does not replace a valid cached snapshot. Match your VNDB release rID, core, platform, architecture and variant (desktop/device/simulator) when reading a report. A result on another platform or architecture is not your result. Submit reproducible observations through the [issue forms](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new/choose).

Release selection keeps the full release title, platform list and known release date separate from the stable VNDB rID. Each installed copy has its own pin; refreshing releases keeps that pin even if the provider omits it. Release details are cached for the current session. After restart, an existing pin still appears with its saved title (or just its rID for older records); platforms and date remain unknown until fetched.

## Picture filters

Open **Filters** in the Manager or **Screen filter** from the in-game menu. Use **Add filter** to append a stage, then select it to edit its parameters or use the arrow and bin buttons to move or remove it. Filters run in the displayed order. **Replace chain** applies a single preset in place of the entire chain; **Original** clears it.

**Add filter → Load HLSL effect…** imports a Magpie format-4 effect as a new stage. HLSL effects require DXC. On platforms without GPU filter support, or when DXC is missing for an HLSL-based stage, the catalog shows why the option is unavailable.

## Patches

Choose a reviewed Luau patch from **Online patches**, or use **Load local patch…** for a `.luau` script or ZIP. Local scripts without catalog binding are marked unreviewed. Catalog patches match a concrete release and verify file fingerprints before writing. Scripts run only when explicitly selected; permitted script downloads use HTTPS and verify declared size and SHA256 before caching under `<game>/.shinki/patch/`. Online entries allow only their declared downloads.

The patch page defaults to **New directory**, which keeps the source game unchanged and does not copy saves. **In-place overwrite** writes to a staging directory, backs up changed/removed originals under `<game>/.shinki/backup/<id>/`, and records hashes in `manifest.json`. **Restore latest patch** refuses to overwrite patched files you have edited. The in-place receipt is `<game>/.shinki/patch-result.json`. Resource patches cannot create or modify EXE files. See [patch contributions](metadata.md).

On Windows, the output, staging and backup directories inherit their parent directories' permissions. Use a trusted output parent and a trusted game directory for in-place patching. Staging does not protect against other writers with access to those directories.

In **New directory** mode on Windows, macOS and Linux, **Choose output** opens a folder picker. Choose the parent location, or create a folder there using the system's folder controls. Manager displays the full proposed output path below that location: `localized-output`, or `localized-output-1`, `localized-output-2`, etc. if a name is already occupied. Check this path before starting. Selection never creates the conversion directory; only a successful conversion publishes it. Cancelling the picker leaves the previous selection unchanged. Existing files and directories are never overwritten, including anything created at the proposed path after selection. Output cannot overlap the game or patch input. **In-place overwrite** does not use the output selection. On iOS the selected parent receives a new `localized-output` child; Android uses a new app-private export directory.

Windows mapped input folders, including read-only Sandbox shares, use their native handle-derived file IDs for overlap checks. Filesystems without the extended `FileIdInfo` query use `GetFileInformationByHandle` identities; errors other than unsupported-query still stop conversion. The patch wizard displays ordinary Windows paths when the verbatim prefix is unnecessary; stored game identities and handle-based containment checks remain unchanged.

New Musica patches require Musica core beta.5 or later; Yorino requires capability bits 256 and 512. RFVP native-script patches require isolated script saves and configured-font priority. Import preserves receipt settings without loading cores. Explicit advanced configuration and launch validate those settings and capabilities against the selected native descriptor. A failure leaves the library entry and receipt intact and explains the required correction or core update; a version label alone is insufficient. Older Managers continue reading the retained v1 patch catalog and legacy scripts.

A converted game's advanced configuration remains unprepared while its receipt is pending, even if the core is already loaded for another entry. **Load configuration** validates and saves that game's receipt against the selected core. Success consumes the pending settings once; failure preserves both the previous settings and the receipt for retry.

## Data and saves

ShinkiEMU uses a new per-platform application data directory. Open its location from Settings; desktop deployments can set `SHINKI_EMU_DATA_DIR` to an absolute path. Android uses the application's private data directory. Pre-rename application databases, caches and product-owned saves are not imported. Existing game installations and original commercial saves remain untouched by that cutover.

Save behavior is core-specific. [Musica](cores/shinki-emu-musica.md) uses original eden/Perseus `.sav` and `.png` files under `<game>/.shinki/musica/minori/<title>/`. Old beta `.asav` files are incompatible and remain unread and untouched; no conversion is provided. ReMinori has a separate save directory. Close the active session before backing up data or exchanging supported original-format files.
