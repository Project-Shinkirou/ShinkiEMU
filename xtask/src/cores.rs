use crate::affected::Affected;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize)]
pub struct Core {
    pub id: String,
    pub family_id: String,
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
    #[serde(default)]
    pub recognition: shinki_emu_metadata::CoreRecognition,
    pub supported_targets: Vec<String>,
    #[serde(default)]
    pub static_manager_targets: Vec<String>,
    #[serde(default)]
    pub static_features: Vec<String>,
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
    pub upstream_identity: shinki_emu_metadata::UpstreamIdentity,
    pub release_sequence: u64,
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
    pub fn identity(&self) -> shinki_emu_metadata::CoreIdentity {
        shinki_emu_metadata::CoreIdentity {
            upstream: self.upstream_identity.clone(),
            adapter: shinki_emu_metadata::AdapterIdentity {
                repo: self.repo.clone(),
                branch: self.branch.clone(),
                commit: self.rev.clone(),
            },
            release_sequence: self.release_sequence,
        }
    }
    fn validate(&self) -> Result<()> {
        self.identity().validate()?;
        if self.upstream != self.upstream_identity.repo {
            bail!("upstream identity must name the original upstream repository");
        }
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
    fn static_manager_cfg(&self) -> Option<&'static str> {
        match self.platform.as_str() {
            "android" => Some("cfg(target_os = \"android\")"),
            "ios" => Some("cfg(target_os = \"ios\")"),
            "ohos" => Some("cfg(target_env = \"ohos\")"),
            _ => None,
        }
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
    /// Distribution restriction of the core; the release workflow refuses these components.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub publish_blocked: String,
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
            if t.manager_local_build && (t.runner != "local-only" || !t.manager_supported) {
                bail!(
                    "{}: locally built Manager must be supported and use a local-only runner",
                    t.triple
                );
            }
        }
        for c in &self.cores {
            if c.id == "manager" || !ids.insert(&c.id) || !slugs.insert(slug(&c.id)) {
                bail!("duplicate core id or release slug: {}", c.id);
            }
            crate::package::portable(&c.id)?;
            if c.family_id.is_empty()
                || c.family_id.len() > 64
                || !c.family_id.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
                || !c.family_id.as_bytes()[0].is_ascii_lowercase()
                || c.display_name.trim().is_empty()
            {
                bail!("invalid family_id or display_name for {}", c.id);
            }
            crate::package::portable(&c.manifest.to_string_lossy().replace('\\', "/"))?;
            if c.manifest.starts_with("ThirdParty") != c.source.is_some() {
                bail!(
                    "external core {} needs a pinned source declaration; in-repo cores must not have one",
                    c.id
                );
            }
            if let Some(source) = &c.source {
                if slug(&c.id).len() > 32 || c.id.contains('/') {
                    bail!("external plugin slug must be one segment of at most 32 bytes");
                }
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
                if !(target.manager_supported || target.manager_local_build)
                    || target.static_manager_cfg().is_none()
                {
                    bail!("invalid static Manager integration");
                }
            }
        }
        self.validate_static_dependencies()?;
        Ok(())
    }
    fn validate_static_dependencies(&self) -> Result<()> {
        let manager_dir = self.root.join("Source/Manager/shinki-emu-manager");
        let manifest: toml::Table =
            toml::from_str(&std::fs::read_to_string(manager_dir.join("Cargo.toml"))?)?;
        self.validate_static_manifest(&manager_dir, &manifest)
    }

    fn validate_static_manifest(&self, manager_dir: &Path, manifest: &toml::Table) -> Result<()> {
        let target_tables = manifest.get("target").and_then(toml::Value::as_table);
        fn dependencies(table: &toml::Value) -> Option<&toml::Table> {
            table.get("dependencies")?.as_table()
        }

        // The validator only needs Cargo's literal triples and the three existing
        // platform selectors. A core in another cfg must not escape that check.
        for (selector, deps) in target_tables
            .into_iter()
            .flatten()
            .filter_map(|(key, table)| Some((key.as_str(), dependencies(table)?)))
        {
            let supported = self.targets.iter().any(|target| {
                target
                    .static_manager_cfg()
                    .is_some_and(|cfg| selector == cfg || selector == target.triple)
            });
            for (key, dep) in deps {
                let package = dep
                    .get("package")
                    .and_then(toml::Value::as_str)
                    .unwrap_or(key);
                if let Some(core) = self.cores.iter().find(|core| core.package == package) {
                    if !supported {
                        bail!(
                            "Manager declares core {} under unsupported target selector {selector}; use a platform table or exact static target triple",
                            core.id
                        );
                    }
                    if key != package {
                        bail!(
                            "Manager static core {} must use dependency name {package}",
                            core.id
                        );
                    }
                }
            }
        }

        if let Some(deps) = manifest.get("dependencies").and_then(toml::Value::as_table) {
            for (key, dep) in deps {
                let package = dep
                    .get("package")
                    .and_then(toml::Value::as_str)
                    .unwrap_or(key);
                if self.cores.iter().any(|core| core.package == package)
                    && dep.get("optional").and_then(toml::Value::as_bool) != Some(true)
                {
                    bail!("Manager core {package} must use a target table or be optional");
                }
            }
        }

        for target in &self.targets {
            let Some(cfg) = target.static_manager_cfg() else {
                continue;
            };
            let triple = &target.triple;
            let deps = |selector| target_tables?.get(selector).and_then(dependencies);
            let platform = deps(cfg);
            let exact = deps(triple.as_str());
            for core in &self.cores {
                let broad = platform.and_then(|deps| deps.get(&core.package));
                let specific = exact.and_then(|deps| deps.get(&core.package));
                if broad.is_some() && specific.is_some() {
                    // Cargo can combine matching dependencies; the exact table does not override.
                    bail!(
                        "Manager {triple} declares static core {} in both platform and exact target tables",
                        core.id
                    );
                }
                match (
                    core.static_manager_targets.contains(triple),
                    broad.or(specific),
                ) {
                    (false, None) => {}
                    (false, Some(_)) => {
                        bail!("Manager {triple} depends on non-static core {}", core.id)
                    }
                    (true, None) => bail!("Manager {triple} is missing static core {}", core.id),
                    (true, Some(dependency)) => {
                        self.validate_static_dependency(manager_dir, triple, core, dependency)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_static_dependency(
        &self,
        manager_dir: &Path,
        triple: &str,
        core: &Core,
        dependency: &toml::Value,
    ) -> Result<()> {
        let string = |key| dependency.get(key).and_then(toml::Value::as_str);
        let features_match = match dependency.get("features") {
            None => core.static_features.is_empty(),
            Some(value) => value.as_array().is_some_and(|actual| {
                actual.iter().all(|feature| {
                    feature
                        .as_str()
                        .is_some_and(|name| core.static_features.iter().any(|f| f == name))
                }) && core
                    .static_features
                    .iter()
                    .all(|feature| actual.iter().any(|v| v.as_str() == Some(feature.as_str())))
            }),
        };
        let source_match = if let Some(source) = &core.source {
            string("git") == Some(source.repo.as_str())
                && string("rev") == Some(source.rev.as_str())
        } else if let Some(path) = string("path") {
            let package_dir = self.root.join(
                core.manifest
                    .parent()
                    .context("in-tree core manifest has no package directory")?,
            );
            std::fs::canonicalize(manager_dir.join(path))
                .ok()
                .is_some_and(|actual| {
                    std::fs::canonicalize(package_dir).is_ok_and(|expected| actual == expected)
                })
        } else {
            false
        };
        if !source_match
            || !features_match
            || dependency
                .get("default-features")
                .and_then(toml::Value::as_bool)
                != Some(false)
            || !dependency
                .get("optional")
                .is_none_or(|v| v.as_bool() == Some(false))
            || dependency
                .get("package")
                .is_some_and(|_| string("package") != Some(core.package.as_str()))
        {
            bail!(
                "Manager {triple} static core {} must match inventory pin/path/features and disable default features without making the dependency optional",
                core.id
            );
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
    /// Like [`Self::require_supported`], but admits a Manager whose target
    /// allows local builds of an unsupported Manager.
    pub fn require_buildable(&self, component: &str, triple: &str) -> Result<()> {
        let t = self.target(triple)?;
        if component == "manager" && t.blocked.is_empty() && t.manager_local_build {
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
                    external: component != "manager" && self.core(component)?.source.is_some(),
                    blocked: self.blocked(component, &t.triple)?.map(str::to_owned),
                    publish_blocked: if component == "manager" {
                        String::new()
                    } else {
                        self.core(component)?.publish_blocked.clone()
                    },
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
        parsed["cores"][0].as_table_mut().unwrap().remove("family_id");
        let core: Result<Core, _> = parsed["cores"][0].clone().try_into();
        assert!(core.is_err());
    }

    #[test]
    fn static_manager_pins_features_and_paths_cannot_drift() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inventory = Inventory::load(root).unwrap();

        let mut changed = inventory.clone();
        changed
            .cores
            .iter_mut()
            .find(|core| core.id == "siglus_rs")
            .unwrap()
            .static_features = vec!["unexpected".into()];
        assert!(changed.validate_static_dependencies().is_err());

        let mut changed = inventory;
        changed
            .cores
            .iter_mut()
            .find(|core| core.id == "shinki.emu.musica")
            .unwrap()
            .manifest = "Source/Families/shinki-emu-cmvs/Cargo.toml".into();
        assert!(changed.validate_static_dependencies().is_err());
    }

    #[test]
    fn exact_android_target_only_embeds_the_inventory_abi() {
        const ARM: &str = "aarch64-linux-android";
        const X64: &str = "x86_64-linux-android";
        const ANDROID: &str = "cfg(target_os = \"android\")";
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inventory = Inventory::load(root).unwrap();
        let core = inventory.cores.iter_mut().find(|c| c.id == "rfvp").unwrap();
        core.static_manager_targets.retain(|triple| triple != X64);
        let package = core.package.clone();
        let manager = root.join("Source/Manager/shinki-emu-manager");
        let mut manifest: toml::Table =
            toml::from_str(&std::fs::read_to_string(manager.join("Cargo.toml")).unwrap()).unwrap();
        let targets = manifest["target"].as_table_mut().unwrap();
        let dependency = targets.get_mut(ANDROID).unwrap()["dependencies"]
            .as_table_mut()
            .unwrap()
            .remove(&package)
            .unwrap();
        targets.insert(
            ARM.into(),
            toml::Value::Table(toml::Table::from_iter([(
                "dependencies".into(),
                toml::Value::Table(toml::Table::from_iter([(
                    package.clone(),
                    dependency.clone(),
                )])),
            )])),
        );
        let check = |manifest: &toml::Table| inventory.validate_static_manifest(&manager, manifest);
        check(&manifest).unwrap();

        let mut broad = manifest.clone();
        let targets = broad["target"].as_table_mut().unwrap();
        targets.remove(ARM);
        targets.get_mut(ANDROID).unwrap()["dependencies"]
            .as_table_mut()
            .unwrap()
            .insert(package.clone(), dependency.clone());
        let err = check(&broad).unwrap_err().to_string();
        assert!(err.contains(X64) && err.contains("rfvp"));

        let mut overlap = manifest.clone();
        overlap["target"][ANDROID]["dependencies"]
            .as_table_mut()
            .unwrap()
            .insert(package.clone(), dependency);
        let err = check(&overlap).unwrap_err().to_string();
        assert!(err.contains(ARM) && err.contains("rfvp"));

        let mut missing = manifest.clone();
        missing["target"].as_table_mut().unwrap().remove(ARM);
        let err = check(&missing).unwrap_err().to_string();
        assert!(err.contains(ARM) && err.contains("rfvp"));

        for (key, value) in [
            ("rev", toml::Value::String("0".repeat(40))),
            ("default-features", toml::Value::Boolean(true)),
            ("optional", toml::Value::Boolean(true)),
            (
                "features",
                toml::Value::Array(vec![toml::Value::String("wrong".into())]),
            ),
        ] {
            let mut invalid = manifest.clone();
            invalid["target"][ARM]["dependencies"]
                .as_table_mut()
                .unwrap()
                .get_mut(&package)
                .unwrap()
                .as_table_mut()
                .unwrap()
                .insert(key.into(), value);
            assert!(check(&invalid).is_err(), "{key}");
        }
    }

    #[test]
    fn alternate_cfg_cannot_hide_a_known_in_tree_core() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inventory = Inventory::load(root).unwrap();
        let manager = root.join("Source/Manager/shinki-emu-manager");
        let mut manifest: toml::Table =
            toml::from_str(&std::fs::read_to_string(manager.join("Cargo.toml")).unwrap()).unwrap();
        let alternate: toml::Table = toml::from_str(
            r#"['cfg(all(target_os = "android", target_arch = "aarch64"))'.dependencies]
            hidden-musica = { package = "shinki-emu-musica", path = "../../Families/shinki-emu-musica", default-features = false }"#,
        )
        .unwrap();
        manifest["target"].as_table_mut().unwrap().extend(alternate);
        let err = inventory
            .validate_static_manifest(&manager, &manifest)
            .unwrap_err()
            .to_string();
        assert!(err.contains("shinki.emu.musica") && err.contains("target_arch"));
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
