use super::{ANDROID_BUNDLE_MANIFEST, BuildArtifact, CorePackage, PRODUCT_SOURCE_COMMIT};
use crate::{build, cores::{Inventory, slug}, dependencies, package, release};
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use shinki_emu_metadata::{CoreIdentity, MAX_CORE_PACKAGE_BYTES};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
    process::Command,
};

#[path = "../../build/android_qa_receipt.rs"]
mod receipt;

pub(super) const APK_ASSET: &str = "assets/android-qa-core-set.json";
const BUILD_RECEIPT: &str = "android-qa-input.json";

struct Core {
    id: String,
    version: String,
    identity: Option<CoreIdentity>,
    filename: String,
}

pub(super) struct Plan<'a> {
    target: &'a str,
    abi: &'a str,
    version: &'a str,
    source_commit: String,
    cores: Vec<Core>,
    omitted: BTreeMap<String, String>,
}

impl<'a> Plan<'a> {
    pub(super) fn new(inv: &Inventory, target: &'a str, abi: &'a str, version: &'a str, subset: &[String]) -> Result<Self> {
        let (cores, omitted) = select_cores(inv, target, version, subset)?;
        let source_commit = clean_commit(inv, None)?;
        Ok(Self { target, abi, version, source_commit, cores, omitted })
    }

    pub(super) fn check_manager(&self, manager: &BuildArtifact) -> Result<()> {
        ensure!(
            manager.target == self.target
                && manager.product_version.as_deref() == Some(self.version)
                && manager.source_commit.as_deref() == Some(self.source_commit.as_str())
                && manager.embedded_core_builds.is_empty(),
            "Android QA needs a same-source, single-ABI Manager without embedded cores"
        );
        Ok(())
    }

    pub(super) fn check_checkout(&self, inv: &Inventory, stage: &Path) -> Result<()> {
        let scratch = stage.parent().context("Android QA temporary output directory")?;
        ensure!(clean_commit(inv, Some(scratch))? == self.source_commit, "Android QA checkout moved during packaging");
        Ok(())
    }

    pub(super) fn stage(
        &self,
        inv: &Inventory,
        stage: &Path,
        out: &Path,
        prepared: &[PathBuf],
    ) -> Result<Vec<u8>> {
        let mut sources = BTreeMap::new();
        for directory in prepared {
            package::plain(directory)?;
            let path = directory.join(BUILD_RECEIPT);
            package::plain(&path)?;
            let mut bytes = Vec::new();
            File::open(&path)?.take(receipt::MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= receipt::MAX_BYTES, "oversized QA input receipt: {}", path.display());
            let input: receipt::Receipt = serde_json::from_slice(&bytes).with_context(|| path.display().to_string())?;
            ensure!(
                input.matches_product(self.abi, self.version, &self.source_commit),
                "QA input receipt has a different source commit, ABI or product version: {}", path.display()
            );
            for entry in input.cores {
                if self.cores.iter().any(|core| core.id == entry.plugin_id) {
                    let id = entry.plugin_id.clone();
                    ensure!(sources.insert(id, (directory, entry)).is_none(), "duplicate prepared QA core");
                }
            }
        }
        if !prepared.is_empty() {
            for core in &self.cores {
                ensure!(sources.contains_key(&core.id), "missing prepared QA ZIP for {}", core.id);
            }
        }
        for (id, reason) in &self.omitted {
            println!("Android QA {id}: omitted ({reason})");
        }
        let mut entries = Vec::with_capacity(self.cores.len());
        for core in &self.cores {
            let archive = stage.join(&core.filename);
            println!("Android QA {} / {}: {}", core.id, self.target, core.filename);
            let entry = if let Some((dir, entry)) = sources.remove(&core.id) {
                ensure!(entry.version == core.version && entry.identity == core.identity,
                    "QA input receipt differs from the locked identity for {}", core.id);
                self.copy_prepared(core, &dir.join(&core.filename), &archive, &entry)?;
                entry
            } else {
                self.build_core(inv, core, &archive, out)?
            };
            entries.push(entry);
        }
        let receipt = receipt::Receipt {
            schema: receipt::SCHEMA.into(), abi: self.abi.into(), product_version: self.version.into(),
            source_commit: self.source_commit.clone(), cores: entries, omitted: self.omitted.clone(),
        };
        ensure!(receipt.matches_product(self.abi, self.version, &self.source_commit), "invalid Android QA receipt");
        let bytes = serde_json::to_vec_pretty(&receipt)?;
        ensure!(bytes.len() <= receipt::MAX_BYTES, "Android QA receipt exceeds the APK asset limit");
        Ok(bytes)
    }

