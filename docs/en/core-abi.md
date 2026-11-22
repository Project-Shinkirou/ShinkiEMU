---
title: Core ABI and SDK
description: C ABI v3 lifecycle, owner threads, CPU leases and core packages.
---

# Core ABI and SDK

## Protocol and entry point

The native protocol is `shinki.emu.native.c.v3`, with numeric ABI version **3**. Dynamic cores export `shinki_emu_query`; static integration registers the same Query function shape. The Rust contract is `Source/FamilyApi/shinki-emu-abi`, and its generated C header is `include/shinki_emu.h`. C++ consumers use this header rather than maintaining another ABI definition. After changing the Rust types, regenerate the header with `cbindgen --config Source/FamilyApi/shinki-emu-abi/cbindgen.toml --crate shinki-emu-abi --output Source/FamilyApi/shinki-emu-abi/include/shinki_emu.h Source/FamilyApi/shinki-emu-abi` (cbindgen 0.29.2) and commit it; the CI tools job fails when the committed header differs from that output.

Only C-layout values cross the boundary: versioned headers, bounded borrowed spans, opaque handles, status codes and function pointers. No Rust-owned containers or unwinding cross the ABI. Libraries and immutable descriptor storage remain resident. Query accepts a version and an output table pointer; unsupported versions leave the output null. Query does no I/O, starts no workers and installs no logger.

## Directory recognition

`shinki_emu_manager_core::detection::detect_game_directory(&Path, &NativeCancellation)` returns `Result<Vec<EngineDetection>, DetectionError>`. A detection contains only `family_id` and `format`; it is independent of installed plugins and native `FamilyProbeCandidate` reports. One result per family is returned in family-ID order. Multiple families require caller selection; an empty vector means unrecognized. Recognition does not establish implementation compatibility or successful execution.

The detector reads files in the root, `Data`, `Data/pack`, `StartData`, `game` and `USRDIR`, matching directory names case-insensitively. It skips links and special files, limits enumeration to 4,096 entries/1,024 files, and limits physical reads to 16 MiB total and 4 MiB per opened file handle. Reopened parsing stages still share the total budget. Cancellation is checked between reads and parser stages; blocked filesystem calls cannot be interrupted. I/O, cancellation and read-limit failures return errors, never partial candidates or cacheable no-match results. Malformed/truncated format candidates are rejected while other files remain eligible. `detection::CACHE_VERSION` changes with recognition rules; caches must also track directory-content changes.
Select.ini's first valid tab-separated append entry is also resolved, without following links or escaping the selected root. Valid Musica keys enable bounded archive-alias discovery, including nested aliases; patch receipts supply confined profile/archive/loose-script paths. Consecutive PAZ `A`–`Z` volumes are read as one logical stream. These paths share the same fixed limits. Coverage is format-specific, not a claim that every input accepted by an engine can be uniquely identified without execution.

The registered families from `build/cores.toml` use these checks:

| Family ID | Resource checks | Coverage limits |
| --- | --- | --- |
| `fvp` | HCB/BCH system-descriptor pointer, entry point, title boundary and syscall records | Magic-less; filename suffix routes structural parsing, never establishes a match |
| `siglus` | Scene PCK/CHS headers/ranges, optional `pack_scn`, and the first valid Select.ini append (UTF-8/CP932) | Unsafe or missing active appends do not fall through to later entries; keys remain launch validation |
| `artemis` | PF2/6/8 complete bounded index with a `system.ini` entry and resource ranges | No project-less PF admission |
| `musica` | Valid keys.v2, decrypted PAZ0/1/2 index and SC structure; archive aliases, configured loose scripts, contiguous volumes | Correct keys and bounded complete resources are required; outside-root private profiles cannot be inferred from a directory-only request |
| `softpal` | Sv20 header/entry point, directly or in a validated PAC index; initial archive.dat profile paths, including manifests inside data.pac/csv.pac | Profile zero only, matching native bootstrap; no later-profile substitution or executable/appid whitelist; outside-root paths and non-PAC wrappers remain unidentified |
| `krkr` | XP3 index structure, or a literal Scripts.execStorage bootstrap whose actual target contains KiriKiri builtin calls outside comments/strings | Dynamic/computed loose TJS, ambiguous scripts, embedded-EXE XP3 and custom archives remain unidentified |
| `ethornell` | BURIKO ARC20 index, or PackFile index containing `ipl._bp`; resource ranges | Does not decompress bootstrap scripts or establish VM compatibility |
| `cmvs` | CPZ3/5/6/7 fixed headers and index ranges, or PS2A script segments | CPZ1/2 remain unknown; encrypted indexes/checksums/profile compatibility remain launch checks |
| `mages` | MPK v2 complete bounded index and resource ranges | MPK alone identifies the family, not Impacto's selected runtime profile |
| `renpy` | RPA2/3 header/index, RPC2 slots, or bounded legacy zlib-RPYC with a complete pickle opcode stream declaring a serialized renpy.ast object | Pickle is inspected as bytes, never imported/constructed/executed. Generic zlib/pickle, RPA1 and source-only layouts remain unidentified |

