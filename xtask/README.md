# ShinkiEMU build tools

Run from the repository root. Use Cargo directly for build, check and test. `cargo xtask` handles selection, packaging, releases and metadata through the locked workspace. Install `cargo-about` before running commands that package binaries:

```sh
cargo install cargo-about --locked --version 0.9.3
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

Each core declares a canonical `family_id` (1–64 lowercase ASCII letters, digits or hyphens, beginning with a letter), matching Manager directory recognition. Several implementations may share a family. Manager-core's build script projects this inventory and the selected target into `static_core_catalog::{entries, lookup}`: each borrowed entry contains only `plugin_id`, `family_id`, `display_name`, `supported` and `embedded`. `supported` means declared standalone or static Manager availability on that target, not installation, loading or game compatibility; `embedded` means static Manager integration. Target-wide blocks disable availability; `publish_blocked` only restricts distribution. Unsupported implementations remain in the catalog, and unknown plugin IDs return `None`. Reading these entries performs no I/O or native registration/query. Installed identity still comes from the existing validated `core.json`; callers keep unknown packages visible for explicit diagnosis rather than querying a library to fill in metadata.

The `static_core_catalog` integration tests compile and run the actual pure catalog module with Windows and both Android ABI inventory projections on the host, from an empty installation directory. These are generation/consumption gates, not Android cross-linking or device execution gates. The existing native `STATIC_CORES` function table determines actual compiled static entrypoints, including optional desktop `static-cmvs`; inventory `embedded = false` must not hide such an entrypoint. Callers use that table's IDs for static availability and selected activation, and catalog entries for display/family/declared-target metadata. Reading either table performs no query or registration.

Build each standalone core through its package under `Source/Cores/<id>/`. Use `shinki-emu-core-` plus the core ID with dots changed to hyphens, preserving case and underscores. Manager inherits the same fork dependencies for its static graph. For a local fork change, use a workspace `[patch]` and refresh the lockfile; see [Build and release](../docs/en/build-and-release.md#work-on-a-fork-locally).

Cargo build scripts bootstrap pinned vcpkg and install manifests when needed. FFmpeg and libass are shared Cargo dependencies. Android Impacto and RFVP use the shared libass build; its headers, pkg-config data and static archives stay on the selected ABI. Windows Manager builds stage DXC through Cargo. The [build guide](../docs/en/build-and-release.md#inputs-and-prerequisites) lists required host tools, SDKs and cache paths.

Android FFmpeg bindings use the target's versioned API 28 triple and NDK sysroot in both `BINDGEN_EXTRA_CLANG_ARGS_<target>` forms (hyphens and underscores). Bindgen prefers the hyphenated form, preserving the API level when cargo-ndk 4.1.2 replaces the underscored form with sysroot-only flags.
For Android, use `cargo ndk` for standalone builds. The packager imports `cargo ndk-env --json` and invokes Cargo so it can collect the actual artifacts and each dependency's build-script outputs.
Portable resource filenames preserve printable ASCII punctuation and internal spaces, except Windows-reserved characters. Leading/trailing spaces, control characters, traversal and Windows device aliases remain rejected; package copy/archive/extraction preserves resource bytes.
ZIP persistence keeps the validated parent directory's canonical absolute path, including Windows extended-path syntax required by tempfile's direct Win32 rename. Public archive names and complete identity hashes stay unchanged; no-replace and reparse/path checks remain enforced before canonicalization. External tools do not receive this internal path.
Embedded core license inputs use the same checked asset traversal as native resources, preserving inventoried notice directories as well as individual license files.
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

Android `supported_targets` currently has standalone rows for `siglus_rs`
(`dynamic`, Rust cdylib; native Kira/CPAL) and `art3m1s-core` (default dynamic
export, Rust cdylib; Vulkan, Luau, Kira/CPAL and static FFmpeg). Neither row
by itself qualifies a core as a replacement for its static APK registration.
Promote Siglus first, then Artemis: boot the website APK normally, install
the matching official ZIP through staging, restart, and verify launch with
native audio, first frame, pause/resume and existing saves on both ABIs.
The process-local Android context extension is negotiated only on launch.
Legacy API-26 packages do not establish this parity.

An Android core is installable only when its inventory has an `android_dynamic`
policy with a verified compatibility floor. Standalone build rows alone do not
admit packages. The default inventory has no production floor; keep the existing
static registrations until each official replacement passes both ABI gates.
`no-core-download` disables only online core install/update, not local official
ZIP verification.

Keep the APK's other embedded registrations until their replacement passes
the same gate:

| Core | Android standalone work |
| --- | --- |
| `shinki.emu.cmvs` | `rust-cdylib`, `dynamic-plugin-export` + `video-ffmpeg`; prove the private FFmpeg and GPU/real audio path. |
| `shinki.emu.musica` | `rust-cdylib`, `dynamic-plugin-export` + `ffmpeg-vcpkg`; retain FFmpeg, libass, FriBidi, fonts and their licenses. |
| `rfvp` | `rust-cdylib`, `dynamic-plugin-export`; retain Android native movie/audio and target libass/FriBidi. GStreamer is a desktop Linux input. |
| `impacto` | Android standalone work is deferred. Keep the existing static registration and its shared libass 0.17.5 provider. |
| `renpy` | `native-shared` for standalone, `static` in the APK; preserve the target CPython/SDL/FFmpeg runtime, resources and LGPL relinking material. |
| `kirikiroid2-web` | `native-shared` for standalone, `static` in the APK; account for native vcpkg/Bison, Java/AIDL, JNI initialization, resources and the FMOD distribution terms. |
| `krkr2` | `native-static` only today; needs a shared adapter and a supported Android Java/native host and resource lifecycle. |

Sena and Ethornell have no Android adapter target. The separate `cmvs-rs`
remains Android-blocked and publish-blocked pending a supported upstream game
sample. ReMinori remains Android-blocked; its AGPL obligations and separate
engine and save identity are unchanged. The APK's existing `MainActivity`
bootstraps `shinki_emu_manager` and supplies the KiriKiri host classes.
Its Java, AIDL, JAR and JNI contract are staged as APK compile inputs under
`android-host/<core>` outside runtime assets and downloadable ZIPs. When the
inventory selects a dynamic-only KiriKiri core, the packager builds its host inputs for each ABI. Static integrations supply those inputs through their Cargo build scripts.

### Other native inputs

Apple native builds use macOS/Xcode and the matching SDK. Cargo prepares target-isolated libass for RFVP and the iOS Manager. iOS packages include `embedded-cores.json` and embedded-core licenses. Use `cargo xtask apple-image --target <triple>` to prepare a private Apple cross image on a Linux build host.

The default OHOS Manager embeds Musica statically. For local QA, `SHINKI_EMU_MANAGER_FEATURES` adds packaging features such as `ohos-bundled-musica` on OHOS or `ios-framework-cores` on iOS. With direct Cargo builds, use `--features` instead. These variants do not enable downloads or qualify for public release.

Cargo prepares Impacto's pinned `libatrac9` and `magic_enum` inputs under `.work/native-downloads/` and Ren’Py's desktop SDK, host CPython/Cython and SDL headers. For Android Ren’Py on non-Linux hosts, set `RENPY_STATIC_ROOT` to the matching ABI's complete runtime tree prepared on Linux. See the [Android prerequisites](../docs/en/build-and-release.md#android).

Native build scripts resolve the pinned inputs and use Cargo's `OUT_DIR` to publish staging inputs, including native dependency paths, resources and notices.

Vcpkg uses `.work/vcpkg-downloads/` for downloads, `.work/vcpkg-binary-cache/` for binary packages, and `.work/vcpkg-installs/<triplet>-<key>/installed/` for each manifest install. `SHINKI_EMU_VCPKG_ROOT` can point to a shared pinned vcpkg checkout. Keep independent worktree build and install outputs separate; on Windows use `CARGO_TARGET_DIR` with a short path if needed.

`package` builds and stages to a new output directory. On Windows run it in an x64 Visual Studio developer shell for redistributables. Android Manager packaging runs the Gradle wrapper and normally builds a universal APK; the `--qa-single-abi` variant uses the local debug key and requires no release signing inputs. Public APKs use `SHINKI_EMU_ANDROID_KEYSTORE` (path) or `SHINKI_EMU_ANDROID_KEYSTORE_BASE64`, plus `SHINKI_EMU_ANDROID_KEYSTORE_PASSWORD`, `SHINKI_EMU_ANDROID_KEY_ALIAS` and `SHINKI_EMU_ANDROID_KEY_PASSWORD`. Partial configuration fails.

`release --dry-run` prints the plan. `release --target` builds and prepares an asset in `.work/release/<tag>/<target>/`; `release --publish` uploads staged assets and opens the core catalog PR when applicable. Pass an explicit `--version` for Manager and in-tree cores, and omit it for external cores. The catalog stores SHA-256 and size for each public asset. `publish_blocked` prevents publication independently of build availability.

Manual `ci.yml` dispatch accepts `since`, using the existing
`cargo xtask matrix --json --ci --since <ref>` input classifier. The matrix
checkout fetches full history so the comparison ref is available. An optional
`components` comma-separated list narrows that affected matrix. For example:
`gh workflow run ci.yml --ref feat/unblock-renpy-linux -f since=ee20b99 -f components=renpy`.
Empty inputs and pushes retain the full matrix. Pull requests run `pr-ci.yml`: the same tools job, plus only the components affected since the base branch, limited to the Windows and Linux x86_64 targets.

For a Windows/Android-only release, use `release_scope=windows-android` on `release.yml` or `--scope windows-android` on the local release commands. Plan, prepare and publish with the same scope. This scope does not change product support.

For an all-platform Manager release, download `release-files-manager@*` from one successful hosted release run for the local checkout's commit. Merge the staged target directories, prepare the OHOS HAP locally with DevEco, and publish from that checkout. Follow [Package and release](../docs/en/build-and-release.md#package-and-release).

Tool behavior tests: `cargo test --locked -p xtask -p shinki-emu-build`. Tests compile their own public synthetic shared libraries; no game installation or private data is used.

The tools jobs also verify the generated C ABI header with cbindgen 0.29.2 and
locked Cargo metadata. After changing the Rust ABI, use the
[header regeneration commands](../docs/en/core-abi.md#protocol-and-entry-point).
