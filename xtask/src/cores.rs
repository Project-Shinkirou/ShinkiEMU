use crate::affected::Affected;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use shinki_emu_build::ohos::{HapCore, Target as OhosTarget};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[path = "../../build/core_adapters.rs"]
mod core_adapters;

#[path = "../../build/android_dynamic.rs"]
#[allow(
    dead_code,
    reason = "xtask validates the policy; only Manager uses the projection"
)]
mod android_dynamic;

#[derive(Clone, Debug, Deserialize)]
pub struct Core {
    pub id: String,
    pub family_id: String,
    pub display_name: String,
    pub family_name: String,
    pub license: String,
    pub resources: Vec<String>,
    #[serde(default)]
    licenses: Vec<String>,
    #[serde(default)]
    adapter_licenses: Vec<String>,
    #[serde(default)]
    pub recognition: shinki_emu_metadata::CoreRecognition,
    pub supported_targets: Vec<String>,
    /// The shared catalog parser validates this policy at inventory load.
    pub(crate) android_dynamic: Option<toml::Table>,
    #[serde(default)]
    pub ios_framework: bool,
    pub ios_resource_subdir: Option<String>,
    pub(crate) ohos_hap: Option<HapCore>,
    pub blocked: Vec<Blocked>,
    pub attribution: Option<Attribution>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Attribution {
    pub adapter_branch: String,
    pub upstream_identity: shinki_emu_metadata::UpstreamIdentity,
    pub release_sequence: u64,
}

impl Core {
    pub fn entry_package(&self) -> String {
        format!("shinki-emu-core-{}", self.id.replace('.', "-"))
    }

    pub fn entry_manifest(&self, root: &Path) -> PathBuf {
        root.join("Source/Cores").join(&self.id).join("Cargo.toml")
    }

    pub fn adapter_package(&self) -> String {
        format!("shinki-emu-{}", core_adapters::adapter_id(&self.id))
    }

    pub fn adapter_source(&self, root: &Path) -> Result<Option<LockedAdapter>> {
        if self.attribution.is_none() {
            return Ok(None);
        }
        let lock = toml::from_str(&std::fs::read_to_string(root.join("Cargo.lock"))?)?;
        Ok(
            core_adapters::locked_adapter(&lock, &self.id).map(|adapter| LockedAdapter {
                repo: adapter.repo.to_owned(),
                commit: adapter.commit.to_owned(),
            }),
        )
    }

    pub fn identity(&self, root: &Path) -> Result<Option<shinki_emu_metadata::CoreIdentity>> {
        let Some(attribution) = &self.attribution else {
            return Ok(None);
        };
        let Some(adapter) = self.adapter_source(root)? else {
            return Ok(None);
        };
        Ok(Some(shinki_emu_metadata::CoreIdentity {
            upstream: attribution.upstream_identity.clone(),
            adapter: shinki_emu_metadata::AdapterIdentity {
                repo: adapter.repo,
                branch: attribution.adapter_branch.clone(),
                commit: adapter.commit,
            },
            release_sequence: attribution.release_sequence,
        }))
    }

    pub fn adapter_manifest(&self, root: &Path, target: &str) -> Result<PathBuf> {
        let package = self.adapter_package();
        let metadata: serde_json::Value = serde_json::from_slice(&crate::dependencies::capture(
            std::process::Command::new("cargo").current_dir(root).args([
                "metadata",
                "--locked",
                "--format-version",
                "1",
                "--filter-platform",
                target,
            ]),
        )?)?;
        metadata["packages"]
            .as_array()
            .context("Cargo metadata packages")?
            .iter()
            .find(|value| value["name"] == package)
            .and_then(|value| value["manifest_path"].as_str())
            .map(PathBuf::from)
            .with_context(|| format!("Cargo package {package} missing for {target}"))
    }