The rules were derived from pinned adapters and upstream parsers, not game-name examples. Musica's PAZ transforms/index parser and SC parser live in the pure `shinki-emu-musica-format` crate shared with playback; it has no VM, ABI, GPU or audio dependency. Manager scans and library/core lists now use the detector, generated `static_core_catalog`, and validated `core.json` identities. They never register or query native providers. The static function table is an entry index only, including optional Windows `static-cmvs`.

The deferred owner activates one selected plugin for explicit advanced configuration, diagnosis, or launch. Activation completion is asynchronous; diagnosis does not open a game. Launch obtains a fresh selected probe and retains the existing open-time recheck. Resident packages and pending updates still require restart; there is no automatic alternate-core fallback. Unknown plugins/bare libraries remain visible without discovery queries.

Advanced-configuration failures belong to the current game/plugin or selected defaults plugin, independently of global notifications. Dismissing a notice or viewing another game keeps the original error associated with its selection; retry, success, or choosing another implementation clears or changes that selection's reason.

Existing schema-7 game IDs, locations, family IDs and nullable `GameSettings.emulator_id` remain intact. Per-game family/format recognition and deferred receipt strings use the existing `app_state` store; no fake plugin ID or duplicate library record is introduced. Cache format 2 stores detector results, keyed by `detection::CACHE_VERSION` and bounded before/after resource stamps, independently of installed cores. Old caches miss; cancellation, I/O and read-limit errors never become no-match results or negative entries.

When a scan fails but its owner remains available, the user can retry recognition in the same application session. Existing records are kept; retry does not initialize cores. A failed owner or inventory startup still requires restart.

## Lifecycle

1. **query** obtains the immutable `CoreApi` table; validate the version, size and required function pointers.
2. **initialize** attaches diagnostics before ordinary core activity.
3. **describe** returns family/plugin identity, version, formats, typed configuration and capabilities. CPU surfaces are required. A nonzero render extent is legal only when the descriptor declares and bounds it.
4. **probe** examines a game directory. `NO_MATCH` is normal; probe strings borrow module storage until the next ordinary call, so the host copies needed metadata first.
5. **open** validates configuration and creates a native session. A failed open cleans up and returns a null session. Successful output includes session/context/output identities and surface metadata.
6. **input** enqueues owned, ordered input; **control** requests pause, resume or a retained retry-safe operation. Pointer coordinates refer to the displayed frame's raster pixels and identity.
7. **state** and **frame** read published state and surfaces. Polling never advances the VM. Failed sessions expose typed errors; retry is offered only when the core declares it safe.
8. **cancel** may run concurrently and must wake blocked native services. It must not wait behind the operation it is cancelling.
9. **close** consumes the handle on every outcome, joins workers and ends callbacks before returning. The host blocks new cancellation calls and waits for in-flight cancellation before closing.
10. **extension** negotiates a named, versioned C table. Unknown extensions return `UNSUPPORTED` with null output. `shinki.module_owner` provides an optional owner for real cross-session resources, created/destroyed on the module owner thread after sessions end.

Ordinary ABI calls are serialized on the host's owner thread. The core's native owner drives its own VM, render/audio scheduling and resource lifetimes. Manager supplies input, control and host services; there is no host advance callback. Cancellation and epoch checks reject late results from obsolete sessions or output generations.

The shared Rust diagnostic bridge forwards each original event once. If a host callback emits tracing events into the same statically linked subscriber, those nested events are not forwarded back through the ABI. A thread-local guard covers formatting and the callback, resets on return or unwind, and leaves other threads and subsequent events independent. This applies to every core using `shinki-emu-abi/diagnostic-bridge`, without changing log levels or worker stack budgets.

## Converted-game compatibility

The existing `Descriptor.capabilities` mask has four optional semantic bits; ABI
version 3, its C layout and fingerprint are unchanged:

- `CAP_NATIVE_SCRIPT_IDENTITY` (64): validate an explicit converted-script hash,
  isolate that script's saves/read state, and retain the existing namespace for
  markerless games.
- `CAP_CONFIGURED_FONT_PRIORITY` (128): a configured font takes priority over
  authored/restored primary face choices, including ruby and UI text.
- `CAP_NATIVE_EMBEDDED_TEXT` (256): native SC display operands decode literal
  Unicode scalar escapes, allowing offline-embedded translations without runtime tables.
- `CAP_NATIVE_NAME_RESOURCES` (512): native SC name constants, original live/history
  display rules and read-only EXE fallback. Yorino requires 256|512=768; its EXE is unchanged.

A Luau recipe can return `required_capabilities = 192` beside `plugin_id` and
`configuration` in its output profile. The receipt retains this requirement.
Manager preserves a recognized game and its receipt strings during native-free import.
Typed configuration and required capabilities are checked against only the selected
native descriptor on explicit advanced configuration or launch, before opening.
Automatic import still rejects a receipt changed after conversion.
Missing capabilities fail closed with a core-update diagnostic; unknown requirements
reject the profile rather than being ignored. Version strings and filenames
are not feature evidence. Legacy profiles omit the
field and require zero bits; zero is also omitted from new serialized receipts.

