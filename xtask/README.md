# ShinkiEMU build tools

Run from the repository root. Cargo subprocesses use `--locked`; update the lockfile deliberately when changing dependencies.

```text
cargo xtask --help
cargo xtask matrix --json
cargo xtask doctor --target x86_64-pc-windows-msvc
cargo xtask deps --target x86_64-pc-windows-msvc
cargo xtask build --target x86_64-pc-windows-msvc --manager --release
cargo xtask build --target x86_64-pc-windows-msvc --all-cores --release
cargo xtask check --target x86_64-pc-windows-msvc --core rfvp
cargo xtask test --core rfvp
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --version 0.1.0-beta.1 --out .work/rfvp-package
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc --version 0.1.0-beta.1
cargo xtask metadata validate
cargo xtask metadata build --out .work/metadata-snapshot
cargo xtask ci --target aarch64-apple-ios --component manager --report .work/ios-report.json
```

`build/cores.toml` owns core identity, manifest, features, license inputs and supported/blocked targets. `build/targets.toml` owns target identity, runner and Manager support. `matrix` emits every combination, including blocked rows. Runner labels are from [GitHub's hosted runner reference](https://docs.github.com/actions/reference/runners/github-hosted-runners): macOS Intel uses `macos-15-intel`, arm64 uses `macos-15`.

`doctor` inspects installed tools and dependency trees and fails on missing prerequisites. It does not install anything. `deps` is the explicit network/preparation command: pinned vcpkg, separate subtitle/FFmpeg installations, locked DXC runtime/licenses and a private cargo-ndk installation. It resolves the subtitle manifest from the exact RoukakuEngine Git revision in Cargo metadata, not a neighboring checkout. Android requires JDK 17, SDK 36 and NDK 30.0.15729638. No command installs system packages.

Each core is built through its own manifest and lockfile into `.work/target/<target>/<plugin_id>`. On Windows, long prefixes use a short directory on the checkout's volume, isolated by repository/target/component hash and checked against an owner record. Set `SHINKI_EMU_TARGET_ROOT` to choose a writable short root. The build receipt in `.work/build` records the actual target directory, Cargo JSON artifact, build-script `OUT_DIR/core-build.json`, revision, toolchain and features. Native adapters must use `shinki-emu-build` and record the actual CMake DLL, resources and licenses. An absent manifest, incorrect ABI/architecture, missing query export or failed supported build is an error.

Packaging consumes release build receipts. Output must be new; staging is in the same parent and publication uses a no-replace rename. Paths, links/reparse points, collisions, hashes, binary architecture and runtime dependency closure are checked without loading binaries. Dependency search is restricted to explicit build/redistributable trees, never the developer's system DLL directory. Cores are independent packages; Manager packages have an initially empty `cores` directory and use the same validated Metadata snapshot as online clients. Android packaging stages JNI/AAR/assets/licenses in a private directory and invokes the pinned Gradle wrapper. Without signing material it produces a clearly named debug-signed CI APK, excluded from Releases. Release signing reuses `SHINKI_EMU_ANDROID_KEYSTORE` (path) or `SHINKI_EMU_ANDROID_KEYSTORE_BASE64`, plus `SHINKI_EMU_ANDROID_KEYSTORE_PASSWORD`, `SHINKI_EMU_ANDROID_KEY_ALIAS` and `SHINKI_EMU_ANDROID_KEY_PASSWORD`. Partial configuration fails; decoded keys remain temporary and are never packaged.

Release archives are deterministic, include build provenance and have corresponding-source ZIPs containing downloaded source archives and their vcpkg ports/patches. Source downloads must match the installed package's SPDX SHA512 values; port recipes and patches must match its recorded SHA256 values. Release sources must be committed, and aggregated platforms must share the same product revision. License or source prerequisites stop publication. A core marked `publish_blocked` may compile but its binary is not uploaded. Resolve its concrete distribution review before changing that field.

`release` writes GitHub only with `--publish` and `GH_TOKEN` or `GITHUB_TOKEN`. It refuses replacement assets with different bytes. Catalog entries are created only after reading back published assets and verifying SHA256 and size, through a normal PR in this repository. Tags are `manager-v<semver>` or `core-<core-slug>-v<semver>`. Dispatch builds only upload Actions artifacts. CI records manifest-declared unsupported rows as `blocked`, with `artifact: null`; supported build/package failures remain failures.

Tool behavior tests: `cargo test --locked -p xtask -p shinki-emu-build`. Tests compile their own public synthetic shared libraries; no game installation or private data is used.
