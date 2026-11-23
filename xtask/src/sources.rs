use crate::{
    cores::{Core, Inventory},
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
/// (breaking LFS pointer checks) and in fork checkouts. Status checks keep the
/// checkout's own settings, or a CRLF product checkout would look modified.
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
            "{} is not the root of the requested fork checkout; refusing to operate on its parent repository",
            directory.display()
        );
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
        bail!(
            "fork checkout {} contains local modifications; commit or move them before explicit deps preparation (nothing was reset)",
            directory.display()
        );
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
/// Verify original upstream reachability independently of organization branch names.
pub fn verify_provenance(inv: &Inventory, core: &Core) -> Result<()> {
    let Some(source) = &core.source else {
        return Ok(());
    };
    let directory = inv.root.join(".work/provenance").join(&core.id);
    if !directory.exists() {
        fs::create_dir_all(&directory)?;
        dependencies::run(
            Command::new("git")
                .args(["init", "--bare", "--quiet"])
                .arg(&directory),
        )?;
    }
    dependencies::run(git(&directory).args([
        "fetch",
        "--filter=blob:none",
        "--no-tags",
        &source.repo,
        &source.rev,
    ]))?;
    let upstream = &source.upstream_identity;
    let namespace = match upstream.ref_kind {
        shinki_emu_metadata::UpstreamRefKind::Tag => "tags",
        shinki_emu_metadata::UpstreamRefKind::Branch => "heads",
    };
    let reference = format!(
        "+refs/{namespace}/{}:refs/proof/upstream",
        upstream.ref_name
    );
    dependencies::run(git(&directory).args([
        "fetch",
        "--filter=blob:none",
        "--no-tags",
        &upstream.repo,
        &reference,
        "+refs/tags/*:refs/proof/tags/*",
    ]))?;
    verify_provenance_at(source, &directory)
}

fn verify_provenance_at(source: &crate::cores::CoreSource, directory: &Path) -> Result<()> {
    let upstream = &source.upstream_identity;
    let at = String::from_utf8(dependencies::capture(
        git(directory).args(["rev-parse", "refs/proof/upstream^{commit}"]),
    )?)?;
    if upstream.ref_kind == shinki_emu_metadata::UpstreamRefKind::Tag
        && at.trim() != upstream.commit
    {
        bail!("upstream tag does not resolve to the declared full commit");
    }
    for descendant in [&source.rev, at.trim()] {
        dependencies::run(git(directory).args([
            "merge-base",
            "--is-ancestor",
            &upstream.commit,
            descendant,
        ]))
        .context(
            "declared upstream commit must be contained in both upstream ref and adapter pin",
        )?;
    }
    if upstream.ref_kind == shinki_emu_metadata::UpstreamRefKind::Branch {
        let tags = dependencies::capture(git(directory).args([
            "for-each-ref",
            &format!("--points-at={}", upstream.commit),
            "--format=%(refname)",
            "refs/proof/tags",
        ]))?;
        if !tags.is_empty() {
            bail!("upstream commit has a verified tag; select that tag rather than a branch");
        }
    }
    Ok(())
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
    pin_checkout_config(directory)?;
    dependencies::run(git_committed_bytes(directory).args(["checkout", "--detach", &source.rev]))?;
    dependencies::run(git_committed_bytes(directory).args([
        "submodule",
        "update",
        "--init",
        "--recursive",
    ]))?;
    verify_at(core, directory, true)
}