Musica embedding profiles require 256 (Yorino: 768); RFVP native GBK/font profiles require 192.
Both use this same descriptor/receipt check, with no version-string fallback.

## Text replacement levels

The descriptor capability bits are the source of truth: no `CAP_TEXT_HOOK` means **none**; `CAP_TEXT_HOOK` alone means **synchronous**; `CAP_TEXT_HOOK | CAP_ASYNC_TEXT` means **async-v1**. `CAP_ASYNC_TEXT` without the hook bit is invalid. The nine pinned external cores currently declare none; external cores may adopt synchronous replacement without changing their scheduling model. Ownership defines SDK cores: Musica is SDK-owned; Artemis remains external despite using SDK audio. CMVS is maintained separately.

ABI version remains **3**. `OpenRequestTextV1` embeds the unchanged base `OpenRequest` and appends a pointer to `AsyncTextServiceV1` (version 1 and explicit size). The host sets the base header size to cover the wrapper. Cores read the tail only when that size covers the wrapper, then validate the service version, size and callbacks. A null pointer disables replacement. Base-only external binaries keep using the unchanged `HostServices.text` synchronous callbacks. An async-v1 core never falls back to the synchronous path.

`submit` copies bounded text items before returning; `poll` returns `PENDING` immediately until completion. Only text advance waits: rendering, animations, input and audio keep running. Results retain the replacement semantics, item IDs and `(session_epoch, context_epoch, request_id)` identity. Request IDs increase within a context. Repeating an unacknowledged submit is idempotent. `CAPACITY` reports the required caller-owned buffers without executing the provider or extensions twice; completed output remains available until `acknowledge`. `cancel` retires only the matching request, while `invalidate` advances the context and discards old requests, source context and provider cache. Callbacks may not wait for network or script completion.

Manager uses the provider's `AsyncTranslationService` behind both paths; the synchronous compatibility callback waits for the same result. Musica submits once and polls with retained buffers from its native pump, without suspending audio. Enter, Space, primary click or Ctrl skips pending translation and displays the original replacement input; the input is consumed so it does not also advance that line. Skip mode also skips pending translation. Skip, load, rollback and close discard late completions; retries retain the failed text event and identity without rerunning the VM.


## CPU surface leases

A Ready `FrameSurface` carries actual dimensions, stride, pixel/color/row metadata, identity and one immutable CPU lease. Validate the metadata and epoch before adopting it. Pending and Unchanged frames do not carry pixel ownership. Preserve the declared raster/logical geometry and color interpretation.

Return each lease exactly once, only after the real upload API no longer reads its pixels. The lease's owner and release callback remain valid after session close and on another thread; release must not touch the VM, device, window, session or host services. Retaining a frame does not keep gameplay alive, nor permit overwriting leased pixels. The ABI's `CpuFrame` wrapper releases an adopted lease even when validation fails.

## SDK ownership

`Source/SDK/shinki-emu-sdk` contains shared mechanisms, not a second game runtime. Its `runtime` values stay inside the process. `native` shares configuration validation, epochs, cancellation, window-result rejection and independent CPU leases; `native_input` copies bounded input without running the VM. `NativeAudioOutput`/`NativeAudioConverter` serve core-owned device threads; `NativeAudioOutput::progress()` reports accepted and audible frames on the 48 kHz source timeline, net of device and resampler latency, so cores can drive their media clocks. `progress_reader()` returns a cloneable `NativeAudioProgressReader` that reads the same timeline from other threads while the device thread is blocked or idle. `write` accepts any length and carries a partial device block; call `flush()` before the device thread idles so every accepted frame becomes audible (the silence padding counts as accepted). Mixing, media clocks, native save formats and game rules remain with each core. Use the [SDK reference](https://github.com/Project-Shinkirou/ShinkiEMU/tree/main/Source/SDK/shinki-emu-sdk) for concrete APIs.

## Core package

A package contains a directory named after the library stem:

```text
<library-stem>/
  core.json
  shinki_emu_<core>.dll       # or libshinki_emu_<core>.so/.dylib
  build.json
  Resources/                 # when the native core requires it
  licenses/
  <native dependency files>
```

`core.json` uses `shinki.emu.core_package.v1` and records `plugin_id`, `library`, `version`, `abi` and file paths/SHA256 hashes. The file inventory includes every payload file except the manifest itself. Libraries require the new filename prefix and query symbol. Packaging validates architecture, dependency closure and licenses, rejects path traversal/links/case collisions, and publishes only to a new output directory. Manager verifies the package before installation; older product protocols and loader aliases are not accepted. See [Build and release](build-and-release.md).

New external packages add `identity` with original upstream and organization adapter provenance; `version` must equal its computed safe identifier. Legacy packages omit it and retain their actual SemVer. New Manager checks catalog/package plugin, version and identity together before staging, and displays the complete upstream/adapter label. The C ABI and descriptor version remain independent. See [version identity rules](build-and-release.md#package-and-release).