    fn build_core(&self, inv: &Inventory, core: &Core, archive: &Path, out: &Path) -> Result<receipt::Core> {
        let artifact = build::package_artifact(inv, self.target, &core.id, &core.version)?;
        super::reject_overlap(&artifact.library, out)?;
        let report = artifact.core_build.as_ref().context("missing Android QA core build report")?;
        let first_party = core.identity.is_none();
        ensure!(
            artifact.target == self.target && report.target == self.target && report.plugin_id == core.id
                && matches!(report.artifact_kind.as_str(), "rust-cdylib" | "native-shared")
                && artifact.product_version.as_deref() == first_party.then_some(self.version)
                && artifact.source_commit.as_deref() == first_party.then_some(self.source_commit.as_str()),
            "{} was not built as an ordinary dynamic core at the expected source and version", core.id
        );
        let scratch = crate::temporary::directory()?;
        let directory = super::stage_built_core(inv, artifact, &core.version, &scratch.path().join("package"), scratch.path())?;
        let manifest: CorePackage = serde_json::from_reader(File::open(directory.join("core.json"))?)?;
        self.verify_declaration(core, &manifest)?;
        check_elf(File::open(directory.join(&manifest.library))?, self.abi)?;
        package::zip_entries(&package::tree(&directory)?, archive)?;
        Ok(receipt::Core {
            plugin_id: core.id.clone(), version: core.version.clone(), identity: core.identity.clone(),
            size: fs::metadata(archive)?.len(), sha256: package::sha256(archive)?,
        })
    }

    fn verify_declaration(&self, core: &Core, manifest: &CorePackage) -> Result<()> {
        package::portable(&manifest.library)?;
        ensure!(
            manifest.schema == "shinki.emu.core_package.v1" && manifest.abi == shinki_emu_build::ABI
                && manifest.plugin_id == core.id && manifest.version == core.version && manifest.identity == core.identity
                && is_core_library(&manifest.library),
            "invalid ordinary Android core package for {}", core.id
        );
        Ok(())
    }