/// Prepare one exact source revision without replacing dirty or unrelated trees.
pub fn prepare(inv: &Inventory, core: &Core, allow_dirty: bool) -> Result<()> {
    let Some(source) = &core.source else {
        return Ok(());
    };
    let directory = core.source_directory(&inv.root)?;
    if fs::symlink_metadata(&directory).is_ok() {
        checkout_root(&directory)?;
        if allow_dirty && revision(&directory)? == source.rev && modified(&directory)? {
            // Native dependencies can be prepared for unpublished adapter work.
            // Do not check out, fetch, or rewrite any part of this source tree.
            return verify_at(core, &directory, false);
        }
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
                    "--config",
                    "core.eol=lf",
                    "--config",
                    "core.filemode=false",
                    "--config",
                    "core.symlinks=false",
                    "--config",
                    "core.longpaths=true",
                    "--",
                    &source.repo,
                ])
                .arg(stage)
                .env("GIT_TERMINAL_PROMPT", "0"),
        )?;
        checkout_pin(core, stage)
    })
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

    #[test]
    fn provenance_requires_real_ref_resolution_and_shared_adapter_ancestry() {
        let (_temp, _inv, core, repository, first, second) = fixture();
        let mut source = core.source.unwrap();
        source.rev = second.clone();
        source.upstream_identity.commit = first.clone();
        source.upstream_identity.ref_kind = shinki_emu_metadata::UpstreamRefKind::Branch;
        dependencies::run(git(&repository).args(["update-ref", "refs/proof/upstream", &second]))
            .unwrap();
        verify_provenance_at(&source, &repository).unwrap();
        source.upstream_identity.ref_kind = shinki_emu_metadata::UpstreamRefKind::Tag;
        assert!(verify_provenance_at(&source, &repository).is_err());
        dependencies::run(git(&repository).args(["update-ref", "refs/proof/upstream", &first]))
            .unwrap();
        verify_provenance_at(&source, &repository).unwrap();
        source.upstream_identity.commit = second;
        assert!(verify_provenance_at(&source, &repository).is_err());
    }

    fn fixture() -> (tempfile::TempDir, Inventory, Core, PathBuf, String, String) {
        let temp = crate::temporary::directory().unwrap();
        let source = temp.path().join("upstream");
        fs::create_dir(&source).unwrap();
        dependencies::run(
            Command::new("git")
                .args(["init", "-b", "shinki"])
                .arg(&source),
        )
        .unwrap();
        fs::write(source.join("Cargo.toml"), "[package]\nname = \"public-source-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n").unwrap();
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
            upstream: "https://github.com/xmoezzz/rfvp".into(),
            upstream_identity: inv
                .core("rfvp")
                .unwrap()
                .source
                .as_ref()
                .unwrap()
                .upstream_identity
                .clone(),
            release_sequence: 1,
        });
        (temp, inv, core, source, first, second)
    }

    #[test]
    fn source_checkout_uses_pinned_commit_and_preserves_dirty_work_before_update() {
        let (_temp, inv, mut core, _upstream, first, second) = fixture();
        prepare(&inv, &core, false).unwrap();
        let directory = verify(&inv, &core, true).unwrap();
        assert_eq!(revision(&directory).unwrap(), first);
        assert_eq!(
            fs::read_to_string(directory.join("payload.txt")).unwrap(),
            "first public source\n"
        );
        fs::write(directory.join("payload.txt"), "user's unpublished change\n").unwrap();
        assert!(prepare(&inv, &core, false).is_err());
        prepare(&inv, &core, true).unwrap();
        assert_eq!(
            fs::read_to_string(directory.join("payload.txt")).unwrap(),
            "user's unpublished change\n"
        );
        core.source.as_mut().unwrap().rev = second.clone();
        assert!(prepare(&inv, &core, false).is_err());
        assert!(prepare(&inv, &core, true).is_err());
        assert_eq!(revision(&directory).unwrap(), first);
        assert_eq!(
            fs::read_to_string(directory.join("payload.txt")).unwrap(),
            "user's unpublished change\n"
        );
        fs::write(directory.join("payload.txt"), "first public source\n").unwrap();
        prepare(&inv, &core, false).unwrap();
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
        assert!(prepare(&inv, &core, false).is_err());
        assert!(prepare(&inv, &core, true).is_err());
        assert_eq!(revision(&inv.root).unwrap(), product_head);
        assert_eq!(
            fs::read_to_string(directory.join("user.txt")).unwrap(),
            "keep this folder"
        );
    }

    #[test]
    fn native_git_sources_archive_the_pinned_commit_without_touching_working_edits() {
        let (temp, _inv, _core, upstream, first, _second) = fixture();
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
