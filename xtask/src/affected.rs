//! `matrix --json --since <ref>`: the job-matrix rows whose inputs changed since a Git ref.
use crate::{cores::Inventory, dependencies};
use anyhow::{Context, Result, bail};
use std::{
    collections::{BTreeMap, BTreeSet},
    process::Command,
};

/// Inputs of every row.
const ALL_FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "build/targets.toml",
];
const ALL_DIRS: &[&str] = &[
    "xtask/",
    "Source/SDK/",
    "Source/FamilyApi/",
    "Source/Tools/",
    "build/vcpkg/",
    "build/ci/",
];
const MANAGER_DIRS: &[&str] = &["Source/Manager/", "Source/Providers/"];

#[derive(Debug, Default)]
pub struct Affected {
    all: bool,
    manager: bool,
    /// Manager targets whose package includes a changed core.
    manager_targets: BTreeSet<String>,
    cores: BTreeSet<String>,
}

impl Affected {
    pub fn row(&self, component: &str, target: &str) -> bool {
        self.all
            || if component == "manager" {
                self.manager || self.manager_targets.contains(target)
            } else {
                self.cores.contains(component)
            }
    }
    fn core<'a>(&mut self, id: &str, manager_targets: impl IntoIterator<Item = &'a str>) {
        self.cores.insert(id.to_owned());
        self.manager_targets
            .extend(manager_targets.into_iter().map(str::to_owned));
    }
}

/// Compare `HEAD` with `reference`, including the paths on both sides of a rename.
pub fn since(inv: &Inventory, reference: &str) -> Result<Affected> {
    if reference.starts_with('-') {
        bail!("--since takes a Git ref, not an option: {reference}");
    }
    let git = |args: &[&str]| {
        dependencies::capture(Command::new("git").arg("-C").arg(&inv.root).args(args))
    };
    let base = String::from_utf8(git(&[
        "rev-parse",
        "--verify",
        "--end-of-options",
        &format!("{reference}^{{commit}}"),
    ])?)?
    .trim()
    .to_owned();
    let names = String::from_utf8(git(&[
        "diff",
        "--name-only",
        "--no-renames",
        "-z",
        &base,
        "HEAD",
        "--",
    ])?)?;
    let paths: Vec<&str> = names.split('\0').filter(|p| !p.is_empty()).collect();
    let (old_cores, new_cores) = if paths.contains(&"build/cores.toml") {
        (
            git(&["show", &format!("{base}:build/cores.toml")])
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok()),
            String::from_utf8(git(&["show", "HEAD:build/cores.toml"])?)?,
        )
    } else {
        (None, String::new())
    };
    classify(inv, &paths, old_cores.as_deref(), &new_cores)
}

/// `old_cores`/`new_cores` are `build/cores.toml` at the ref and at `HEAD`; they are
/// read only when that file changed. A missing or unreadable old inventory affects all rows.
fn classify(
    inv: &Inventory,
    paths: &[&str],
    old_cores: Option<&str>,
    new_cores: &str,
) -> Result<Affected> {
    let mut affected = Affected::default();
    for &path in paths {
        if path.ends_with(".md") || path.starts_with("docs/") || path.starts_with("Metadata/") {
            continue;
        }
        if ALL_FILES.contains(&path) || ALL_DIRS.iter().any(|dir| path.starts_with(dir)) {
            affected.all = true;
        } else if MANAGER_DIRS.iter().any(|dir| path.starts_with(dir)) {
            affected.manager = true;
        } else if path == "build/cores.toml" {
            core_table_changes(inv, &mut affected, old_cores, new_cores)?;
        } else if let Some((id, _)) = path
            .strip_prefix("Source/Cores/")
            .and_then(|tail| tail.split_once('/'))
            && let Ok(core) = inv.core(id)
        {
            affected.core(
                id,
                bundled_targets(inv, core.ios_framework, core.ohos_hap.is_some()),
            );
        } else if let Some(core) = family_core(inv, path) {
            affected.core(
                &core.id,
                bundled_targets(inv, core.ios_framework, core.ohos_hap.is_some())
                    .chain(core.static_manager_targets.iter().map(String::as_str)),
            );
        } else {
            affected.all = true;
        }
        if affected.all {
            break;
        }
    }
    Ok(affected)
}

fn bundled_targets(
    inv: &Inventory,
    ios_framework: bool,
    ohos_hap: bool,
) -> impl Iterator<Item = &str> {
    inv.targets
        .iter()
        .map(|target| target.triple.as_str())
        .filter(move |triple| {
            (ios_framework && crate::apple::ios_frameworks::target_supported(triple))
                || (ohos_hap && shinki_emu_build::ohos::Target::from_rust_target(triple).is_some())
        })
}

