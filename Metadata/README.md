# ShinkiEMU Metadata

This directory is the sole source for the public metadata snapshot. Data is CC0-1.0; Luau recipes are MIT OR Apache-2.0 (see LICENSE).

- Compatibility: `compatibility/entries/<vid>/<rid>/<core>/<platform>-<architecture>-<variant>.json`.
- Core releases: `engines/cores/<core>/<platform>/<version>.json`.
- Presets: `engines/presets/<rid>/<core>.json`.
- Patches: release-bound `patches/entries/<id>.json` using `shinki.emu.patches.v2`; reviewed Luau scripts remain in `patches/`.

`cargo xtask metadata schema` exports schemas from the Rust DTOs. `cargo xtask metadata validate` checks schemas, paths, references and limits. `cargo xtask metadata build --out <new-dir>` generates a deterministic snapshot with `patches-v2.json`, release-binding sidecars (`patches/<script-stem>.patch.json`) and a SHA256 index tied to the source commit. Generated snapshots are not source data.

Official intake, validation and snapshot builds require exact core IDs from [`build/cores.toml`](../build/cores.toml). The metadata parsers accept any syntactically valid, path-safe core ID and preserve its case.

Every catalog patch is an offline embedding recipe with nonempty concrete `releases` and a `downloads` declaration array. Each release uses the existing VNDB `release_id`/`vn_id`, localized title, actual edition label, optional actual game version and a nonempty `{path,size,sha256}` source fingerprint group. Manager identifies exactly one matching release before creating writable transaction state in either output mode. Local `.luau` scripts or ZIP packages use the same executor, are labelled unreviewed, and may run without a release manifest; those scripts validate their own inputs. Optional local release bindings use `<script-stem>.patch.json`.

The [Lucifen LPK repacking recipe](../Tools/Research/lucifen-oretsuba-cn.luau) is a local script outside the exported snapshot. Select it as a local patch with a matching original installation and authorized translation files; the repository supplies only the recipe.

Every recipe supports a new independent directory (default) and optional in-place overwrite through the existing `.shinki/backup` transaction. Manager and its CLI (`patch apply --in-place`, `patch restore`) retain backup/restore support; failure restores changed game files. New-directory output does not copy saves. ZIP packages require root `patch.luau` and safe relative modules/resources; `.luau` modules use `ctx.load`, and ZIP resources are read-only through `ctx.open("package", path)`. `ctx.musica_embed(profile, translations, resource_paths, destination)` materializes native SC with Musica's existing native readers; no runtime translation table is installed. Core semantic requirements use the existing output-profile `required_capabilities` gate.

On Windows, patch output and transaction directories inherit their parent directories' permissions. Run patches in trusted directories; staging does not isolate files from other writers who have access.

Patches modify resources, not executable code. An output EXE must be a byte-identical copy of the same source file; in-place transactions retain omitted original EXEs. Yorino constants use existing SC syntax in `scripts/shinki-names.sc` (`.pragma shinki_name_unknown` and `.pragma shinki_history_name_unknown` with scalar literals), while the core generates history brackets and masks. It requires native capabilities 768; older cores are refused. `ctx.musica_profile(buffer)` reads private keys.v2 with the native reader, accepts the old `astra.emu.musica.keys.v2` identifier and normalizes it only in memory; user-selected source key files are never rewritten.

Yorino recipe version 3 launches the original `test.sc` entry directly. This edition has no Manager frontend definition, so its earlier `title` receipt cannot build a title menu. Reapply the current recipe to the original and authorized patch in a new directory instead of editing a prior receipt.

The optional local Yorino [Recall](../Tools/Research/musica-yorino-recall.luau) and [CG](../Tools/Research/musica-yorino-cg.luau) recipes prepare separate overlays for the recognized original Japanese install and its private `key.toml`. Each is a fixed-title Memories shortcut to the selected gallery, not the complete native title or Memories hub. They leave the catalogued translation's direct entry unchanged. Use a Manager/core build that supports authored previews and, for CG, composite variants. From the repository root:

```powershell
New-Item -ItemType Directory gallery-recipe/common -Force | Out-Null
Copy-Item Tools/Research/musica-yorino-recall.luau, Tools/Research/musica-yorino-cg.luau, Tools/Research/musica-yorino-gallery-common.luau gallery-recipe/
Copy-Item Metadata/patches/common/pe.luau, Metadata/patches/common/binary.luau, Metadata/patches/common/musica_gallery_table.luau gallery-recipe/common/
# Choose "recall" or "cg"; use a separate game copy for each overlay.
$gallery = "cg"
shinki-emu-manager patch apply --script "gallery-recipe/musica-yorino-$gallery.luau" --source ORIGINAL_GAME_COPY --output gallery-overlay
Copy-Item "gallery-overlay/$gallery-layout.json", gallery-overlay/shinki-patch-result.json -Destination ORIGINAL_GAME_COPY
```

