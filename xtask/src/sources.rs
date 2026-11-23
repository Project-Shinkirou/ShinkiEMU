use crate::{
    cores::Inventory,
    dependencies, package,
};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn git(directory: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(directory)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0");
    command
}

/// Committed bytes: a runner's core.autocrlf=true would rewrite LF in archives
/// (breaking LFS pointer checks). Status checks use the checkout's own
/// settings, so a CRLF product checkout does not look modified.
fn git_committed_bytes(directory: &Path) -> Command {
    let mut command = git(directory);
    command.args([
        "-c",
        "core.autocrlf=false",
        "-c",
        "core.eol=lf",
        "-c",
        "core.filemode=false",
        "-c",
        "core.symlinks=false",
        "-c",
        "core.longpaths=true",
    ]);
    command
}

fn pin_checkout_config(directory: &Path) -> Result<()> {
    for (key, value) in [
        ("core.autocrlf", "false"),
        ("core.eol", "lf"),
        ("core.filemode", "false"),
        ("core.symlinks", "false"),
    ] {
        dependencies::run(git(directory).args(["config", "--local", "--replace-all", key, value]))?;
    }
    Ok(())
}

#[cfg(test)]
fn revision(directory: &Path) -> Result<String> {
    Ok(String::from_utf8(dependencies::capture(
        git(directory).args(["rev-parse", "HEAD"]),
    )?)?
    .trim()
    .to_owned())
}

fn checkout_root(directory: &Path) -> Result<()> {
    package::plain(directory)?;
    let reported = String::from_utf8(dependencies::capture(
        git(directory).args(["rev-parse", "--show-toplevel"]),
    )?)?;
    if fs::canonicalize(reported.trim())? != fs::canonicalize(directory)? {
        bail!(
            "{} is not the root of the requested checkout",
            directory.display()
        );
    }
    Ok(())
}

fn modified(directory: &Path) -> Result<bool> {
    Ok(!dependencies::capture(git(directory).args([
        "status",
        "--porcelain=v1",
        "--untracked-files=all",
        "--ignore-submodules=none",
    ]))?
    .is_empty())
}

fn require_clean(directory: &Path) -> Result<()> {
    if modified(directory)? {
        bail!(
            "Ren’Py build checkout {} contains local modifications; commit or move them before dependency preparation",
            directory.display()
        );
    }
    Ok(())
}

fn copy_input(source: &Path, dest: &Path) -> Result<()> {
    package::plain(source)
        .with_context(|| format!("required corresponding-source input {}", source.display()))?;
    fs::create_dir_all(dest.parent().context("source input parent")?)?;
    fs::copy(source, dest)?;
    Ok(())
}

fn sha512(path: &Path) -> Result<String> {
    use sha2::Digest;
    use std::io::Read;
    package::plain(path)?;
    let mut file = fs::File::open(path)?;
    let mut digest = sha2::Sha512::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hex::encode(digest.finalize()))
}

/// Registry ports are identified by the Git tree their SPDX record names, which
/// can predate the registry HEAD. Overlay ports record no tree; they ship in
/// the fork snapshot. Returns whether a recipe was extracted.
pub(crate) fn port_recipe(spdx: &Value, registry: &Path, dest: &Path) -> Result<bool> {
    let port = spdx["packages"]
        .as_array()
        .context("SPDX packages")?
        .iter()
        .find(|p| p["SPDXID"] == "SPDXRef-port")
        .context("SPDX port recipe")?;
    let Some(tree) = port["externalRefs"].as_array().and_then(|refs| {
        refs.iter()
            .filter_map(|r| r["referenceLocator"].as_str())
            .find_map(|r| r.strip_prefix("gitoid:tree:sha1:"))
    }) else {
        return Ok(false);
    };
    let archive = dependencies::capture(git_committed_bytes(registry).args([
        "archive",
        "--format=tar",
        tree,
    ]))?;
    fs::create_dir_all(dest)?;
    tar::Archive::new(archive.as_slice()).unpack(dest)?;
    Ok(true)
}

