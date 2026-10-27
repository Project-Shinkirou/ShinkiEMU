use super::{copy_tree, verify_vendor};
use crate::{dependencies, package};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path, process::Command};

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .with_context(|| format!("Cargo metadata missing {key}"))
}

fn definition(source: &str) -> Result<(String, toml::Table)> {
    let mut table = toml::Table::new();
    if let Some(git) = source.strip_prefix("git+") {
        let (reference, _) = git.rsplit_once('#').context("precise Git source")?;
        let mut url = reqwest::Url::parse(reference)?;
        for (key, value) in url.query_pairs() {
            if !matches!(key.as_ref(), "rev" | "tag" | "branch") {
                bail!("unsupported Cargo Git reference {key}");
            }
            table.insert(key.into_owned(), value.into_owned().into());
        }
        url.set_query(None);
        table.insert("git".into(), url.to_string().into());
        Ok((format!("git+{reference}"), table))
    } else if source == "registry+https://github.com/rust-lang/crates.io-index" {
        Ok(("crates-io".into(), table))
    } else {
        let registry = source
            .strip_prefix("registry+")
            .or_else(|| source.strip_prefix("sparse+"))
            .context("unsupported Cargo package source")?;
        table.insert("registry".into(), registry.into());
        Ok((source.into(), table))
    }
}

