use crate::{
    cores::{Core, Inventory},
    dependencies, package,
};
use anyhow::{Context, Result, bail};
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
                    "--",
                    &source.repo,
                ])
                .arg(stage)
                .env("GIT_TERMINAL_PROMPT", "0"),
        )?;
        checkout_pin(core, stage)
    })
}

/// Append tracked files and every gitlink at its recorded revision. Never use
/// `git archive` alone for a fork: it silently leaves submodules empty.
fn append_revision(
    builder: &mut tar::Builder<fs::File>,
    repository: &Path,
    rev: &str,
    prefix: &Path,
    recursive: bool,
) -> Result<()> {
    let raw = tempfile::NamedTempFile::new()?;
    dependencies::run(
        git(repository).args(["archive", "--format=tar", rev, "-o"]).arg(raw.path()),
    )?;
    for entry in tar::Archive::new(fs::File::open(raw.path())?).entries()? {
        let mut entry = entry?;
        let relative = entry.path()?.into_owned();
        let path = prefix.join(&relative);
        let mut header = entry.header().clone();
        if header.entry_type().is_file() && entry.size() <= 1024 {
            let size = entry.size() as usize;
            let mut bytes = [0u8; 1024];
            std::io::Read::read_exact(&mut entry, &mut bytes[..size])?;
            if bytes[..size].starts_with(b"version https://git-lfs.github.com/spec/v1\n") {
                let pointer = std::str::from_utf8(&bytes[..size])?;
                let oid = pointer.lines().find_map(|line| line.strip_prefix("oid sha256:"))
                    .context("Git LFS source lacks SHA256")?;
                if oid.len() != 64 || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
                    bail!("invalid Git LFS source SHA256 for {}", relative.display());
                }
                let expected: u64 = pointer.lines().find_map(|line| line.strip_prefix("size "))
                    .context("Git LFS source lacks size")?.parse()?;
                let source = repository.join(&relative);
                if fs::metadata(&source).with_context(|| format!("missing Git LFS source {}", source.display()))?.len() != expected
                    || !package::sha256(&source)?.eq_ignore_ascii_case(oid)
                {
                    bail!("Git LFS source {} is missing, unmaterialized or changed; expected SHA256 {oid}", source.display());
                }
                header.set_size(expected);
                header.set_cksum();
                builder.append_data(&mut header, path, fs::File::open(source)?)?;
            } else {
                builder.append_data(&mut header, path, &bytes[..size])?;
            }
        } else {
            builder.append_data(&mut header, path, &mut entry)?;
        }
    }
    if recursive {
        for (path, pin) in gitlinks(repository, rev)? {
            let child = repository.join(&path);
            checkout_root(&child).with_context(|| format!("missing recursive source {}", child.display()))?;
            if revision(&child)? != pin {
                bail!("submodule {} is not at recorded revision {pin}", child.display());
            }
            require_clean(&child)?;
            append_revision(builder, &child, &pin, &prefix.join(path), true)?;
        }
    }
    Ok(())
}

fn gitlinks(repository: &Path, rev: &str) -> Result<Vec<(PathBuf, String)>> {
    let listing = dependencies::capture(git(repository).args(["ls-tree", "-rz", rev]))?;
    let mut links = Vec::new();
    for item in listing.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let item = std::str::from_utf8(item)?;
        let (record, path) = item.split_once('\t').context("git tree entry")?;
        if let Some(pin) = record.strip_prefix("160000 commit ") {
            links.push((PathBuf::from(path), pin.to_owned()));
        }
    }
    Ok(links)
}

fn copy_tree(source: &Path, dest: &Path) -> Result<()> {
    package::plain(source)?;
    fs::create_dir_all(dest)?;
    for item in walkdir::WalkDir::new(source).follow_links(false) {
        let item = item?;
        package::plain(item.path())?;
        let to = dest.join(item.path().strip_prefix(source)?);
        if item.file_type().is_dir() {
            fs::create_dir_all(to)?;
        } else {
            fs::copy(item.path(), to)?;
        }
    }
    Ok(())
}

fn copy_input(source: &Path, dest: &Path) -> Result<()> {
    package::plain(source).with_context(|| format!("required corresponding-source input {}", source.display()))?;
    fs::create_dir_all(dest.parent().context("source input parent")?)?;
    fs::copy(source, dest)?;
    Ok(())
}

/// Cargo verifies checksums while reading the vendor tree. Validate it before
/// publication as well, including files not compiled on this release target.
pub(crate) fn verify_vendor(vendor: &Path) -> Result<()> {
    package::plain(vendor)?;
    for entry in fs::read_dir(vendor)? {
        let dir = entry?.path();
        package::plain(&dir)?;
        let record: serde_json::Value = serde_json::from_slice(
            &fs::read(dir.join(".cargo-checksum.json"))
                .with_context(|| format!("vendored source missing checksum manifest: {}", dir.display()))?,
        )?;
        for (name, expected) in record["files"].as_object().context("vendor file checksums")? {
            let relative = Path::new(name);
            if relative.is_absolute() || relative.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
                bail!("unsafe vendored source path {name}");
            }
            if package::sha256(&dir.join(relative))? != expected.as_str().context("vendor SHA256")? {
                bail!("vendored source checksum mismatch: {}/{name}", dir.display());
            }
        }
    }
    Ok(())
}