    fn copy_prepared(&self, core: &Core, archive: &Path, snapshot: &Path, entry: &receipt::Core) -> Result<()> {
        package::plain(archive)?;
        let input = File::open(archive)?;
        ensure!(input.metadata()?.len() == entry.size && entry.size <= MAX_CORE_PACKAGE_BYTES,
            "prepared core ZIP size mismatch: {}", archive.display());
        let mut input = input.take(entry.size + 1);
        let mut file = fs::OpenOptions::new().create_new(true).read(true).write(true).open(snapshot)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        let mut copied = 0;
        loop {
            let count = input.read(&mut buffer)?;
            if count == 0 { break; }
            file.write_all(&buffer[..count])?;
            hasher.update(&buffer[..count]);
            copied += count as u64;
        }
        ensure!(copied == entry.size && hex::encode(hasher.finalize()) == entry.sha256,
            "prepared core ZIP hash mismatch: {}", archive.display());
        file.rewind()?;
        let mut archive = zip::ZipArchive::new(io::BufReader::new(file))?;
        ensure!(archive.len() <= 8192, "prepared core ZIP exceeds the member limit");
        let mut bytes = Vec::new();
        const MANIFEST_LIMIT: u64 = 1024 * 1024;
        archive.by_name("core.json")?.take(MANIFEST_LIMIT + 1).read_to_end(&mut bytes)?;
        ensure!((bytes.len() as u64) <= MANIFEST_LIMIT, "prepared core manifest exceeds the size limit");
        let manifest: CorePackage = serde_json::from_slice(&bytes)?;
        self.verify_declaration(core, &manifest)?;
        let mut members = manifest.files.iter().map(|record| (record.path.as_str(), record)).collect::<BTreeMap<_, _>>();
        ensure!(members.len() == manifest.files.len() && members.contains_key(manifest.library.as_str())
            && members.keys().filter(|path| is_core_library(path)).count() == 1
            && members.keys().any(|path| path.starts_with("licenses/"))
            && members.contains_key(PRODUCT_SOURCE_COMMIT) == core.identity.is_none(),
            "prepared ZIP does not contain ordinary package members for {}", core.id);
        let mut names = BTreeSet::new();
        let mut extracted = 0_u64;
        for index in 0..archive.len() {
            let mut item = archive.by_index(index)?;
            let name = item.name()?.into_owned();
            package::portable(&name)?;
            ensure!(!item.is_symlink() && !item.is_dir() && names.insert(name.to_ascii_lowercase()),
                "invalid prepared ZIP member {name}");
            if name == "core.json" { continue; }
            let member = members.remove(name.as_str()).with_context(|| format!("unlisted ZIP member {name}"))?;
            let basename = name.rsplit('/').next().unwrap_or(&name);
            ensure!(!crate::binary::system(basename, self.target),
                "prepared ZIP contains a device system library: {name}");
            ensure!(name == PRODUCT_SOURCE_COMMIT
                    || !name.split('/').next().is_some_and(|part| part.eq_ignore_ascii_case(PRODUCT_SOURCE_COMMIT)),
                "prepared ZIP contains an invalid product source stamp path: {name}");
            extracted = extracted.saturating_add(item.size());
            ensure!(extracted <= 2 * 1024 * 1024 * 1024, "prepared core ZIP exceeds the unpacked size limit");
            let digest = if name == PRODUCT_SOURCE_COMMIT {
                let mut commit = [0; 40];
                item.read_exact(&mut commit)?;
                ensure!(item.read(&mut [0])? == 0 && commit.as_slice() == self.source_commit.as_bytes(),
                    "prepared first-party core has a different source commit");
                package::sha256_reader(commit.as_slice())?
            } else if basename.ends_with(".so") || basename.contains(".so.") {
                let header = check_elf(&mut item, self.abi)?;
                package::sha256_reader(header.as_slice().chain(item))?
            } else {
                package::sha256_reader(item)?
            };
            ensure!(digest == member.sha256, "prepared ZIP member hash mismatch: {name}");
        }
        ensure!(members.is_empty(), "prepared ZIP is missing declared members");
        Ok(())
    }
}

fn select_cores(
    inv: &Inventory, target: &str, version: &str, subset: &[String],
) -> Result<(Vec<Core>, BTreeMap<String, String>)> {
    let spec = inv.target(target)?;
    let mut requested = subset.iter().map(String::as_str).collect::<BTreeSet<_>>();
    ensure!(requested.len() == subset.len(), "duplicate --qa-core ID");
    let mut cores = Vec::new();
    let mut omitted = BTreeMap::new();
    for core in &inv.cores {
        let explicit = requested.remove(core.id.as_str());
        let Some(policy) = &core.android_dynamic else {
            ensure!(!explicit, "{} is not an android_dynamic core", core.id);
            continue;
        };
        let reason = if !core.publish_blocked.is_empty() {
            Some(core.publish_blocked.as_str())
        } else {
            inv.blocked(&core.id, target)?
        };
        if let Some(reason) = reason {
            ensure!(!explicit, "{} / {target} is not release-eligible: {reason}", core.id);
            omitted.insert(core.id.clone(), reason.into());
        } else if !subset.is_empty() && !explicit {
            omitted.insert(core.id.clone(), "omitted by explicit --qa-core selection".into());
        } else {
            let (version, identity) = if core.attribution.is_some() {
                let identity = core.identity(&inv.root)?.context("Android QA external core needs its locked identity")?;
                (identity.version(), Some(identity))
            } else {
                let minimum = policy["minimum_release"].as_str().context("Android QA first-party release floor")?;
                ensure!(!semver::Version::parse(version)?.cmp_precedence(&semver::Version::parse(minimum)?).is_lt(),
                    "{version} is below the Android release floor {minimum} for {}", core.id);
                (version.to_owned(), None)
            };
            let filename = format!("shinki-emu-core-{}-{version}-android-{}.zip", slug(&core.id), spec.architecture);
            package::portable(&filename)?;
            cores.push(Core { id: core.id.clone(), version, identity, filename });
        }
    }
    ensure!(requested.is_empty(), "unknown --qa-core IDs: {requested:?}");
    ensure!(!cores.is_empty(), "no release-eligible QA core rows for {target}");
    cores.sort_unstable_by(|a, b| a.id.cmp(&b.id));
    Ok((cores, omitted))
}

