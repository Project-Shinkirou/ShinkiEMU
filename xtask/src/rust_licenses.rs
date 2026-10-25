use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

fn canonical_source() -> Result<Value> {
    let inventory: toml::Value = toml::from_str(include_str!("../../build/dependencies.toml"))?;
    let source = &inventory["rust_license_texts"];
    let manifest: toml::Value = toml::from_str(include_str!("../Cargo.toml"))?;
    let lock: toml::Value = toml::from_str(include_str!("../../Cargo.lock"))?;
    let pinned = lock["package"]
        .as_array()
        .context("Cargo.lock packages")?
        .iter()
        .any(|p| {
            p["name"] == source["package"]
                && p["version"] == source["version"]
                && p.get("checksum") == source.get("sha256")
        });
    let version = source["version"].as_str().context("SPDX source version")?;
    if !pinned
        || source["package"].as_str() != Some("spdx")
        || source["license_list_version"].as_str() != Some(spdx::identifiers::VERSION)
        || manifest["dependencies"]["spdx"]["version"]
            .as_str()
            .and_then(|v| v.strip_prefix('='))
            != Some(version)
    {
        bail!("canonical SPDX text source does not match the build inventory and Cargo pins");
    }
    Ok(serde_json::to_value(source)?)
}

fn provided_text(license: &str, files: &BTreeMap<PathBuf, String>) -> bool {
    let license = license.strip_prefix("LicenseRef-").unwrap_or(license);
    let stem = license.rsplit_once('-').filter(|(_, version)| {
        version.bytes().any(|b| b.is_ascii_digit())
            && version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    }).map_or(license, |(stem, _)| stem);
    files.values().any(|name| {
        let Some(name) = Path::new(name).file_stem().and_then(|s| s.to_str()) else { return false; };
        ["", "LICENSE-", "LicenseRef-"].iter().any(|prefix| {
            name.get(..prefix.len()).is_some_and(|p| p.eq_ignore_ascii_case(prefix))
                && name.get(prefix.len()..).is_some_and(|name| {
                    name.eq_ignore_ascii_case(license) || name.eq_ignore_ascii_case(stem)
                })
        })
    })
}

fn canonical_texts(
    expression: &str,
    provided: &BTreeMap<PathBuf, String>,
) -> Result<BTreeMap<&'static str, &'static str>> {
    // Cargo historically accepted "/" for OR and deprecated SPDX identifiers.
    // Keep identifier matching strict: custom/unknown texts cannot be invented.
    let mode = spdx::ParseMode {
        allow_slash_as_or_operator: true,
        allow_deprecated: true,
        allow_postfix_plus_on_gpl: true,
        ..spdx::ParseMode::STRICT
    };
    let expression = spdx::Expression::parse_mode(expression, mode)
        .with_context(|| format!("invalid SPDX expression {expression}"))?;
    let mut texts = BTreeMap::new();
    for requirement in expression.requirements() {
        if let Some(id) = requirement.req.license.id() {
            if !provided_text(id.name, provided) {
                texts.insert(id.name, id.text());
            }
        } else {
            let license = requirement.req.license.to_string();
            if !license.starts_with("LicenseRef-") || !provided_text(&license, provided) {
                bail!("unknown canonical SPDX identifier: {license}");
            }
        }
        if let Some(addition) = &requirement.req.addition {
            match addition {
                spdx::AdditionItem::Spdx(id) => {
                    texts.insert(id.name, id.text());
                }
                _ => bail!("unknown canonical SPDX exception: {addition}"),
            }
        }
    }
    Ok(texts)
}

fn source_license_files(root: &Path, files: &mut BTreeMap<PathBuf, String>) -> Result<()> {
    for item in
        fs::read_dir(root).with_context(|| format!("reading licenses in {}", root.display()))?
    {
        let item = item?;
        let name = item
            .file_name()
            .to_str()
            .context("UTF-8 source filename")?
            .to_owned();
        let filename = name.to_ascii_uppercase();
        if ["LICENSE", "COPYING", "COPYRIGHT", "NOTICE"]
            .iter()
            .any(|prefix| filename.starts_with(prefix))
        {
            crate::package::plain(&item.path())?;
            if item.file_type()?.is_file() {
                files.insert(dunce::canonicalize(item.path())?, name);
            } else if item.file_type()?.is_dir() {
                for (file, relative) in crate::package::tree(&item.path())? {
                    files.insert(dunce::canonicalize(file)?, format!("{name}/{relative}"));
                }
            }
        } else if filename == "FONTS" && item.file_type()?.is_dir() {
            // Embedded font licenses often use OFL.txt/UFL.txt or the font's
            // own name, rather than a LICENSE prefix.
            for (file, relative) in crate::package::tree(&item.path())? {
                if file.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                    ["txt", "md", "rst"].iter().any(|extension| s.eq_ignore_ascii_case(extension))
                }) {
                    files.insert(dunce::canonicalize(file)?, format!("{name}/{relative}"));
                }
            }
        }
    }
    Ok(())
}

/// Resolve the same manifest and features as the packaged Cargo build.
pub(crate) fn metadata(
    inv: &crate::cores::Inventory,
    target: &str,
    component: &str,
) -> Result<Value> {
    if component == "manager" {
        return crate::build::cargo_metadata(inv, target);
    }
    let mut command = std::process::Command::new("cargo");
    command.current_dir(&inv.root).args([
        "metadata", "--locked", "--offline", "--format-version", "1", "--filter-platform", target,
    ]);
    if component != "manager" {
        let core = inv.core(component)?;
        command.arg("--manifest-path").arg(inv.root.join(&core.manifest));
        if !core.features.is_empty() {
            command.arg("--features").arg(core.features.iter().map(|f| format!("{}/{f}", core.package)).collect::<Vec<_>>().join(","));
        }
    }
    Ok(serde_json::from_slice(&crate::dependencies::capture(&mut command)?)?)
}