fn vendor(manifest: &Path, root: &Path) -> Result<()> {
    fs::create_dir_all(root.join("vendor"))?;
    let output = dependencies::capture(
        Command::new("cargo")
            .current_dir(manifest.parent().context("manifest directory")?)
            .args(["vendor", "--locked", "--offline", "--versioned-dirs", "--manifest-path"])
            .arg(manifest)
            .arg(root.join("vendor")),
    )?;
    let mut config: toml::Value = toml::from_str(std::str::from_utf8(&output)?)?;
    if let Some(sources) = config.get_mut("source").and_then(toml::Value::as_table_mut) {
        for (_, source) in sources.iter_mut() {
            if source.get("directory").is_some() {
                source["directory"] = toml::Value::String("vendor".into());
            }
        }
    }
    let path = root.join(".cargo/config.toml");
    let mut existing: toml::Value = if path.is_file() {
        toml::from_str(&fs::read_to_string(&path)?)?
    } else {
        toml::Value::Table(Default::default())
    };
    let table = existing.as_table_mut().context("Cargo config table")?;
    if let Some(source) = config.get("source") {
        table.insert("source".into(), source.clone());
    }
    table.insert("net".into(), toml::Value::Table(
        [("offline".into(), toml::Value::Boolean(true))].into_iter().collect(),
    ));
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, toml::to_string(&existing)?)?;
    verify_vendor(&root.join("vendor"))
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
        if count == 0 { break; }
        digest.update(&buffer[..count]);
    }
    Ok(hex::encode(digest.finalize()))
}

