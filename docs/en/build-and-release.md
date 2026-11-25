---
title: Build from Source
description: Local Cargo builds, pinned native prerequisites and the supported build matrix.
---

# Build from Source

Build the Manager and standalone cores with Cargo from the repository root. Use `cargo xtask` for the target matrix and CI. See the [Release & Maintainer Runbook](release-runbook.md) for packaging, publication and historical releases, and [Headless execution](headless.md) for frame acceptance, replay and UI screenshots.

## Game-local on-disk layout

ShinkiEMU writes its own files inside game directories under `<game>/.shinki/`. Use `musica/saves/` for Musica, `patch/` for downloaded patch artifacts, `staging/` for transactions, `backup/<id>/` for original files and their manifest, and `patch-result.json` for the in-place patch result. Validate paths and resolve directories without following links. Third-party cores retain their upstream storage contracts.

## Inputs and prerequisites

Cargo resolves external-core pins from [Cargo.toml](../../Cargo.toml) and [Cargo.lock](../../Cargo.lock). See [native inputs](../../build/dependencies.toml), [core inventory](../../build/cores.toml) and [target definitions](../../build/targets.toml).

Clone with `git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU`. In an existing clone, run `git submodule update --init --recursive` to initialize the workspace's `ThirdParty/cpal` patch. Install Git and rustup; [`rust-toolchain.toml`](../../rust-toolchain.toml) selects Rust 1.99.1. Add the Rust target and install the native compiler, CMake, Ninja and SDK needed for your target.

Native build scripts bootstrap the pinned vcpkg checkout under `.work/vcpkg`, install per manifest and triplet under `.work/vcpkg-installs/<triplet>-<key>/installed/`, and cache downloads and binaries under `.work/vcpkg-downloads/` and `.work/vcpkg-binary-cache/`. Set `SHINKI_EMU_VCPKG_ROOT` to use a shared checkout at the pinned revision. Install host system packages and SDKs separately. KiriKiri's parser generator runs on the build host: Windows build scripts can fetch the pinned WinFlexBison; on other hosts install Bison, or select it with `BISON_EXECUTABLE` (KrKr2 requires Bison 3.8.2 or newer).

Install the pinned host license tool before packaging, running `ci`, or preparing a release:

```sh
cargo install cargo-about --locked --version 0.9.3
```

### Windows x64

Run Cargo and packaging in an x64 Visual Studio developer shell (`vcvars64.bat`, or `Launch-VsDevShell.ps1 -Arch amd64` in PowerShell). Install the Visual Studio C++ tools, CMake, Ninja and NASM. The packager gets MSVC redistributables such as `VCRUNTIME140.dll` from `%VCToolsRedistDir%\x64`.

`Assets/Effects/dxc.lock.json` records exact ZIP member names, not filesystem paths. The pinned DXC archive uses backslashes in its raw member names; ZIP listing tools may display them as forward slashes.

