---
name: shinkiemu-new-core
description: Use when writing a new ShinkiEMU core from scratch on the in-repo SDK (a crate under Source/Families/ implementing C ABI v3), covering crate layout, the query/CoreApi table, worker and frame leases, core-build.json, build/cores.toml registration, mobile static embedding, tests, docs pages, the catalog and release tags. Not for wrapping an existing third-party engine; use shinkiemu-integrate-existing-core for that.
---

# Write a new ShinkiEMU core on the SDK

The reference core is CMVS (`Source/Families/shinki-emu-cmvs`). Musica (`Source/Families/shinki-emu-musica`) is the second in-repo example. Copy their structure. Don't invent a second convention.

Contract docs, which you link to rather than copy: [Core ABI](../../../docs/en/core-abi.md), [Build and release](../../../docs/en/build-and-release.md), [Metadata](../../../docs/en/metadata.md), [SDK README](../../../Source/SDK/shinki-emu-sdk/README.md).

## 1. Names

- **plugin_id**: the stable identity used in `Descriptor.plugin_id`, `core-build.json`, `build/cores.toml` `id`, the catalog and saves. In-repo cores use `shinki.emu.<family>` (`shinki.emu.cmvs`). The allowed characters are `[A-Za-z0-9._-]`, at most 128 bytes, enforced by `CoreBuildManifest::validate` (`Source/Tools/shinki-emu-build/src/lib.rs:52-62`). Never rename it after release.
- **slug**: derived, never written by hand. Lowercase the plugin_id and replace `.`/`_` with `-` (`shinki.emu.cmvs` → `shinki-emu-cmvs`). It names the docs page, the release tag `core-<slug>-v<semver>` and the release archives. xtask rejects two cores with the same slug (`xtask/src/cores.rs:185`).
- **Crate**: `Source/Families/shinki-emu-<family>`, package `shinki-emu-<family>`. Add it to the root `Cargo.toml` `[workspace] members`.
- **family_id**: the engine family string in the descriptor (`"cmvs"`). Several cores may share one family; `ReMinori` and `shinki.emu.musica` are both `musica`.

## 2. Crate skeleton

`Cargo.toml`, as in `Source/Families/shinki-emu-cmvs/Cargo.toml`:

```toml
[package]
name = "shinki-emu-<family>"
version.workspace = true
edition.workspace = true
license.workspace = true
build = "build.rs"

[lib]
crate-type = ["rlib", "cdylib"]   # cdylib = desktop core; rlib = static Manager link

[dependencies]
shinki-emu-abi = { path = "../../FamilyApi/shinki-emu-abi", features = ["diagnostic-bridge"] }
shinki-emu-sdk = { path = "../../SDK/shinki-emu-sdk", default-features = false, features = ["runtime", "audio", "device-audio" /* only what you use */] }

[build-dependencies]
shinki-emu-build = { path = "../../Tools/shinki-emu-build" }
```

`build.rs`, as in `Source/Families/shinki-emu-cmvs/build.rs`:

```rust
use shinki_emu_build::{BuildAsset, CoreBuildManifest, write_core_build_manifest};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for input in ["HOST", "TARGET", "PROFILE"] {
        println!("cargo:rerun-if-env-changed={input}");
    }
    println!("cargo:rerun-if-changed=src");
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR")?)
        .join("../../..").canonicalize()?;
    let mut manifest = CoreBuildManifest::new("shinki.emu.<family>", "rust-cdylib")?;
    for file in ["LICENSE-MIT", "LICENSE-APACHE"] {
        manifest.licenses.push(BuildAsset { source: root.join(file), dest: format!("licenses/shinki.emu.<family>/{file}") });
    }
    write_core_build_manifest(&manifest)?;
    Ok(())
}
```

