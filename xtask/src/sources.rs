use crate::{
    cores::{Core, Inventory},
    dependencies, package,
};
use anyhow::{bail, Context, Result};
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
        bail!("{} is not the root of the requested fork checkout; refusing to operate on its parent repository", directory.display());
    }
    Ok(())
}

pub struct SourceState {
    pub commit: String,
    pub dirty: bool,
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

pub fn state(directory: &Path) -> Result<SourceState> {
    checkout_root(directory)?;
    Ok(SourceState {
        commit: revision(directory)?,
        dirty: modified(directory)?,
    })
}

fn require_clean(directory: &Path) -> Result<()> {
    if modified(directory)? {
        bail!("fork checkout {} contains local modifications; commit or move them before explicit deps preparation (nothing was reset)", directory.display());
    }
    Ok(())
}

fn adapter_relative(core: &Core) -> Result<PathBuf> {
    let mut components = core.manifest.components();
    if components
        .next()
        .is_none_or(|component| component.as_os_str() != "ThirdParty")
        || components.next().is_none()
    {
        bail!("external adapter manifest must be inside one ThirdParty checkout");
    }
    let relative: PathBuf = components.collect();
    if relative.as_os_str().is_empty() {
        bail!("external adapter manifest path is missing");
    }
    Ok(relative)
}

fn verify_at(core: &Core, directory: &Path, clean: bool) -> Result<()> {
    let source = core
        .source
        .as_ref()
        .context("external source declaration missing")?;
    checkout_root(directory)?;
    let actual = revision(directory)?;
    if actual != source.rev {
        bail!(
            "{} source revision mismatch: expected {}, found {}; run cargo xtask deps --core {}",
            core.id,
            source.rev,
            actual,
            core.id
        );
    }
    if clean {
        require_clean(directory)?;
    }
    let manifest = directory.join(adapter_relative(core)?);
    package::plain(&manifest).with_context(|| {
        format!(
            "pinned source {}@{} lacks adapter manifest {}",
            source.repo,
            source.rev,
            manifest.display()
        )
    })?;
    if !manifest.is_file() {
        bail!("adapter manifest is not a file: {}", manifest.display());
    }
    Ok(())
}

/// Read-only preflight. Only explicit deps preparation fetches or checks out code.
pub fn verify(inv: &Inventory, core: &Core, clean: bool) -> Result<PathBuf> {
    let directory = core.source_directory(&inv.root)?;
    if core.source.is_some() {
        verify_at(core, &directory, clean)?;
    }
    Ok(directory)
}

fn checkout_pin(core: &Core, directory: &Path) -> Result<()> {
    let source = core
        .source
        .as_ref()
        .context("external source declaration missing")?;
    let object = format!("{}^{{commit}}", source.rev);
    let available = git(directory).args(["cat-file", "-e", &object]).output()?;
    if !available.status.success() {
        dependencies::run(git(directory).args([
            "fetch",
            "--filter=blob:none",
            "--no-tags",
            &source.repo,
            &source.rev,
        ]))?;
    }
    dependencies::run(git(directory).args(["checkout", "--detach", &source.rev]))?;
    dependencies::run(git(directory).args(["submodule", "update", "--init", "--recursive"]))?;
    verify_at(core, directory, true)
}

/// Prepare one exact source revision without replacing dirty or unrelated trees.
pub fn prepare(inv: &Inventory, core: &Core) -> Result<()> {
    let Some(source) = &core.source else {
        return Ok(());
    };
    let directory = core.source_directory(&inv.root)?;
    if fs::symlink_metadata(&directory).is_ok() {
        checkout_root(&directory)?;
        require_clean(&directory)?;
        return checkout_pin(core, &directory);
    }
    package::atomic_directory(&directory, |stage| {
        dependencies::run(
            Command::new("git")
                .args([
                    "clone",
                    "--filter=blob:none",
                    "--no-checkout",
                    "--single-branch",
                    "--branch",
                    &source.branch,
                    "--config",
                    "core.autocrlf=false",
                    "--",
                    &source.repo,
                ])
                .arg(stage)
                .env("GIT_TERMINAL_PROMPT", "0"),
        )?;
        checkout_pin(core, stage)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cores::CoreSource;

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

    fn fixture() -> (tempfile::TempDir, Inventory, Core, PathBuf, String, String) {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("upstream");
        fs::create_dir(&source).unwrap();
        dependencies::run(
            Command::new("git")
                .args(["init", "-b", "shinki"])
                .arg(&source),
        )
        .unwrap();
        fs::write(source.join("Cargo.toml"), "[package]\nname = \"public-source-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
        let first = commit(&source, "first public source\n");
        let second = commit(&source, "second public source\n");
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inv = Inventory::load(repository).unwrap();
        inv.root = temp.path().join("product");
        fs::create_dir(&inv.root).unwrap();
        let mut core = inv.core("rfvp").unwrap().clone();
        core.manifest = "ThirdParty/fixture/Cargo.toml".into();
        core.source = Some(CoreSource {
            repo: source.to_string_lossy().into_owned(),
            branch: "shinki".into(),
            rev: first.clone(),
        });
        (temp, inv, core, source, first, second)
    }

    #[test]
    fn source_checkout_uses_pinned_commit_and_preserves_dirty_work_before_update() {
        let (_temp, inv, mut core, _upstream, first, second) = fixture();
        prepare(&inv, &core).unwrap();
        let directory = verify(&inv, &core, true).unwrap();
        assert_eq!(revision(&directory).unwrap(), first);
        assert_eq!(
            fs::read_to_string(directory.join("payload.txt")).unwrap(),
            "first public source\n"
        );
        fs::write(directory.join("payload.txt"), "user's unpublished change\n").unwrap();
        core.source.as_mut().unwrap().rev = second.clone();
        assert!(prepare(&inv, &core).is_err());
        assert_eq!(revision(&directory).unwrap(), first);
        assert_eq!(
            fs::read_to_string(directory.join("payload.txt")).unwrap(),
            "user's unpublished change\n"
        );
        fs::write(directory.join("payload.txt"), "first public source\n").unwrap();
        prepare(&inv, &core).unwrap();
        assert_eq!(revision(&directory).unwrap(), second);
        assert_eq!(
            fs::read_to_string(directory.join("payload.txt")).unwrap(),
            "second public source\n"
        );
    }

    #[test]
    fn existing_plain_folder_cannot_redirect_checkout_into_the_product_repository() {
        let (_temp, inv, core, _upstream, _first, _second) = fixture();
        dependencies::run(Command::new("git").args(["init"]).arg(&inv.root)).unwrap();
        fs::write(inv.root.join("Cargo.toml"), "[workspace]\n").unwrap();
        let product_head = commit(&inv.root, "product source\n");
        let directory = core.source_directory(&inv.root).unwrap();
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("user.txt"), "keep this folder").unwrap();
        assert!(prepare(&inv, &core).is_err());
        assert_eq!(revision(&inv.root).unwrap(), product_head);
        assert_eq!(
            fs::read_to_string(directory.join("user.txt")).unwrap(),
            "keep this folder"
        );
    }
}
