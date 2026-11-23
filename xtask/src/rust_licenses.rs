use crate::{build::BuildArtifact, cores::Inventory, dependencies};
use anyhow::{Context, Result};
use std::{collections::BTreeSet, fs, path::{Path, PathBuf}, process::Command};

pub(crate) fn entries(inv: &Inventory, artifact: &BuildArtifact, scratch: &Path) -> Result<Vec<(PathBuf, String)>> {
    let manifest = if artifact.component == "manager" {
        inv.root.join("Source/Manager/shinki-emu-manager/Cargo.toml")
    } else {
        inv.root.join(&inv.core(&artifact.component)?.manifest)
    };
    let report = std::path::absolute(scratch)?.join("rust-licenses.html");
    let mut command = Command::new("cargo-about");
    command.current_dir(&inv.root)
        .args(["generate", "--locked", "--fail", "--manifest-path"])
        .arg(&manifest)
        .arg("--config").arg(inv.root.join("about.toml"))
        .arg("--output-file").arg(&report)
        .args(["--target", &artifact.target]);
    if artifact.component == "manager" && artifact.target.ends_with("-linux-android") {
        let other = if artifact.target.starts_with("aarch64-") { "x86_64-linux-android" } else { "aarch64-linux-android" };
        command.args(["--target", other]);
    }
    for feature in &artifact.features {
        command.args(["--features", feature]);
    }
    command.arg(inv.root.join("about.hbs"));
    dependencies::run(&mut command).context("generate Rust notices with cargo-about (cargo install cargo-about --version 0.9.3 --locked)")?;

    let mut links = locked_source_links(&fs::read_to_string(inv.root.join("Cargo.lock"))?)?;
    // A standalone adapter workspace can also have its own lockfile.
    if let Some(lock) = manifest.ancestors().skip(1).take_while(|dir| *dir != inv.root)
        .map(|dir| dir.join("Cargo.lock")).find(|file| file.is_file())
    {
        links.extend(locked_source_links(&fs::read_to_string(lock)?)?);
    }
    let sources = scratch.join("pinned-sources.txt");
    fs::write(&sources, links.into_iter().collect::<Vec<_>>().join("\n") + "\n")?;
    Ok(vec![
        (report, "licenses/rust-licenses.html".into()),
        (sources, "licenses/pinned-sources.txt".into()),
    ])
}

fn locked_source_links(lock: &str) -> Result<BTreeSet<String>> {
    let lock: toml::Value = toml::from_str(lock)?;
    let packages = lock["package"].as_array().context("Cargo.lock packages")?;
    let mut links = BTreeSet::new();
    for package in packages {
        let Some(source) = package.get("source").and_then(toml::Value::as_str)
            .and_then(|source| source.strip_prefix("git+https://github.com/")) else { continue };
        let (repo, commit) = source.rsplit_once('#').context("missing locked Git commit")?;
        let repo = repo.split('?').next().unwrap_or(repo).trim_end_matches(".git");
        links.insert(format!("https://github.com/{repo}/tree/{commit}"));
    }
    Ok(links)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_links_use_resolved_commits_and_deduplicate_shared_checkouts() {
        let lock = r#"
[[package]]
name = "core"
source = "git+https://github.com/Project-Shinkirou/example.git?branch=shinki#0123456789abcdef0123456789abcdef01234567"
[[package]]
name = "engine"
source = "git+https://github.com/Project-Shinkirou/example.git?branch=shinki#0123456789abcdef0123456789abcdef01234567"
[[package]]
name = "registry"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;
        assert_eq!(locked_source_links(lock).unwrap().into_iter().collect::<Vec<_>>(), [
            "https://github.com/Project-Shinkirou/example/tree/0123456789abcdef0123456789abcdef01234567"
        ]);
    }
}
