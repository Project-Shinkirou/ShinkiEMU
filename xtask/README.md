# ShinkiEMU build tools

Run from the repository root. Cargo subprocesses use `--locked`; update the lockfile deliberately when changing dependencies.

```text
cargo xtask --help
cargo xtask matrix
cargo xtask doctor --target x86_64-pc-windows-msvc
cargo xtask deps --target x86_64-pc-windows-msvc
cargo xtask deps --target x86_64-pc-windows-msvc --all-cores
cargo xtask build --target x86_64-pc-windows-msvc --manager --release
cargo xtask build --target x86_64-pc-windows-msvc --all-cores --release
cargo xtask check --target x86_64-pc-windows-msvc --core rfvp
cargo xtask test --core rfvp
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --out .work/rfvp-package
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc
cargo xtask release --component rfvp --dry-run
cargo xtask metadata validate
cargo xtask metadata build --out .work/metadata-snapshot
cargo xtask ci --target aarch64-apple-ios --component manager --report .work/ios-report.json
```

`build/cores.toml` owns core identity, pinned external source repository/branch/revision, manifest, features, native prerequisites, license inputs and supported/blocked targets. `build/targets.toml` owns target identity, runner and Manager support. `matrix` emits every combination, including blocked rows. Runner labels are from [GitHub's hosted runner reference](https://docs.github.com/actions/reference/runners/github-hosted-runners): macOS Intel uses `macos-15-intel`, arm64 uses `macos-15`.

Each core declares a canonical `family_id` (1–64 lowercase ASCII letters, digits or hyphens, beginning with a letter), matching Manager directory recognition. Several implementations may share a family. Manager-core's build script projects this inventory and the selected target into `static_core_catalog::{entries, lookup}`: each borrowed entry contains only `plugin_id`, `family_id`, `display_name`, `supported` and `embedded`. `supported` means declared standalone or static Manager availability on that target, not installation, loading or game compatibility; `embedded` means static Manager integration. Target-wide blocks disable availability; `publish_blocked` only restricts distribution. Unsupported implementations remain in the catalog, and unknown plugin IDs return `None`. Reading these entries performs no I/O or native registration/query. Installed identity still comes from the existing validated `core.json`; callers keep unknown packages visible for explicit diagnosis rather than querying a library to fill in metadata.

The `static_core_catalog` integration tests compile and run the actual pure catalog module with Windows and both Android ABI inventory projections on the host, from an empty installation directory. These are generation/consumption gates, not Android cross-linking or device execution gates. The existing native `STATIC_CORES` function table determines actual compiled static entrypoints, including optional desktop `static-cmvs`; inventory `embedded = false` must not hide such an entrypoint. Callers use that table's IDs for static availability and selected activation, and catalog entries for display/family/declared-target metadata. Reading either table performs no query or registration.

`doctor` inspects installed tools, source revisions and the selected component's dependency trees; missing prerequisites fail without installation. `deps` is the explicit network/preparation command. No selection means Manager prerequisites; `--core` prepares that core, and `--all-cores` prepares supported cores individually. External core repositories are ignored working checkouts rather than product submodules. `deps` clones/fetches the configured fork and checks out its exact pinned commit, initializes that fork's own submodules, and refuses local modifications instead of resetting them. CPAL remains a small product submodule; mlua uses the registry release.

Native requirements are scoped per component. Only consumers prepare the pinned vcpkg subtitle/FFmpeg trees or DXC runtime. Subtitle lookup runs locked Cargo metadata for the selected core's own manifest and enabled build features, then uses triplets from that exact RoukakuEngine Git checkout with the workspace-owned, baseline-pinned libass manifest; it does not infer optional dependencies from the desktop Manager graph or a neighboring checkout. Android uses JDK 27, SDK 37.2, NDK 30.0.16248370 and private cargo-ndk 4.1.2. No command installs system packages.

The Android Manager is the exception to separate subtitle installation: Impacto and RFVP share libass 0.17.5 from each ABI's Impacto vcpkg tree. Android Impacto preparation overlays the libass port from the locked product registry, overriding the fork's older baseline. Both consumers use that installation's headers, pkg-config metadata and archive. Impacto ASS rendering and RFVP's explicitly supplied font remain enabled. FriBidi is static in this closure; its license and corresponding sources ship with Impacto's other native dependencies.

Android FFmpeg bindings use the target's versioned API 28 triple and NDK sysroot in both `BINDGEN_EXTRA_CLANG_ARGS_<target>` forms (hyphens and underscores). Bindgen prefers the hyphenated form, preserving the API level when cargo-ndk 4.1.2 replaces the underscored form with sysroot-only flags.
Android Manager builds import `cargo ndk-env --json` and run Cargo directly. This preserves Cargo's artifact and build-script JSON, including every embedded core's resource/license manifest; cargo-ndk's build wrapper consumes those messages.
Portable resource filenames preserve printable ASCII punctuation and internal spaces, except Windows-reserved characters. Leading/trailing spaces, control characters, traversal and Windows device aliases remain rejected; package copy/archive/extraction preserves resource bytes.
ZIP persistence keeps the validated parent directory's canonical absolute path, including Windows extended-path syntax required by tempfile's direct Win32 rename. Public archive names and complete identity hashes stay unchanged; no-replace and reparse/path checks remain enforced before canonicalization. External tools do not receive this internal path.
Embedded core license inputs use the same checked asset traversal as native resources, preserving inventoried notice directories as well as individual license files.
Android packaging preserves underscore-prefixed runtime directories and verifies every APK bundled-manifest file against the staged bytes. Public Android system libraries, including GLES1 and camera2ndk, are resolved by the OS; NDK import stubs are not APK runtime inputs. Source-runtime preparation disables Python bytecode writes to keep the verified host interpreter unchanged across ABI preparation.

### Standalone Android core packages

Build and package each ABI separately with the pinned NDK (`aarch64-linux-android`
for `arm64-v8a`, `x86_64-linux-android` for `x86_64`). A desktop-hosted build
cross-links the actual Android ABI; an x86_64 device gate uses the x86_64
package, not translated arm64.

```sh
target=x86_64-linux-android # repeat with aarch64-linux-android
cargo xtask deps --target "$target" --core siglus_rs
cargo xtask build --target "$target" --core siglus_rs --release
cargo xtask package --target "$target" --core siglus_rs --out ".work/qa/$target/siglus"
```

`core.json` retains the ABI v3, official identity/recognition, and hashed
library, resources, dependency and license files. The standalone entrypoint
must be a little-endian ELF64 DSO for the requested machine with a visible
`shinki_emu_query` function, the Android API 28 identification note, and 16 KiB
compatible `PT_LOAD` segments. Private imports require `DT_RUNPATH=$ORIGIN`
and a sibling runtime closure. Package only the selected target's prepared
inputs, including the pinned NDK's `libc++_shared.so` when required; public
device libraries are not packaged. Static FFmpeg comes from the pinned
target vcpkg triplet with its notices, recipes and relinking sources.

Android `supported_targets` currently has standalone rows for `siglus_rs`
(`dynamic`, Rust cdylib; native Kira/CPAL) and `art3m1s-core` (default dynamic
export, Rust cdylib; Vulkan, Luau, Kira/CPAL and static FFmpeg). Neither row
by itself qualifies a core as a replacement for its static APK registration.
Promote Siglus first, then Artemis: boot the website APK normally, install
the matching official ZIP through staging, restart, and verify launch with
native audio, first frame, pause/resume and existing saves on both ABIs.
The process-local Android context extension is negotiated only on launch.
Legacy API-26 packages do not establish this parity.

Keep the APK's other embedded registrations until their replacement passes
the same gate:

| Core | Android standalone work |
| --- | --- |
| `shinki.emu.cmvs` | `rust-cdylib`, `dynamic-plugin-export` + `video-ffmpeg`; prove the private FFmpeg and GPU/real audio path. |
| `shinki.emu.musica` | `rust-cdylib`, `dynamic-plugin-export` + `ffmpeg-vcpkg`; retain FFmpeg, libass, FriBidi, fonts and their licenses. |
| `rfvp` | `rust-cdylib`, `dynamic-plugin-export`; retain Android native movie/audio and target libass/FriBidi. GStreamer is a desktop Linux input. |
| `impacto` | `native-shared` for standalone, `static` in the APK; preserve its pinned vcpkg inputs, libass/FFmpeg, profiles and resource root. |
| `renpy` | `native-shared` for standalone, `static` in the APK; preserve the target CPython/SDL/FFmpeg runtime, resources and LGPL relinking material. |
| `kirikiroid2-web` | `native-shared` for standalone, `static` in the APK; account for native vcpkg/Bison, Java/AIDL, JNI initialization, resources and the FMOD distribution terms. |
| `krkr2` | `native-static` only today; needs a shared adapter and a supported Android Java/native host and resource lifecycle. |

Sena and Ethornell have no Android adapter target. The separate `cmvs-rs`
remains Android-blocked and publish-blocked pending a supported upstream game
sample. ReMinori remains Android-blocked; its AGPL obligations and separate
engine and save identity are unchanged. The APK's existing `MainActivity`
bootstraps `shinki_emu_manager` and supplies the bundled native host classes;
an APK build must retain those inputs while it still embeds KiriKiri cores.

### Other native inputs

Apple RFVP and the iOS Manager prepare static libass 0.17.5 on a native macOS/Xcode host with the same vcpkg baseline and pinned RoukakuEngine triplet overlay. The built-in Apple triplets map both Darwin architectures and all three iOS triples to target-isolated installs. No Apple GStreamer tree is needed. Static Manager dependencies and exact Git pins must match `static_manager_targets` in the inventory; dynamic export features stay disabled. iOS packages include all five Rust cores' licenses and an `embedded-cores.json` source inventory.

Impacto's pinned `libatrac9` and `magic_enum` sources live under `.work/native-downloads/`. RenPy preparation downloads its official SDK, CPython 3.12.7, Cython 3.2.4, Cython source-license files and SDL 2.0.20 headers. Windows uses the pinned NuGet development tree and cp312 wheel. Linux builds the pinned CPython source with the host C compiler and loads Cython's pinned Python sources from its standard library. Both interpreters are task-owned under `.work/native-downloads/python`; no system interpreter or developer cache is used. Linux preserves archive executable modes and verifies the SDK's `librenpython.so`, which contains CPython as well as Ren'Py and SDL. Build environments receive `RENPY_SDK_ROOT`, `RENPY_PYTHON_DEV`, `RENPY_SDL_INCLUDE`, `Python3_EXECUTABLE` and `RENPY_CYTHON_LICENSE` automatically on either host.

Linux SDL headers come from the pinned source archive and run its configure
step to generate the native `SDL_config.h`; the Visual C++ archive contains
Windows-only configuration headers and is used only on Windows.

`deps --core <id>` also prepares a core listed in the target's `static_manager_targets`; this does not enable standalone dynamic-core installation. Linux's SDL source archive supplies `LICENSE.txt`, while the pinned Windows VC header archive supplies `COPYING.txt`. Android Renpy uses the official target CPython 3.12.8 source/runtime stage documented by its fork, separately from the host Cython interpreter.

Every native download is SHA256-pinned in `build/dependencies.toml`. Extraction is staged, rejects unsafe paths/links/collisions, and publishes only a complete tree. A local verification record binds every file to the archive recipe. A verified existing tree is reused without downloading; modified trees fail without overwriting their files. Bytecode writes are disabled for the task-owned build interpreter so builds do not mutate the verified installation.

Native subprocesses use `.work/vcpkg-binary-cache`, `.work/vcpkg-downloads` and `.work/tmp` rather than profile caches, with vcpkg concurrency capped at 4 and install operations waiting for the shared lock. The vcpkg registry checkout has complete history: historical port trees must be available locally, so registry acquisition never uses a partial or shallow clone. CMake respects an explicit `CMAKE_GENERATOR`; otherwise it prefers Ninja when available inside an MSVC developer environment.

On Windows hosts, `deps` uses the exact CMake and PowerShell versions in the pinned vcpkg registry's `scripts/vcpkg-tools.json` for vcpkg installs, regardless of `VCPKG_FORCE_SYSTEM_BINARIES`. vcpkg reuses a matching executable or acquires its pinned archive with SHA512 verification. Seed only matching archives into the worktree's own `.work/vcpkg-downloads`; no system installation is required. Packages built with different tools keep their original ABI keys.

For parallel worktrees, share trusted binary caches as read-only sources and write only to the current worktree's cache. For example, set `VCPKG_BINARY_SOURCES` to `clear;files,E:/shared/vcpkg-cache,read;files,E:/worktree/.work/vcpkg-binary-cache,readwrite`. vcpkg still matches full package ABIs. Keep registry checkouts, downloads, install trees, temporaries and build receipts private to each build owner.

Each core is built through its own manifest and lockfile into `.work/target/<target>/<plugin_id>`. On Windows, long prefixes use `.work/t/<repository-target-component-hash>` inside the checkout and are checked against an owner record. Set `SHINKI_EMU_TARGET_ROOT` to choose another writable short root. CMake receives ordinary Windows paths after removing Rust's verbatim-path prefix. The build receipt in `.work/build` records the actual target directory, Cargo JSON artifact, build-script `OUT_DIR/core-build.json`, revision, toolchain and features. Native adapters must use `shinki-emu-build` and record the actual CMake DLL, resources and licenses. An absent manifest, incorrect ABI/architecture, missing query export or failed supported build is an error.

Build receipts record whether source worktrees changed or were dirty during compilation. Local development packages retain that provenance; Release preparation requires a clean receipt at the configured source revision. Corresponding-source archives include the selected fork's pinned repository/revision, not whichever branch happens to be checked out.

Manager release builds take `SHINKI_EMU_PRODUCT_VERSION`, or the existing `SHINKI_EMU_CI_VERSION` when it is absent; conflicting inputs fail. Development builds without either use the Manager package version. Cargo receives the selected product version, and its actual `manager-build-version.json` must agree before xtask records `product_version`. Package/release versions must match that compiled identity on every target and both Android ABIs; an old receipt without the identity must be rebuilt, not relabeled. The official beta.7 Windows binary's internal beta.6 version is a known historical bug; published beta.7 assets are not rewritten.

Committed Git symbolic links retain their exact targets, including long PAX link targets in the nested Ren'Py rebuild recipes. Source archiving never follows a working-tree link or replaces it with Git's raw-header placeholder.

Packaging consumes release build receipts. Output must be new; staging is in the same parent and publication uses a no-replace rename. Paths, links/reparse points, collisions, hashes, binary architecture and runtime dependency closure are checked without loading binaries. Dependency search is restricted to explicit build/redistributable trees, never the developer's system DLL directory. Cores are independent packages; Manager packages have an initially empty `cores` directory and use the same validated Metadata snapshot as online clients. Android packaging stages JNI/AAR/assets/licenses in a private directory and invokes the pinned Gradle wrapper. Without signing material it produces a clearly named debug-signed CI APK, excluded from Releases. Release signing reuses `SHINKI_EMU_ANDROID_KEYSTORE` (path) or `SHINKI_EMU_ANDROID_KEYSTORE_BASE64`, plus `SHINKI_EMU_ANDROID_KEYSTORE_PASSWORD`, `SHINKI_EMU_ANDROID_KEY_ALIAS` and `SHINKI_EMU_ANDROID_KEY_PASSWORD`. Partial configuration fails; decoded keys remain temporary and are never packaged.

Release archives are deterministic, include build provenance and have corresponding-source ZIPs containing downloaded source archives and their vcpkg ports/patches. Source downloads must match the installed package's SPDX SHA512 values; port recipes and patches must match its recorded SHA256 values. Release sources must be committed, and aggregated platforms must share the same product revision. License or source prerequisites stop publication. A core marked `publish_blocked` may compile but its binary is not uploaded. Resolve its concrete distribution review before changing that field.

`release` writes GitHub only with `--publish` and `GH_TOKEN` or `GITHUB_TOKEN`. Existing published tags/assets stay immutable. Manager/self-owned cores use their own SemVer; external core versions are verified upstream tag/branch plus a digest of full upstream/adapter provenance and their increasing publication sequence. Omit `--version` for external cores; a supplied incompatible version is rejected. `release --dry-run` verifies the real refs without building/publishing. Fan-out resolves each component independently. Indexed `cores-v2.json` adds typed identities while `cores.json` preserves the legacy v1 install catalog. See the [identity encoding and ordering contract](../docs/en/build-and-release.md#package-and-release). CI records declared unsupported rows as `blocked`, with `artifact: null`; supported build/package failures remain failures.

Manual `ci.yml` dispatch accepts `since`, using the existing
`cargo xtask matrix --json --ci --since <ref>` input classifier. The matrix
checkout fetches full history so the comparison ref is available. An optional
`components` comma-separated list narrows that affected matrix. For example:
`gh workflow run ci.yml --ref feat/unblock-renpy-linux -f since=ee20b99 -f components=renpy`.
Empty inputs and pushes retain the full matrix. Pull requests run `pr-ci.yml`: the same tools job, plus only the components affected since the base branch, limited to the Windows and Linux x86_64 targets.

For a Windows/Android-only release, dispatch `ci.yml` with
`release_scope=windows-android` and no `since`, `components` or `targets`.
Use `--scope windows-android` for the local `release` plan, preparation and
publication, or set `release_scope=windows-android` on `release.yml`. The same
scope must be present in each prepared receipt. Publication requires all
supported build rows of the selected component in that scope, including
Windows binary and source ZIPs and any standalone Android core rows. The
single Android Manager package uses clean release builds from both ABIs and
includes separate corresponding-source ZIPs for both. Only an APK with the
existing production signer, matching versionName, newer versionCode and
Android API 28 minimum may publish. Its debug-signed equivalent is for QA.

Before publishing scoped packages, set `SHINKI_EMU_RELEASE_CI_RUN_ID` (or
`release.yml` input `ci_run_id`) to the successful same-scope exact-head
`ci.yml` run. Download `release.yml` `prepare_only` artifacts and audit them
before publishing from the original clean commit. `--scope` omitted and tag
pushes keep all platforms and the OHOS local Manager handoff. Scope does not
change which platforms the product supports.

Tool behavior tests: `cargo test --locked -p xtask -p shinki-emu-build`. Tests compile their own public synthetic shared libraries; no game installation or private data is used.