Rules that `CoreBuildManifest::validate` enforces (`shinki-emu-build/src/lib.rs:50-97`):
- `rust-cdylib` has no `native_library`; `native-shared` requires an absolute, existing `native_library`.
- Asset sources are absolute and exist. Destinations start with `Resources/` or `licenses/` and contain no `\`, `:`, `.` or `..` segments.
- Every license file you ship must also be listed in `build/cores.toml` `licenses`.

`build.rs` validates inputs and writes `core-build.json` into `OUT_DIR`. It never runs Cargo recursively and never publishes anything. xtask reads Cargo's JSON output and never guesses target paths.

## 3. The C ABI entry: `src/native.rs`

Read these CMVS locations in order:

| What | Where |
|---|---|
| Panic/error boundary around every export | `Source/Families/shinki-emu-cmvs/src/native.rs:28-50` (`boundary`) |
| Static `CoreApi` table with a `Header` | `native.rs:52-65` |
| Rust-path query for static registration, and the unmangled `shinki_emu_query` for dynamic loading | `native.rs:69-86` |
| `initialize`, `describe`, `probe` (probe joins its worker before returning) | `native.rs:87-202` |
| `open`: validate the request, spawn the worker, wait for the ready handshake | `native.rs:203-332` |
| `input`, `control`, `state`: queue commands, never block on the VM | `native.rs:333-412` |
| `frame`: `shinki_emu_sdk::native::lease_cpu` | `native.rs:413-461` |
| `cancel` (async-signal-cheap flag) and `close` (joins the worker) | `native.rs:462-487` |
| Descriptor, capabilities and config schema | `src/native/config.rs:14,61-83` |
| Config entries decoded from the `OpenRequest` | `src/native/config.rs:133-136` (`shinki_emu_sdk::native::config_entries`) |
| Worker pump | `src/native/worker.rs:143-243` |

The minimum shape:

```rust
static API: abi::CoreApi = abi::CoreApi {
    header: abi::Header::of::<abi::CoreApi>(),
    initialize: Some(initialize), describe: Some(describe), probe: Some(probe),
    open: Some(open), input: Some(input), control: Some(control), state: Some(state),
    frame: Some(frame), cancel: Some(cancel), close: Some(close), extension: Some(extension),
};