/// Collect every installed target port, including static libraries. The pinned
/// vcpkg Git bundle also carries host build-helper ports and toolchain scripts.
fn native_sources(
    inv: &Inventory,
    component: &str,
    target: &str,
    root: &Path,
    metadata: &serde_json::Value,
) -> Result<String> {
    let selected = dependencies::needs(inv, target, component)?;
    let mut installations = Vec::new();
    if selected.subtitle {
        installations.push(("subtitle", dependencies::subtitle_install(inv, target)?));
    }
    if selected.ffmpeg {
        installations.push(("ffmpeg", if target == "x86_64-unknown-linux-gnu" {
            dependencies::linux_install(inv, target, "ffmpeg")
        } else { inv.root.join(".work/vcpkg-ffmpeg/installed/x64-windows") }));
    }
    if selected.native_vcpkg {
        installations.push(("native", inv.root.join(".work/native-vcpkg").join(component).join("installed").join(
            if target == "x86_64-unknown-linux-gnu" { "x64-linux-dynamic" } else { "x64-windows" }
        )));
    }
    let mut required = std::collections::BTreeSet::new();
    let mut commands = String::new();
    for (group, installed) in &installations {
        package::plain(&installed.join("share"))?;
        for entry in fs::read_dir(installed.join("share"))? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() { continue; }
            let port = entry.file_name().to_str().context("port name")?.to_owned();
            let spdx = entry.path().join("vcpkg.spdx.json");
            package::plain(&spdx).with_context(|| format!("installed port {port} missing source provenance"))?;
            let record: serde_json::Value = serde_json::from_slice(&fs::read(&spdx)?)?;
            let overlay = if selected.native_vcpkg {
                inv.core(component)?.source_directory(&inv.root)?.join("vcpkg/ports").join(&port)
            } else { PathBuf::new() };
            let recipe = if overlay.join("portfile.cmake").is_file() {
                overlay
            } else { inv.root.join(".work/vcpkg/ports").join(&port) };
            package::verify_port_sources(&record, &recipe)?;
            copy_tree(&recipe, &root.join("native/ports").join(group).join(&port))?;
            copy_input(&spdx, &root.join("native/provenance").join(group).join(format!("{port}.spdx.json")))?;
            for resource in record["packages"].as_array().context("SPDX packages")? {
                if !resource["SPDXID"].as_str().is_some_and(|s| s.starts_with("SPDXRef-resource-")) { continue; }
                let checksum = resource["checksums"].as_array()
                    .and_then(|a| a.iter().find(|c| c["algorithm"] == "SHA512"))
                    .and_then(|c| c["checksumValue"].as_str())
                    .context("native source resource missing SHA512")?;
                if checksum.len() != 128 || !checksum.bytes().all(|b| b.is_ascii_hexdigit()) {
                    bail!("invalid native source SHA512 for {port}");
                }
                required.insert(checksum.to_ascii_lowercase());
            }
        }
        let triplet = installed.file_name().context("native triplet")?.to_string_lossy();
        let install_root = installed.parent().context("install root")?.strip_prefix(&inv.root)?.to_string_lossy().replace('\\', "/");
        let manifest = match *group {
            "subtitle" => "build/vcpkg/subtitle".to_owned(),
            "ffmpeg" => "build/vcpkg/ffmpeg".to_owned(),
            _ => inv.core(component)?.source_directory(&inv.root)?.strip_prefix(&inv.root)?.to_string_lossy().replace('\\', "/"),
        };
        commands.push_str(&format!(
            "& $vcpkg install --vcpkg-root=\".work/vcpkg\" --no-downloads --triplet {triplet} --x-manifest-root=\"{manifest}\" --x-install-root=\"{install_root}\" --overlay-ports=\"native/ports/{group}\""
        ));
        if *group == "subtitle" {
            commands.push_str(" --overlay-triplets=\"native/subtitle-triplets\"");
        }
        commands.push('\n');
    }
    collect_archives(&inv.root.join(".work/vcpkg-downloads"), &root.join(".work/vcpkg-downloads"), required)?;
    if selected.subtitle {
        // The pinned engine supplies custom mixed-linkage triplets.
        let engine = metadata["packages"].as_array().context("Cargo packages")?.iter()
            .find(|p| p["name"] == "roukaku-subtitle").context("roukaku-subtitle source missing")?;
        let manifest = Path::new(engine["manifest_path"].as_str().context("subtitle manifest")?);
        copy_tree(&manifest.parent().unwrap().join("triplets"), &root.join("native/subtitle-triplets"))?;
    }
    if !installations.is_empty() {
        let repo = inv.root.join(".work/vcpkg");
        let pins: toml::Value = toml::from_str(&fs::read_to_string(inv.root.join("build/dependencies.toml"))?)?;
        let pin = pins["vcpkg_commit"].as_str().context("vcpkg commit")?;
        if revision(&repo)? != pin { bail!("vcpkg source is not at pinned revision {pin}"); }
        // Versioned manifests need baseline/tree objects, not just a checkout.
        // Retaining the reachable history makes vcpkg registry resolution offline.
        dependencies::run(
            git(&repo).args(["bundle", "create"]).arg(root.join("native/vcpkg.bundle")).arg("HEAD"),
        )?;
        let tool = if target.contains("windows") {
            "$vcpkg = (Get-Command vcpkg.exe).Source"
        } else {
            "vcpkg=$(command -v vcpkg)"
        };
        commands = format!(
            "git clone --no-checkout native/vcpkg.bundle .work/vcpkg\ngit -C .work/vcpkg checkout --detach {pin}\n{tool}\n{commands}"
        );
    }
    if selected.impacto_sources {
        native_archives(inv, root, &["libatrac9", "magic_enum"])?;
        for name in ["libatrac9", "magic_enum"] {
            copy_tree(&inv.root.join(".work/native-downloads").join(name), &root.join(".work/native-downloads").join(name))?;
        }
    }
    if selected.renpy {
        renpy_sources(inv, root)?;
    }
    Ok(commands)
}

pub(crate) fn collect_archives(
    downloads: &Path,
    dest: &Path,
    mut required: std::collections::BTreeSet<String>,
) -> Result<()> {
    if required.is_empty() { return Ok(()); }
    for entry in fs::read_dir(downloads)? {
        let path = entry?.path();
        if path.is_file() && required.remove(&sha512(&path)?) {
            copy_input(&path, &dest.join(path.file_name().unwrap()))?;
        }
    }
    if !required.is_empty() {
        bail!("required corresponding-source archives missing or SHA512 mismatch in {}: {}", downloads.display(), required.into_iter().collect::<Vec<_>>().join(", "));
    }
    Ok(())
}

fn native_archives(inv: &Inventory, root: &Path, names: &[&str]) -> Result<()> {
    let pins: toml::Value = toml::from_str(&fs::read_to_string(inv.root.join("build/dependencies.toml"))?)?;
    for name in names {
        let pin = &pins["native"][name];
        let filename = pin["file_name"].as_str().context("native archive filename")?;
        let source = inv.root.join(".work/downloads").join(filename);
        if package::sha256(&source)? != pin["sha256"].as_str().context("native archive SHA256")? {
            bail!("native archive checksum mismatch: {filename}");
        }
        copy_input(&source, &root.join(".work/downloads").join(filename))?;
    }
    Ok(())
}

