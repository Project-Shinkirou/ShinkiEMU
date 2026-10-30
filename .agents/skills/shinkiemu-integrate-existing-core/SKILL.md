---
name: shinkiemu-integrate-existing-core
description: Use when bringing an existing third-party engine implementation (a Rust crate or a C/C++ project) into ShinkiEMU as an external core, or when changing an existing fork adapter, its pin, its Cargo.lock entries, its save/config behaviour or its license bundle. Covers the Project-Shinkirou fork with a single `shinki` branch, a thin adapter crate or vendored C header, the worker/frame-lease mapping, vcpkg deps, sources zip, publish_blocked and upstream PRs. Not for a new in-repo core; use shinkiemu-new-core for that.
---

# Bring an existing engine into ShinkiEMU

An external core is the upstream engine left as it is, plus a thin adapter that implements C ABI v3. Both live in an organization fork. ShinkiEMU pins the fork by exact commit and does not copy the adapter.

Contract docs, which you link to rather than copy: [Core ABI](../../../docs/en/core-abi.md), [Build and release](../../../docs/en/build-and-release.md), [ThirdParty/README](../../../ThirdParty/README.md), [THIRD_PARTY_NOTICES](../../../THIRD_PARTY_NOTICES.md), [ORIGINS](../../../ORIGINS.md). The SDK runtime model (worker, frames, lifecycle, boundary) is described in [shinkiemu-new-core](../shinkiemu-new-core/SKILL.md).

## Worked examples

| Core | Kind | Adapter | Static query in the Manager |
|---|---|---|---|
| siglus_rs | Rust | `shinki/shinki-emu-siglus_rs` (with a `[package.metadata.shinki]` provenance block) | `shinki_emu_siglus_rs::query` |
| rfvp | Rust | `crates/shinki-emu-rfvp`; `src/lib.rs:496-529`: `CoreApi` + `cfg_attr(feature = "dynamic-plugin-export", no_mangle)` | `shinki_emu_rfvp::shinki_emu_query` |
| ReMinori | Rust, AGPL | `crates/shinki` (`lib.rs:965-1000`, `build.rs`, `MODIFICATIONS.md`) | `shinki_emu_ReMinori::shinki_emu_query` |
| sena-rs | Rust | `crates/shinki-emu-sena-rs`; `lib.rs:41-53`: `cfg_attr(feature = "diagnostic-bridge", no_mangle)` + `pub const QUERY`; worker in `session.rs:131-191` | `shinki_emu_sena_rs::QUERY` |
| ethornell-rs | Rust | `crates/shinki-emu-ethornell-rs` (`lib.rs:1282-1317`) | `shinki_emu_ethornell_rs::QUERY` |
| art3m1s-core | Rust + Luau | `crates/shinki-emu-art3m1s-core`, feature `static-core` | `shinki_emu_art3m1s_core::artemis_query` |
| impacto | C++ | `shinki/` Cargo wrapper + `src/shinki/core.cpp`; vendored `vendor/shinki-emu-abi/include/shinki_emu.h` | (not embedded) |
| kirikiroid2-web | C++ | native shared library with the vendored header | (not embedded) |
| renpy | C++ + Python | `shinki/core.cpp` + `shinki_renpy_bridge.py`; embeds the official Ren'Py SDK | `shinki_emu_renpy::QUERY` |

All paths are inside the fork checkout at `ThirdParty/<dir>/`, which `cargo xtask deps --core <id>` populates at the pinned revision.

## 1. Fork setup

1. Create `Project-Shinkirou/<upstream-repo-name>` as a fork. Keep the upstream repository name and case for the plugin_id (`siglus_rs`, `ReMinori`, `sena-rs`). Keep upstream's history, authors and default branch.
2. The fork has exactly one integration branch, `shinki`. Put feature work on `shinki-<topic>` and land it on `shinki` by fast-forward. Never rewrite `shinki`.
3. Add `upstream` as a remote and branch `shinki` from the upstream commit you build on. Record that base commit in the fork's modification notice (step 7).
4. External cores are **not** submodules. Pin them in `build/cores.toml`:

