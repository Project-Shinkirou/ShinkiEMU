---
title: Build and release
description: Cargo orchestration, native build scripts, packaging and explicit blocked targets.
---

# Build and release

Build the Manager and standalone cores with Cargo from the repository root. Use `cargo xtask` for the target matrix, packaging, releases and metadata. For input replay and headless frame or UI checks, see [Headless execution](headless.md).

## Game-local on-disk layout

ShinkiEMU writes its own files inside game directories under `<game>/.shinki/`. Use `musica/saves/` for Musica, `patch/` for downloaded patch artifacts, `staging/` for transactions, `backup/<id>/` for original files and their manifest, and `patch-result.json` for the in-place patch result. Validate paths and resolve directories without following links. Third-party cores retain their upstream storage contracts.

## Inputs and prerequisites

Clone with `git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU`. In an existing clone, run `git submodule update --init --recursive` to initialize the workspace's `ThirdParty/cpal` patch. Install Git and rustup; [`rust-toolchain.toml`](../../rust-toolchain.toml) selects Rust 1.99.1. Add the Rust target and install the native compiler, CMake, Ninja and SDK needed for your target.

Native build scripts bootstrap the pinned vcpkg checkout under `.work/vcpkg`, install per manifest and triplet under `.work/vcpkg-installs/<triplet>-<key>/installed/`, and cache downloads and binaries under `.work/vcpkg-downloads/` and `.work/vcpkg-binary-cache/`. Set `SHINKI_EMU_VCPKG_ROOT` to use a shared checkout at the pinned revision. Install host system packages and SDKs separately. KiriKiri's parser generator runs on the build host: Windows build scripts can fetch the pinned WinFlexBison; on other hosts install Bison, or select it with `BISON_EXECUTABLE` (KrKr2 requires Bison 3.8.2 or newer).

Install the pinned host license tool before packaging, running `ci`, or preparing a release:

```sh
cargo install cargo-about --locked --version 0.9.3
```

### Windows x64

Run Cargo and packaging in an x64 Visual Studio developer shell (`vcvars64.bat`, or `Launch-VsDevShell.ps1 -Arch amd64` in PowerShell). Install the Visual Studio C++ tools, CMake, Ninja and NASM. The packager gets MSVC redistributables such as `VCRUNTIME140.dll` from `%VCToolsRedistDir%\x64`.

### Linux desktop

On Ubuntu 24.04, install the development packages used by the desktop builds:

```sh
sudo apt-get install -y --no-install-recommends build-essential cmake ninja-build pkg-config nasm yasm patchelf autoconf automake libtool libltdl-dev autoconf-archive python3-jinja2 python3-venv libxtst-dev libxxf86vm-dev libxinerama-dev bison flex gperf libssl-dev libasound2-dev libudev-dev libx11-dev libx11-xcb-dev libxcb1-dev libxrandr-dev libxi-dev libxcursor-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libfontconfig1-dev libegl1-mesa-dev libgl1-mesa-dev libvulkan-dev libgtk-3-dev libwebkit2gtk-4.1-dev unzip
sudo apt-get install -y --no-install-recommends libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libglib2.0-dev
```

RFVP discovers GStreamer and GLib through `pkg-config`; its core ZIP does not include them. On an Ubuntu runtime system install `libgstreamer1.0-0 libgstreamer-plugins-base1.0-0 libglib2.0-0t64 gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav` (use `libglib2.0-0` on releases before the t64 transition).

Run `./shinki-emu-manager` from the unpacked Manager directory on X11 or Wayland with a graphics driver. The package uses the distribution's glibc, libstdc++, fontconfig, udev and ALSA libraries, plus GTK 3 and WebKitGTK 4.1 for store sign-in (`libwebkit2gtk-4.1-0` on Ubuntu pulls in GTK 3). Impacto also needs `libdrm2`. KiriKiri2 uses system OpenGL, X11, OpenMP, Pango and GTK (`libgl1 libglx0 libopengl0 libx11-6 libgomp1 libpangocairo-1.0-0 libpangoft2-1.0-0 libgtk-3-0t64` on Ubuntu 24.04).