fn renpy_sources(inv: &Inventory, root: &Path) -> Result<()> {
    const PIN: &str = "7bfab40c1174f622f644b24669afd5fb167fbb79";
    let repo = inv.root.join(".work/renpy-build");
    checkout_root(&repo).context("Ren'Py corresponding source requires .work/renpy-build")?;
    if revision(&repo)? != PIN { bail!("renpy-build must be at {PIN}"); }
    require_clean(&repo)?;
    fs::create_dir_all(root.join("native"))?;
    let mut builder = tar::Builder::new(fs::File::create(root.join("native/renpy-build.tar"))?);
    append_revision(&mut builder, &repo, PIN, Path::new(""), true)?;
    builder.finish()?;
    // The SDK supplies bytecode without dist-info. This fork-side inventory
    // records versions read from that bytecode, not the different recipe pins.
    let inventory = inv.core("renpy")?.source_directory(&inv.root)?
        .join("licenses/renpy/python-sources.json");
    let lock: serde_json::Value = serde_json::from_slice(&fs::read(&inventory)?)?;
    if lock["renpy_build"] != PIN || lock["sdk"] != "8.5.3.26051504" {
        bail!("Ren'Py runtime source inventory revision mismatch");
    }
    let packages = lock["packages"].as_array().context("Ren'Py Python source packages")?;
    for required in ["chardet", "certifi"] {
        if !packages.iter().any(|p| p["name"] == required && p["copyleft"] == true) {
            bail!("Ren'Py source inventory lacks {required}");
        }
    }
    for item in packages {
        let filename = item["file_name"].as_str().context("Python source filename")?;
        package::portable(filename)?;
        let archive = inv.root.join(".work/downloads").join(filename);
        if package::sha256(&archive)? != item["sha256"].as_str().context("Python source SHA256")? {
            bail!("Python source checksum mismatch: {filename}");
        }
        copy_input(&archive, &root.join("native/python-sources").join(filename))?;
        if let Some(wheel) = item.get("wheel") {
            copy_hashed_archive(inv, wheel, &root.join("native/python-sources"))?;
        }
    }
    for tool in lock["tools"].as_array().context("Python build tools")? {
        copy_hashed_archive(inv, tool, &root.join("native/python-sources"))?;
    }
    copy_input(&inventory, &root.join("native/python-sources/sources.json"))?;
    let native_inventory = inv.core("renpy")?.source_directory(&inv.root)?
        .join("licenses/renpy/native-sources.json");
    let native: serde_json::Value = serde_json::from_slice(&fs::read(&native_inventory)?)?;
    if native["renpy_build"] != PIN { bail!("Ren'Py native source revision mismatch"); }
    let inputs = native["sources"].as_array().context("Ren'Py native source archives")?;
    for name in ["aom", "libavif", "libyuv", "assimp", "cpython-mingw", "renpy", "pyobjus"] {
        if !inputs.iter().any(|p| p["name"] == name) { bail!("Ren'Py source closure lacks {name}"); }
    }
    for input in inputs {
        let filename = input["file_name"].as_str().context("Ren'Py native source filename")?;
        package::portable(filename)?;
        let archive = inv.root.join(".work/downloads").join(filename);
        if package::sha256(&archive)? != input["sha256"].as_str().context("Ren'Py native source SHA256")? {
            bail!("Ren'Py native source checksum mismatch: {filename}");
        }
        copy_input(&archive, &root.join("native/renpy-sources").join(filename))?;
    }
    copy_input(&native_inventory, &root.join("native/renpy-sources/sources.json"))?;
    native_archives(inv, root, &["renpy-sdk", "python", "cython", "cython-license", "sdl2"])?;
    for name in ["renpy-sdk", "python", "cython-license", "sdl2"] {
        copy_tree(&inv.root.join(".work/native-downloads").join(name), &root.join(".work/native-downloads").join(name))?;
    }
    Ok(())
}