fn native_git_source(downloads: &Path, port: &str, location: &str, dest: &Path) -> Result<()> {
    let (repo, rev) = location
        .strip_prefix("git+")
        .and_then(|source| source.rsplit_once('@'))
        .context("native Git source requires a repository and commit")?;
    if !repo.starts_with("https://")
        || rev.len() != 40
        || !rev.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("native Git source requires an HTTPS repository and exact commit");
    }
    package::portable(port)?;
    let archive = dest.join(format!("{port}-{rev}.tar.gz"));
    if archive.exists() {
        return Ok(());
    }
    let cached = downloads.join("git-tmp");
    let cached_rev = dependencies::capture(
        git_committed_bytes(&cached)
            .args(["rev-parse", "--verify"])
            .arg(format!("{rev}^{{commit}}")),
    );
    let checkout;
    let source = if cached_rev
        .as_ref()
        .is_ok_and(|actual| String::from_utf8_lossy(actual).trim() == rev)
    {
        cached.as_path()
    } else {
        checkout = crate::temporary::directory()?;
        dependencies::run(git_committed_bytes(checkout.path()).args(["init", "-q"]))?;
        dependencies::run(git_committed_bytes(checkout.path()).args([
            "fetch",
            "-q",
            "--depth",
            "1",
            "--no-tags",
            repo,
            rev,
        ]))?;
        let actual = dependencies::capture(
            git_committed_bytes(checkout.path()).args(["rev-parse", "FETCH_HEAD"]),
        )?;
        if String::from_utf8_lossy(&actual).trim() != rev {
            bail!("native Git repository did not return pinned commit {rev}");
        }
        checkout.path()
    };
    fs::create_dir_all(dest)?;
    dependencies::run(
        git_committed_bytes(source)
            .args(["archive", "--format=tar.gz", "-o"])
            .arg(archive)
            .arg(rev),
    )
}

