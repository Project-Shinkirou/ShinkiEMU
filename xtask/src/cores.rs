use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize)]
pub struct Core {
    pub id: String,
    pub display_name: String,
    pub family_name: String,
    pub license: String,
    pub manifest: PathBuf,
    pub package: String,
    pub features: Vec<String>,
    pub dependencies: Vec<String>,
    pub artifact_kind: String,
    pub resources: Vec<String>,
    pub licenses: Vec<String>,
    pub supported_targets: Vec<String>,
    #[serde(default)]
    pub static_manager_targets: Vec<String>,
    pub blocked: Vec<Blocked>,
    pub publish_blocked: String,
    pub source: Option<CoreSource>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoreSource {
    pub repo: String,
    pub branch: String,
    pub rev: String,
    /// Original upstream project this organization fork tracks.
    pub upstream: String,
}

impl Core {
    pub fn source_directory(&self, repository: &Path) -> Result<PathBuf> {
        if self.source.is_none() {
            return Ok(repository.to_owned());
        }
        let relative = self
            .manifest
            .strip_prefix("ThirdParty")
            .context("external core manifests must be under ThirdParty")?;
        let directory = relative
            .components()
            .next()
            .context("external core directory missing")?;
        Ok(repository.join("ThirdParty").join(directory.as_os_str()))
    }
}

impl CoreSource {
    fn validate(&self) -> Result<()> {
        let repository = self
            .repo
            .strip_prefix("https://github.com/Project-Shinkirou/")
            .context("external core source must be an organization GitHub repository")?;
        crate::package::portable(repository)?;
        if repository.contains('/') {
            bail!("core source must name one GitHub repository");
        }
        crate::package::portable(&self.branch)?;
        if self.branch.contains("..") || self.branch.split('/').any(|part| part.ends_with(".lock"))
        {
            bail!("invalid source branch");
        }
        if self.rev.len() != 40
            || !self
                .rev
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("core source revision must be an exact lowercase 40-hex commit");
        }
        let upstream = self
            .upstream
            .strip_prefix("https://github.com/")
            .context("core upstream must be a GitHub repository URL")?;
        let mut parts = upstream.split('/');
        let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
            bail!("core upstream must name one GitHub owner/repository");
        };
        crate::package::portable(owner)?;
        crate::package::portable(name)?;
        if owner.eq_ignore_ascii_case("Project-Shinkirou") {
            bail!("core upstream must be the original project, not the organization fork");
        }
        Ok(())
    }
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
    pub tools: Vec<String>,
    pub manager_supported: bool,
    pub manager_blocked: String,
    /// Non-empty: every component on this target is blocked with this reason,
    /// overriding core `supported_targets` and `manager_supported`.
    #[serde(default)]
    pub blocked: String,
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
        let c: CoreFile = toml::from_str(&std::fs::read_to_string(root.join("build/cores.toml"))?)?;
        let t: TargetFile =
            toml::from_str(&std::fs::read_to_string(root.join("build/targets.toml"))?)?;
        if c.schema != "shinki.emu.build_cores.v1" || t.schema != "shinki.emu.build_targets.v1" {
            bail!("unsupported build inventory schema");
        }
        let result = Self {
            root: root.to_owned(),
            cores: c.cores,
            targets: t.targets,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<()> {
        if self.cores.is_empty() || self.targets.is_empty() {
            bail!("empty build matrix");
        }
        let mut ids = BTreeSet::new();
        let mut slugs = BTreeSet::new();
        let mut triples = BTreeSet::new();
        for t in &self.targets {
            if !triples.insert(&t.triple) {
                bail!("duplicate target {}", t.triple);
            }
            if !t.manager_supported && t.manager_blocked.is_empty() {
                bail!("target needs a Manager blocked reason");
            }
        }
        for c in &self.cores {
            if c.id == "manager" || !ids.insert(&c.id) || !slugs.insert(slug(&c.id)) {
                bail!("duplicate core id or release slug: {}", c.id);
            }
            crate::package::portable(&c.id)?;
            crate::package::portable(&c.manifest.to_string_lossy().replace('\\', "/"))?;
            if c.manifest.starts_with("ThirdParty") != c.source.is_some() {
                bail!(
                    "external core {} needs a pinned source declaration; in-repo cores must not have one",
                    c.id
                );
            }
            if let Some(source) = &c.source {
                source.validate()?;
                c.source_directory(&self.root)?;
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
            if seen != triples {
                bail!("core {} does not describe every target", c.id);
            }
            for t in &c.static_manager_targets {
                let target = self.target(t)?;
                if !target.manager_supported
                    || !matches!(target.platform.as_str(), "android" | "ios")
                {
                    bail!("invalid static Manager integration");
                }
            }
        }
        self.validate_static_dependencies()?;
        Ok(())
    }
    fn validate_static_dependencies(&self) -> Result<()> {
        let manifest: toml::Value = toml::from_str(&std::fs::read_to_string(
            self.root
                .join("Source/Manager/shinki-emu-manager/Cargo.toml"),
        )?)?;
        for platform in ["android", "ios"] {
            let key = format!("cfg(target_os = \"{platform}\")");
            let dependencies = manifest["target"][&key]["dependencies"]
                .as_table()
                .context("Manager target dependencies")?;
            for core in &self.cores {
                let embedded = self.targets.iter().any(|target| {
                    target.platform == platform
                        && core.static_manager_targets.contains(&target.triple)
                });
                let dependency = dependencies.get(&core.package);
                if !embedded {
                    if dependency.is_some() {
                        bail!("Manager {platform} depends on non-static core {}", core.id);
                    }
                    continue;
                }
                let dependency = dependency.with_context(|| {
                    format!("Manager {platform} is missing static core {}", core.id)
                })?;
                if let Some(source) = &core.source {
                    if dependency.get("git").and_then(toml::Value::as_str) != Some(&source.repo)
                        || dependency.get("rev").and_then(toml::Value::as_str) != Some(&source.rev)
                        || dependency.get("features").is_some_and(|features| {
                            features.as_array().is_none_or(|features| !features.is_empty())
                        })
                    {
                        bail!("Manager {platform} static core {} pin/features differ from inventory", core.id);
                    }
                } else {
                    let expected = core
                        .manifest
                        .parent()
                        .and_then(|path| path.strip_prefix("Source").ok())
                        .context("in-repo static manifest parent")?;
                    let expected = format!("../../{}", expected.to_string_lossy());
                    if dependency.get("path").and_then(toml::Value::as_str) != Some(expected.as_str())
                        || dependency.get("git").is_some()
                        || dependency.get("rev").is_some()
                    {
                        bail!("Manager {platform} in-repo static core {} path differs from inventory", core.id);
                    }
                }
                if dependency.get("default-features").and_then(toml::Value::as_bool) == Some(true) {
                    bail!("Manager {platform} static core {} must disable default features", core.id);
                }
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
                    blocked: self.blocked(component, &t.triple)?.map(str::to_owned),
                });
            }
        }
        Ok(rows)
    }
    /// GitHub job matrix: supported combinations only. Blocked combinations need no
    /// runner; `rows()` and release notes still report them. With `ci`, Android
    /// Manager rows after the first are dropped: `xtask ci` builds every Android ABI
    /// into the same universal APK from either triple.
    pub fn job_matrix(&self, ci: bool) -> Result<Matrix> {
        let mut android_manager = false;
        let include = self
            .rows()?
            .into_iter()
            .filter(|row| {
                if row.blocked.is_some() {
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
    fn static_manager_pins_and_declared_integrations_cannot_drift() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inventory = Inventory::load(root).unwrap();
        let mut changed = inventory.clone();
        changed
            .cores
            .iter_mut()
            .find(|core| core.id == "rfvp")
            .unwrap()
            .source
            .as_mut()
            .unwrap()
            .rev = "0".repeat(40);
        assert!(changed.validate_static_dependencies().is_err());
        let mut changed = inventory;
        changed
            .cores
            .iter_mut()
            .find(|core| core.id == "rfvp")
            .unwrap()
            .static_manager_targets
            .clear();
        assert!(changed.validate_static_dependencies().is_err());
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
                .job_matrix(false)
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
    fn ci_matrix_builds_the_universal_android_manager_once() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inventory = Inventory::load(root).unwrap();
        let android_managers = |ci| {
            inventory
                .job_matrix(ci)
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