pub(crate) fn bundle(inv: &Inventory, component: &str, target: &str, out: &Path) -> Result<()> {
    let temporary = inv.root.join(".work/tmp");
    fs::create_dir_all(&temporary)?;
    let temp = tempfile::tempdir_in(&temporary)?;
    let product_commit = revision(&inv.root)?;
    require_clean(&inv.root)?;
    let metadata = crate::rust_licenses::metadata(inv, target, component)?;
    let product = temp.path().join("product.tar");
    let mut builder = tar::Builder::new(fs::File::create(&product)?);
    append_revision(&mut builder, &inv.root, &product_commit, Path::new(""), false)?;
    // Include local path dependencies which live in product gitlinks (e.g. CPAL).
    for (path, pin) in gitlinks(&inv.root, &product_commit)? {
        if metadata["packages"].as_array().context("Cargo packages")?.iter().any(|p| {
            p["source"].is_null() && p["manifest_path"].as_str().is_some_and(|s| Path::new(s).starts_with(inv.root.join(&path)))
        }) {
            let repo = inv.root.join(&path);
            if revision(&repo)? != pin { bail!("product submodule {} revision mismatch", path.display()); }
            require_clean(&repo)?;
            append_revision(&mut builder, &repo, &pin, &path, true)?;
        }
    }
    builder.finish()?;
    let root = temp.path().join("source");
    fs::create_dir(&root)?;
    tar::Archive::new(fs::File::open(&product)?).unpack(&root)?;
    let mut references = serde_json::json!({
        "product": {"repo":"https://github.com/Project-Shinkirou/ShinkiEMU","rev":product_commit},
        "core": null, "target": target
    });
    let (manifest, package, features) = if component == "manager" {
        (inv.root.join("Cargo.toml"), "shinki-emu-manager".to_owned(), Vec::new())
    } else {
        let core = inv.core(component)?;
        if let Some(source) = &core.source {
            let repo = verify(inv, core, true)?;
            let archive = temp.path().join("core.tar");
            let mut builder = tar::Builder::new(fs::File::create(&archive)?);
            append_revision(&mut builder, &repo, &source.rev, Path::new(""), true)?;
            builder.finish()?;
            let dest = root.join(repo.strip_prefix(&inv.root)?);
            fs::create_dir_all(&dest)?;
            tar::Archive::new(fs::File::open(&archive)?).unpack(dest)?;
            references["core"] = serde_json::to_value(source)?;
        }
        (inv.root.join(&core.manifest), core.package.clone(), core.features.clone())
    };
    vendor(&manifest, &root)?;
    // Resolve against the extracted tree and its generated source replacement,
    // so omitted path crates or vendor packages cannot become a partial bundle.
    let mut check = Command::new("cargo");
    check.current_dir(&root).args(["metadata", "--offline", "--frozen", "--format-version", "1", "--filter-platform", target, "--manifest-path"])
        .arg(root.join(manifest.strip_prefix(&inv.root)?));
    if !features.is_empty() {
        check.arg("--features").arg(features.iter().map(|f| format!("{package}/{f}")).collect::<Vec<_>>().join(","));
    }
    dependencies::capture(&mut check).context("corresponding source does not resolve offline from its vendor tree")?;
    let native_commands = native_sources(inv, component, target, &root, &metadata)?;
    fs::write(root.join("source-references.json"), serde_json::to_vec_pretty(&references)?)?;
    let manifest_relative = manifest.strip_prefix(&inv.root)?.to_string_lossy().replace('\\', "/");
    let feature_args = if features.is_empty() { String::new() } else { format!(" --features {}", features.join(",")) };
    let mut instructions = format!(
        "# Rebuild {component} for {target}\n\nProduct commit: {product_commit}. Exact core revision: see source-references.json.\n\
         Unpack source.tar into a new directory and run these commands there in a developer PowerShell (Windows) or the equivalent shell on the target OS.\n\
         Prerequisites: the Rust toolchain in rust-toolchain.toml, the {target} standard library, the system C/C++ compiler/SDK, CMake and Ninja. Build tools and OS SDKs are system prerequisites.\n\
         Cargo uses .cargo/config.toml and vendor/, including exact pinned Git dependencies. No registry or Git network access is needed. Keep Cargo.lock unchanged.\n\n```powershell\n\
         $env:CARGO_NET_OFFLINE = 'true'\n\
         $env:CARGO_TARGET_DIR = \"$PWD/target\"\n"
    );
    if !native_commands.is_empty() {
        instructions.push_str(
            "$env:VCPKG_DOWNLOADS = \"$PWD/.work/vcpkg-downloads\"\n\
             $env:VCPKG_BINARY_SOURCES = 'clear'\n\
             $env:VCPKG_FORCE_SYSTEM_BINARIES = '1'\n\
             # A system vcpkg executable, CMake and Ninja must be on PATH.\n"
        );
        instructions.push_str(&native_commands);
        if dependencies::needs(inv, target, component)?.subtitle {
            let install = dependencies::subtitle_install(inv, target)?;
            let prefix = install.parent().and_then(Path::parent).context("subtitle prefix")?
                .strip_prefix(&inv.root)?.to_string_lossy().replace('\\', "/");
            instructions.push_str(&format!("$env:ROUKAKU_SUBTITLE_VCPKG_ROOT = \"$PWD/{prefix}\"\n"));
            if target.contains("linux") {
                instructions.push_str(&format!("$env:PKG_CONFIG_PATH = \"$PWD/{prefix}/installed/x64-linux-dynamic/lib/pkgconfig\"\n"));
            }
        }
        if dependencies::needs(inv, target, component)?.ffmpeg {
            instructions.push_str("$env:VCPKG_ROOT = \"$PWD/.work/vcpkg-ffmpeg\"\n");
        }
    }
    let selected = dependencies::needs(inv, target, component)?;
    if selected.native_vcpkg {
        instructions.push_str(&format!(
            "$env:VCPKG_ROOT = \"$PWD/.work/vcpkg\"\n$env:SHINKI_EMU_VCPKG_INSTALLED = \"$PWD/.work/native-vcpkg/{component}/installed\"\n$env:SHINKI_EMU_VCPKG_TRIPLET = \"{}\"\n",
            if target.contains("linux") { "x64-linux-dynamic" } else { "x64-windows" }
        ));
    }
    if selected.impacto_sources || selected.renpy {
        instructions.push_str("$env:SHINKI_EMU_NATIVE_DOWNLOADS = \"$PWD/.work/native-downloads\"\n");
    }
    if selected.renpy {
        instructions.push_str(
            "$env:RENPY_SDK_ROOT = \"$PWD/.work/native-downloads/renpy-sdk\"\n\
             $env:RENPY_PYTHON_DEV = \"$PWD/.work/native-downloads/python\"\n\
             $env:RENPY_SDL_INCLUDE = \"$PWD/.work/native-downloads/sdl2/include\"\n\
             $env:Python3_EXECUTABLE = \"$PWD/.work/native-downloads/python/python.exe\"\n\
             $env:RENPY_CYTHON_LICENSE = \"$PWD/.work/native-downloads/cython-license/LICENSE.txt\"\n"
        );
    }
    if selected.bison {
        instructions.push_str("$env:BISON_EXECUTABLE = (Get-Command win_bison.exe).Source\n");
    }
    let build_command = if target.contains("android") && component == "manager" {
        format!("cargo ndk -t {} -P 26 build", if target.starts_with("aarch64") { "arm64-v8a" } else { "x86_64" })
    } else {
        format!("cargo build --target {target}")
    };
    let artifact = if component != "manager" || target.contains("android") { "--lib" } else { "--bin shinki-emu-manager" };
    instructions.push_str(&format!(
        "{build_command} --offline --frozen --release --manifest-path {manifest_relative} -p {package} {artifact}{feature_args}\n```\n\n\
         Native ports/patches and SPDX build provenance are under native/; source archives are under .work/vcpkg-downloads. \
         Static libraries can be modified, rebuilt with these recipes, and relinked by rebuilding the consumer above. \
         Compatible replacement and reverse engineering for debugging modifications to LGPL components are permitted.\n"
    ));
    if selected.renpy {
        instructions.push_str(
            "\n## Ren'Py combined-runtime rebuild and relink\n\n\
             The shipped SDK is 8.5.3.26051504, SHA256 ff57648f9c04f27e381c48af6d8e3ee3cdec296bed4d3831f47f09b0a71b505e. \
             Its SDK metadata names Ren'Py source 39895c1e017f0b36ffea2447d97eccd69d76ee1c and renpy-build tag renpy-8.5.3.26051504, resolved to 7bfab40c1174f622f644b24669afd5fb167fbb79. \
             The SDK bytecode contains urllib3 2.2.2 and certifi 2026.04.22; the build recipe instead pins urllib3 2.7.0 and leaves certifi unpinned. \
             This mismatch establishes that the tag alone does not reproduce the Python payload; the actual copied versions and source hashes are in native/python-sources/sources.json. \
             It does not establish a different renpy-build commit. Keep those versions when rebuilding the copied Python payload.\n\n\
             native/renpy-build.tar contains the complete pinned recipes, source/ffmpeg-4.3.1.tar.gz, source/fribidi-1.0.7.tar.bz2, SDL sources and patches. \
             native/renpy-sources supplies exact additional static-library source snapshots and the official SDK Ren'Py source; native/python-sources includes readable chardet and certifi sdists. \
             These sources rebuild the LGPL parts inside librenpython.dll; libEGL.dll/libGLESv2.dll and the Windows compiler/OS SDK remain separate permissive/system inputs, with notices.\n\n\
             Rebuilding the combined DLL uses upstream's Ubuntu 24.04 cross-build environment (see renpy-build/README.rst). \
             Use the llvm-mingw-20241217-ucrt-ubuntu-20.04-x86_64 toolchain, host Python 3.12 with Cython 3.2.4/Jinja2/requests, autoconf/automake/libtool, CMake, Ninja, NASM and the system development libraries listed in prepare.sh. \
             Supply these build tools before disconnecting; do not run the online prepare.sh. Extract the archives and run the supplied offline driver:\n\n\
             ```sh\nmkdir -p native/renpy-build\ntar -xf native/renpy-build.tar -C native/renpy-build\n\
             mkdir -p native/renpy-build/tmp/tars\n\
             cp \"$LLVM_MINGW_ARCHIVE\" native/renpy-build/tmp/tars/llvm-mingw-20241217-ucrt-ubuntu-20.04-x86_64.tar.xz\n\
             python3 native/renpy-offline.py --platform windows --arch x86_64 --stop renpython\n```\n\n\
             The link command in tasks/renpython.py combines librenpy.a, FFmpeg, FriBidi and the other static libraries into librenpython.dll. \
             Modify the LGPL sources/patches, rebuild those libraries, and run that link task again, retaining compatible exports. \
             Replace .work/native-downloads/renpy-sdk/lib/py3-windows-x86_64/librenpython.dll with the rebuilt DLL and rebuild the adapter above. \
             The adapter's shinki/CMakeLists.txt rebuilds Resources/overrides/display.pyd from the modified fork source and installs the bridge; replacing only display.pyd does not relink FFmpeg/FriBidi.\n"
        );
        fs::write(root.join("native/renpy-offline.py"), renpy_offline_driver())?;
    }
    if !target.contains("windows") {
        instructions = instructions
            .replace("```powershell", "```sh")
            .replace("$env:", "export ")
            .replace(" = ", "=")
            .replace("& $vcpkg ", "\"$vcpkg\" ");
    }
    let readme = temp.path().join("BUILD-FROM-SOURCE.md");
    fs::write(&readme, &instructions)?;
    fs::write(root.join("BUILD-FROM-SOURCE.md"), instructions)?;
    let archive = temp.path().join("source.tar");
    let mut builder = tar::Builder::new(fs::File::create(&archive)?);
    builder.append_dir_all(".", &root)?;
    builder.finish()?;
    package::zip_entries(&[(archive, "source.tar".into()), (readme, "BUILD-FROM-SOURCE.md".into())], out)
}