fn is_core_library(name: &str) -> bool {
    !name.contains('/') && name.rsplit_once('.').is_some_and(|(stem, extension)| {
        extension.eq_ignore_ascii_case("so") && ["libshinki_emu_", "shinki_emu_"].iter().any(|prefix| {
            stem.get(..prefix.len()).is_some_and(|start| start.eq_ignore_ascii_case(prefix))
        })
    })
}

fn clean_commit(inv: &Inventory, excluded_output: Option<&Path>) -> Result<String> {
    let mut command = Command::new("git");
    command.current_dir(&inv.root)
        .args(["status", "--porcelain=v1", "--untracked-files=normal", "--ignore-submodules=none"]);
    if let Some(output) = excluded_output
        && let Ok(relative) = dunce::canonicalize(output)?.strip_prefix(dunce::canonicalize(&inv.root)?)
    {
        // Ignore only the package's own temporary directory on this invocation.
        command.args(["--", ".", &format!(":(exclude,literal){}", relative.to_string_lossy().replace('\\', "/"))]);
    }
    let status = dependencies::capture(&mut command)?;
    ensure!(status.is_empty(), "Android QA requires a clean checkout and pinned submodules:\n{}", String::from_utf8_lossy(&status));
    build::source_commit(inv)
}

pub(super) fn prepare(inv: &Inventory, target: &str, abi: &str, version: &str, out: &Path, subset: &[String]) -> Result<PathBuf> {
    let version = release::resolve_version(inv, "manager", version)?;
    let plan = Plan::new(inv, target, abi, &version, subset)?;
    package::atomic_directory(out, |stage| {
        let bytes = plan.stage(inv, stage, out, &[])?;
        fs::write(stage.join(BUILD_RECEIPT), bytes)?;
        plan.check_checkout(inv, stage)
    })?;
    Ok(out.to_owned())
}

fn check_elf(mut input: impl Read, abi: &str) -> Result<[u8; 64]> {
    let machine = receipt::machine(abi).context("unsupported Android QA ABI")?;
    let mut header = [0; 64];
    input.read_exact(&mut header)?;
    ensure!(receipt::android_shared_object_header(&header, machine), "expected Android {abi} ELF64 shared library");
    Ok(header)
}

pub(super) fn stage_asset(stage: &Path, bytes: &[u8]) -> Result<()> {
    let dest = stage.join("debug-assets");
    fs::create_dir(&dest)?;
    fs::write(dest.join(receipt::ASSET), bytes)?;
    Ok(())
}

