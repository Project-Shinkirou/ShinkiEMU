---
title: Metadata & Patches
description: Submit game compatibility reports and Luau patches, and understand maintainer review and snapshot builds.
---

# Metadata & Patches

`Metadata/` is the single source of truth shared by the official portal and ShinkiEMU Manager at `https://project-shinkirou.github.io/data/shinkiemu/v1/`. Community contributors submit compatibility reports and Luau patches through structured GitHub issue forms.

## 1. Submitting a Compatibility Report

Open the [Compatibility Report](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml) form and record your observed gameplay results:

1. **Find VNDB IDs**: Copy the work `vID` (e.g. `v17`) and exact release `rID` (e.g. `r1234`) from the VNDB URLs.
2. **Record exact versions & target**: Check Manager version (**Settings → About**), exact case-sensitive core ID and core version (**Cores**), plus OS platform, CPU architecture (`x86_64` / `aarch64`), and variant (`desktop` on PC; `device` or `simulator` on mobile).
3. **Pick a status**:

| Status | Meaning |
|---|---|
| `perfect` | Video, audio, scripts, and save/load work smoothly throughout |
| `completable` | Playable from start to finish with minor glitches |
| `flawed` | Playable in part, with major bugs affecting gameplay |
| `boot_only` | Reaches title or menus only; not playable further |
| `unplayable` | Fails to launch or crashes immediately |

> [!CAUTION]
> Never attach commercial game assets, game text, decryption keys, or saves. Successful builds or engine detection alone do not count as compatibility reports.

Each report is keyed by `(release_id, emulator, platform, architecture, variant)` and stored at `Metadata/compatibility/entries/<vid>/<rid>/<plugin_id>/<platform>-<architecture>-<variant>.json`.

## 2. Authoring & Submitting Luau Patches

Use the [Luau Patch Submission](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml) form (see [shinkiemu-patch](../../.agents/skills/shinkiemu-patch/SKILL.md) for recipe details).

### Required Elements

- **Identity & metadata**: Stable ASCII `id`, SemVer `version`, `title` and `summary` (required `zh-CN` and `en`, optional `ja`), exact core IDs, source URL, license, and authorization link where applicable.
- **Release bindings (`releases`)**: Concrete VNDB `release_id`, `vn_id`, edition title, `edition`, `game_version`, and source file fingerprints (`path`, `size`, `sha256`). Manager verifies fingerprints before starting any writable transaction.
- **Explicit downloads (`downloads`)**: Every `ctx.download(url, sha256, size)` call must be declared with an HTTPS URL, 64-character lowercase SHA-256, and byte size (1 B – 1 GiB). Empty `downloads` forbids network downloads. Redirects must remain HTTPS; files are cached in `<game>/.shinki/patch/` and re-verified on reuse.
- **Sandbox rules**: Scripts run in the shared host sandbox — no process spawning, arbitrary networking, path escapes, symlinks, or EXE creation/modification. Submission checks validate fields and Luau syntax without executing scripts or fetching URLs.

> [!NOTE]
> Naming a license alone does not establish distribution authorization; include explicit permission references for third-party translations or assets.

## 3. Maintainer Review & Snapshot Pipeline

### Intake Review Flow

1. A maintainer with `write`, `maintain`, or `admin` permission reviews the issue and applies `compat:accepted` or `patch:accepted`.
2. Automated intake treats the issue body purely as data, records the body SHA-256 and `updated_at`, and opens a review PR with keyed entries and `Metadata/intake/<issue>.json` without executing scripts.
3. Editing the issue body after approval automatically revokes the approval label and closes the candidate PR for re-review.

### Automated Core Release Catalog

Core release automation opens a data-only PR adding `Metadata/engines/cores/<plugin_id>/<platform>/<version>.json`. Once the trusted metadata workflow passes schema checks, it merges automatically and the portal's scheduled build publishes the updated snapshot.

### Snapshot Schemas & Backwards Compatibility

- **Cores**: Publishes `cores-v2.json` (`shinki.emu.cores.v2`, with third-party `identity`) alongside legacy `cores.json` (v1).
- **Compatibility**: Publishes `compatibility-v5.json` (core version limit 88 bytes for full third-party identities) alongside legacy `compatibility.json` (v4, 64-byte limit).
- **Patches**: Publishes `patches-v2.json` (`shinki.emu.patches.v2`) from `Metadata/patches/entries/` while retaining `Metadata/legacy` outputs (`patches.json`, `patches/catalog.json`) for older Managers.
- **Deterministic bounds**: `index.json` records SHA-256, size, and source commit for every file (≤ 8 MiB per file, ≤ 64 MiB total, ≤ 4096 files).

### Validation & Build Commands

```sh
cargo xtask metadata schema
cargo xtask metadata validate
cargo xtask metadata build --root Metadata --out .work/metadata-snapshot
cargo xtask metadata resolve-core --plugin-id rfvp --platform windows-x86_64 --abi shinki.emu.native.c.v3
cargo xtask metadata intake --kind compatibility --event event.json --out .work/compat-candidate
cargo xtask metadata intake --kind patch --event event.json --out .work/patch-candidate
```

Schemas are exported to `Metadata/schema/` directly from Rust DTOs; never hand-maintain a parallel schema.