fn copy_hashed_archive(inv: &Inventory, record: &serde_json::Value, dest: &Path) -> Result<()> {
    let name = record["file_name"].as_str().context("archive filename")?;
    package::portable(name)?;
    let archive = inv.root.join(".work/downloads").join(name);
    if package::sha256(&archive)? != record["sha256"].as_str().context("archive SHA256")? {
        bail!("source archive checksum mismatch: {name}");
    }
    copy_input(&archive, &dest.join(name))
}

fn renpy_offline_driver() -> &'static str {
    r#"#!/usr/bin/env python3
"""Run pinned upstream build tasks with archive-backed source preparation."""
import json, os, pathlib, runpy, shlex, shutil, subprocess, sys, tarfile

native = pathlib.Path(__file__).resolve().parent
root = native / "renpy-build"
sources = json.loads((native / "renpy-sources/sources.json").read_text())["sources"]
for item in sources:
    dest = root / ("renpy" if item["name"] == "renpy" else "unpacked/" + item["name"])
    if dest.exists():
        continue
    dest.mkdir(parents=True, exist_ok=True)
    with tarfile.open(native / "renpy-sources" / item["file_name"]) as archive:
        for member in archive.getmembers():
            parts = pathlib.PurePosixPath(member.name).parts[item["strip_components"]:]
            if not parts:
                continue
            member.name = "/".join(parts)
            archive.extract(member, dest, filter="data")