pub(super) fn verify_apk(
    archive: &mut zip::ZipArchive<impl Read + io::Seek>,
    qa: Option<(&Path, &str, &[u8])>,
) -> Result<()> {
    let mut receipts = 0;
    let mut native = BTreeSet::new();
    for name in archive.file_names() {
        let name = name?;
        if name.eq_ignore_ascii_case(APK_ASSET) {
            ensure!(qa.is_some(), "official Android packaging refuses a local QA receipt");
            receipts += 1;
        }
        if qa.is_some() && name.starts_with("lib/") && !name.ends_with('/') {
            ensure!(native.insert(name), "duplicate native entry in Android QA APK");
        }
    }
    let Some((stage, abi, expected)) = qa else { return Ok(()) };
    ensure!(receipts == 1, "Android QA APK must contain exactly one local receipt");
    let staged = package::tree(&stage.join("jniLibs"))?;
    ensure!(native.len() == staged.len() && staged.iter().all(|(_, name)| native.contains(format!("lib/{name}").as_str())),
        "Android QA APK native files differ from Manager staging");
    check_elf(archive.by_name(&format!("lib/{abi}/libshinki_emu_manager.so"))?, abi)?;
    let mut actual = Vec::new();
    let asset = archive.by_name(APK_ASSET)?;
    ensure!(!asset.is_symlink() && asset.size() == expected.len() as u64, "invalid Android QA APK receipt asset");
    asset.take(expected.len() as u64 + 1).read_to_end(&mut actual)?;
    ensure!(actual == expected, "Android QA APK receipt differs from the prepared core set");
    let manifest_hash = package::sha256(&stage.join(ANDROID_BUNDLE_MANIFEST))?;
    ensure!(package::sha256_reader(archive.by_name(ANDROID_BUNDLE_MANIFEST)?)? == manifest_hash,
        "Android QA APK bundled extraction list differs from Manager staging");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: &str = "aarch64-linux-android";
    const ABI: &str = "arm64-v8a";
    const VERSION: &str = "0.1.0-qa.1";
    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    fn inventory() -> Inventory {
        Inventory::load(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap()
    }

    fn elf(machine: u16) -> [u8; 64] {
        let mut header = [0; 64];
        header[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        header[16..18].copy_from_slice(&3_u16.to_le_bytes());
        header[18..20].copy_from_slice(&machine.to_le_bytes());
        header[20..24].copy_from_slice(&1_u32.to_le_bytes());
        header
    }

    #[test]
    fn android_qa_inventory_keeps_locked_dynamic_rows_and_reports_restrictions() {
        let inv = inventory();
        let (rows, omitted) = select_cores(&inv, TARGET, VERSION, &[]).unwrap();
        assert!(rows.iter().any(|row| row.id == "shinki.emu.musica" && row.version == VERSION));
        let rfvp = rows.iter().find(|row| row.id == "rfvp").unwrap();
        assert_eq!(rfvp.identity, inv.core("rfvp").unwrap().identity(&inv.root).unwrap());
        assert!(rfvp.identity.is_some());
        assert!(!rows.iter().any(|row| row.id == "renpy"));
        assert!(omitted["renpy"].contains("verify LGPL relinking"));
        assert!(select_cores(&inv, TARGET, VERSION, &["renpy".into()]).is_err());
        assert!(select_cores(&inv, TARGET, "0.1.0-beta.1", &["shinki.emu.musica".into()]).is_err());
        let (selected, omitted) = select_cores(&inv, TARGET, VERSION, &["rfvp".into()]).unwrap();
        assert_eq!(selected.len(), 1);
        assert!(omitted["shinki.emu.musica"].contains("--qa-core"));
    }

    #[test]
    fn android_qa_prepared_zip_checks_source_architecture_and_bytes() {
        let root = crate::temporary::directory().unwrap();
        let inv = inventory();
        let (cores, omitted) = select_cores(&inv, TARGET, VERSION, &["shinki.emu.musica".into()]).unwrap();
        let plan = Plan { target: TARGET, abi: ABI, version: VERSION, source_commit: COMMIT.into(), cores, omitted };
        let core = &plan.cores[0];
        let prepared = root.path().join("prepared");
        let sources = root.path().join("sources");
        fs::create_dir(&prepared).unwrap();
        fs::create_dir(&sources).unwrap();
        let archive = prepared.join(&core.filename);
        let library = "libshinki_emu_musica.so";
        let write = |machine, commit: &str| {
            let manifest_file = sources.join("core.json");
            let assets: [(&str, &[u8]); 3] = [
                (library, &elf(machine)),
                (PRODUCT_SOURCE_COMMIT, commit.as_bytes()),
                ("licenses/NOTICE", b"QA fixture"),
            ];
            let mut files = Vec::new();
            let mut entries = Vec::new();
            for (name, contents) in assets {
                let source = sources.join(name);
                fs::create_dir_all(source.parent().unwrap()).unwrap();
                fs::write(&source, contents).unwrap();
                files.push(super::super::FileRecord { path: name.into(), sha256: package::sha256(&source).unwrap() });
                entries.push((source, name.into()));
            }
            let manifest = CorePackage {
                schema: "shinki.emu.core_package.v1".into(), plugin_id: core.id.clone(),
                library: library.into(), version: VERSION.into(), identity: None,
                recognition: Default::default(), abi: shinki_emu_build::ABI.into(), files,
            };
            fs::write(&manifest_file, serde_json::to_vec(&manifest).unwrap()).unwrap();
            entries.push((manifest_file, "core.json".into()));
            if archive.exists() { fs::remove_file(&archive).unwrap(); }
            package::zip_entries(&entries, &archive).unwrap();
        };
        let input = |abi: &str, commit: &str, hash: Option<String>| {
            let receipt = receipt::Receipt {
                schema: receipt::SCHEMA.into(), abi: abi.into(), product_version: VERSION.into(),
                source_commit: commit.into(), omitted: BTreeMap::new(),
                cores: vec![receipt::Core {
                    plugin_id: core.id.clone(), version: VERSION.into(), identity: None,
                    size: fs::metadata(&archive).unwrap().len(),
                    sha256: hash.unwrap_or_else(|| package::sha256(&archive).unwrap()),
                }],
            };
            fs::write(prepared.join(BUILD_RECEIPT), serde_json::to_vec(&receipt).unwrap()).unwrap();
        };
        let stage = |name: &str| {
            let stage = root.path().join(name);
            fs::create_dir(&stage).unwrap();
            plan.stage(&inv, &stage, &stage, std::slice::from_ref(&prepared))
        };
        write(183, COMMIT);
        input(ABI, &"a".repeat(40), None);
        assert!(stage("stale").unwrap_err().to_string().contains("different source commit"));
        input("x86_64", COMMIT, None);
        assert!(stage("wrong-abi").unwrap_err().to_string().contains("ABI"));
        input(ABI, COMMIT, Some("f".repeat(64)));
        assert!(stage("bad-hash").unwrap_err().to_string().contains("hash mismatch"));
        write(62, COMMIT);
        input(ABI, COMMIT, None);
        assert!(stage("x64-elf").unwrap_err().to_string().contains("ELF64"));
        write(183, &"a".repeat(40));
        input(ABI, COMMIT, None);
        assert!(stage("stale-payload").unwrap_err().to_string().contains("different source commit"));
        write(183, COMMIT);
        input(ABI, COMMIT, None);
        let bytes = stage("verified").unwrap();
        let assembled: receipt::Receipt = serde_json::from_slice(&bytes).unwrap();
        let snapshot = root.path().join("verified").join(&core.filename);
        assert_eq!(assembled.cores[0].sha256, package::sha256(&snapshot).unwrap());
        assert_eq!(fs::read(archive).unwrap(), fs::read(snapshot).unwrap());
    }

    #[test]
    fn android_qa_apk_is_separate_from_official_packaging() {
        let root = crate::temporary::directory().unwrap();
        let stage = root.path();
        let library = format!("{ABI}/libshinki_emu_manager.so");
        let manager = stage.join("jniLibs").join(&library);
        fs::create_dir_all(manager.parent().unwrap()).unwrap();
        fs::write(&manager, elf(183)).unwrap();
        let manifest = stage.join(ANDROID_BUNDLE_MANIFEST);
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(&manifest, b"metadata/index.json\n").unwrap();
        let bytes = b"QA receipt bytes";
        stage_asset(stage, bytes).unwrap();
        let official = stage.join("official.apk");
        let mut entries = vec![(manager, format!("lib/{library}")), (manifest, ANDROID_BUNDLE_MANIFEST.into())];
        package::zip_entries(&entries, &official).unwrap();
        super::super::reject_android_qa_apk(&official).unwrap();
        let qa = stage.join("qa.apk");
        entries.push((stage.join("debug-assets").join(receipt::ASSET), APK_ASSET.into()));
        package::zip_entries(&entries, &qa).unwrap();
        assert!(super::super::reject_android_qa_apk(&qa).is_err());
        let mut archive = zip::ZipArchive::new(File::open(&qa).unwrap()).unwrap();
        verify_apk(&mut archive, Some((stage, ABI, bytes))).unwrap();
        assert!(verify_apk(&mut archive, Some((stage, ABI, b"changed"))).is_err());
    }
}
