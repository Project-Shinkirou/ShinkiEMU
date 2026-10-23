use anyhow::{bail, Context, Result};
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
    pub artifact_kind: String,
    pub resources: Vec<String>,
    pub licenses: Vec<String>,
    pub supported_targets: Vec<String>,
    #[serde(default)]
    pub static_manager_targets: Vec<String>,
    pub blocked: Vec<Blocked>,
    pub publish_blocked: String,
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
    pub status: &'static str,
    pub reason: Option<String>,
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
                if !target.manager_supported || target.platform != "android" {
                    bail!("invalid static Manager integration");
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
    pub fn matrix(&self) -> Result<Matrix> {
        let mut include = Vec::new();
        for t in &self.targets {
            for component in
                std::iter::once("manager").chain(self.cores.iter().map(|c| c.id.as_str()))
            {
                let reason = self.blocked(component, &t.triple)?.map(str::to_owned);
                include.push(MatrixRow {
                    target: t.triple.clone(),
                    component: component.into(),
                    runner: t.runner.clone(),
                    platform: t.platform.clone(),
                    architecture: t.architecture.clone(),
                    variant: t.variant.clone(),
                    status: if reason.is_some() {
                        "blocked"
                    } else {
                        "supported"
                    },
                    reason,
                });
            }
        }
        Ok(Matrix { include })
    }
}
