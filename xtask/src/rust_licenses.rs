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
    let stem = license
        .rsplit_once('-')
        .filter(|(_, version)| {
            version.bytes().any(|b| b.is_ascii_digit())
                && version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        })
        .map_or(license, |(stem, _)| stem);
    files.values().any(|name| {
        let Some(name) = Path::new(name).file_stem().and_then(|s| s.to_str()) else {
            return false;
        };
        ["", "LICENSE-", "LicenseRef-"].iter().any(|prefix| {
            name.get(..prefix.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
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

fn license_name(name: &str) -> bool {
    if name.rsplit_once('.').is_some_and(|(_, extension)| {
        ["rs", "py", "c", "cc", "cpp", "h", "hpp"]
            .iter()
            .any(|source| extension.eq_ignore_ascii_case(source))
    }) {
        return false;
    }
    ["LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT"]
        .iter()
        .any(|prefix| {
            name.get(..prefix.len())
                .is_some_and(|value| value.eq_ignore_ascii_case(prefix))
        })
        || ["OFL.txt", "UFL.txt"]
            .iter()
            .any(|value| name.eq_ignore_ascii_case(value))
        || (name.ends_with(".txt")
            && ["license", "licence"].iter().any(|word| {
                name.as_bytes()
                    .windows(word.len())
                    .any(|value| value.eq_ignore_ascii_case(word.as_bytes()))
            }))
}

fn source_license_files(
    root: &Path,
    recursive: bool,
    files: &mut BTreeMap<PathBuf, String>,
) -> Result<()> {
    let entries = walkdir::WalkDir::new(root)
        .min_depth(1)
        .max_depth(if recursive { usize::MAX } else { 1 })
        .into_iter()
        // Hidden trees (.direnv, .github, editor caches) are packaging residue,
        // not notices for the crate; id3 ships Nix inputs under .direnv.
        .filter_entry(|entry| {
            entry.depth() == 0 || !entry.file_name().to_string_lossy().starts_with('.')
        });
    for item in entries {
        let item = item.with_context(|| format!("reading licenses in {}", root.display()))?;
        let relative = item.path().strip_prefix(root)?;
        let name = item.file_name().to_str().context("UTF-8 source filename")?;
        if item.file_type().is_dir() {
            if !recursive && license_name(name) {
                for (file, path) in crate::package::tree(item.path())? {
                    files.insert(dunce::canonicalize(file)?, format!("{name}/{path}"));
                }
            }
            continue;
        }
        let in_license_path = relative
            .components()
            .any(|part| part.as_os_str().to_str().is_some_and(license_name));
        let font_notice = name.ends_with(".txt")
            && ["ttf", "otf", "woff", "woff2"]
                .iter()
                .any(|extension| item.path().with_extension(extension).is_file());
        if in_license_path || font_notice {
            crate::package::plain(item.path())?;
            files.insert(
                dunce::canonicalize(item.path())?,
                relative
                    .to_str()
                    .context("UTF-8 license path")?
                    .replace('\\', "/"),
            );
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
        "metadata",
        "--locked",
        "--format-version",
        "1",
        "--filter-platform",
        target,
    ]);
    if component != "manager" {
        let core = inv.core(component)?;
        command
            .arg("--manifest-path")
            .arg(inv.root.join(&core.manifest));
        if !core.features.is_empty() {
            command.arg("--features").arg(
                core.features
                    .iter()
                    .map(|f| format!("{}/{f}", core.package))
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
    }
    Ok(serde_json::from_slice(&crate::dependencies::capture(
        &mut command,
    )?)?)
}
/// Readable, stable discriminator for a package source.
fn source_tag(source: Option<&str>) -> String {
    use sha2::Digest;
    match source {
        None => "path".to_owned(),
        Some("registry+https://github.com/rust-lang/crates.io-index") => "crates-io".to_owned(),
        Some(source) => {
            let revision = source
                .strip_prefix("git+")
                .and_then(|git| git.rsplit_once('#'))
                .map(|(_, revision)| revision)
                .filter(|revision| {
                    revision.len() >= 12 && revision.bytes().all(|b| b.is_ascii_hexdigit())
                });
            match revision {
                Some(revision) => format!("git-{}", &revision[..12]),
                None => format!(
                    "source-{}",
                    &hex::encode(sha2::Sha256::digest(source))[..12]
                ),
            }
        }
    }
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
    // The same name-version can resolve from several sources (crates.io swf-0.2.2 and
    // ruffle's git swf-0.2.2); each needs its own notice group.
    let mut versions = BTreeMap::<(&str, &str), usize>::new();
    for id in &selected {
        let p = packages
            .get(id)
            .with_context(|| format!("cargo package missing: {id}"))?;
        *versions
            .entry((
                p["name"].as_str().context("cargo package name")?,
                p["version"].as_str().context("cargo package version")?,
            ))
            .or_default() += 1;
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
        let group = if versions[&(name, version)] > 1 {
            format!(
                "licenses/rust/{name}-{version}+{}",
                source_tag(p["source"].as_str())
            )
        } else {
            format!("licenses/rust/{name}-{version}")
        };
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
        source_license_files(root, true, &mut files)?;
        if files.is_empty()
            && p["source"]
                .as_str()
                .is_none_or(|source| source.starts_with("git+"))
            && !(p["source"].is_null()
                && metadata["workspace_root"]
                    .as_str()
                    .is_some_and(|workspace| root.starts_with(workspace)))
            && let Some(checkout) = root.ancestors().find(|dir| dir.join(".git").exists())
        {
            // Git and fork-checkout path crates can keep their provided copyright/license
            // at the checkout root (mlua-sys, siglus_rs crates/*), outside the crate directory.
            source_license_files(checkout, false, &mut files)?;
        }
        if files.is_empty()
            && p["source"].is_null()
            && let Some(workspace) = metadata["workspace_root"].as_str()
            && root.starts_with(workspace)
        {
            source_license_files(Path::new(workspace), false, &mut files)?;
        }
        let expression = p["license"].as_str().filter(|s| !s.trim().is_empty());
        let mut canonical = false;
        let has_package_text = !files.is_empty();
        let primary = files.values().any(|name| {
            !name
                .split('/')
                .next()
                .is_some_and(|dir| dir.eq_ignore_ascii_case("fonts"))
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
