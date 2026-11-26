---
title: Release & Maintainer Runbook
description: Cargo-based packaging, signing, publication gates and historical release records.
---

# Release & Maintainer Runbook

Build and test locally using [Build from Source](build-and-release.md). This runbook covers Cargo-based packaging, signing, release gates and historical delivery records.

[Game-local on-disk layout](build-and-release.md#game-local-on-disk-layout) applies to both local builds and packaged products.

## Package and release

`package` runs the locked release build and stages Cargo's reported library, resources and native runtime inputs in a new output directory. Core packages keep the ABI v3 `core.json` and its file hashes. Packages include Rust license notices from `cargo-about`, explicit core and native notices, and the Manager's offline metadata where applicable. Retain applicable third-party license and relinking obligations.

```sh
cargo xtask package --target x86_64-pc-windows-msvc --core rfvp --out .work/rfvp-package
cargo xtask package --target x86_64-pc-windows-msvc --manager --version 0.1.0-qa.1 --out .work/manager-package
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-qa.1 --out .work/android-test
cargo xtask package --target x86_64-unknown-linux-gnu --manager --version 0.1.0-qa.1 --out .work/linux-manager

# Local one-ABI APK for an ARM64 test device
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-qa.1 --qa-single-abi --out .work/android-arm64-test

# Local ARM64 gameplay QA APK with the dynamic Musica companion
cargo xtask package --target aarch64-linux-android --manager --version 0.1.0-qa.musica.1 --qa-single-abi --qa-bundle-musica --out .work/android-arm64-musica-test
```

For a nightly or development test, choose a distinct SemVer such as the `0.1.0-qa.1` shown above. For offline Gradle after the first successful sync, set `SHINKI_EMU_GRADLE_OFFLINE=1`. The packager passes `--version` to the build; there is no separate prerequisite build receipt.

The normal Android Manager package builds both ABIs into one universal APK. Set `GRADLE_USER_HOME` for the debug key. Without release signing inputs, the APK is debug-signed for testing. Use `--qa-single-abi` for a local one-ABI debug APK. The APK embeds Impacto and stages the KiriKiri Java hosts; other admitted cores ship as separate packages installed through Manager. The core packages do not need Gradle signing.

`--qa-bundle-musica` requires `--qa-single-abi --manager`, a new output directory, and no Android release signing inputs. It builds Manager and the dynamic Musica core from the same checkout with the same explicit product version. Use a fresh QA version for a new checkout. Install the `*-qa-musica-debug-signed.apk` and add your own game files in Manager; the APK contains no game data and needs no separate Musica ZIP. The packager verifies the signed APK's native library and bundled core files and writes `android-qa-musica.json` with their build provenance, the APK hash, and the debug certificate fingerprint. Normal APKs, release catalogs, and separate core-package admission are unchanged.

For a release APK, `cargo xtask package` checks the official signing certificate, package ID `org.shinkirou.shinkiemu`, the expected `versionName` and `versionCode`, and `minSdk` 28. `release.yml` sets `SHINKI_EMU_ANDROID_VERSION_CODE` from `github.run_number`. Each published APK needs a higher code than the last one. Keep the existing workflow file and run-number sequence: recreating, renaming or replacing the workflow can restart numbering at 1 and prevent installed copies from updating.

The APK metadata check accepts `minSdkVersion` from current Android Build Tools
and `sdkVersion` from older badging output; both must indicate API 28.

macOS packaging produces a `.app`. iOS distribution is for testing: simulators use ad hoc-signed `.app` bundles; the device `.ipa` needs an Apple Development certificate and matching provisioning profile. The hosted device release signs only the checked-out workflow commit on a protected `Project-Shinkirou/ShinkiEMU` main branch, release branch, or Manager tag. It requires the Actions secrets `SHINKI_EMU_IOS_CERTIFICATE_BASE64` (PKCS#12), `SHINKI_EMU_IOS_CERTIFICATE_PASSWORD`, `SHINKI_EMU_IOS_PROVISIONING_PROFILE_BASE64`, and `SHINKI_EMU_IOS_ENTITLEMENTS_BASE64`. A missing secret fails the device release row before signing.

Ordinary CI builds the device Manager and all eight Framework executables, verifies their native closure, and records an unsigned build report; it receives no signing credentials and produces no installable IPA. The iOS release records require all eight signed Frameworks and the exact product commit. `SHINKI_EMU_MANAGER_FEATURES` adds comma-separated Cargo features during packaging; the `ohos-bundled-musica` HAP variant is for local QA, not release assets.

Pass an explicit product `--version` (SemVer) to package or release the Manager and in-tree cores. Omit it for external cores.

An external core's identity combines `upstream_identity`, the adapter repository and commit resolved from `Cargo.lock`, the `adapter_branch` attribution and a per-core `release_sequence`. Use the real upstream tag or branch and full upstream commit. The external version is `<tag|branch>-<ref slug>-<digest>`: lowercase ASCII alphanumerics in the first 16 ref bytes and replace other bytes with hyphens; hash the UTF-8 upstream repo/kind/ref/commit, adapter repo/branch/commit and decimal sequence with SHA-256, ending each field with LF. Release tags are `manager-v<semver>`, `core-<slug>-v<semver>` for in-tree cores and `core-<slug>-<version>` for external cores. For release tags, lowercase the core ID and replace dots and underscores with hyphens. Increment the sequence for a changed external pin or a new publication; the latest typed catalog entry is selected by sequence.

```sh
cargo xtask release --component rfvp --dry-run
cargo xtask release --component rfvp --target x86_64-pc-windows-msvc
```

`--dry-run` prints the version, tag, build targets and known per-target `publish_blocked` reasons without building a product. `--target` builds and prepares the asset and `release.json` under `.work/release/<tag>/<target>/`. `--publish` uses `GH_TOKEN` or `GITHUB_TOKEN` to publish staged assets and opens a catalog PR for a core. Publication checks the staged asset hash; the catalog carries the public asset's SHA-256 and size. Use the same `--scope windows-android` during planning, preparation and publication for a Windows/Android-only release; the default scope is `all`.

Each release record must name the product commit being published. Prepare the package again if the record is from a different commit or does not record a commit.

Before distribution, check `licenses/pinned-sources.txt` and the corresponding-source and relinking material for the binaries actually shipped. The standalone Ren’Py core remains publication-blocked until its [source and relinking requirements](../licenses/renpy/README.md) are fulfilled. The same restriction applies to any Manager APK that embeds Ren’Py. Local packaging remains available.

[The release workflow](../../.github/workflows/release.yml) prepares hosted assets. Dispatch it with `components` and a shared product `version` for Manager and any selected in-tree cores. Select `release_scope=windows-android` for a Windows/Android-only release, or `prepare_only=true` to stage without publishing.

For a prepare-only workflow, `source_commit` may specify the exact 40-character product commit. It is valid only with `prepare_only=true`. The workflow dispatch SHA is not the product SHA when this override is used.

For an all-platform Manager release, use one successful run with no `source_commit` override. Set `RUN_ID` to that run's ID; compare the two commit SHAs before downloading:

```sh
gh run view "$RUN_ID" --json headSha --jq .headSha
git rev-parse HEAD
gh run download "$RUN_ID" --pattern 'release-files-manager@*' --dir .work/release
```

All supported targets must have publishable packages. A debug-signed Android APK cannot stand in for the production APK.

The download creates one directory per artifact. Merge their `<tag>/<target>/` directories into `.work/release/<tag>/`. With DevEco, the signing directory and `cargo-about` configured, prepare the HAP and publish from that checkout. Set `VERSION` to the run's product version:

```sh
cargo xtask release --component manager --target aarch64-unknown-linux-ohos --version "$VERSION"
cargo xtask release --component manager --version "$VERSION" --scope all --publish
```

See [Metadata](metadata.md) for snapshot and catalog commands and [Core ABI](core-abi.md) for the native package format.

## Release history

Release notes: [beta.4](releases/manager-0.1.0-beta.4.md), [beta.5](releases/manager-0.1.0-beta.5.md), [beta.6](releases/manager-0.1.0-beta.6.md), [beta.8](releases/manager-0.1.0-beta.8.md).

**Historical release: Manager 0.1.0-beta.8.** Tag [`manager-v0.1.0-beta.8`](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v0.1.0-beta.8) points to source commit [`051a05bceff139e60ef6f973781ffa1f093fb2fc`](https://github.com/Project-Shinkirou/ShinkiEMU/commit/051a05bceff139e60ef6f973781ffa1f093fb2fc). Published on 2026-11-03 at 07:04:31 UTC, it has 19 public assets: nine platform packages, nine corresponding-source archives and the OHOS handoff record. The [exact-head CI](https://github.com/Project-Shinkirou/ShinkiEMU/actions/runs/41543961859) passed the full matrix, and the [OHOS handoff gate](https://github.com/Project-Shinkirou/ShinkiEMU/actions/runs/41576487998) passed.

| Platform | Actual beta.8 delivery |
| --- | --- |
| Windows x86_64, Linux x86_64, macOS aarch64/x86_64 | Manager desktop packages; install standalone cores separately. |
| Android | One universal release APK for aarch64 and x86_64, embedding 8 cores: RFVP, Siglus, Artemis, Musica, KiriKiri2, CMVS (new), Impacto and Ren'Py. |
| iOS | Three test packages: aarch64 device-test, aarch64 simulator-test and x86_64 simulator-test. Each embeds 6 cores: RFVP, Siglus, Musica, Sena, Ethornell and ReMinori. |
| OHOS aarch64 | Experimental debug-test-signed HAP with 1 embedded core, Musica; signed with the OpenHarmony SDK test PKI. Device/emulator launch and retail-device installability remain unverified. |

The Android release-certificate SHA256 remains `c415bd462d26574758ed455055ff528a3bd6036e39d5ac6887308f8cf69e97fc`. The release asset list and the source commit's [`build/cores.toml`](https://github.com/Project-Shinkirou/ShinkiEMU/blob/051a05bceff139e60ef6f973781ffa1f093fb2fc/build/cores.toml) describe the delivered targets and embedded inventories; Android's two ABI rows share the single APK. New Musica patches require Musica core `0.1.0-beta.5` or later. **Manager beta.7 and all earlier public tags and assets remain immutable.**

**Historical release: Manager [0.1.0-beta.7](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v0.1.0-beta.7).** It was published from `fd9c78a4cabf1d80430744e4bb5d140c2a837bb9` with all nine targets. Android embedded seven cores; the three iOS device/simulator test packages embedded six cores. OHOS used SDK debug-test-PKI signing. Its official Windows binary self-reports `0.1.0-beta.6`. Later source features and core pin updates do not change these historical downloads.

The [KiriKiri2 sequence2 release](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/core-kirikiroid2-web-branch-web-1e3603089b4951c8e2831d978ea6fe5a8097bceeb0a56fdeb65aa3953e67eecf) publishes Windows, Linux and both macOS architectures from product `fe4e6f2d82c8b61f3f14e13d48af6d57722b9952`, adapter `d72046c28e6ada623acdb1d72f7075edd488797d`. The same adapter is statically embedded in beta.7 Android; this does not add KiriKiri to iOS or OHOS. Corresponding sources and original licenses, including the separate FMOD terms, accompany the desktop packages.
The [Impacto sequence3 release](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/core-impacto-tag-0-10-8-930-96de7434c8b1a57df7ff24dcee829282194873fe596677056fa168485cb1555e) publishes Windows, Linux and both macOS architectures from product `5b197c1c656186d3a535bb3486311623516166e5`, with upstream `0.10.8.930` and adapter `34b89c29123d025d96f73ac9a746fb388e572065`. All four corresponding-source/native-input/license/package audits and unauthenticated downloads of all eight binary/source assets passed. This standalone sequence3 does not replace Manager6's embedded adapter `910a820ba3ea8d4dd8d6e19c560df1d5a3bc5157`/sequence2 or rewrite the published sequence1 assets; catalogs select sequence3 independently of publication completion order.
The [Ren’Py sequence1 release](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/core-renpy-tag-8-5-3-26051504-5f537560e1a1b852070dec65946d6c027e9f4f7482bddd8349ed47960c383133) publishes all four desktop platforms from product `05326439dcfafb81df79f3dabe0997fb696ad3f3`, upstream `8.5.3.26051504` and adapter `a52d9d9c006223c2b5a276c05e1b97ffeecbe66f`. Every actual corresponding-source archive passed the complete 389-entry pinned rebuild-recipe audit, including its 107-byte symbolic-link target. All eight public binary/source assets were downloaded without authentication and their hashes matched the accepted prepared records; original native license bytes were checked against the pinned fork and original inputs.