Windows reuses loaded DLLs by basename, so Impacto and KiriKiri must agree on the `fmt.dll` ABI; the current Impacto pin uses fmt 11.2.0 to address [#37](https://github.com/Project-Shinkirou/ShinkiEMU/issues/37).

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

The [Gradle 9.8.2 compatibility matrix](https://docs.gradle.org/9.8.2/userguide/compatibility.html#java_runtime) confirms JDK 27 can run the wrapper. [Android Gradle Plugin 9.4](https://developer.android.com/build/releases/agp-9-4-0-release-notes) requires at least JDK 17; this project uses AGP 9.4.1 and still targets JVM 17 for Java and Kotlin bytecode.

```sh
cargo install cargo-ndk --locked --version 4.1.2
rustup target add aarch64-linux-android x86_64-linux-android
sdkmanager --sdk_root="$ANDROID_HOME" --channel=0 "platforms;android-37.2" "build-tools;37.0.0" "ndk;30.0.16248370"
```

For Android `xtask package`, Cargo uses the NDK's CMake toolchain with the same ABI and API 28 as cargo-ndk. This also covers dependencies such as shaderc that invoke CMake from a Cargo build script.

Building the standalone Android Ren’Py core requires its matching runtime tree. On a provisioned Ubuntu 24.04 x86_64 build host, Cargo prepares it under `.work/native-downloads/renpy-android/<Rust target>/<recipe key>/`. The recipe needs Python 3 with Jinja2, LLVM/Clang/LLD 18, ccache and the host development packages listed in [CI](../../.github/workflows/ci.yml). On a non-Linux build host, set `RENPY_STATIC_ROOT` to the complete runtime tree prepared on Linux for the matching ABI, including `include/`, `lib/`, `Resources/` and notices. Switch trees when building the other ABI. The Ren’Py recipe uses its own pinned Linux NDK r29; the ShinkiEMU Cargo build uses the NDK r30 above. See [Ren’Py](cores/renpy.md) for the recipe inputs.

### Apple

For native builds, install Xcode, select it with `xcode-select` and accept its license. Deployment minimums are macOS 13 and iOS 16. Install the relevant Rust target: `x86_64-apple-darwin` or `aarch64-apple-darwin` for macOS, `aarch64-apple-ios` for iOS devices, and `aarch64-apple-ios-sim` or `x86_64-apple-ios` for simulators.

The iOS Manager's embedded [KrKr2](cores/krkr2.md) also needs `rustup component add llvm-tools-preview`.

For a Linux-hosted Apple build, install Docker with Buildx, cross 0.2.5 and apple-codesign 0.29.0 (`rcodesign`). Find the matching `macosx`, `iphoneos` or `iphonesimulator` SDK from [Xcode](https://developer.apple.com/download/all/) on macOS via `xcrun --sdk <sdk> --show-sdk-path`. Transfer it privately. Point `SHINKI_APPLE_SDK_DIR` to the extracted SDK directory containing `SDKSettings.json`, `System/` and `usr/`, then build the image for the selected triple. Keep SDKs and SDK images private.

On Linux x86_64, the pinned [Buildx v0.38.1 binary](https://github.com/docker/buildx/releases/download/v0.38.1/buildx-v0.38.1.linux-amd64) has SHA-256 `e25d5cd675092bb5d1d03632fdc46cdf3fede7b514dc5f023a105e00d3ce5ad4`. Verify the download, then install it as the executable `$DOCKER_CONFIG/cli-plugins/docker-buildx` for a private Docker configuration without sudo. Confirm with `docker buildx version`. Leave at least 8 GiB free on `/` to build the image.

```sh
cargo xtask apple-image --target aarch64-apple-darwin
cross build --locked --release --target aarch64-apple-darwin -p shinki-emu-manager
```

#### Private iOS bundled frameworks

On native macOS with Xcode, `ios-framework-cores` packages Shinki CMVS and Musica as signed ABI v3 frameworks instead of registering those two statically. The other cores remain static. Choose one target: `aarch64-apple-ios` for `iphoneos`, or `aarch64-apple-ios-sim` / `x86_64-apple-ios` for `iphonesimulator`.

Install `cargo-about` as above. For a device build, set `SHINKI_EMU_IOS_SIGN_IDENTITY` to an Apple Development signing identity with its private key in Keychain. Set `SHINKI_EMU_IOS_PROVISIONING_PROFILE` to the development profile for `org.shinkirou.shinkiemu` and the test device, and `SHINKI_EMU_IOS_ENTITLEMENTS` to the matching entitlements file. The simulator uses ad hoc signing and needs none of these inputs.

```sh
target=aarch64-apple-ios-sim
version=0.1.0-qa.1
SHINKI_EMU_MANAGER_FEATURES=ios-framework-cores cargo xtask package \
  --target "$target" --manager --version "$version" --out ".work/ios-framework-qa-$target"
```

The simulator output is `ShinkiEMU-simulator-test.app`; the device output is a `*-device-test.ipa` containing `Payload/ShinkiEMU.app` with `embedded.mobileprovision`. The app contains `Frameworks/<id>.framework/<id>` and `cores/<id>/core.json`. Packaging signs the frameworks before the app. Opening Manager or scanning games should leave both frameworks unloaded. Launch a known game per core, check ABI v3 selection and a first frame, then close and reopen in the same process. Check logs for dyld errors or duplicate Objective-C classes.

### OpenHarmony Manager

Use DevEco Studio and its OpenHarmony native SDK 5.0.5 (API 17). Install the `aarch64-unknown-linux-ohos` Rust target and set `OHOS_SDK_NATIVE` to `sdk/default/openharmony/native`. For HAP packaging, set `SHINKI_EMU_OHOS_SIGNING_DIR` to a directory outside the checkout. The packager configures the SDK linker and produces a HAP signed with the SDK's test PKI. The HAP is experimental; retail installation and on-device launch are unverified.

#### Bundled Musica device acceptance

The default OHOS Manager statically embeds Musica. The private `ohos-bundled-musica` feature packages a separate `libshinki_emu_core_musica.so` in the signed HAP and loads it on activation. The Manager resolves the fixed sibling beside its OS-installed library, or in that same HAP; it does not load code from extracted assets or writable app data. The [OpenHarmony musl loader](https://github.com/openharmony/third_party_musl/blob/OpenHarmony-5.0.0-Release/ldso/linux/dynlink.c) uses the caller’s namespace and requires directly loaded HAP entries to be uncompressed and page-aligned. Passing the absolute installed path avoids basename lookup through environment or inherited search paths.

Use a dedicated ARM64 device on API 12 or later with Vulkan, audio output, developer debugging enabled and existing trust for the SDK test PKI. If installation rejects the test signature, stop there; do not change device trust. With the SDK’s `hdc` on `PATH`, build the HAP in PowerShell:

```powershell
$version = '0.1.0-qa.1'
$env:SHINKI_EMU_MANAGER_FEATURES = 'ohos-bundled-musica'
cargo xtask package --target aarch64-unknown-linux-ohos --manager --version $version --out .work/ohos-musica-qa
Remove-Item Env:SHINKI_EMU_MANAGER_FEATURES
```

After packaging succeeds, connect the test device, select its key from `hdc list targets`, and set `$device` to that key:

```powershell
$hap = ".work/ohos-musica-qa/ShinkiEMU-${version}-ohos-arm64-v8a-bundled-musica-debug-test-signed.hap"
hdc -t $device install $hap
```

After `hdc` reports a successful install, start the app in debug mode:

```powershell
hdc -t $device shell aa start -D -a EntryAbility -b org.shinkirou.shinkiemu
```

1. Attach the DevEco native debugger at startup. Run LLDB `image list -f` before resuming and again after opening the core list and scanning a lawful Musica game. Neither list should show `libshinki_emu_core_musica.so`. Check `filesDir/ShinkiEMU/diagnostics` for the current run: there should be no `shinki.emu.ohos.core_open` yet.
2. If needed, select Musica without opening core settings. Press **Play** with the original PAZ/SC files and a valid private `key.toml`. Expect one `shinki.emu.ohos.core_open`, then `shinki.emu.ohos.core_abi_ready` for `shinki.emu.musica`, ABI 3, from the installed path. `image list -f` should now include the library.
3. Advance past boot to a recognizable frame and listen for music or voice; compare this with `shinki.emu.ohos.audio_started`. Close and reopen the game in the same Manager process. Playback should restart without another module load. Report the device, OS build and result without sharing keys or game data.

#### Private x86_64 simulator QA

Use the local `x86_64-unknown-linux-ohos` Manager row for a dedicated HarmonyOS x86_64 simulator. It has no standalone core packages and is excluded from hosted CI, releases and the public core catalog. Keep the HAP and its `ohos.json` receipt private. The receipt records the source commit, build features, target, ABI and signed HAP hash.

Use DevEco's API 26 OpenHarmony native SDK with x86_64 sysroot libraries and `libc++_shared.so`. Set `OHOS_SDK_NATIVE` and the private signing directory as above, and install `cargo-about`.

The triplet declares `OHOS` for Meson ports so that vcpkg cross-compiles them even on x86_64 Windows. The pinned FFmpeg port uses its Linux configure branch, but still builds with the DevEco OHOS compiler and sysroot.

From a committed checkout and an x64 Visual Studio developer shell:

```powershell
rustup target add --toolchain 1.99.1 x86_64-unknown-linux-ohos
$env:SHINKI_EMU_MANAGER_FEATURES = 'ohos-bundled-musica'
cargo xtask package --target x86_64-unknown-linux-ohos --manager --version 0.1.0-qa.1 --out .work/ohos-x64-qa
Remove-Item Env:SHINKI_EMU_MANAGER_FEATURES
```

Omit the feature for the statically embedded Musica build. The HAP name includes `ohos-x86_64-local-qa`. Before installation, confirm that a newly created, approved API 26 simulator has separate writable data, reports an x86_64 guest ABI, already trusts the SDK test PKI, and has debugging, a usable Vulkan adapter and audio. Use its exact `hdc` identifier; stop on signature rejection. An image labelled `x86` does not establish the guest ABI, and API 26 toolchain support does not validate an API 17 image. Successful packaging or a visible Manager UI is not evidence of gameplay.

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

## Application icons

**Canonical application icon**: Windows, macOS, iOS, Android, and Linux packages use `Assets/Branding/shinkiemu-icon.svg`. Android draws only the adaptive foreground over a separate indigo background. OpenHarmony keeps its own application resources. See [Brand Kit](../../Assets/Branding/BRANDING.md).

## Package and release

See [Package and release](release-runbook.md#package-and-release) for package commands, signing, external core versions and release gates.
