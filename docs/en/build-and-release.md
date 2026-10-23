---
title: Build and release
description: Cargo orchestration, native build scripts, packaging and explicit blocked targets.
---

# Build and release

## Inputs and prerequisites

Clone the repository with its recursive submodules. Use Rust/Cargo and the target's native toolchain. Windows native builds require an x64 MSVC developer environment, CMake, Ninja and Git. Android uses JDK 17, NDK 30.0.15729638 and cargo-ndk 4.1.2. Tool versions and downloads are inputs in `build/dependencies.toml`; core manifests/features/licenses are in `build/cores.toml`, and target/runner definitions in `build/targets.toml`.

Ren’Py additionally needs the official pinned SDK: `RENPY_SDK_ROOT`, `RENPY_PYTHON_DEV`, `RENPY_SDL_INCLUDE` and the official Cython 3.2.4 compiler matching CPython 3.12.7. These are upstream build/runtime dependencies, not replacement product build tools. Inspect the selected fork's `shinki/sdk.json` for SDK inputs.

Run from the repository root:

```sh
cargo xtask --help
cargo xtask matrix --json
cargo xtask doctor --target x86_64-pc-windows-msvc
cargo xtask deps --target x86_64-pc-windows-msvc
cargo xtask doctor --target x86_64-pc-windows-msvc --core renpy
```

`doctor` diagnoses missing inputs and exits nonzero; it does not install them. `deps` is the explicit download/preparation entry and can be narrowed with `--core <plugin_id>`. It prepares locked native dependencies rather than installing system packages. Ordinary commands do not prepare downloads implicitly; Cargo still needs its resolved dependencies available.

## Build, check and test

```sh
cargo xtask build --target x86_64-pc-windows-msvc --manager --release
cargo xtask build --target x86_64-pc-windows-msvc --core rfvp --release
cargo xtask build --target x86_64-pc-windows-msvc --all-cores --release
cargo xtask check --target x86_64-pc-windows-msvc --manager
cargo xtask check --target x86_64-pc-windows-msvc --core rfvp
cargo xtask test --manager
cargo xtask test --core rfvp
cargo xtask test
```

Select exactly one of `--manager`, `--core` and `--all-cores` for build. Check takes one Manager/core selection; tests run on the host and have no `--target` flag. Test without a selection runs xtask and build-helper tests. Native regressions may require real GPU/audio devices and public fixtures.

Each core owns a real `build.rs`. It validates target inputs and records `core-build.json` in Cargo's `OUT_DIR`. C++ adapters drive their native CMake builds; pure Rust cores are compiled by Cargo. Build scripts do not recursively run Cargo or publish releases. xtask reads `compiler-artifact` and `build-script-executed` JSON, not guessed target paths. Components build independently under `.work/target/<target>/<component>`; receipts are under `.work/build/`. Do not combine Artemis's Lua 5.1 graph and Manager's Luau graph with a workspace-wide all-features build.

## Matrix and CI

`matrix --json` reads the authoritative manifests and emits each core and Manager across nine target triples. Read current combinations and reasons in the [core directory](https://project-shinkirou.github.io/en/shinkiemu/cores/), rather than copying a support table into documentation. Android RFVP static-in-Manager integration is distinct from standalone dynamic-core packaging.

```sh
cargo xtask ci --target x86_64-pc-windows-msvc --component rfvp --report .work/reports/rfvp-windows.json
```

Declared unsupported combinations produce `blocked`, a reason and `artifact: null`. Missing native iOS/OpenHarmony hosts are not fixed by installing an SDK; OpenHarmony also requires `OHOS_SDK_NATIVE`. A compilation failure for a declared supported combination is `failed` and exits nonzero. `--all-cores` skips declared blocked combinations but does not hide supported-core failures. `publish_blocked` records distribution restrictions separately from build availability.

## Package and release

Build the selected component first, then package its recorded output:

```sh
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --version 0.1.0-beta.1 --out .work/packages/rfvp
cargo xtask package --target x86_64-pc-windows-msvc --manager --version 0.1.0-beta.1 --out .work/packages/manager
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc --version 0.1.0-beta.1
```

Output directories must be new. Packaging rejects unsafe paths, links, collisions, missing licenses, wrong architectures, missing query exports and unresolved native dependencies. A core package includes the library, resources, dependencies, licenses, provenance and hashed `core.json`. Source bundles retain applicable corresponding-source/relinking inputs.

Release tags are `manager-v<semver>` or `core-<slug>-v<semver>`. Slugs lowercase plugin_id and replace dots/underscores with hyphens (`ReMinori` → `reminori`); the plugin_id itself is unchanged. Release without `--publish` prepares local artifacts. Publishing requires `--publish` and `GH_TOKEN` or `GITHUB_TOKEN`; it uploads real successful targets and refuses an empty release or a changed same-name asset. Core publication verifies remote bytes before proposing a catalog PR under `Metadata/engines/cores/`. Manager releases are independent of core releases. Distribution restrictions and debug-signed Android test APKs cannot be bypassed by tagging.

Metadata commands are described in [Metadata](metadata.md); native package requirements in [Core ABI](core-abi.md).