/// Metadata must be resolved with `--filter-platform` for the packaged target.
/// Follow normal and build edges, including proc macros, but not test-only edges.
pub(crate) fn entries(
    metadata: &Value,
    package: &str,
    scratch: &Path,
) -> Result<Vec<(PathBuf, String)>> {
    let canonical_source = canonical_source()?;
    let packages = metadata["packages"]
        .as_array()
        .context("cargo metadata packages")?;
    let packages: BTreeMap<_, _> = packages
        .iter()
        .map(|p| Ok((p["id"].as_str().context("cargo package id")?, p)))
        .collect::<Result<_>>()?;
    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .context("cargo metadata resolve nodes")?;
    let nodes: BTreeMap<_, _> = nodes
        .iter()
        .map(|n| Ok((n["id"].as_str().context("cargo resolve id")?, n)))
        .collect::<Result<_>>()?;
    let root = packages
        .iter()
        .find(|(_, p)| p["name"] == package)
        .map(|(id, _)| *id)
        .with_context(|| format!("package {package} absent from cargo metadata"))?;
    let mut pending = vec![root];
    let mut selected = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !selected.insert(id) {
            continue;
        }
        let node = nodes
            .get(id)
            .with_context(|| format!("cargo resolve node missing: {id}"))?;
        for dep in node["deps"]
            .as_array()
            .context("cargo resolve dependencies")?
        {
            let kinds = dep["dep_kinds"]
                .as_array()
                .context("cargo dependency kinds")?;
            if kinds
                .iter()
                .any(|kind| kind["kind"].is_null() || kind["kind"] == "build")
            {
                pending.push(dep["pkg"].as_str().context("cargo dependency package id")?);
            }
        }
    }
    let mut entries = Vec::new();
    let mut missing = Vec::new();
    let mut canonical_packages = 0;
    let mut canonical_count = 0;
    for id in &selected {
        let p = packages
            .get(id)
            .with_context(|| format!("cargo package missing: {id}"))?;
        let name = p["name"].as_str().context("cargo package name")?;
        let version = p["version"].as_str().context("cargo package version")?;
        let group = format!("licenses/rust/{name}-{version}");
        crate::package::portable(&group)?;
        let root = Path::new(
            p["manifest_path"]
                .as_str()
                .context("cargo source manifest")?,
        )
        .parent()
        .context("cargo source directory")?;
        let mut files = BTreeMap::new();
        if let Some(file) = p["license_file"].as_str() {
            let file = root.join(file);
            crate::package::plain(&file)?;
            if !file.is_file() {
                bail!(
                    "{name}-{version}: license_file is not a file: {}",
                    file.display()
                );
            }
            let name = file
                .file_name()
                .context("license filename")?
                .to_str()
                .context("UTF-8 license filename")?
                .to_owned();
            files.insert(dunce::canonicalize(file)?, name);
        }
        source_license_files(root, &mut files)?;
        if files.is_empty()
            && p["source"]
                .as_str()
                .is_some_and(|source| source.starts_with("git+"))
            && let Some(checkout) = root.ancestors().find(|dir| dir.join(".git").exists())
        {
            // Git workspace crates can keep their provided copyright/license at
            // the checkout root (for example mlua-sys), outside the crate directory.
            source_license_files(checkout, &mut files)?;
        }
        if files.is_empty()
            && p["source"].is_null()
            && let Some(workspace) = metadata["workspace_root"].as_str()
            && root.starts_with(workspace)
        {
            source_license_files(Path::new(workspace), &mut files)?;
        }
        let expression = p["license"].as_str().filter(|s| !s.trim().is_empty());
        let mut canonical = false;
        let has_package_text = !files.is_empty();
        let primary = files.values().any(|name| {
            !name.split('/').next().is_some_and(|dir| dir.eq_ignore_ascii_case("fonts"))
        });
        if !primary {
            let Some(expression) = expression else {
                missing.push(format!("{name}-{version}"));
                continue;
            };
            let texts = canonical_texts(expression, &files)
                .with_context(|| format!("collecting canonical licenses for {name}-{version}"))?;
            canonical = !texts.is_empty();
            canonical_packages += usize::from(canonical);
            for (id, text) in texts {
                let dest = format!("{group}/LICENSE-{id}");
                crate::package::portable(&dest)?;
                let path = scratch.join(&dest);
                fs::create_dir_all(path.parent().context("canonical license directory")?)?;
                fs::write(&path, text)?;
                entries.push((path, dest));
                canonical_count += 1;
            }
        }
        for (file, filename) in files {
            let dest = format!("{group}/{filename}");
            crate::package::portable(&dest)?;
            entries.push((file, dest));
        }
        let record = scratch.join(&group).join("package.json");
        fs::create_dir_all(record.parent().context("license record directory")?)?;
        fs::write(
            &record,
            serde_json::to_vec_pretty(&serde_json::json!({
                "name": name, "version": version, "license": expression, "source": p["source"],
                "text_origin": if canonical && has_package_text { "package+canonical-spdx" } else if canonical { "canonical-spdx" } else { "package" },
                "canonical_text_source": if canonical { Some(&canonical_source) } else { None },
            }))?,
        )?;
        entries.push((record, format!("{group}/package.json")));
    }
    if !missing.is_empty() {
        bail!(
            "Rust dependencies missing both license files and SPDX expression: {}",
            missing.join(", ")
        );
    }
    eprintln!(
        "Rust licenses: {} packages, {} source license/notice files, {} canonical texts for {} packages",
        selected.len(),
        entries.len() - selected.len() - canonical_count,
        canonical_count,
        canonical_packages,
    );
    Ok(entries)
}
