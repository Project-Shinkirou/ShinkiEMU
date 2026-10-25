use anyhow::{bail, Context, Result};
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

fn canonical_texts(expression: &str) -> Result<BTreeMap<&'static str, &'static str>> {
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
        let id = requirement.req.license.id().with_context(|| {
            format!("unknown canonical SPDX identifier: {}", requirement.req.license)
        })?;
        texts.insert(id.name, id.text());
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

/// Metadata must be resolved with `--filter-platform` for the packaged target.
/// Follow normal and build edges, including proc macros, but not test-only edges.
pub(crate) fn entries(metadata: &Value, scratch: &Path) -> Result<Vec<(PathBuf, String)>> {
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
        .find(|(_, p)| p["name"] == "shinki-emu-manager")
        .map(|(id, _)| *id)
        .context("Manager package absent from cargo metadata")?;
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
        let root = Path::new(p["manifest_path"].as_str().context("cargo source manifest")?)
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
            let name = file.file_name().context("license filename")?
                .to_str().context("UTF-8 license filename")?.to_owned();
            files.insert(dunce::canonicalize(file)?, name);
        }
        for item in
            fs::read_dir(root).with_context(|| format!("reading source for {name}-{version}"))?
        {
            let item = item?;
            let name = item.file_name().to_str().context("UTF-8 source filename")?.to_owned();
            let filename = name.to_ascii_uppercase();
            if ["LICENSE", "COPYING", "NOTICE"]
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
            }
        }
        let expression = p["license"].as_str().filter(|s| !s.trim().is_empty());
        let mut canonical = false;
        if files.is_empty() {
            let Some(expression) = expression else {
                missing.push(format!("{name}-{version}"));
                continue;
            };
            let texts = canonical_texts(expression)
                .with_context(|| format!("collecting canonical licenses for {name}-{version}"))?;
            for (id, text) in texts {
                let dest = format!("{group}/LICENSE-{id}");
                crate::package::portable(&dest)?;
                let path = scratch.join(&dest);
                fs::create_dir_all(path.parent().context("canonical license directory")?)?;
                fs::write(&path, text)?;
                entries.push((path, dest));
                canonical_count += 1;
            }
            canonical = true;
            canonical_packages += 1;
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
                "text_origin": if canonical { "canonical-spdx" } else { "package" },
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