/// Copy the downloaded archives vcpkg recorded by SHA512 in the installed SPDX.
pub(crate) fn collect_archives(
    downloads: &Path,
    dest: &Path,
    mut required: std::collections::BTreeSet<String>,
) -> Result<()> {
    if required.is_empty() {
        return Ok(());
    }
    for entry in fs::read_dir(downloads)? {
        let path = entry?.path();
        if path.is_file() && required.remove(&sha512(&path)?) {
            copy_input(&path, &dest.join(path.file_name().unwrap()))?;
        }
    }
    if !required.is_empty() {
        bail!(
            "required corresponding-source archives missing or SHA512 mismatch in {}: {}",
            downloads.display(),
            required.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    Ok(())
}

pub(crate) const RENPY_BUILD_PIN: &str = "7bfab40c1174f622f644b24669afd5fb167fbb79";

pub(crate) fn prepare_renpy_build(inv: &Inventory) -> Result<()> {
    let repo = inv.root.join(".work/renpy-build");
    // A fresh `--no-checkout` clone has no index yet, so `git status` reports
    // every tracked file as deleted; only an existing checkout must be clean
    // before it is moved to the pin.
    let checkout = |directory: &Path, existing: bool| -> Result<()> {
        checkout_root(directory)?;
        pin_checkout_config(directory)?;
        if existing {
            require_clean(directory)?;
        }
        if !git(directory)
            .args(["cat-file", "-e", RENPY_BUILD_PIN])
            .status()?
            .success()
        {
            dependencies::run(git(directory).args([
                "fetch",
                "--no-tags",
                "https://github.com/renpy/renpy-build.git",
                RENPY_BUILD_PIN,
            ]))?;
        }
        dependencies::run(git_committed_bytes(directory).args([
            "checkout",
            "--detach",
            RENPY_BUILD_PIN,
        ]))?;
        dependencies::run(git_committed_bytes(directory).args([
            "submodule",
            "update",
            "--init",
            "--recursive",
        ]))?;
        require_clean(directory)
    };
    if repo.exists() {
        return checkout(&repo, true);
    }
    package::atomic_directory(&repo, |stage| {
        dependencies::run(
            Command::new("git")
                .args([
                    "clone",
                    "--no-checkout",
                    "--config",
                    "core.autocrlf=false",
                    "--config",
                    "core.eol=lf",
                    "--config",
                    "core.filemode=false",
                    "--config",
                    "core.symlinks=false",
                    "https://github.com/renpy/renpy-build.git",
                ])
                .arg(stage),
        )?;
        checkout(stage, false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(directory: &Path, text: &str) -> String {
        fs::write(directory.join("payload.txt"), text).unwrap();
        dependencies::run(git(directory).args(["add", "Cargo.toml", "payload.txt"])).unwrap();
        dependencies::run(git(directory).args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "public fixture",
        ]))
        .unwrap();
        revision(directory).unwrap()
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, String) {
        let temp = crate::temporary::directory().unwrap();
        let source = temp.path().join("upstream");
        fs::create_dir(&source).unwrap();
        dependencies::run(Command::new("git").args(["init"]).arg(&source)).unwrap();
        fs::write(source.join("Cargo.toml"), "[package]\nname = \"public-source-fixture\"\nversion = \"0.1.0\"\n").unwrap();
        let first = commit(&source, "first public source\n");
        commit(&source, "second public source\n");
        (temp, source, first)
    }

    #[test]
    fn native_git_sources_archive_the_pinned_commit_without_touching_working_edits() {
        let (temp, upstream, first) = fixture();
        let downloads = temp.path().join("downloads");
        fs::create_dir(&downloads).unwrap();
        let cached = downloads.join("git-tmp");
        fs::rename(upstream, &cached).unwrap();
        fs::write(cached.join("payload.txt"), "preserved working edit\n").unwrap();
        let output = temp.path().join("sources");
        native_git_source(
            &downloads,
            "native-dependency",
            &format!("git+https://example.invalid/native-source@{first}"),
            &output,
        )
        .unwrap();
        let archive =
            fs::File::open(output.join(format!("native-dependency-{first}.tar.gz"))).unwrap();
        let extracted = temp.path().join("extracted-git-source");
        tar::Archive::new(flate2::read::GzDecoder::new(archive))
            .unpack(&extracted)
            .unwrap();
        assert_eq!(
            fs::read_to_string(extracted.join("payload.txt")).unwrap(),
            "first public source\n"
        );
        assert_eq!(
            fs::read_to_string(cached.join("payload.txt")).unwrap(),
            "preserved working edit\n"
        );
    }

    #[test]
    fn installed_registry_tree_wins_over_head_and_overlays_are_skipped() {
        let temp = crate::temporary::directory().unwrap();
        let registry = temp.path().join("registry");
        let head = registry.join("ports/example");
        fs::create_dir_all(&head).unwrap();
        dependencies::run(git(&registry).args(["init"])).unwrap();
        let commit = || {
            dependencies::run(git(&registry).args(["add", "."])).unwrap();
            dependencies::run(git(&registry).args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "recipe fixture",
            ]))
            .unwrap()
        };
        fs::write(head.join("portfile.cmake"), "original recipe\n").unwrap();
        fs::write(head.join("fix.patch"), "original patch\n").unwrap();
        commit();
        let tree = String::from_utf8(
            dependencies::capture(git(&registry).args(["rev-parse", "HEAD:ports/example"]))
                .unwrap(),
        )
        .unwrap();
        fs::write(head.join("portfile.cmake"), "new HEAD recipe\n").unwrap();
        commit();
        let provenance = serde_json::json!({"packages":[{"SPDXID":"SPDXRef-port", "externalRefs":[{
            "referenceLocator":format!("gitoid:tree:sha1:{}", tree.trim())
        }]}]});
        let output = temp.path().join("bundled");
        assert!(port_recipe(&provenance, &registry, &output).unwrap());
        assert_eq!(
            fs::read_to_string(output.join("portfile.cmake")).unwrap(),
            "original recipe\n"
        );
        assert_eq!(
            fs::read_to_string(output.join("fix.patch")).unwrap(),
            "original patch\n"
        );
        let overlay = serde_json::json!({"packages":[{"SPDXID":"SPDXRef-port", "downloadLocation":"NOASSERTION"}]});
        let skipped = temp.path().join("overlay");
        assert!(!port_recipe(&overlay, &registry, &skipped).unwrap());
        assert!(!skipped.exists());
    }
}
