# ShinkiEMU build tools

Run from the repository root. Use Cargo directly for build, check and test. `cargo xtask` handles selection, packaging, releases and metadata through the locked workspace. Install `cargo-about` before running commands that package binaries:

```sh
cargo install cargo-about --version 0.9.3 --locked --features cli --bin cargo-about
```

```text
cargo xtask --help
cargo xtask matrix
cargo xtask matrix --json --ci --since origin/main
cargo build --locked --release -p shinki-emu-manager
cargo build --locked --release -p shinki-emu-core-rfvp
cargo check --locked -p shinki-emu-core-rfvp
cargo test --locked -p shinki-emu-musica
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --out .work/rfvp-package
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc
cargo xtask release --component rfvp --dry-run
cargo xtask metadata validate
cargo xtask metadata build --out .work/metadata-snapshot
cargo xtask ci --target aarch64-apple-ios --component manager --report .work/ios-report.json
```

[`build/cores.toml`](../build/cores.toml) supplies core IDs, licenses, attribution and target policy. [`build/targets.toml`](../build/targets.toml) supplies triples, runners and Manager support. `matrix` lists declared rows and blocked reasons; `--json` lists supported hosted rows. Root `Cargo.toml` holds Git pins and `Cargo.lock` records the resolved fork commits. For each external core the published source is that public fork commit.

Each core declares a canonical `family_id` (1–64 lowercase ASCII letters, digits or hyphens, beginning with a letter), matching Manager directory recognition. Several implementations may share a family. The Manager's data-only core catalog projects this inventory into plugin identity, declared target availability, first-party status, Android admission policy and recognition data. Reading the catalog does not install, load or query a core. Manager activates separately built binaries, including the signed iOS Frameworks and OHOS HAP libraries.