```toml
[cores.source]
repo = "https://github.com/Project-Shinkirou/<repo>"
branch = "shinki"
rev = "<full 40-hex lowercase commit on shinki>"
upstream = "https://github.com/<owner>/<repo>"
```

`CoreSource::validate` (`xtask/src/cores.rs:58-95`) accepts only organization repos, a 40-hex lowercase `rev` and a GitHub `upstream`. The `manifest` must be under `ThirdParty/`, and only `ThirdParty/` manifests may have a source (`cores.rs:190-195`). The only real submodule in ShinkiEMU is `ThirdParty/cpal`.

## 2. Adapter shape

**Rust upstream: a thin adapter crate inside the fork.**
- Add one crate (e.g. `crates/shinki-emu-<id>` or `crates/shinki`) with `crate-type = ["rlib", "cdylib"]`. Add `"staticlib"` only if a native host needs it, as sena-rs does.
- Depend on `shinki-emu-abi` (and, if you use it, `shinki-emu-sdk`) by git on `https://github.com/Project-Shinkirou/ShinkiEMU` at a full `rev`. Do the same for `shinki-emu-build` under `[build-dependencies]`. Inside ShinkiEMU the root `Cargo.toml` `[patch."https://github.com/Project-Shinkirou/ShinkiEMU"]` (lines 176-179) redirects these to the in-tree crates, so the fork's pin only matters for standalone fork builds. Keep the pin on a real ShinkiEMU commit anyway.
- `build.rs` calls `CoreBuildManifest::new("<plugin_id>", "rust-cdylib")` and lists the fork's `LICENSE` and modification notice under `licenses/<plugin_id>/`. ReMinori's `crates/shinki/build.rs` is the template. Assert supported targets there, so unsupported triples fail early with a clear message.

**C/C++ upstream: a vendored header and a native shared library.**
- Copy `Source/FamilyApi/shinki-emu-abi/include/shinki_emu.h` into the fork, e.g. `vendor/shinki-emu-abi/include/shinki_emu.h` as impacto does, and implement the `CoreApi` in C++ (`src/shinki/`).
- A small Cargo package (`shinki/Cargo.toml` + `build.rs`) drives CMake (impacto: `configure_cmake` with the vcpkg toolchain) and writes `core-build.json` with `artifact_kind = "native-shared"` and an absolute `native_library`.
- **Re-copy the header whenever you bump the ShinkiEMU ABI pin.** At 151a79c the vendored copies in impacto, renpy and kirikiroid2-web match ShinkiEMU `9cd899ba`. They lack the later `CAP_ASYNC_TEXT`/`AsyncTextServiceV1` additions. Those additions are only additive, but a stale header hides new capabilities.

**Query exports: dynamic vs static.**
- Desktop loads `shinki_emu_query` by its unmangled name. Mobile Managers link the `rlib` and call a Rust path, so two cores that both export an unmangled `shinki_emu_query` would collide in one binary.
- Gate the unmangled export behind a feature that only the dynamic build enables. rfvp and ReMinori use `#[cfg_attr(feature = "dynamic-plugin-export", unsafe(no_mangle))]`; sena-rs uses `diagnostic-bridge`. Put that feature in `cores.toml` `features`, and leave it out of `static_features`.
- Expose a Rust-path entry for `STATIC_CORES`: either the function itself (`shinki_emu_rfvp::shinki_emu_query`), a `pub const QUERY: Query = shinki_emu_query;` (sena-rs `lib.rs:53`, ethornell-rs `lib.rs:1317`), or a named function (`artemis_query`, `siglus_rs::query`).
- `query` writes null first, returns `UNSUPPORTED` for any version other than `ABI_VERSION`, and never creates native state.

## 3. Mapping the upstream main loop