for item in sources:
    name = item["name"]
    if name in ("renpy", "cpython-mingw", "pyobjus"):
        continue
    dest = root / "tmp/source" / name
    if not dest.exists():
        shutil.copytree(root / "unpacked" / name, dest)
        patch = {"assimp": "assimp.diff", "libyuv": "libyuv-noshared.diff"}.get(name)
        if patch:
            subprocess.run(["patch", "-p1", "-i", str(root / "patches" / patch)], cwd=dest, check=True)
# Upstream's Windows Python task clones this local prepared source by name.
python_source = root / "unpacked/cpython-mingw"
if not (python_source / ".git").exists():
    subprocess.run(["git", "init", "-b", "mingw-v3.12.7"], cwd=python_source, check=True)
    subprocess.run(["git", "add", "."], cwd=python_source, check=True)
    subprocess.run(["git", "-c", "user.name=Source bundle", "-c", "user.email=source@example.invalid",
                    "-c", "commit.gpgsign=false", "commit", "-m", "Archive-backed Python source"], cwd=python_source, check=True)
python_archives = native / "python-sources"
# The shipped Python payload includes this already-generated permissive module,
# but ShinkiEMU does not redistribute or rebuild the proprietary Steam SDK.
steam = root / "tmp/py3/steam"
steam.mkdir(parents=True, exist_ok=True)
shutil.copy2(root / "steamapi/steamapi.py", steam / "steamapi.py")
os.environ["PYTHONHASHSEED"] = "0"
os.environ.update(PIP_NO_INDEX="1", PIP_FIND_LINKS=str(python_archives),
                  PIP_NO_BUILD_ISOLATION="1", PIP_DISABLE_PIP_VERSION_CHECK="1")
os.chdir(root)
sys.path.insert(0, str(root))
import tasks
import renpybuild.task
# Downloads are replaced with the exact extracted source inputs above; the
# cross compiler archive is supplied as a preinstalled system build tool.
renpybuild.task.tasks[:] = [t for t in renpybuild.task.tasks if t.task != "download"]
import tasks.python3
def install_python_libraries(c):
    c.run("{{ install }}/bin/hostpython3 -s -m ensurepip")
    inventory = json.loads((python_archives / "sources.json").read_text())
    def install(records):
        c.run("{{ install }}/bin/hostpython3 -s -m pip install --no-index --no-build-isolation --no-compile --no-deps " +
              " ".join(shlex.quote(str(python_archives / p["file_name"])) for p in records))
    install(inventory["tools"])
    install([p.get("wheel", p) for p in inventory["packages"]])
def unpack_pyobjus(c):
    c.clean()
    c.copytree(str(root / "unpacked/pyobjus"), "pyobjus")
    c.chdir("pyobjus")
    c.patch("pyobjus/ffi-h.diff")