/// Cargo vendor flattens all source identities, rejecting equal name/version
/// pairs even when their code differs. Keep one directory source per identity.
/// Cargo itself still performs Git manifest normalization (workspace inheritance,
/// targets and path dependencies); no dependency in the shipped lock is patched.
pub(super) fn collect(manifest: &Path, root: &Path) -> Result<toml::Table> {
    // No target filter and every feature: corresponding sources must contain the
    // whole lockfile, which Cargo resolves with all features of all members.
    let metadata: Value = serde_json::from_slice(&dependencies::capture(
        Command::new("cargo")
            .current_dir(manifest.parent().context("manifest directory")?)
            .args([
                "metadata",
                "--locked",
                "--all-features",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(manifest),
    )?)?;
    let packages = metadata["packages"].as_array().context("Cargo packages")?;
    let lock_path = Path::new(text(&metadata, "workspace_root")?).join("Cargo.lock");
    let lock: toml::Value = toml::from_str(&fs::read_to_string(&lock_path)?)?;
    let mut groups: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for package in packages {
        if let Some(source) = package["source"].as_str() {
            groups.entry(source).or_default().push(package);
        }
    }
    let scratch = tempfile::tempdir_in(root.parent().context("source staging parent")?)?;
    let mut sources = toml::Table::new();
    for (index, (source, members)) in groups.iter().enumerate() {
        let directory = format!("vendor/source-{index}");
        let destination = root.join(&directory);
        fs::create_dir_all(&destination)?;
        let (key, mut source_config) = definition(source)?;
        if source.starts_with("git+") {
            let project = scratch.path().join(format!("source-{index}"));
            fs::create_dir_all(&project)?;
            let mut dependencies = toml::Table::new();
            for (i, member) in members.iter().enumerate() {
                let mut dep = source_config.clone();
                dep.insert("package".into(), text(member, "name")?.into());
                dep.insert(
                    "version".into(),
                    format!("={}", text(member, "version")?).into(),
                );
                dep.insert("default-features".into(), false.into());
                dependencies.insert(format!("package-{i}"), dep.into());
            }
            let mut patches = toml::Table::new();
            for (i, package) in packages.iter().enumerate() {
                let Some(other) = package["source"].as_str() else {
                    continue;
                };
                let (_, config) = definition(other)?;
                // Patches are URL-scoped, not revision-scoped. Never patch the
                // source being collected, including another reference to it.
                if config.get("git").is_some() && config.get("git") == source_config.get("git") {
                    continue;
                }
                let patch_key = if let Some(url) = config.get("git") {
                    url.as_str().context("Git patch URL")?.to_owned()
                } else if other == "registry+https://github.com/rust-lang/crates.io-index" {
                    "crates-io".into()
                } else {
                    config["registry"]
                        .as_str()
                        .context("registry patch URL")?
                        .to_owned()
                };
                let entry = patches
                    .entry(patch_key)
                    .or_insert_with(|| toml::Table::new().into());
                entry.as_table_mut().context("patch table")?.insert(format!("package-{i}"), toml::Value::try_from(serde_json::json!({
                    "package": text(package, "name")?,
                    "path": Path::new(text(package, "manifest_path")?).parent().context("package directory")?,
                }))?);
            }
            // Other sources are paths only in this disposable normalization
            // graph. They are collected independently at their original IDs.
            let project_manifest = toml::Value::try_from(serde_json::json!({
                "package": {"name":"shinki-source-vendor", "version":"0.0.0", "edition":"2024"},
                "workspace": {}, "lib":{"path":"lib.rs"},
            }))?;
            let mut project_manifest = project_manifest
                .as_table()
                .context("vendor manifest")?
                .clone();
            project_manifest.insert("dependencies".into(), dependencies.into());
            project_manifest.insert("patch".into(), patches.into());
            fs::write(
                project.join("Cargo.toml"),
                toml::to_string(&project_manifest)?,
            )?;
            fs::write(project.join("lib.rs"), "")?;
            // Offline Cargo cannot look up a Git tag/branch the original fetch
            // never recorded as a ref; the original lock supplies precise revisions.
            fs::copy(&lock_path, project.join("Cargo.lock"))?;
            let vendored = project.join("vendor");
            dependencies::capture(
                Command::new("cargo")
                    .current_dir(&project)
                    .args(["vendor", "--offline", "--versioned-dirs"])
                    .arg(&vendored),
            )
            .with_context(|| format!("normalizing locked Git source {source}"))?;
            for member in members {
                let name = format!("{}-{}", text(member, "name")?, text(member, "version")?);
                copy_tree(&vendored.join(&name), &destination.join(name))?;
            }
        } else {
            for member in members {
                let name = format!("{}-{}", text(member, "name")?, text(member, "version")?);
                let package_root = Path::new(text(member, "manifest_path")?)
                    .parent()
                    .context("registry package root")?;
                let registry = package_root.parent().context("registry index")?;
                let archive = registry
                    .parent()
                    .and_then(Path::parent)
                    .context("registry cache")?
                    .join("cache")
                    .join(registry.file_name().context("registry directory")?)
                    .join(format!("{name}.crate"));
                let checksum = lock["package"]
                    .as_array()
                    .context("locked packages")?
                    .iter()
                    .find(|p| {
                        p["name"].as_str() == member["name"].as_str()
                            && p["version"].as_str() == member["version"].as_str()
                            && p.get("source").and_then(toml::Value::as_str) == Some(source)
                    })
                    .and_then(|p| p["checksum"].as_str())
                    .context("locked registry checksum")?;
                if package::sha256(&archive)? != checksum {
                    bail!("registry archive checksum mismatch: {name}");
                }
                tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&archive)?))
                    .unpack(&destination)?;
                let dir = destination.join(&name);
                let mut files = BTreeMap::new();
                for entry in walkdir::WalkDir::new(&dir).follow_links(false) {
                    let entry = entry?;
                    package::plain(entry.path())?;
                    if entry.file_type().is_file() {
                        files.insert(
                            entry
                                .path()
                                .strip_prefix(&dir)?
                                .to_string_lossy()
                                .replace('\\', "/"),
                            package::sha256(entry.path())?,
                        );
                    }
                }
                fs::write(
                    dir.join(".cargo-checksum.json"),
                    serde_json::to_vec(&serde_json::json!({"package":checksum,"files":files}))?,
                )?;
            }
        }
        verify_vendor(&destination)?;
        let replacement = format!("shinki-vendor-{index}");
        source_config.insert("replace-with".into(), replacement.clone().into());
        sources.insert(key, source_config.into());
        sources.insert(
            replacement,
            toml::Value::try_from(serde_json::json!({"directory":directory}))?,
        );
    }
    Ok(sources)
}