### Android

Install cargo-ndk 4.1.2 and both Rust targets for the universal Manager APK. Set `ANDROID_HOME` to the SDK directory and `ANDROID_NDK_HOME` to its `ndk/30.0.16248370` directory. APK packaging needs JDK 27 (`JAVA_HOME` and its `bin` on `PATH`), SDK platform 37.2 and build-tools 37.0.0. The project supplies the Gradle wrapper.

```sh
cargo install cargo-ndk --locked --version 4.1.2
rustup target add aarch64-linux-android x86_64-linux-android
sdkmanager --sdk_root="$ANDROID_HOME" --channel=0 "platforms;android-37.2" "build-tools;37.0.0" "ndk;30.0.16248370"
```

The Android Manager embeds Ren’Py. On a provisioned Ubuntu 24.04 x86_64 build host, Cargo prepares the target runtime under `.work/native-downloads/renpy-android/<Rust target>/<recipe key>/`. The recipe needs Python 3 with Jinja2, LLVM/Clang/LLD 18, ccache and the host development packages listed in [CI](../../.github/workflows/ci.yml). On a non-Linux build host, set `RENPY_STATIC_ROOT` to the complete runtime tree prepared on Linux for the matching ABI, including `include/`, `lib/`, `Resources/` and notices. Switch trees when building the other ABI. The Ren’Py recipe uses its own pinned Linux NDK r29; the ShinkiEMU Cargo build uses the NDK r30 above. See [Ren’Py](cores/renpy.md) for the recipe inputs.

### Apple

For native builds, install Xcode, select it with `xcode-select` and accept its license. Deployment minimums are macOS 13 and iOS 16. Install the relevant Rust target: `x86_64-apple-darwin` or `aarch64-apple-darwin` for macOS, `aarch64-apple-ios` for iOS devices, and `aarch64-apple-ios-sim` or `x86_64-apple-ios` for simulators.

The iOS Manager's embedded [KrKr2](cores/krkr2.md) also needs `rustup component add llvm-tools-preview`.