for task in renpybuild.task.tasks:
    if task.name == "python3" and task.task == "pip":
        task.function = install_python_libraries
    if task.name == "pyobjus" and task.task in ("host_unpack", "unpack"):
        task.function = unpack_pyobjus
runpy.run_module("renpybuild", run_name="__main__")
"#
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
    fn source_archive_expands_gitlinks_and_rejects_dirty_submodules() {
        let (temp, inv, _core, upstream, _first, _second) = fixture();
        dependencies::run(Command::new("git").args(["init"]).arg(&inv.root)).unwrap();
        fs::write(inv.root.join("Cargo.toml"), "[workspace]\n").unwrap();
        dependencies::run(git(&inv.root).args(["-c", "protocol.file.allow=always", "submodule", "add"])
            .arg(&upstream).arg("nested")).unwrap();
        dependencies::run(git(&inv.root).args(["add", ".gitmodules", "nested"])).unwrap();
        let pin = commit(&inv.root, "outer\n");
        let archive = temp.path().join("source.tar");
        let mut builder = tar::Builder::new(fs::File::create(&archive).unwrap());
        append_revision(&mut builder, &inv.root, &pin, Path::new(""), true).unwrap();
        builder.finish().unwrap();
        let extracted = temp.path().join("extracted");
        tar::Archive::new(fs::File::open(&archive).unwrap()).unpack(&extracted).unwrap();
        assert_eq!(fs::read(extracted.join("nested/payload.txt")).unwrap(), fs::read(upstream.join("payload.txt")).unwrap());
        fs::write(inv.root.join("nested/payload.txt"), "unpublished change").unwrap();
        let mut builder = tar::Builder::new(fs::File::create(temp.path().join("rejected.tar")).unwrap());
        assert!(append_revision(&mut builder, &inv.root, &pin, Path::new(""), true).is_err());
    }

    #[test]
    fn source_bundle_resolves_from_a_clean_extracted_directory() {
        let temp = tempfile::tempdir().unwrap();
        let original = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inv = Inventory::load(original).unwrap();
        inv.root = temp.path().join("product");
        fs::create_dir_all(inv.root.join("src")).unwrap();
        fs::write(inv.root.join("Cargo.toml"), "[package]\nname=\"shinki-emu-manager\"\nversion=\"0.1.0\"\nedition=\"2024\"\n").unwrap();
        fs::write(inv.root.join("src/lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();
        fs::write(inv.root.join(".gitignore"), "/.work/\n").unwrap();
        dependencies::run(Command::new("cargo").current_dir(&inv.root).args(["generate-lockfile", "--offline"])).unwrap();
        dependencies::run(Command::new("git").args(["init"]).arg(&inv.root)).unwrap();
        dependencies::run(git(&inv.root).args(["add", "Cargo.toml", "Cargo.lock", ".gitignore", "src"])).unwrap();
        commit(&inv.root, "product source\n");
        let output = temp.path().join("source.zip");
        bundle(&inv, "manager", "x86_64-pc-windows-msvc", &output).unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
        let source = zip.by_name("source.tar").unwrap();
        let extracted = temp.path().join("fresh");
        tar::Archive::new(source).unpack(&extracted).unwrap();
        dependencies::capture(Command::new("cargo").current_dir(extracted).args(["metadata", "--offline", "--frozen", "--format-version", "1"])).unwrap();
    }

    #[test]
    fn source_archive_materializes_only_checksum_matched_lfs_payloads() {
        use sha2::Digest;
        let (temp, _inv, _core, upstream, _first, _second) = fixture();
        let payload = b"materialized public fixture bytes";
        let oid = hex::encode(sha2::Sha256::digest(payload));
        let pointer = format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n", payload.len());
        let pin = commit(&upstream, &pointer);
        fs::write(upstream.join("payload.txt"), payload).unwrap();
        let archive = temp.path().join("materialized.tar");
        let mut builder = tar::Builder::new(fs::File::create(&archive).unwrap());
        append_revision(&mut builder, &upstream, &pin, Path::new(""), true).unwrap();
        builder.finish().unwrap();
        let extracted = temp.path().join("materialized");
        tar::Archive::new(fs::File::open(archive).unwrap()).unpack(&extracted).unwrap();
        assert_eq!(fs::read(extracted.join("payload.txt")).unwrap(), payload);
        fs::write(upstream.join("payload.txt"), pointer).unwrap();
        let mut builder = tar::Builder::new(fs::File::create(temp.path().join("pointer.tar")).unwrap());
        assert!(append_revision(&mut builder, &upstream, &pin, Path::new(""), true).is_err());
        fs::write(upstream.join("payload.txt"), vec![b'x'; payload.len()]).unwrap();
        let mut builder = tar::Builder::new(fs::File::create(temp.path().join("modified.tar")).unwrap());
        assert!(append_revision(&mut builder, &upstream, &pin, Path::new(""), true).is_err());
    }
}