    pub fn source_directory(&self, root: &Path, target: &str) -> Result<PathBuf> {
        if self.attribution.is_none() {
            return Ok(root.to_owned());
        }
        let manifest = self.adapter_manifest(root, target)?;
        let output = crate::dependencies::capture(
            std::process::Command::new("git")
                .current_dir(manifest.parent().context("adapter manifest directory")?)
                .args(["rev-parse", "--show-toplevel"]),
        )?;
        Ok(PathBuf::from(String::from_utf8(output)?.trim()))
    }

    pub fn license_files(&self, root: &Path, target: &str) -> Result<Vec<(PathBuf, &str)>> {
        let mut files: Vec<_> = self
            .licenses
            .iter()
            .map(|path| (root.join(path), path.as_str()))
            .collect();
        if !self.adapter_licenses.is_empty() {
            let directory = self.source_directory(root, target)?;
            files.extend(
                self.adapter_licenses
                    .iter()
                    .map(|path| (directory.join(path), path.as_str())),
            );
        }
        Ok(files)
    }
}

pub struct LockedAdapter {
    pub repo: String,
    pub commit: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Blocked {
    pub target: String,
    pub reason: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Target {
    pub triple: String,
    pub runner: String,
    pub platform: String,
    pub architecture: String,
    pub variant: String,
    pub manager_supported: bool,
    pub manager_blocked: String,
    /// Buildable and publishable on a host with the native SDK, but never
    /// scheduled on a GitHub runner. Publication requires an exact-head
    /// local artifact and successful remote CI.
    #[serde(default)]
    pub manager_local_build: bool,
    /// Non-empty: every component on this target is blocked with this reason,
    /// overriding core `supported_targets` and `manager_supported`.
    #[serde(default)]
    pub blocked: String,
}

impl Target {
    pub(crate) fn is_local_qa(&self) -> bool {
        OhosTarget::from_rust_target(&self.triple).is_some_and(OhosTarget::local_qa)
    }
}

#[derive(Deserialize)]
struct CoreFile {
    schema: String,
    cores: Vec<Core>,
}
#[derive(Deserialize)]
struct TargetFile {
    schema: String,
    targets: Vec<Target>,
}
#[derive(Clone, Debug)]
pub struct Inventory {
    pub root: PathBuf,
    pub cores: Vec<Core>,
    pub targets: Vec<Target>,
}
#[derive(Serialize)]
pub struct Matrix {
    pub include: Vec<MatrixRow>,
}
#[derive(Serialize)]
pub struct MatrixRow {
    pub target: String,
    pub component: String,
    pub external: bool,
    pub runner: String,
    pub platform: String,
    pub architecture: String,
    pub variant: String,
    /// Declared reason this combination is not built; never part of a job matrix.
    #[serde(skip)]
    pub blocked: Option<String>,
}

pub fn slug(id: &str) -> String {
    id.to_ascii_lowercase().replace(['.', '_'], "-")
}
impl Inventory {
    pub fn load(root: &Path) -> Result<Self> {
        let c: toml::Value =
            toml::from_str(&std::fs::read_to_string(root.join("build/cores.toml"))?)?;
        for core in c
            .get("cores")
            .and_then(toml::Value::as_array)
            .context("core inventory")?
        {
            android_dynamic::parse(core).map_err(anyhow::Error::msg)?;
        }
        let c: CoreFile = c.try_into()?;
        let t: TargetFile =
            toml::from_str(&std::fs::read_to_string(root.join("build/targets.toml"))?)?;
        if c.schema != "shinki.emu.build_cores.v2" || t.schema != "shinki.emu.build_targets.v1" {
            bail!("unsupported build inventory schema");
        }
        let result = Self {
            root: root.to_owned(),
            cores: c.cores,
            targets: t.targets,
        };
        result.validate()?;
        crate::apple::ios_frameworks::validate_inventory(&result)?;
        Ok(result)
    }
    fn validate(&self) -> Result<()> {
        if self.cores.is_empty() || self.targets.is_empty() {
            bail!("empty build matrix");
        }
        let mut ids = BTreeSet::new();
        let mut slugs = BTreeSet::new();
        let mut triples = BTreeSet::new();
        let mut standalone_triples = BTreeSet::new();
        for t in &self.targets {
            if !triples.insert(&t.triple) {
                bail!("duplicate target {}", t.triple);
            }
            if t.is_local_qa() {
                if t.runner != "local-only" || t.manager_supported || t.manager_local_build {
                    bail!(
                        "{}: private QA target cannot be scheduled or published",
                        t.triple
                    );
                }
            } else {
                standalone_triples.insert(&t.triple);
            }
            if !t.manager_supported && t.manager_blocked.is_empty() {
                bail!("target needs a Manager blocked reason");
            }
            if t.manager_local_build && (t.runner != "local-only" || !t.manager_supported) {
                bail!(
                    "{}: locally built Manager must be supported and use a local-only runner",
                    t.triple
                );
            }
        }
        let mut has_ohos_companion = false;
        for c in &self.cores {
            if c.id == "manager" || !ids.insert(&c.id) || !slugs.insert(slug(&c.id)) {
                bail!("duplicate core id or release slug: {}", c.id);
            }
            crate::package::portable(&c.id)?;
            if let Some(recipe) = &c.ohos_hap {
                recipe.validate(&c.id)?;
                if has_ohos_companion || c.attribution.is_some() {
                    bail!("the OHOS HAP requires one first-party companion core");
                }
                has_ohos_companion = true;
                for target in [OhosTarget::Arm64, OhosTarget::X64] {
                    let triple = target.rust_target();
                    self.require_buildable("manager", triple)?;
                    if c.supported_targets.iter().any(|t| t == triple) {
                        bail!("{} is a separate HAP library on {triple}", c.id);
                    }
                }
            }
            if c.family_id.is_empty()
                || c.family_id.len() > 64
                || !c
                    .family_id
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
                || !c.family_id.as_bytes()[0].is_ascii_lowercase()
                || c.display_name.trim().is_empty()
            {
                bail!("invalid family_id or display_name for {}", c.id);
            }
            if c.attribution.is_some() && (slug(&c.id).len() > 32 || c.id.contains('/')) {
                bail!("external plugin slug must be one segment of at most 32 bytes");
            }
            let mut seen = BTreeSet::new();
            for t in &c.supported_targets {
                self.target(t)?;
                if !seen.insert(t) {
                    bail!("duplicate supported target");
                }
            }
            for b in &c.blocked {
                self.target(&b.target)?;
                if b.reason.is_empty() || !seen.insert(&b.target) {
                    bail!("invalid blocked target for {}", c.id);
                }
            }
            if seen != standalone_triples {
                bail!(
                    "core {} must describe exactly the non-QA standalone targets",
                    c.id
                );
            }
            if c.ios_framework
                && (!c
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
                    || c.supported_targets
                        .iter()
                        .any(|target| crate::apple::ios_frameworks::target_supported(target)))
            {
                bail!("invalid iOS Framework recipe for {}", c.id);
            }
            if let Some(dir) = &c.ios_resource_subdir {
                if !c.ios_framework {
                    bail!("{} declares iOS resources without a Framework", c.id);
                }
                crate::package::portable(dir)?;
            }
        }
        Ok(())
    }
    pub fn core(&self, id: &str) -> Result<&Core> {
        self.cores
            .iter()
            .find(|c| c.id == id)
            .with_context(|| format!("unknown core {id}"))
    }
    pub fn target(&self, triple: &str) -> Result<&Target> {
        self.targets
            .iter()
            .find(|t| t.triple == triple)
            .with_context(|| format!("unknown target {triple}"))
    }
    pub fn blocked(&self, component: &str, triple: &str) -> Result<Option<&str>> {
        let t = self.target(triple)?;
        if !t.blocked.is_empty() {
            return Ok(Some(t.blocked.as_str()));
        }
        if component == "manager" {
            return Ok((!t.manager_supported).then_some(t.manager_blocked.as_str()));
        }
        let c = self.core(component)?;
        if t.is_local_qa() {
            return Ok(Some("Private OHOS QA only; no standalone core packages."));
        }
        Ok(c.blocked
            .iter()
            .find(|b| b.target == triple)
            .map(|b| b.reason.as_str()))
    }
    pub fn require_supported(&self, component: &str, triple: &str) -> Result<()> {
        if let Some(reason) = self.blocked(component, triple)? {
            bail!("{component}/{triple} blocked: {reason}");
        }
        Ok(())
    }
    /// Packaging additionally admits a Manager on an explicitly local QA target.
    pub fn require_buildable(&self, component: &str, triple: &str) -> Result<()> {
        let t = self.target(triple)?;
        if component == "manager"
            && t.blocked.is_empty()
            && (t.manager_local_build || t.is_local_qa())
        {
            return Ok(());
        }
        self.require_supported(component, triple)
    }
    /// Every Manager/core combination on every declared target, with its blocked reason.
    pub fn rows(&self) -> Result<Vec<MatrixRow>> {
        let mut rows = Vec::new();
        for t in &self.targets {
            for component in
                std::iter::once("manager").chain(self.cores.iter().map(|c| c.id.as_str()))
            {
                rows.push(MatrixRow {
                    target: t.triple.clone(),
                    component: component.into(),
                    runner: t.runner.clone(),
                    platform: t.platform.clone(),
                    architecture: t.architecture.clone(),
                    variant: t.variant.clone(),
                    external: component != "manager" && self.core(component)?.attribution.is_some(),
                    blocked: self.blocked(component, &t.triple)?.map(str::to_owned),
                });
            }
        }
        Ok(rows)
    }
    /// GitHub job matrix: supported combinations with hosted runners only.
    /// Local-only packages join at the exact-head publication handoff. With
    /// `ci`, Android Manager rows after the first are dropped because either
    /// row builds the same universal APK.
    pub fn job_matrix(&self, ci: bool, affected: Option<&Affected>) -> Result<Matrix> {
        let mut android_manager = false;
        let include = self
            .rows()?
            .into_iter()
            .filter(|row| {
                if row.blocked.is_some()
                    || row.runner == "local-only"
                    || affected.is_some_and(|a| !a.row(&row.component, &row.target))
                {
                    return false;
                }
                if ci && row.component == "manager" && row.platform == "android" {
                    return !std::mem::replace(&mut android_manager, true);
                }
                true
            })
            .collect();
        Ok(Matrix { include })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_ids_are_required_and_canonical_but_may_have_multiple_implementations() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inventory = Inventory::load(root).unwrap();
        for invalid in ["", "Siglus", "../cmvs", " softpal", "1family", "引擎"] {
            let mut changed = inventory.clone();
            changed.cores[0].family_id = invalid.into();
            assert!(changed.validate().is_err(), "{invalid}");
        }
        let mut changed = inventory.clone();
        changed.cores[0].display_name = " \n".into();
        assert!(changed.validate().is_err());
        let mut changed = inventory;
        changed.cores[1].family_id = changed.cores[0].family_id.clone();
        changed.validate().unwrap();
        let source = std::fs::read_to_string(root.join("build/cores.toml")).unwrap();
        let mut parsed: toml::Value = toml::from_str(&source).unwrap();
        parsed["cores"][0]
            .as_table_mut()
            .unwrap()
            .remove("family_id");
        let core: Result<Core, _> = parsed["cores"][0].clone().try_into();
        assert!(core.is_err());
    }

    #[test]
    fn published_identity_uses_the_resolved_adapter_commit() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inventory = Inventory::load(root).unwrap();
        let core = inventory.core("rfvp").unwrap();
        let temp = crate::temporary::directory().unwrap();
        let lock = temp.path().join("Cargo.lock");
        let commit = "f".repeat(40);
        std::fs::write(
            &lock,
            format!(
                r#"
            [[package]]
            name = "shinki-emu-rfvp"
            source = "git+https://github.com/Project-Shinkirou/rfvp?rev=requested#{commit}"
        "#
            ),
        )
        .unwrap();
        let identity = core.identity(temp.path()).unwrap().unwrap();
        assert_eq!(identity.adapter.commit, commit);
        assert_eq!(
            identity.adapter.repo,
            "https://github.com/Project-Shinkirou/rfvp"
        );
        assert_eq!(
            identity.adapter.branch,
            core.attribution.as_ref().unwrap().adapter_branch
        );

        std::fs::write(lock, "[[package]]\nname = \"shinki-emu-rfvp\"\n").unwrap();
        assert!(core.identity(temp.path()).unwrap().is_none());
    }

    #[test]
    fn target_block_overrides_supported_manager_and_cores() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inventory = Inventory::load(root).unwrap();
        let triple = "x86_64-pc-windows-msvc";
        assert!(inventory.blocked("manager", triple).unwrap().is_none());
        assert!(inventory.blocked("rfvp", triple).unwrap().is_none());
        inventory
            .targets
            .iter_mut()
            .find(|target| target.triple == triple)
            .unwrap()
            .blocked = "deferred".into();
        for component in ["manager", "rfvp"] {
            assert_eq!(
                inventory.blocked(component, triple).unwrap(),
                Some("deferred")
            );
            assert!(inventory.require_supported(component, triple).is_err());
        }
        assert!(
            inventory
                .rows()
                .unwrap()
                .iter()
                .filter(|row| row.target == triple)
                .all(|row| row.blocked.as_deref() == Some("deferred"))
        );
        assert!(
            !inventory
                .job_matrix(false, None)
                .unwrap()
                .include
                .iter()
                .any(|row| row.target == triple)
        );
        assert!(
            inventory
                .blocked("manager", "aarch64-linux-android")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn local_only_manager_joins_release_but_never_runs_on_hosted_ci() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inventory = Inventory::load(root).unwrap();
        let triple = "aarch64-unknown-linux-ohos";
        assert!(inventory.require_supported("manager", triple).is_ok());
        assert!(inventory.require_buildable("manager", triple).is_ok());
        assert!(inventory.require_buildable("rfvp", triple).is_err());
        assert!(inventory.rows().unwrap().iter().any(|row| {
            row.target == triple && row.component == "manager" && row.blocked.is_none()
        }));
        assert!(
            !inventory
                .job_matrix(true, None)
                .unwrap()
                .include
                .iter()
                .any(|row| row.target == triple)
        );
        let index = inventory
            .targets
            .iter()
            .position(|target| target.triple == triple)
            .unwrap();
        inventory.targets[index].blocked = "deferred".into();
        assert!(inventory.require_buildable("manager", triple).is_err());
        inventory.targets[index].blocked.clear();
        inventory.targets[index].manager_supported = false;
        assert!(inventory.validate().is_err());
    }

    #[test]
    fn private_ohos_qa_target_is_package_only() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inventory = Inventory::load(root).unwrap();
        let triple = OhosTarget::X64.rust_target();
        assert!(inventory.require_buildable("manager", triple).is_ok());
        assert!(inventory.require_supported("manager", triple).is_err());
        for core in &inventory.cores {
            assert!(inventory.require_buildable(&core.id, triple).is_err());
        }
        for ci in [false, true] {
            assert!(
                inventory
                    .job_matrix(ci, None)
                    .unwrap()
                    .include
                    .iter()
                    .all(|row| row.target != triple)
            );
        }
        let row = inventory
            .targets
            .iter_mut()
            .find(|row| row.triple == triple)
            .unwrap();
        row.manager_supported = true;
        assert!(inventory.validate().is_err());
    }

    #[test]
    fn ci_matrix_builds_the_universal_android_manager_once() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inventory = Inventory::load(root).unwrap();
        let android_managers = |ci| {
            inventory
                .job_matrix(ci, None)
                .unwrap()
                .include
                .iter()
                .filter(|row| row.component == "manager" && row.platform == "android")
                .count()
        };
        assert_eq!(android_managers(false), 2);
        assert_eq!(android_managers(true), 1);
    }
}