/// Locate an in-tree adapter; families without their own adapter are shared inputs.
fn family_core<'a>(inv: &'a Inventory, path: &str) -> Option<&'a crate::cores::Core> {
    let (dir, _) = path.strip_prefix("Source/Families/")?.split_once('/')?;
    inv.cores
        .iter()
        .find(|core| core.attribution.is_none() && core.adapter_package() == dir)
}

/// A per-core table that differs affects that core and the Manager packages
/// that include it (before or after the change); other changes affect every row.
fn core_table_changes(
    inv: &Inventory,
    affected: &mut Affected,
    old: Option<&str>,
    new: &str,
) -> Result<()> {
    let Some(Ok(mut old)) = old.map(toml::from_str::<toml::Table>) else {
        affected.all = true;
        return Ok(());
    };
    let mut new: toml::Table = toml::from_str(new).context("build/cores.toml at HEAD")?;
    let (Ok(old_cores), Ok(new_cores)) = (by_id(&mut old), by_id(&mut new)) else {
        affected.all = true;
        return Ok(());
    };
    if old != new {
        affected.all = true;
        return Ok(());
    }
    for id in old_cores.keys().chain(new_cores.keys()) {
        let (before, after) = (old_cores.get(id), new_cores.get(id));
        if before == after {
            continue;
        }
        for core in [before, after].into_iter().flatten() {
            let static_targets = core
                .get("static_manager_targets")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(toml::Value::as_str);
            let ios_framework =
                core.get("ios_framework").and_then(toml::Value::as_bool) == Some(true);
            let ohos_hap = core.get("ohos_hap").is_some();
            affected.core(
                id,
                static_targets.chain(bundled_targets(inv, ios_framework, ohos_hap)),
            );
        }
    }
    Ok(())
}

