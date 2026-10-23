# ShinkiEMU Metadata

This directory is the sole source for the public metadata snapshot. Data is CC0-1.0; Luau recipes are MIT OR Apache-2.0 (see LICENSE).

- Compatibility: `compatibility/entries/<vid>/<rid>/<core>/<platform>-<architecture>-<variant>.json`.
- Core releases: `engines/cores/<core>/<platform>/<version>.json`.
- Presets: `engines/presets/<rid>/<core>.json`.
- Patches: `patches/entries/<id>.json`; reviewed Luau scripts remain in `patches/`.

`cargo xtask metadata schema` exports schemas from the Rust DTOs. `cargo xtask metadata validate` checks schemas, paths, references and limits. `cargo xtask metadata build --out <new-dir>` generates a deterministic snapshot, including recipe-only `patches/catalog.json`, `patches.json` and a SHA256 index tied to the source commit. Generated snapshots are not source data.

Use the compatibility or patch issue form. Maintainers review and approve reports before intake opens a normal PR. Empty compatibility data means no reports, not verified compatibility. A patch with no release IDs is declared applicable by work only; its releases have not been verified. Hosted attachments require a public authorization record and a pinned hash. External attachments are links only and are never executed automatically.