/// Static Manager registration calls this through its Rust path.
pub unsafe extern "C" fn <family>_query(version: u32, out: *mut *const abi::CoreApi) -> abi::Status {
    if out.is_null() { return abi::INVALID_ARGUMENT; }
    unsafe { out.write(std::ptr::null()) };
    if version != abi::ABI_VERSION { return abi::UNSUPPORTED; }
    unsafe { out.write(&API) };
    abi::OK
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shinki_emu_query(version: u32, out: *mut *const abi::CoreApi) -> abi::Status {
    unsafe { <family>_query(version, out) }
}
```

`query` never creates native state. Every export runs inside `boundary` so a panic becomes `PANICKED` plus an `Error`, and never unwinds across `extern "C"`.

## 4. Runtime model

- **Worker owns the engine.** `open` spawns one thread that owns the VM, the renderer and audio. API calls send `Command`s over a channel. The CMVS pump (`worker.rs:153-241`) does `recv_timeout(8ms)`, applies control/input, advances by the measured elapsed time (0 while paused), and publishes a frame.
- **Frames.** Publish an immutable pixel buffer under a mutex and bump `generation` only when the buffer changes (`worker.rs:226-237`). `frame` returns `FRAME_PENDING` before the first frame and `FRAME_UNCHANGED` when the caller's identity matches. Otherwise it returns a CPU lease from `lease_cpu` that keeps the buffer alive until the host releases it. A held lease must never stall the VM.
- **Lifecycle.** States are `RUNNING`/`PAUSED`/`FINISHED`/`FAILED` (`worker.rs:172-225`). `cancel` only sets the flag (`ServiceContext::is_cancelled`). `close` joins the worker and releases every media/audio owner even after a failure. The worker calls `services.invalidate()` before cleanup (`worker.rs:122-124`).
- **Window/modal requests** go through `shinki_emu_sdk::native::window_request` (`worker.rs:210`). Never open your own OS window from a core.
- **Audio** goes through the SDK `audio`/`device-audio` features (kira). Don't add a second audio stack.
- **Config.** Declare fields once in your family descriptor. CMVS builds the ABI schema from `crate::family::cmvs_descriptor().configuration` (`config.rs:16`). The Manager rejects stored values for unknown field ids, so removing a field is a migration. See the save-locations policy below.
- **Diagnostics.** Enable `shinki-emu-abi/diagnostic-bridge` and log through `tracing` with stable `event = "shinki.emu.<family>.<event>"` names.
- **Saves.** For own engines, write the original engine's save files to their original location and byte format, so the original executable can read them. Anything the original keeps in the registry, AppData or Documents goes under `<game>/.shinki/<plugin-or-family>/` (CMVS: `<game>/.shinki/cmvs/registry.json`). Every other file ShinkiEMU writes inside a game directory also goes under `<game>/.shinki/` ([Build and release](../../../docs/en/build-and-release.md#game-local-on-disk-layout)).

## 5. Register the core

Add a `[[cores]]` entry to `build/cores.toml`, modelled on `shinki.emu.cmvs`:

```toml
[[cores]]
id = "shinki.emu.<family>"
display_name = "Shinki <Family>"
family_name = "<Family>"
license = "MIT OR Apache-2.0"
manifest = "Source/Families/shinki-emu-<family>/Cargo.toml"
package = "shinki-emu-<family>"
features = []
dependencies = []            # deps recipes such as "ffmpeg"; empty for pure Rust
artifact_kind = "rust-cdylib"
resources = []
licenses = ["LICENSE-MIT", "LICENSE-APACHE"]
supported_targets = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"]
publish_blocked = ""
[[cores.blocked]]
target = "aarch64-linux-android"
reason = "<concrete reason>"
# ...one [[cores.blocked]] for every other triple in build/targets.toml
```

The inventory loader (`xtask/src/cores.rs:169-227`) enforces these rules:
- Every triple in `build/targets.toml` appears exactly once, in `supported_targets` or in `[[cores.blocked]]` with a non-empty reason.
- In-repo cores have no `[cores.source]`; only `ThirdParty/` manifests have one.
- `publish_blocked` is a non-empty reason while license or notice obligations are unmet. That core is never released.

Don't edit `build/targets.toml` for a new core. A target-level `blocked` reason overrides every core.

## 6. Static embedding for Android/iOS

Mobile Managers don't load downloaded libraries. To embed:
1. Add the triples to `static_manager_targets` (Android/iOS only; validated at `cores.rs:216-223`) and list the Cargo features in `static_features`.
2. Add the crate under `[target.'cfg(target_os = "android")'.dependencies]` (and/or `ios`) in `Source/Manager/shinki-emu-manager/Cargo.toml`, with `default-features = false` and exactly `static_features`. In-repo cores use a `path` dependency; Musica is the example at `Cargo.toml:96`. `validate_static_dependencies` (`cores.rs:228-298`) fails on any mismatch in the path, pin or features.
3. Register the Rust-path query in `STATIC_CORES` (`Source/Manager/shinki-emu-manager/src/manager_app/owner_worker.rs:268-287`), for example `("shinki.emu.musica", shinki_emu_musica::musica_query)`.

Packaging writes `embedded-cores.json` and the embedded core's licenses into the Manager package.

## 7. Tests

- Put unit and behaviour tests next to the code (`src/native/tests.rs` in CMVS). Run them with `cargo xtask test --core shinki.emu.<family>`.
- Cover consumer-visible behaviour: format and budget boundaries, corrupt or oversized input, save round-trips and integrity, cancellation during open, pause/resume, stale frame identities, lease lifetime beyond the worker, and worker shutdown after a failure.
- Use synthetic fixtures generated in the test or committed as small, self-made files. Never commit commercial game data, text, keys or saves.
- Don't add tests that pin wording, constants, path spelling or forwarding.

## 8. Build, package, run locally

```sh
cargo xtask doctor --target x86_64-pc-windows-msvc --core shinki.emu.<family>
cargo xtask deps   --target x86_64-pc-windows-msvc --core shinki.emu.<family>
cargo xtask build  --target x86_64-pc-windows-msvc --core shinki.emu.<family> --release
cargo xtask check  --target x86_64-pc-windows-msvc --core shinki.emu.<family>
cargo xtask package --target x86_64-pc-windows-msvc --core shinki.emu.<family> --version 0.1.0-beta.1 --out .work/packages/<slug>
cargo xtask matrix
```

Packaging fails on a missing `shinki_emu_query` export, missing licenses, a wrong architecture or unresolved native dependencies. To run a package, install it into a desktop Manager built with `cargo xtask build --target <triple> --manager --release`.

## 9. Docs, catalog, release

- Add `docs/{zh-cn,en,ja}/cores/<slug>.md`, following an existing page such as `docs/en/cores/shinki-emu-cmvs.md`. Link it from each locale's `index.md` and add a row to the README engine tables (`README.md`, `README.zh-CN.md`, `README.ja.md`).
- Release tag: `core-<slug>-v<semver>`. Pushing it starts `.github/workflows/release.yml`. Remote GitHub CI is the gate and does the publishing; local runners are only an optional pre-check. Publishing opens the catalog PR under `Metadata/engines/cores/<plugin_id>/<platform>/<version>.json` itself. Don't hand-write catalog entries for unpublished builds.
- Run `cargo xtask metadata validate` after any `Metadata/` change.

## Finish checklist

- [ ] The plugin_id is the same in the descriptor, `build.rs` and `cores.toml`; the slug is unique.
- [ ] Every export is wrapped in `boundary`; `query` creates no state; `close` joins the worker.
- [ ] Frames bump `generation` only on change; held leases never block the VM.
- [ ] Game-directory writes go under `<game>/.shinki/`, apart from original-format saves.
- [ ] The `cores.toml` entry covers every target; the licenses match `build.rs`.
- [ ] Static embedding (if any) matches across `cores.toml`, the Manager `Cargo.toml` and `STATIC_CORES`.
- [ ] Behaviour tests on synthetic fixtures pass under `cargo xtask test --core <id>`.
- [ ] Docs pages exist in 3 locales; README tables are updated; `cargo xtask metadata validate` passes.