For a Linux-hosted Apple build, install Docker with Buildx, cross 0.2.5 and apple-codesign 0.29.0 (`rcodesign`). Find the matching `macosx`, `iphoneos` or `iphonesimulator` SDK from [Xcode](https://developer.apple.com/download/all/) on macOS via `xcrun --sdk <sdk> --show-sdk-path`. Transfer it privately. Point `SHINKI_APPLE_SDK_DIR` to the extracted SDK directory containing `SDKSettings.json`, `System/` and `usr/`, then build the image for the selected triple. Keep SDKs and SDK images private.

```sh
cargo xtask apple-image --target aarch64-apple-darwin
cross build --locked --release --target aarch64-apple-darwin -p shinki-emu-manager
```

### OpenHarmony Manager

Use DevEco Studio and its OpenHarmony native SDK 5.0.5 (API 17). Install the `aarch64-unknown-linux-ohos` Rust target and set `OHOS_SDK_NATIVE` to `sdk/default/openharmony/native`. For HAP packaging, set `SHINKI_EMU_OHOS_SIGNING_DIR` to a directory outside the checkout. The packager configures the SDK linker and produces a HAP signed with the SDK's test PKI. The HAP is experimental; retail installation and on-device launch are unverified.

## Build, check and test

Use the Manager package `shinki-emu-manager`, or the standalone entry package under [`Source/Cores/`](../../Source/Cores/). Its package name starts with `shinki-emu-core-`; substitute hyphens for dots in the core ID and preserve case and underscores. For example, `rfvp` becomes `shinki-emu-core-rfvp`, and `shinki.emu.musica` becomes `shinki-emu-core-shinki-emu-musica`.

```sh
cargo build --locked --release -p shinki-emu-manager
cargo build --locked --release -p shinki-emu-core-shinki-emu-musica
cargo check --locked -p shinki-emu-core-rfvp
cargo test --locked -p shinki-emu-musica
cargo test --locked -p xtask -p shinki-emu-build
```

Use `--target <triple>` for a specific native target, and run host tests against the crate that owns the behavior. Cargo features control local build variants, for example `--features no-core-download` for Manager. `SHINKI_EMU_PRODUCT_VERSION` optionally sets the Manager build's product version; a local build without a version override uses the Manager package version.

For Android, use cargo-ndk with API 28. For example, build the ARM64 Manager and the standalone x86_64 Siglus core:

```sh
cargo ndk -t arm64-v8a -P 28 build --locked --release -p shinki-emu-manager --lib
cargo ndk -t x86_64 -P 28 build --locked --release -p shinki-emu-core-siglus_rs
```

Use [`build/cores.toml`](../../build/cores.toml) to distinguish standalone `supported_targets`, `static_manager_targets` and explicit blocks. A build row is not a gameplay compatibility result. Android standalone packages only become installable through a verified `android_dynamic` policy. iOS uses bundled cores and OHOS includes Musica; neither installs downloaded cores. `publish_blocked` governs distribution separately from build support.

### Compile-in stores

Manager enables `store-steam`, `store-dlsite` and `store-dmm` by default. For Steam only, pass `--no-default-features --features store-steam` to Cargo. Add a provider crate implementing the `shinki-emu-store` traits, wire its optional dependency and feature, and register it in `manager_app/store.rs`. Providers return opaque plans, fixed secret-free errors and owned titles. Put temporary files in the supplied `.shinki/store/<provider>/` directory, and store credentials through `SecretStore`.

## Add or update a core pin

1. For a standalone library, add an entry crate under `Source/Cores/<id>/`, following the closest existing entry. An in-tree Rust entry uses a path dependency and exposes the ABI query through a `cdylib`; a Rust fork entry inherits the workspace dependency and enables its standalone export feature if needed. Native adapters supply the library through a build script. For static Manager integration, depend directly on the implementation with target-specific static features and register its Rust query in `static_cores.rs`.
2. Declare or update the fork's `git` URL and full `rev` in root [`Cargo.toml`](../../Cargo.toml) under `[workspace.dependencies]`. `build/cores.toml` holds product metadata: IDs, licenses and notices, target support, static embedding and external upstream attribution/release sequence. `adapter_licenses` paths are relative to the fork root; `licenses` paths are relative to ShinkiEMU.
3. Run `cargo metadata --format-version 1` without `--locked` once to refresh [`Cargo.lock`](../../Cargo.lock). Review the lockfile diff for the intended additions or pin changes; avoid a blanket dependency update. Commit the manifest and lockfile together. Use `--locked` for normal builds.

For each external core, **the published source is the public fork commit recorded in `Cargo.lock`**. Make that commit publicly reachable before publishing packages. In-tree cores' source is the ShinkiEMU release commit.

### Work on a fork locally

Keep a local fork outside this repository and add a temporary patch in the workspace root `Cargo.toml`. The table key is the original Git URL, and the path points to the adapter crate inside the local fork. For RFVP:

```toml
[patch."https://github.com/Project-Shinkirou/rfvp"]
shinki-emu-rfvp = { path = "../rfvp/crates/shinki-emu-rfvp" }
```

Use the fork's real package name and compatible version. Run `cargo metadata --format-version 1` to resolve the patch, then build the same entry package. Remove the patch and refresh the lockfile against the public Git pin before committing a release change.

## Matrix and CI

```sh
cargo xtask matrix
cargo xtask matrix --json --ci --since origin/main
cargo xtask ci --target x86_64-pc-windows-msvc --component rfvp --report .work/reports/rfvp-windows.json
```

`matrix` lists declared rows and block reasons. JSON lists supported hosted rows; `--ci` omits the duplicate Android x86_64 Manager row when ARM64 drives the universal APK build. `--since` filters the matrix to affected rows. `ci` packages its selected row and reports the artifact or blocked reason. OpenHarmony is a local DevEco row.

## Package and release

`package` runs the locked release build and stages Cargo's reported library, resources and native runtime inputs in a new output directory. Core packages keep the ABI v3 `core.json` and its file hashes. Packages include Rust license notices from `cargo-about`, explicit core and native notices, and the Manager's offline metadata where applicable. Retain applicable third-party license and relinking obligations.

```sh
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --out .work/rfvp-package
cargo xtask package --target x86_64-pc-windows-msvc --manager --version 0.1.0-beta.1 --out .work/manager-package
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-beta.1 --out .work/android-test
```

The normal Android Manager package builds both ABIs into one universal APK; use a provisioned Linux host for it. Set `GRADLE_USER_HOME` for the debug key. Without release signing inputs, the APK is debug-signed for testing. Use `--qa-single-abi` for a local one-ABI debug APK, for example with a matching Ren’Py runtime tree on a non-Linux host. Standalone Android cores are core packages, not APKs, and do not need Gradle signing.

For a release APK, `cargo xtask package` checks the official signing certificate, package ID `org.shinkirou.shinkiemu`, the expected `versionName` and `versionCode`, and `minSdk` 28. `release.yml` sets `SHINKI_EMU_ANDROID_VERSION_CODE` from `github.run_number`. Each published APK needs a higher code than the last one. Keep the existing workflow file and run-number sequence: recreating, renaming or replacing the workflow can restart numbering at 1 and prevent installed copies from updating.

macOS packaging produces a `.app`; iOS produces a simulator test `.app` or device test `.ipa`. Default Apple signing is ad hoc; device installation requires valid provisioning.

`SHINKI_EMU_MANAGER_FEATURES` adds comma-separated Cargo features during packaging. The private `ios-framework-cores` iOS and `ohos-bundled-musica` HAP variants are for local QA, not release assets.

Pass an explicit product `--version` (SemVer) to package or release the Manager and in-tree cores. Omit it for external cores.

An external core's identity combines `upstream_identity`, the adapter repository and commit resolved from `Cargo.lock`, the `adapter_branch` attribution and a per-core `release_sequence`. Use the real upstream tag or branch and full upstream commit. The external version is `<tag|branch>-<ref slug>-<digest>`: lowercase ASCII alphanumerics in the first 16 ref bytes and replace other bytes with hyphens; hash the UTF-8 upstream repo/kind/ref/commit, adapter repo/branch/commit and decimal sequence with SHA-256, ending each field with LF. Release tags are `manager-v<semver>`, `core-<slug>-v<semver>` for in-tree cores and `core-<slug>-<version>` for external cores. For release tags, lowercase the core ID and replace dots and underscores with hyphens. Increment the sequence for a changed external pin or a new publication; the latest typed catalog entry is selected by sequence.

```sh
cargo xtask release --component rfvp --dry-run
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc
```

`--dry-run` prints the version, tag and targets without building. `--target` builds and prepares the asset and `release.json` under `.work/release/<tag>/<target>/`. `--publish` uses `GH_TOKEN` or `GITHUB_TOKEN` to publish staged assets and opens a catalog PR for a core. Publication checks the staged asset hash; the catalog carries the public asset's SHA-256 and size. Use the same `--scope windows-android` during planning, preparation and publication for a Windows/Android-only release; the default scope is `all`.

[The release workflow](../../.github/workflows/release.yml) prepares hosted assets. Dispatch it with `components` and a shared product `version` for Manager and any selected in-tree cores. Select `release_scope=windows-android` for a Windows/Android-only release, or `prepare_only=true` to stage without publishing.

For an all-platform Manager release, use one successful run with no `source_commit` override. Set `RUN_ID` to that run's ID; compare the two commit SHAs before downloading:

```sh
gh run view "$RUN_ID" --json headSha --jq .headSha
git rev-parse HEAD
gh run download "$RUN_ID" --pattern 'release-files-manager@*' --dir .work/release
```

The download creates one directory per artifact. Merge their `<tag>/<target>/` directories into `.work/release/<tag>/`. With DevEco, the signing directory and `cargo-about` configured, prepare the HAP and publish from that checkout. Set `VERSION` to the run's product version:

```sh
cargo xtask release --component manager --target aarch64-unknown-linux-ohos --version "$VERSION"
cargo xtask release --component manager --version "$VERSION" --scope all --publish
```

See [Metadata](metadata.md) for snapshot and catalog commands and [Core ABI](core-abi.md) for the native package format.
