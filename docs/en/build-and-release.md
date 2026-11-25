---
title: Build from Source
description: Local toolchain setup, cargo xtask build/check/test commands, and per-platform developer workflows.
---

# Build from Source

This guide covers everyday local building, checking, and testing of ShinkiEMU. For full maintainer release engineering — pinned vcpkg/FFmpeg/Ren'Py SDK details, Apple cross-build SDK extraction, OHOS handoff gates, corresponding-source bundling, and historical release records — see the [Release & Maintainer Runbook](release-runbook.md). For headless frame acceptance and Manager UI screenshots, see the [Headless Runner](headless.md).

## Toolchain & Prerequisites

- **Rust toolchain**: Rust 2024 edition, pinned to **Rust 1.99.1** via `rust-toolchain.toml`.
- **Clone with submodules**: `ThirdParty/cpal` is the only Git submodule (external cores are fetched by `cargo xtask deps` from `build/cores.toml`):
  ```sh
  git clone --recurse-submodules https://github.com/Project-Shinkirou/ShinkiEMU.git
  ```
- **Authoritative manifests**: `build/dependencies.toml` (tools & downloads), `build/cores.toml` (cores & features), `build/targets.toml` (targets & runners).

| Platform | Prerequisites |
|---|---|
| **Windows x64** | x64 MSVC Developer environment, CMake, Ninja, Git |
| **Linux x64** | GCC/Clang, CMake, Ninja, pkg-config, NASM, WebKitGTK 4.1 dev packages, audio/graphics dev libs (see apt command below) |
| **macOS / iOS** | Native Xcode (or Linux `cross 0.2.5` + private official Apple SDKs; see [Release Runbook](release-runbook.md)) |
| **Android** | JDK 27, Android SDK platform 37.2, build-tools 37.0.0, NDK r30 (`30.0.16248370`), `cargo-ndk 4.1.2` |
| **OpenHarmony** | DevEco Studio Native SDK 5.0.5 (API 17, `OHOS_SDK_NATIVE`) |

## Common `cargo xtask` Commands

Run from the repository root and scope to `--manager` or a single `--core` so you do not mix unrelated native dependency graphs:

```sh
# Inspect build matrix and local prerequisites (doctor checks only; never installs OS packages)
cargo xtask matrix
cargo xtask doctor --target x86_64-pc-windows-msvc

# Explicitly fetch pinned external core forks and native dependencies
cargo xtask deps --target x86_64-pc-windows-msvc
cargo xtask deps --target x86_64-pc-windows-msvc --core rfvp

# Check, build, and test
cargo xtask check --target x86_64-pc-windows-msvc --manager
cargo xtask check --target x86_64-pc-windows-msvc --core rfvp
cargo xtask build --target x86_64-pc-windows-msvc --manager --release
cargo xtask build --target x86_64-pc-windows-msvc --core rfvp --release
cargo xtask test --manager
cargo xtask test --core rfvp
cargo xtask test
cargo xtask metadata validate
```

- Unselected `cargo xtask test` runs `xtask` and build-helper tests (including icon generation and metadata intake parsing).
- Pass `--allow-dirty-sources` to `deps` when iterating on uncommitted adapter fork edits (HEAD must still match the pinned commit; release builds reject dirty sources).
- Build artifacts are isolated under `.work/target/<target>/<component>` and receipts under `.work/build/`.

## Compiled-In Store Features

`shinki-emu-store` defines object-safe provider/session traits, cancellation, and secret-store boundaries. Manager enables `store-steam`, `store-dlsite`, and `store-dmm` by default. Build Steam-only with `--no-default-features --features store-steam`. Adding a new store requires a provider crate, an optional Cargo feature, and one `registry.register(...)` line in `manager_app/store.rs`.

## Linux Desktop

Install native development packages on Ubuntu 22.04 / 24.04:

```sh
sudo apt-get install -y --no-install-recommends build-essential cmake ninja-build pkg-config nasm patchelf autoconf automake libtool libltdl-dev autoconf-archive python3-jinja2 python3-venv libxtst-dev libxxf86vm-dev libxinerama-dev bison flex gperf libssl-dev libasound2-dev libudev-dev libx11-dev libx11-xcb-dev libxcb1-dev libxrandr-dev libxi-dev libxcursor-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libfontconfig1-dev libegl1-mesa-dev libgl1-mesa-dev libvulkan-dev mesa-vulkan-drivers xvfb xauth unzip jq
sudo apt-get install -y --no-install-recommends libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libglib2.0-dev gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav libgtk-3-dev libwebkit2gtk-4.1-dev
```

Then prepare dependencies, build, and package:

```sh
cargo xtask deps --target x86_64-unknown-linux-gnu
export SHINKI_EMU_PRODUCT_VERSION=0.1.0-beta.8
cargo xtask build --target x86_64-unknown-linux-gnu --manager --release
cargo xtask package --target x86_64-unknown-linux-gnu --manager --version 0.1.0-beta.8 --out .work/linux-manager
```

End-user Linux runtime packages are listed in [Troubleshooting — Linux Runtime Libraries](troubleshooting.md#linux-runtime-libraries).

## Android Local Test APK

Configure `JAVA_HOME` (JDK 27), `ANDROID_HOME`, and `ANDROID_NDK_HOME` (`ndk/30.0.16248370`), then run:

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo xtask deps --target aarch64-linux-android
cargo xtask deps --target x86_64-linux-android
export SHINKI_EMU_PRODUCT_VERSION=0.1.0-beta.8
cargo xtask build --target aarch64-linux-android --manager --release
cargo xtask build --target x86_64-linux-android --manager --release
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-beta.8 --out .work/android-test
```

Packaging requires release build receipts for both `arm64-v8a` and `x86_64` from the same commit. Without a release signing key, xtask outputs `ShinkiEMU-<version>-android-universal-debug-signed.apk` for testing; set `SHINKI_EMU_GRADLE_OFFLINE=1` for offline Gradle runs after the initial sync.

## Disk Layout & Application Icons

- **Game directory isolation**: Everything ShinkiEMU writes inside a game directory lives under `<game>/.shinki/` (`musica/`, `patch/`, `staging/`, `backup/<id>/`, `patch-result.json`), with symlink rejection.
- **Canonical application icon**: Windows, macOS, iOS, Android, and Linux packages use `Assets/Branding/shinkiemu-icon.svg`. Android draws only the adaptive foreground over a separate indigo background. OpenHarmony keeps its own application resources. See [Brand Kit](../../Assets/Branding/BRANDING.md).

## Maintainer Packaging & Release

For pinned dependency details, Ren'Py SDK inputs, Apple cross-builds, OpenHarmony HAP handoff, corresponding-source and license collection, third-party core version hashing, and release history, read the [Release & Maintainer Runbook](release-runbook.md).

## Game-local on-disk layout

See [Game-local on-disk layout](release-runbook.md#game-local-on-disk-layout).

## Package and release

See [Package and release](release-runbook.md#package-and-release).