Most engines own a `loop { poll events; update; render; present }` and a window. The adapter turns that into a session:
- **One worker thread per session** runs the upstream loop. `open` spawns it and waits for a ready signal with a bounded `recv_timeout` that checks cancellation on every step. sena-rs `session.rs:131-191` is the template: a `sync_channel(1)` ready handshake; `Disconnected` joins the worker and returns its recorded error; cancellation returns `CANCELLED`. In C++, impacto runs the loop in its worker `Run` at `src/shinki/core.cpp:459`.
- **Replace the window with offscreen output.** Render to an offscreen target, read it back into immutable CPU buffers, and publish them with an incremented generation. ReMinori keeps three immutable slots, so held leases never stop VM or render progress. `frame` hands out a `CpuLease` (impacto `GetFrame` at `core.cpp:905`; rfvp `lib.rs:443-455`).
- **Input/control/state** become commands queued to the worker. Pause stops the upstream clock; it does not block the thread.
- **Panics and exceptions** never cross the ABI. Wrap every export and the worker body in `catch_unwind` (Rust) or `try/catch` (C++), turn the failure into `SESSION_FAILED` with a message, and still let `close` join and release everything.
- **Windows/modals** go through the host's window/modal services. They must never be opened directly by the engine.
- Keep upstream's VM, scheduling, composition, audio mixer, menus and save code. The adapter only translates. If upstream needs a hook (an offscreen target, a cancellable wait), add the smallest general hook and offer it upstream (step 9).

## 4. Saves, keys and config: minimal adaptation

- External engines keep the upstream project's own save, key and config behaviour, locations and formats, AppData/Documents included. Never redirect them into `.shinki`. Only our own engines (CMVS, Musica) use `<game>/.shinki/<core>/`.
- Every file that ShinkiEMU itself (the adapter) writes in a game directory goes under `<game>/.shinki/`.
- Don't add managed save roots, game-directory overlap guards, required `save_root`/`save_directory` fields or invented AppData namespaces. ReMinori removed its adapter-only `save_directory` and now uses upstream's `userdata/` resolution, passing the game directory in place of the executable directory (`ReMinori/MODIFICATIONS.md`, 2026-10-29).
- Before you finish, diff the save/key/config paths against upstream at the fork base. Give a reason for every remaining difference in the modification notice.
- Removing a config field is a migration. Stored values for unknown field ids fail launch, so the Manager prunes retired ids. Migrate old adapter-only paths once, without clobbering, then read only the upstream path.

## 5. Native dependencies

- Declare native inputs in `cores.toml` `dependencies` using existing `deps` recipes. The recipe names are parsed in `xtask/src/dependencies.rs:64-76`: `subtitle`, `ffmpeg`, `dxc`, `native-vcpkg`, `impacto-sources`, `renpy`, `bison`, `gstreamer`. The vcpkg manifests and triplets are under `build/vcpkg/`.
- `cargo xtask deps --target <triple> --core <id>` fetches the fork at its pin and prepares locked native inputs. `build.rs` only consumes and validates them; it never downloads anything.
- Add a new recipe only when an existing one can't express the dependency. Pin its source exactly and route its license into the package.

## 6. Register and embed