Build each standalone core through its package under `Source/Cores/<id>/`. Use `shinki-emu-core-` plus the core ID with dots changed to hyphens, preserving case and underscores. For a local fork change, use a workspace `[patch]` and refresh the lockfile; see [Build and release](../docs/en/build-and-release.md#work-on-a-fork-locally).

Cargo build scripts bootstrap pinned vcpkg and install manifests when needed. FFmpeg and libass are shared Cargo dependencies. Android Impacto and RFVP use the shared libass build; its headers, pkg-config data and static archives stay on the selected ABI. Windows Manager builds stage DXC through Cargo. The [build guide](../docs/en/build-and-release.md#inputs-and-prerequisites) lists required host tools, SDKs and cache paths.

Android FFmpeg bindings use the target's versioned API 28 triple and NDK sysroot in both `BINDGEN_EXTRA_CLANG_ARGS_<target>` forms (hyphens and underscores). Bindgen prefers the hyphenated form, preserving the API level when cargo-ndk 4.1.2 replaces the underscored form with sysroot-only flags.
For Android, use `cargo ndk` for standalone builds. The packager imports `cargo ndk-env --json` and invokes Cargo so it can collect the actual artifacts and each dependency's build-script outputs.
Portable resource filenames preserve printable ASCII punctuation and internal spaces, except Windows-reserved characters. Leading/trailing spaces, control characters, traversal and Windows device aliases remain rejected; package copy/archive/extraction preserves resource bytes.
ZIP persistence keeps the validated parent directory's canonical absolute path, including Windows extended-path syntax required by tempfile's direct Win32 rename. Public archive names and complete identity hashes stay unchanged; no-replace and reparse/path checks remain enforced before canonicalization. External tools do not receive this internal path.
Core license inputs use checked asset traversal, preserving inventoried notice directories as well as individual license files.
Android packaging preserves underscore-prefixed runtime directories. Public Android system libraries such as GLES1 and camera2ndk come from the OS; NDK import stubs are not APK runtime inputs.

### Standalone Android core packages

Build and package each ABI separately with the pinned NDK (`aarch64-linux-android`
for `arm64-v8a`, `x86_64-linux-android` for `x86_64`). A desktop-hosted build
cross-links the actual Android ABI; an x86_64 device gate uses the x86_64
package, not translated arm64.

```sh
cargo ndk -t x86_64 -P 28 build --locked --release -p shinki-emu-core-siglus_rs
target=x86_64-linux-android # use -t arm64-v8a and aarch64-linux-android for ARM64
cargo xtask package --target "$target" --core siglus_rs --out ".work/qa/$target/siglus"
```

`core.json` retains the ABI v3, official identity/recognition, and hashed
library, resources, dependency and license files. Package the selected target's native runtime inputs, including the NDK's `libc++_shared.so` when needed. Static FFmpeg comes from the pinned target triplet; retain its notices and license obligations.

Android’s standalone build rows and install admission are separate. An Android package must match the target ABI, ABI v3, the official catalog’s archive SHA-256/size and identity, and the per-core `android_dynamic` minimum release. First-party Android packages also require a verified `source-commit.txt`. Downloads and local official ZIPs go through the same checks; `no-core-download` disables online core install/update, not verified local ZIPs.

Impacto is packaged as an Android core ZIP for each ABI with its `Resources/` tree, native libraries and notices; no Impacto files or query are linked into the universal Manager APK. The APK still carries the Java/AIDL, JAR and JNI host inputs for KiriKiri under `android-host/<core>`. Manager loads admitted implementations only when explicitly activated and passes the process-local Android context only to cores whose policy requires it.

The two KiriKiri host-side Cargo libraries generate these Java, AIDL and JAR inputs once per APK, independently of the Android ABI. `package --manager` does not invoke either KiriKiri native build; `package --core` builds the selected native core separately.

When admitting a new package, test the exact archive on each ABI: install through Manager, restart, verify lazy activation, launch to a real frame with native audio, pause/resume, and verify existing saves. Do not use a translated ARM64 package or historical API-26 results for x86_64 acceptance. Ren’Py standalone packaging retains its CPython/SDL/FFmpeg runtime and resources; its source/relinking [publication block](../docs/licenses/renpy/README.md) still applies.

### Other native inputs

Apple native builds use macOS/Xcode and the matching SDK. iOS Manager packaging requires native Xcode and builds eight independently signed ABI v3 frameworks from the inventory. `ios.json` binds their signed executables and source identities to the Manager commit; CI and release preparation verify it against the archive. Use `cargo xtask apple-image --target <triple>` to prepare a private Apple cross image on a Linux build host for macOS builds.

DevEco builds the OHOS Manager HAP locally and signs it with the OpenHarmony SDK test PKI. The `ohos_hap` inventory selects dynamic ABI v3 cores for the target ABI; the production roster currently contains Musica. `ohos.json` records every core and native dependency in the signed archive. ARM64 is the distribution test HAP; x86_64 is local simulator QA only. See the [OHOS prerequisites](../docs/en/build-and-release.md#openharmony-manager).

To test signing and native assembly without hvigor or a device, prepare the SDK and signing environment above, set `SHINKI_EMU_OHOS_FIXTURE_DIR` outside the repository, and run `cargo test --locked -p xtask ohos_signed_multicore_hap -- --ignored --nocapture`. The test builds and signs two-core ARM64 and x64 fixture HAPs. On a Linux x64 host, extract the x64 fixture's `libs/x86_64` directory. Build `xtask/tests/fixtures/ohos_hap.c` with `-D_GNU_SOURCE -DFIXTURE_HOST -I Source/FamilyApi/shinki-emu-abi/include -ldl`, set `LD_LIBRARY_PATH` to the extracted directory, and pass the absolute path and plugin ID of each core (`test.alpha`, `test.beta`) to the probe. This exercises the ELF libraries and their ABI queries, not the device's install-time trust.

For desktop core testing, package the standalone core with `cargo xtask package --core <id> --target <triple> --version <version> --out <dir>`. Run `shinki-emu-manager headless --core <core.zip> --game-dir <dir> --frames 60 --output <dir>` or use `shinki-emu-headless run` with the same options.

Cargo prepares Impacto's pinned `libatrac9` and `magic_enum` inputs under `.work/native-downloads/` and Ren’Py's desktop SDK, host CPython/Cython and SDL headers. For Android Ren’Py on non-Linux hosts, set `RENPY_STATIC_ROOT` to the matching ABI's complete runtime tree prepared on Linux. See the [Android prerequisites](../docs/en/build-and-release.md#android).

Native build scripts resolve the pinned inputs and use Cargo's `OUT_DIR` to publish staging inputs, including native dependency paths, resources and notices.

Vcpkg uses `.work/vcpkg-downloads/` for downloads, `.work/vcpkg-binary-cache/` for binary packages, and `.work/vcpkg-installs/<triplet>-<key>/installed/` for each manifest install. `SHINKI_EMU_VCPKG_ROOT` can point to a shared pinned vcpkg checkout. Keep independent worktree build and install outputs separate; on Windows use `CARGO_TARGET_DIR` with a short path if needed.

`package` builds and stages to a new output directory. On Windows run it in an x64 Visual Studio developer shell for redistributables. `package --manager` builds the universal Android APK; without release signing inputs it uses the local debug key. Public APKs use `SHINKI_EMU_ANDROID_KEYSTORE` (path) or `SHINKI_EMU_ANDROID_KEYSTORE_BASE64`, plus `SHINKI_EMU_ANDROID_KEYSTORE_PASSWORD`, `SHINKI_EMU_ANDROID_KEY_ALIAS` and `SHINKI_EMU_ANDROID_KEY_PASSWORD`. Partial configuration fails.

Use `package --core <id>` to package a dynamic Android core for the selected ABI. Debug and release Managers both use the official catalog to verify core ZIPs.

Android packaging checks the production certificate when release-signed, the package ID, `versionName`, `versionCode` and `minSdk` 28. `release.yml` derives `SHINKI_EMU_ANDROID_VERSION_CODE` from `github.run_number`. Keep the existing workflow file and sequence; a reset to 1 can block upgrades. Published APKs must use increasing codes.

`release --dry-run` prints the plan. `release --target` builds and prepares an asset in `.work/release/<tag>/<target>/`; `release --publish` uploads staged assets and opens the core catalog PR when applicable. Pass an explicit `--version` for Manager and in-tree cores, and omit it for external cores. The catalog stores SHA-256 and size for each public asset. Game acceptance and distribution review are maintainer decisions, not build inputs.

Manual `ci.yml` dispatch accepts `since`, using the existing
`cargo xtask matrix --json --ci --since <ref>` input classifier. The matrix
checkout fetches full history so the comparison ref is available. An optional
`components` comma-separated list narrows that affected matrix. For example:
`gh workflow run ci.yml --ref main -f since=<base-commit> -f components=renpy`.
Empty inputs and pushes retain the full matrix. Pull requests run `pr-ci.yml`: the same tools job, plus only the components affected since the PR event's base commit, limited to the Windows and Linux x86_64 targets. A manual PR CI run compares against the repository's default branch.

CI uploads build reports automatically. To also upload binaries, dispatch CI with `upload_packages=true` for the selected components after checking redistribution rights.

Changing a core also selects the iOS Manager rows that package it as a Framework. Standalone Android ZIP changes do not select Manager. The Musica companion library is an input to the local HarmonyOS HAP, which remains outside the hosted matrix.

For a Windows/Android-only release, use `release_scope=windows-android` on `release.yml` or `--scope windows-android` on the local release commands. Plan, prepare and publish with the same scope. This scope does not change product support.

For an all-platform Manager release, use one successful hosted release run without a `source_commit` override. Match its `headSha` to local `HEAD`, merge that run's `release-files-manager@*` artifacts under `.work/release/`, prepare the OHOS HAP with DevEco and publish from that checkout. See [the commands](../docs/en/release-runbook.md#package-and-release).

Tool behavior tests: `cargo test --locked -p xtask -p shinki-emu-build`. Tests compile their own public synthetic shared libraries; no game installation or private data is used.

The tools jobs also verify the generated C ABI header with cbindgen 0.29.2 and
locked Cargo metadata. After changing the Rust ABI, use the
[header regeneration commands](../docs/en/core-abi.md#protocol-and-entry-point).