/// Remove the `[[cores]]` array from `table`, keyed by core id.
fn by_id(table: &mut toml::Table) -> Result<BTreeMap<String, toml::Value>> {
    let Some(toml::Value::Array(cores)) = table.remove("cores") else {
        bail!("build/cores.toml has no [[cores]] array");
    };
    cores
        .into_iter()
        .map(|core| {
            let id = core
                .get("id")
                .and_then(toml::Value::as_str)
                .context("core without id")?
                .to_owned();
            Ok((id, core))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn inventory() -> (Inventory, String) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let cores = std::fs::read_to_string(root.join("build/cores.toml")).unwrap();
        (Inventory::load(root).unwrap(), cores)
    }
    fn rows(inv: &Inventory, affected: &Affected) -> BTreeSet<(String, String)> {
        inv.job_matrix(false, Some(affected))
            .unwrap()
            .include
            .into_iter()
            .map(|row| (row.component, row.target))
            .collect()
    }
    fn every_row(inv: &Inventory) -> BTreeSet<(String, String)> {
        rows(
            inv,
            &Affected {
                all: true,
                ..Affected::default()
            },
        )
    }
    fn changed(inv: &Inventory, paths: &[&str]) -> BTreeSet<(String, String)> {
        rows(inv, &classify(inv, paths, None, "").unwrap())
    }
    const IOS_MANAGERS: &[&str] = &[
        "aarch64-apple-ios",
        "aarch64-apple-ios-sim",
        "x86_64-apple-ios",
    ];

    fn core_rows(
        inv: &Inventory,
        core: &str,
        manager_targets: &[&str],
    ) -> BTreeSet<(String, String)> {
        every_row(inv)
            .into_iter()
            .filter(|(component, target)| {
                component == core
                    || component == "manager" && manager_targets.contains(&target.as_str())
            })
            .collect()
    }

    #[test]
    fn documentation_and_catalog_paths_affect_no_rows() {
        let (inv, _) = inventory();
        assert!(
            changed(
                &inv,
                &[
                    "docs/en/build-and-release.md",
                    "docs/assets/diagram.svg",
                    "Metadata/engines/cores/rfvp/windows-x64/0.1.0.json",
                    "xtask/README.md",
                    "Source/Manager/shinki-emu-manager/NOTES.md",
                ],
            )
            .is_empty()
        );
    }

    #[test]
    fn shared_build_inputs_and_unknown_paths_affect_every_row() {
        let (inv, _) = inventory();
        let all = every_row(&inv);
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "build/targets.toml",
            "xtask/src/release.rs",
            "Source/SDK/shinki-emu-sdk/src/lib.rs",
            "Source/FamilyApi/src/lib.rs",
            "Source/Tools/shinki-emu-build/src/lib.rs",
            "build/vcpkg/subtitle/vcpkg.json",
            "build/ci/linux-manager-smoke.sh",
            "build/dependencies.toml",
            "Platforms/Android/app/build.gradle.kts",
            ".github/workflows/ci.yml",
            // A family directory without its own core manifest is shared code.
            "Source/Families/shinki-emu-musica-keys/src/lib.rs",
        ] {
            assert_eq!(changed(&inv, &[path]), all, "{path}");
        }
    }

    #[test]
    fn manager_sources_affect_only_manager_rows() {
        let (inv, _) = inventory();
        let manager: BTreeSet<_> = every_row(&inv)
            .into_iter()
            .filter(|(component, _)| component == "manager")
            .collect();
        assert!(!manager.is_empty());
        for path in [
            "Source/Manager/shinki-emu-manager/src/main.rs",
            "Source/Providers/shinki-emu-metadata/src/lib.rs",
        ] {
            assert_eq!(changed(&inv, &[path]), manager, "{path}");
        }
    }

    #[test]
    fn family_sources_affect_their_cores_and_bundled_manager_rows() {
        let (inv, _) = inventory();
        let musica = changed(&inv, &["Source/Families/shinki-emu-musica/src/lib.rs"]);
        assert_eq!(musica, core_rows(&inv, "shinki.emu.musica", IOS_MANAGERS));
        let cmvs = changed(&inv, &["Source/Families/shinki-emu-cmvs/Cargo.toml"]);
        assert_eq!(cmvs, core_rows(&inv, "shinki.emu.cmvs", IOS_MANAGERS));
    }

    #[test]
    fn entry_changes_rebuild_manager_only_when_the_package_uses_the_entry() {
        let (inv, _) = inventory();
        for (path, core, manager_targets) in [
            (
                "Source/Cores/siglus_rs/src/lib.rs",
                "siglus_rs",
                IOS_MANAGERS,
            ),
            (
                "Source/Cores/shinki.emu.adv32/src/lib.rs",
                "shinki.emu.adv32",
                &[][..],
            ),
        ] {
            let expected = core_rows(&inv, core, manager_targets);
            assert!(expected.iter().any(|(component, _)| component == core));
            assert_eq!(changed(&inv, &[path]), expected);
        }
    }

    #[test]
    fn ohos_companion_changes_invalidate_the_local_hap() {
        let (inv, current) = inventory();
        let target = shinki_emu_build::ohos::Target::Arm64.rust_target();
        let (core, _) = crate::ohos::companion(&inv, target).unwrap();
        for path in [
            format!("Source/Cores/{}/src/lib.rs", core.id),
            format!("Source/Families/{}/src/lib.rs", core.adapter_package()),
        ] {
            let affected = classify(&inv, &[&path], None, "").unwrap();
            assert!(affected.row("manager", target), "{path}");
        }
        let other = inv.cores.iter().find(|other| other.id != core.id).unwrap();
        let path = format!("Source/Cores/{}/src/lib.rs", other.id);
        assert!(
            !classify(&inv, &[&path], None, "")
                .unwrap()
                .row("manager", target)
        );

        let mut without_companion: toml::Table = toml::from_str(&current).unwrap();
        let total_removed = without_companion
            .get_mut("cores")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .filter_map(|core| core.as_table_mut().unwrap().remove("ohos_hap"))
            .count();
        assert_eq!(total_removed, 1);
        let without_companion = toml::to_string(&without_companion).unwrap();
        let mut without_inventory = inv.clone();
        for core in &mut without_inventory.cores {
            core.ohos_hap = None;
        }
        for (inventory, before, after) in [
            (
                &without_inventory,
                current.as_str(),
                without_companion.as_str(),
            ),
            (&inv, without_companion.as_str(), current.as_str()),
        ] {
            let affected = classify(inventory, &["build/cores.toml"], Some(before), after).unwrap();
            assert!(affected.row("manager", target));
        }
    }

    #[test]
    fn inventory_changes_affect_only_the_edited_core_tables() {
        let (inv, current) = inventory();
        let table: toml::Table = toml::from_str(&current).unwrap();
        let edit = |change: &dyn Fn(&mut toml::Table)| {
            let mut old = table.clone();
            change(&mut old);
            let old = toml::to_string(&old).unwrap();
            rows(
                &inv,
                &classify(&inv, &["build/cores.toml"], Some(&old), &current).unwrap(),
            )
        };
        let rfvp = edit(&|old| {
            let cores = old["cores"].as_array_mut().unwrap();
            let rfvp = cores
                .iter_mut()
                .find(|core| core["id"].as_str() == Some("rfvp"));
            rfvp.unwrap()["display_name"] = "RFVP (previous)".into();
        });
        assert_eq!(rfvp, core_rows(&inv, "rfvp", IOS_MANAGERS));
        assert!(!rfvp.iter().any(|(component, _)| component == "siglus_rs"));

        assert!(edit(&|_| {}).is_empty(), "formatting-only edits");
        assert_eq!(
            edit(&|old| {
                old["schema"] = "shinki.emu.build_cores.v0".into();
            }),
            every_row(&inv)
        );
        assert_eq!(
            rows(
                &inv,
                &classify(&inv, &["build/cores.toml"], None, &current).unwrap()
            ),
            every_row(&inv),
            "new inventory file"
        );
    }
}