Import the resulting game copy in Manager. Use a fresh copy with no existing receipt; do not overwrite a catalogued conversion. The recipes validate against the unchanged archives and reject `--in-place` and unsupported executables. Recall checks named SC labels. CG reads the ordered variants, 21-slot layout and navigation coordinates from the recognized PE, checks thumbnail and variant membership in the original archives, and reads size-dependent art geometry from PNG headers. The CG viewer opens the stock image at its origin, without reconstructing the original drag-panning or page-slide animation. To remove an overlay, delete only its two generated files from the copy.

The Perseus conversion keeps title launch and supplies `configuration.frontend`. It requires the matching unmodified `perseus.exe` in the original game directory. The recipe reads the executable's CG grouping, Flash routes, music order and gallery geometry as data, then validates the generated layout against the copied archives using the core's gallery loader. It neither runs nor copies the executable. A different executable is rejected. The generated gallery uses resource availability rather than the viewed-CG history.

The gallery extractor recognizes one SHA-256/size and a table of relative offsets for that PE32 build. It checks the instruction forms at those offsets; it does not discover layouts across executable revisions. CG groups and variants, Flash slots, page counts, track order, and hit rectangles come from the file. Title variants, archive plate naming and placement conventions, and the roles of decoded controls remain recipe data.

The shared Musica CG reader preserves the order of the null-terminated PE32 pointer tables and splits groups at `*`. The same format is confirmed in Yorino, Eden English and ef first; Eden and ef first select between multiple tables and use different page sizes. Table selection, filtering, and composition are not inferred from filenames. Only Perseus currently uses the shared decoder in its catalog recipe.

The generated gallery requires `CAP_AUTHORED_GALLERY_CLIP` (1024). Manager checks the actual selected core descriptor before preparing settings or launching. Older receipts without this layout retain their existing requirements.

Eden and ef name extraction uses each executable pair's display-name pointer, matching actual SC operands. Its second pointer is a Japanese reference name, not the offline SC identity. The same offline table also recognizes the pinned patch's GBK target bytes, because these Chinese archives already contain localized names. Both identities are resolved during conversion; output scripts use native scalar literals without a runtime table.

Sakura Moyu recipe version 2 writes native GBK HCB and sets `font_path="font/HanaMinA.ttf"` with required capabilities 192. Use that shipped patch face for dialogue, names and ruby: its cmap covers all 3,801 non-whitespace characters in the pinned patch's translations, while AaGuiqi, Ryumin and Yu Mincho have gaps. Their subtitle fonts remain available for the original ASS styles. A version-1 conversion receipt containing `translation_table` and no `font_path` is still an external-table output; reapply the current recipe to the original game and authorized patch in a new directory. Changing encoding or installing a newer core alone does not convert that output. Fonts and translated game data are never redistributed in this repository.

Legacy `patches.json` (`patches.v1`) and `patches/catalog.json` are frozen from `legacy/`, with their scripts under generated `patches/legacy/`. They remain usable by old Managers. New Managers consume only `patches-v2.json`; they do not fall back to work-only or multi-category v1 data. Existing core v1 and compatibility v4 catalogs keep their established rules.

`ctx.download(url, sha256, size)` accepts HTTPS without credentials, a mandatory SHA256 and size 1..1073741824. It writes only verified hash-named files under game `.shinki/patch`, returns a read-only Source handle, checks cached files again and removes failed partial downloads. Catalog scripts may download only exact URL/SHA256/size triples in their entry's `downloads`; missing/empty declarations allow none. Local scripts have the same protocol/hash/size restrictions without a catalog allowlist. `ctx.check(label, passed, detail)` reports a check and aborts on failure. `ctx.fail(code, message)` and native Luau `error(message)` abort the transaction; Manager retains the error text and reports rollback. No process execution, ambient filesystem access or other networking is exposed.

Use the compatibility or Luau patch issue form. Intake validates fields and compiles Luau syntax without running the script, then opens a review PR containing the entry and `Metadata/patches/<id>.luau`; maintainers approve before merging. No trial mode, synthetic-download fallback or second executor is present.
