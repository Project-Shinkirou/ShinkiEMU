use crate::{
    cores::{Inventory, slug},
    dependencies, package,
};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{env, fs, io::Read, path::Path};
const API: &str = "https://api.github.com/repos/Project-Shinkirou/ShinkiEMU";
struct GitHub {
    client: reqwest::blocking::Client,
    token: String,
}
impl GitHub {
    fn new() -> Result<Self> {
        let token = env::var("GH_TOKEN")
            .or_else(|_| env::var("GITHUB_TOKEN"))
            .context("--publish requires GH_TOKEN or GITHUB_TOKEN")?;
        if token.trim().is_empty() {
            bail!("empty GitHub credential");
        }
        Ok(Self {
            client: dependencies::client()?,
            token,
        })
    }
    fn request(&self, method: reqwest::Method, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client
            .request(method, url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
    }
    fn get_optional(&self, url: &str) -> Result<Option<Value>> {
        let r = self.request(reqwest::Method::GET, url).send()?;
        if r.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Ok(Some(r.error_for_status()?.json()?))
    }
    fn post(&self, path: &str, value: &Value) -> Result<Value> {
        Ok(self
            .request(reqwest::Method::POST, &format!("{API}/{path}"))
            .json(value)
            .send()?
            .error_for_status()?
            .json()?)
    }
    fn asset_hash(&self, asset: &Value) -> Result<(String, u64)> {
        let id = asset["id"].as_u64().context("asset id")?;
        let mut response = self
            .client
            .get(format!("{API}/releases/assets/{id}"))
            .bearer_auth(&self.token)
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Accept", "application/octet-stream")
            .send()?
            .error_for_status()?;
        use sha2::Digest;
        let mut hash = sha2::Sha256::new();
        let mut size = 0u64;
        let mut bytes = [0u8; 64 * 1024];
        loop {
            let count = response.read(&mut bytes)?;
            if count == 0 {
                break;
            }
            hash.update(&bytes[..count]);
            size = size
                .checked_add(count as u64)
                .context("asset size overflow")?;
        }
        Ok((hex::encode(hash.finalize()), size))
    }
    fn upload(&self, release: &Value, path: &Path) -> Result<Value> {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .context("asset filename")?;
        package::portable(name)?;
        let expected = package::sha256(path)?;
        let size = fs::metadata(path)?.len();
        let id = release["id"].as_u64().context("release id")?;
        let assets = self
            .get_optional(&format!("{API}/releases/{id}/assets?per_page=100"))?
            .context("release asset list")?;
        if let Some(existing) = assets
            .as_array()
            .context("assets")?
            .iter()
            .find(|a| a["name"] == name)
        {
            let (hash, actual_size) = self.asset_hash(existing)?;
            if hash != expected || actual_size != size {
                bail!("published asset {name} differs; refusing replacement");
            }
            return Ok(existing.clone());
        }
        let base = release["upload_url"]
            .as_str()
            .context("upload URL")?
            .split('{')
            .next()
            .unwrap();
        if !base
            .starts_with("https://uploads.github.com/repos/Project-Shinkirou/ShinkiEMU/releases/")
        {
            bail!("unexpected GitHub upload endpoint");
        }
        let asset: Value = self
            .request(reqwest::Method::POST, base)
            .query(&[("name", name)])
            .header("Content-Type", "application/zip")
            .header("Content-Length", size)
            .body(fs::File::open(path)?)
            .send()?
            .error_for_status()?
            .json()?;
        let (hash, remote_size) = self.asset_hash(&asset)?;
        if hash != expected || remote_size != size || asset["size"].as_u64() != Some(size) {
            bail!("published asset hash/size mismatch");
        }
        Ok(asset)
    }
    fn catalog_pr(&self, path: &str, entry: &Value, tag: &str) -> Result<()> {
        let main = self
            .get_optional(&format!("{API}/git/ref/heads/main"))?
            .context("main ref missing")?;
        let main_sha = main["object"]["sha"].as_str().context("main SHA")?;
        let branch = format!("metadata/release-{tag}");
        if self
            .get_optional(&format!("{API}/git/ref/heads/{branch}"))?
            .is_none()
        {
            self.post(
                "git/refs",
                &json!({"ref":format!("refs/heads/{branch}"),"sha":main_sha}),
            )?;
        } else {
            let compare = self
                .get_optional(&format!("{API}/compare/main...{branch}"))?
                .context("catalog branch comparison")?;
            let prefix = format!(
                "Metadata/engines/cores/{}/",
                entry["plugin_id"].as_str().context("catalog plugin")?
            );
            if compare["files"]
                .as_array()
                .context("catalog branch changes")?
                .iter()
                .any(|file| {
                    !file["filename"]
                        .as_str()
                        .is_some_and(|name| name.starts_with(&prefix))
                })
            {
                bail!("catalog branch contains unrelated changes; refusing to modify it");
            }
        }
        let mut content_url = reqwest::Url::parse(&format!("{API}/contents/{path}"))?;
        content_url.query_pairs_mut().append_pair("ref", &branch);
        let existing = self.get_optional(content_url.as_str())?;
        let mut bytes = serde_json::to_vec_pretty(entry)?;
        bytes.push(b'\n');
        let encoded = STANDARD.encode(&bytes);
        let unchanged = existing
            .as_ref()
            .and_then(|old| old["content"].as_str())
            .is_some_and(|s| s.replace(['\n', '\r'], "") == encoded);
        if !unchanged {
            let mut update = json!({"message":format!("metadata: catalog {tag}"),"content":encoded,"branch":branch});
            if let Some(old) = existing {
                update["sha"] = old["sha"].clone();
            }
            self.request(reqwest::Method::PUT, &format!("{API}/contents/{path}"))
                .json(&update)
                .send()?
                .error_for_status()?;
        }
        let mut prs_url = reqwest::Url::parse(&format!("{API}/pulls"))?;
        prs_url
            .query_pairs_mut()
            .append_pair("state", "open")
            .append_pair("head", &format!("Project-Shinkirou:{branch}"));
        let prs = self.get_optional(prs_url.as_str())?.context("PR list")?;
        if prs.as_array().context("PRs")?.is_empty() {
            self.post("pulls",&json!({"title":format!("Catalog {tag}"),"head":branch,"base":"main","body":"Catalog generated from published Release bytes. Review license, platform, hash and size before merging."}))?;
        }
        Ok(())
    }
}
fn published_asset_url(tag: &str, name: &str) -> Result<String> {
    package::portable(name)?;
    let mut url =
        reqwest::Url::parse("https://github.com/Project-Shinkirou/ShinkiEMU/releases/download/")?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("release URL cannot hold path segments"))?
        .pop_if_empty()
        .push(tag)
        .push(name);
    Ok(url.into())
}

