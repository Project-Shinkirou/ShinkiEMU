use crate::{fields::*, MetadataError, CORE_RELEASE_URL_PREFIX};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PATCH_CATALOG_SCHEMA_VERSION: &str = "shinki.emu.patches.v1";
pub const MAX_PATCH_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PatchKind {
    Recipe,
    External,
    Hosted,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchArtifact {
    pub url: String,
    #[schemars(regex(pattern = r"^[0-9a-f]{64}$"))]
    pub sha256: String,
    #[schemars(range(min = 1, max = 1073741824))]
    pub size: u64,
    pub file_name: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchRecipe {
    pub script: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchCatalogEntry {
    pub id: String,
    pub title: BTreeMap<String, String>,
    pub version: String,
    pub vn_ids: Vec<String>,
    pub release_ids: Vec<String>,
    pub emulators: Vec<String>,
    pub kind: PatchKind,
    pub source_url: Option<String>,
    pub license: String,
    pub authorization_url: Option<String>,
    pub summary: BTreeMap<String, String>,
    pub issue_url: Option<String>,
    pub artifact: Option<PatchArtifact>,
    pub recipe: Option<PatchRecipe>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchCatalog {
    #[schemars(regex(pattern = r"^shinki\.emu\.patches\.v1$"))]
    pub schema: String,
    pub entries: Vec<PatchCatalogEntry>,
}
/// A repository-relative path: no links, drive names, ADS or traversal components.
pub fn is_safe_metadata_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && !path.contains('\\')
        && !path.contains("..")
        && path.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && !p.ends_with('.')
                && !p.ends_with(' ')
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
        })
}
fn https(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
            && u.port().is_none()
    })
}
fn localized(values: &BTreeMap<String, String>, max: usize) -> bool {
    values.len() == 2
        && ["zh-CN", "en"]
            .iter()
            .all(|l| values.get(*l).is_some_and(|v| is_bounded_text(v, 1, max)))
}
fn unique(values: &[String], valid: impl Fn(&str) -> bool) -> bool {
    let mut seen = BTreeSet::new();
    values.len() <= 4096 && values.iter().all(|v| valid(v) && seen.insert(v))
}
impl PatchCatalogEntry {
    pub fn validate(&self) -> Result<(), MetadataError> {
        let invalid = || MetadataError::SchemaMismatch("patch.entry");
        if !is_plugin_id(&self.id)
            || self.id == "."
            || self.id == ".."
            || !localized(&self.title, 256)
            || !localized(&self.summary, 2048)
            || self.version.len() > 64
            || semver::Version::parse(&self.version).is_err()
            || !is_bounded_text(&self.license, 1, 256)
            || self.vn_ids.is_empty()
            || self.emulators.is_empty()
            || !unique(&self.vn_ids, is_vndb_vn_id)
            || !unique(&self.release_ids, is_vndb_release_id)
            || !unique(&self.emulators, is_known_core_id)
        {
            return Err(invalid());
        }
        for value in [&self.source_url, &self.authorization_url, &self.issue_url]
            .into_iter()
            .flatten()
        {
            if value.len() > 2048 || !https(value) {
                return Err(invalid());
            }
        }
        if let Some(a) = &self.artifact {
            if !https(&a.url)
                || !is_sha256_hex(&a.sha256)
                || a.size == 0
                || a.size > MAX_PATCH_ARTIFACT_BYTES
                || !is_safe_metadata_relative_path(&a.file_name)
                || a.file_name.contains('/')
            {
                return Err(invalid());
            }
        }
        match self.kind {
            PatchKind::Recipe => {
                if self.artifact.is_some()
                    || !self.recipe.as_ref().is_some_and(|r| {
                        is_safe_metadata_relative_path(&r.script) && r.script.ends_with(".luau")
                    })
                {
                    return Err(invalid());
                }
            }
            PatchKind::External => {
                if self.recipe.is_some() || (self.artifact.is_none() && self.source_url.is_none()) {
                    return Err(invalid());
                }
            }
            PatchKind::Hosted => {
                if self.recipe.is_some() || self.authorization_url.is_none() {
                    return Err(invalid());
                }
                let Some(a) = &self.artifact else {
                    return Err(invalid());
                };
                let Some(tail) = a.url.strip_prefix(CORE_RELEASE_URL_PREFIX) else {
                    return Err(invalid());
                };
                let Some((tag, name)) = tail.split_once('/') else {
                    return Err(invalid());
                };
                if !crate::cores::is_release_tag(tag) || name != a.file_name {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}
impl PatchCatalog {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if self.schema != PATCH_CATALOG_SCHEMA_VERSION || self.entries.len() > 4096 {
            return Err(MetadataError::SchemaMismatch("patches"));
        }
        let mut ids = BTreeSet::new();
        for e in &self.entries {
            e.validate()?;
            if !ids.insert(&e.id) {
                return Err(MetadataError::Duplicate(e.id.clone()));
            }
        }
        Ok(())
    }
}
pub fn parse_patch_catalog(bytes: &[u8]) -> Result<PatchCatalog, MetadataError> {
    let c: PatchCatalog = serde_json::from_value(
        crate::parse_unique_json(bytes)
            .map_err(|_| MetadataError::SchemaMismatch("patches.json"))?,
    )
    .map_err(|_| MetadataError::SchemaMismatch("patches.json"))?;
    c.validate()?;
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hosted() -> PatchCatalogEntry {
        PatchCatalogEntry {
            id: "public-test".into(),
            title: BTreeMap::from([
                ("zh-CN".into(), "公开测试".into()),
                ("en".into(), "Public test".into()),
            ]),
            version: "1.0.0".into(),
            vn_ids: vec!["v1".into()],
            release_ids: vec![],
            emulators: vec!["ReMinori".into()],
            kind: PatchKind::Hosted,
            source_url: None,
            license: "MIT".into(),
            authorization_url: Some("https://example.org/license".into()),
            summary: BTreeMap::from([
                ("zh-CN".into(), "测试附件".into()),
                ("en".into(), "Test attachment".into()),
            ]),
            issue_url: None,
            recipe: None,
            artifact: Some(PatchArtifact {
                url: format!("{CORE_RELEASE_URL_PREFIX}patch-public-test-v1.0.0/test.bin"),
                sha256: "a".repeat(64),
                size: 12,
                file_name: "test.bin".into(),
            }),
        }
    }
    #[test]
    fn hosted_requires_authorization_hash_and_trusted_asset() {
        hosted().validate().unwrap();
        for mutate in [
            (|e: &mut PatchCatalogEntry| e.authorization_url = None) as fn(&mut PatchCatalogEntry),
            |e| e.artifact.as_mut().unwrap().sha256 = "wrong".into(),
            |e| e.artifact.as_mut().unwrap().url = "https://evil.example/test.bin".into(),
            |e| e.artifact.as_mut().unwrap().size = MAX_PATCH_ARTIFACT_BYTES + 1,
            |e| e.artifact.as_mut().unwrap().file_name = "../test.bin".into(),
        ] {
            let mut e = hosted();
            mutate(&mut e);
            assert!(e.validate().is_err());
        }
    }
}