- Add a `[[cores]]` entry with `manifest = "ThirdParty/<dir>/<adapter>/Cargo.toml"`, `[cores.source]`, `licenses = ["ThirdParty/<dir>/LICENSE", ...]`, and one supported or blocked entry per triple in `build/targets.toml`. See [shinkiemu-new-core §5](../shinkiemu-new-core/SKILL.md#5-register-the-core) for the field rules.
- For Android/iOS static embedding, set `static_manager_targets` and `static_features`. Add the fork under the matching `[target.'cfg(target_os = "…")'.dependencies]` in `Source/Manager/shinki-emu-manager/Cargo.toml` with `git` = `[cores.source].repo`, `rev` = the same full SHA, and `default-features = false` (lines 86-101). Then add the Rust-path query to `STATIC_CORES` in `Source/Manager/shinki-emu-manager/src/manager_app/owner_worker.rs:268-287`. `validate_static_dependencies` (`xtask/src/cores.rs:228-298`) fails when the repo, rev or features differ from the inventory.
- A statically linked core shares the Manager's dependency graph. It must resolve to the same kira (patched to `Project-Shinkirou/kira` in the root `[patch.crates-io]`, `Cargo.toml:181-185`), the same mlua/mlua-sys, cpal and wgpu majors. Two versions of a `links =` crate fail resolution. Align the fork rather than adding another patch. Artemis uses Luau on every platform, including Android static embedding (`ThirdParty/README.md`).

## 7. Licenses and the sources bundle

- Keep upstream's `LICENSE`, attribution and notices. Add a modification notice to the fork (`MODIFICATIONS.md` in ReMinori). It records the upstream URL, the base revision, what the adapter changed and why, and where the corresponding source is published.
- List every license file in both `build.rs` (`licenses/<plugin_id>/…`) and `cores.toml` `licenses`, and add the core to `THIRD_PARTY_NOTICES.md` and `ORIGINS.md`.
- Each release ships `shinki-emu-<slug>-<version>-<platform>-sources.zip`. It holds the product tree at the release commit and the fork at its exact pin, plus `rust-sources/` for crates whose licenses require source distribution ([Build and release](../../../docs/en/build-and-release.md#package-and-release)). Only committed content enters it.
- Set `publish_blocked = "<reason>"` until every license and notice obligation is met. Blocked cores are never released.
- Never commit commercial game data, text, keys, private profiles or saves. Tests use synthetic fixtures.

## 8. Pin bump and Cargo.lock discipline

1. Land the fork change on `shinki` by fast-forward and take its full SHA.
2. Replace the old SHA with the new one as text in `build/cores.toml` and, for static cores, in the Manager `Cargo.toml`. In `Cargo.lock`, change only the fork's `source = "git+…?rev=<old>#<old>"` lines, the same way.
3. Never run `cargo update -p <crate>` or a bare `cargo update` for a pin bump. Both drift unrelated registry versions (wgpu-hal's `windows` version has broken D3D12 builds this way).
4. Accept the lock only when `git diff -- Cargo.lock` shows just the fork's rev lines, and `cargo metadata --locked` succeeds.
5. If the fork pins RoukakuEngine directly (rfvp: `roukaku-subtitle`), bump it to the same RoukakuEngine rev as ShinkiEMU in the same change. Otherwise a duplicate `links` crate fails `--locked` resolution.

## 9. Upstreaming

- Fixes that are not specific to ShinkiEMU (VM bugs, format edge cases, portability) go upstream as PRs from a `upstream/<topic>` branch based on `upstream/main`. Include a synthetic test and run upstream's own checks.
- Keep adapter-only changes (ABI glue, offscreen output, packaging) in the fork, and list them in the modification notice.
- Once an upstream PR lands, drop the fork's copy when the fork next rebases onto upstream.

## Release

Tag `core-<slug>-v<semver>`; the slug is the lowercased plugin_id with `.`/`_` → `-`, so `ReMinori` → `reminori`. Remote GitHub CI is the gate and does the publishing, including the catalog PR under `Metadata/engines/cores/`. Local runners are only an optional pre-check.

## Finish checklist

- [ ] The fork has a single `shinki` branch with upstream history; the `upstream` remote is set; the base commit is recorded.
- [ ] `[cores.source]` has the full SHA; static Manager deps carry the same SHA and exact `static_features`; `STATIC_CORES` is updated.
- [ ] The adapter keeps the upstream VM, render, audio and saves; only the ABI glue and minimal hooks are added.
- [ ] The unmangled `shinki_emu_query` is feature-gated for dynamic builds; a Rust-path query exists for static builds.
- [ ] C/C++: the vendored `shinki_emu.h` matches the pinned ShinkiEMU ABI.
- [ ] Panics/exceptions are contained; `open` is cancellable; `close` joins the worker; held leases never stall progress.
- [ ] Save/key/config paths match upstream; any remaining differences are justified in the modification notice.
- [ ] Licenses are listed in `build.rs`, `cores.toml`, `THIRD_PARTY_NOTICES.md` and `ORIGINS.md`; `publish_blocked` is cleared only when they are met.
- [ ] The `Cargo.lock` diff touches only the fork rev lines; `cargo metadata --locked` passes.
- [ ] `docs/{zh-cn,en,ja}/cores/<slug>.md` link the upstream project; the README engine tables are updated.
- [ ] General fixes are offered upstream as PRs.