/// Keep all byte verification inside the draft transaction. Public URLs are not
/// readable until publication, and upload responses still carry untagged URLs.
fn draft_transaction<T>(
    prepare: impl FnOnce() -> Result<T>,
    publish: impl FnOnce() -> Result<()>,
    cleanup: impl FnOnce() -> Result<()>,
) -> Result<T> {
    let result = prepare().and_then(|value| {
        publish()?;
        Ok(value)
    });
    if result.is_err() {
        cleanup().context("removing incomplete draft Release")?;
    }
    result
}

pub fn run(
    inv: &Inventory,
    target: &str,
    component: &str,
    version: &str,
    publish: bool,
) -> Result<()> {
    inv.target(target)?;
    if component != "manager" {
        inv.core(component)?;
    }
    let core_source = if component == "manager" {
        None
    } else {
        inv.core(component)?.source.as_ref()
    };
    let source_identity = serde_json::to_value(core_source)?;
    semver::Version::parse(version)?;
    let tag = if component == "manager" {
        format!("manager-v{version}")
    } else {
        format!("core-{}-v{version}", slug(component))
    };
    package::portable(&tag)?;
    if let Some(reason) = inv.blocked(component, target)? {
        println!("{component}/{target}: blocked: {reason}; artifact=null");
        return Ok(());
    }
    if component != "manager" && !inv.core(component)?.publish_blocked.is_empty() {
        bail!(
            "{component}: publish_blocked: {}",
            inv.core(component)?.publish_blocked
        );
    }
    let release_root = inv.root.join(".work/release").join(&tag);
    let root = release_root.join(target);
    let record_path = root.join("release.json");
    let commit = String::from_utf8(dependencies::capture(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&inv.root)
            .args(["rev-parse", "HEAD"]),
    )?)?
    .trim()
    .to_owned();
    let dirty = dependencies::capture(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&inv.root)
            .args(["status", "--porcelain", "--untracked-files=all"]),
    )?;
    if !dirty.is_empty() {
        bail!(
            "release requires committed product/fork sources so corresponding-source archives match the binaries"
        );
    }
    if !record_path.exists() {
        let receipt = crate::build::load_receipt(inv, target, component)?;
        let expected_source = core_source
            .map(|source| source.rev.as_str())
            .unwrap_or(&commit);
        if receipt.source_dirty || receipt.source_commit != expected_source {
            bail!(
                "release requires a clean build receipt at the configured source revision; rebuild after committing/pinning sources"
            );
        }
        package::atomic_directory(&root, |work| {
            let staged = work.join("package");
            let package_dir = package::package(inv, target, component, version, &staged)?;
            let platform = if inv.target(target)?.platform == "ios" {
                format!(
                    "ios-{}-{}-test",
                    inv.target(target)?.architecture,
                    inv.target(target)?.variant
                )
            } else {
                format!(
                    "{}-{}",
                    inv.target(target)?.platform,
                    inv.target(target)?.architecture
                )
            };
            let filename = format!(
                "shinki-emu-{}-{}-{version}-{platform}.zip",
                if component == "manager" {
                    "manager"
                } else {
                    "core"
                },
                slug(component)
            );
            let archive = work.join(&filename);
            package::zip_entries(&package::tree(&package_dir)?, &archive)?;
            let source = work.join(format!(
                "shinki-emu-{}-{version}-{platform}-sources.zip",
                slug(component)
            ));
            package::source_bundle(inv, component, target, &source)?;
            let publish_blocked = if target.contains("android") {
                let android: Value =
                    serde_json::from_slice(&fs::read(package_dir.join("android.json"))?)?;
                if android["schema"] != "shinki.emu.android_package.v1"
                    || android["target"] != target
                {
                    bail!("Android signing record identity mismatch");
                }
                if android["signing"] == "release" {
                    ""
                } else {
                    "This bundle contains a debug-signed CI test APK; it is not a Release installer."
                }
            } else {
                ""
            };
            let record = json!({"schema":"shinki.emu.release.v1","component":component,"version":version,"target":target,"platform":platform,"source_commit":commit,
            "adapter_commit":receipt.source_commit,"source_dirty":receipt.source_dirty,"core_source":core_source,
            "archive":{"name":filename,"sha256":package::sha256(&archive)?,"size":fs::metadata(&archive)?.len()},
            "sources":{"name":source.file_name().unwrap().to_string_lossy(),"sha256":package::sha256(&source)?,"size":fs::metadata(&source)?.len()},
            "publish_blocked":publish_blocked});
            fs::write(
                work.join("release.json"),
                serde_json::to_vec_pretty(&record)?,
            )?;
            Ok(())
        })?;
    }
    let requested: Value = serde_json::from_slice(&fs::read(&record_path)?)?;
    validate_record(&requested, &root, component, version)?;
    if requested["core_source"] != source_identity {
        bail!("release record core source differs from the configured pin");
    }
    if requested["source_commit"] != commit {
        bail!("release record does not match the checked-out source commit");
    }
    if !publish {
        println!("{}", record_path.display());
        return Ok(());
    }
    if env::var("GITHUB_REF_TYPE").ok().as_deref() == Some("tag")
        && env::var("GITHUB_REF_NAME").ok().as_deref() != Some(tag.as_str())
    {
        bail!("tag does not match component/version");
    }
    // Aggregate all successful target records before the first remote mutation.
    let mut records = Vec::new();
    let mut blocked = Vec::new();
    for t in &inv.targets {
        if let Some(reason) = inv.blocked(component, &t.triple)? {
            blocked.push(format!("- {}: {reason} (artifact=null)", t.triple));
            continue;
        }
        let directory = release_root.join(&t.triple);
        let path = directory.join("release.json");
        if !path.is_file() {
            blocked.push(format!(
                "- {}: no successful build/package in this release run (artifact=null)",
                t.triple
            ));
            continue;
        }
        let record: Value = serde_json::from_slice(&fs::read(path)?)?;
        validate_record(&record, &directory, component, version)?;
        if record["core_source"] != source_identity {
            bail!("release platform core source differs from the configured pin");
        }
        if record["source_commit"] != commit {
            bail!("release platforms come from different product revisions");
        }
        if record["platform"] != format!("{}-{}", t.platform, t.architecture) {
            bail!("release platform identity mismatch");
        }
        if let Some(reason) = record["publish_blocked"].as_str().filter(|s| !s.is_empty()) {
            blocked.push(format!(
                "- {}: publish_blocked: {reason} (artifact=null)",
                t.triple
            ));
            continue;
        }
        records.push((directory, record));
    }
    if records.is_empty() {
        bail!("zero publishable artifacts; no Release created");
    }
    let artifacts = records
        .iter()
        .map(|(_, r)| {
            format!(
                "- {}: `{}` ({}, {} bytes)",
                r["target"].as_str().unwrap_or(""),
                r["archive"]["name"].as_str().unwrap_or(""),
                r["archive"]["sha256"].as_str().unwrap_or(""),
                r["archive"]["size"]
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let notes = format!(
        "{component} {version}\n\nArtifacts:\n{artifacts}\n\nBlocked platforms:\n{}\n\nSource bundles contain exact product/fork revisions, original vcpkg downloads, ports and patches for relinking. Android debug-signed test APKs remain Actions artifacts.\n",
        blocked.join("\n")
    );
    let gh = GitHub::new()?;
    let existing = gh.get_optional(&format!("{API}/releases/tags/{tag}"))?;
    let created = existing.is_none();
    if existing
        .as_ref()
        .is_some_and(|release| release["draft"] != true)
    {
        bail!("release tag already published; refusing to modify published assets");
    }
    let release = if let Some(existing) = existing {
        existing
    } else {
        gh.post("releases",&json!({"tag_name":tag,"target_commitish":records[0].1["source_commit"],"name":format!("{component} {version}"),"body":notes,"draft":true,"prerelease":!semver::Version::parse(version)?.pre.is_empty()}))?
    };
    let id = release["id"].as_u64().context("release id")?;
    let assets = draft_transaction(
        || -> Result<Vec<(Value, Value)>> {
            let mut assets = Vec::new();
            for (directory, record) in &records {
                gh.upload(
                    &release,
                    &directory.join(
                        record["sources"]["name"]
                            .as_str()
                            .context("source archive")?,
                    ),
                )?;
                let asset = gh.upload(
                    &release,
                    &directory.join(record["archive"]["name"].as_str().context("core archive")?),
                )?;
                assets.push((record.clone(), asset));
            }
            Ok(assets)
        },
        || {
            gh.request(reqwest::Method::PATCH, &format!("{API}/releases/{id}"))
                .json(&json!({"draft":false,"body":notes}))
                .send()?
                .error_for_status()?;
            Ok(())
        },
        || {
            if created || release["draft"] == true {
                gh.request(reqwest::Method::DELETE, &format!("{API}/releases/{id}"))
                    .send()?
                    .error_for_status()?;
            }
            Ok(())
        },
    )?;
    if component != "manager" {
        let c = inv.core(component)?;
        for (record, asset) in assets {
            let url = published_asset_url(&tag, asset["name"].as_str().context("asset name")?)?;
            // upload() reads each source/binary back through the authenticated
            // asset API while the Release is still a draft.
            let hash = record["archive"]["sha256"]
                .as_str()
                .context("archive hash")?;
            let size = record["archive"]["size"].as_u64().context("archive size")?;
            let platform = record["platform"].as_str().context("release platform")?;
            let entry = json!({"plugin_id":component,"display_name":c.display_name,"family_name":c.family_name,"version":version,"abi":"shinki.emu.native.c.v3","platform":platform,"url":url,"sha256":hash,"size":size,"license":c.license,"release_tag":tag});
            let dto: shinki_emu_metadata::CoreCatalogEntry = serde_json::from_value(entry.clone())?;
            dto.validate()?;
            gh.catalog_pr(
                &format!("Metadata/engines/cores/{component}/{platform}/{version}.json"),
                &entry,
                &tag,
            )?;
        }
    }
    Ok(())
}

fn validate_record(record: &Value, root: &Path, component: &str, version: &str) -> Result<()> {
    if record["schema"] != "shinki.emu.release.v1"
        || record["component"] != component
        || record["version"] != version
    {
        bail!("release record identity mismatch");
    }
    if record["target"].as_str() != root.file_name().and_then(|s| s.to_str()) {
        bail!("release record target mismatch");
    }
    let commit = record["source_commit"]
        .as_str()
        .context("release source commit")?;
    if commit.len() != 40 || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("release source revision must be an exact Git commit");
    }
    if record["source_dirty"] != false {
        bail!("release receipt has dirty or incomplete source provenance");
    }
    let expected_adapter = if record["core_source"].is_null() {
        &record["source_commit"]
    } else {
        &record["core_source"]["rev"]
    };
    if &record["adapter_commit"] != expected_adapter {
        bail!("release adapter revision differs from its pinned source");
    }
    for key in ["archive", "sources"] {
        let name = record[key]["name"]
            .as_str()
            .context("release asset filename")?;
        package::portable(name)?;
        if name.contains('/') {
            bail!("release asset must be a filename");
        }
        let path = root.join(name);
        if Some(package::sha256(&path)?.as_str()) != record[key]["sha256"].as_str()
            || Some(fs::metadata(path)?.len()) != record[key]["size"].as_u64()
        {
            bail!("release file hash/size mismatch");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn draft_verification_failure_cleans_up_without_publishing() {
        let published = Cell::new(false);
        let cleaned = Cell::new(false);
        let result = draft_transaction::<()>(
            || bail!("remote asset bytes differ"),
            || {
                published.set(true);
                Ok(())
            },
            || {
                cleaned.set(true);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(!published.get());
        assert!(cleaned.get());
    }

    #[test]
    fn draft_is_published_only_after_verification_and_removed_if_publication_fails() {
        let verified = Cell::new(false);
        let cleaned = Cell::new(false);
        assert!(
            draft_transaction(
                || {
                    verified.set(true);
                    Ok(42)
                },
                || {
                    assert!(verified.get());
                    bail!("publication rejected")
                },
                || {
                    cleaned.set(true);
                    Ok(())
                },
            )
            .is_err()
        );
        assert!(cleaned.get());
        let value = draft_transaction(
            || Ok(42),
            || Ok(()),
            || bail!("successful release must not be deleted"),
        )
        .unwrap();
        assert_eq!(value, 42);
    }

    #[test]
    fn catalog_urls_use_final_tags_and_escape_asset_path_segments() {
        let url = published_asset_url("core-example-v0.1.0-beta.2", "core-example.zip").unwrap();
        assert_eq!(
            url,
            "https://github.com/Project-Shinkirou/ShinkiEMU/releases/download/core-example-v0.1.0-beta.2/core-example.zip"
        );
        assert!(published_asset_url("tag", "../asset.zip").is_err());
    }
}
