use crate::{
    cores::{Inventory, slug},
    dependencies, package,
};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    env, fs,
    path::{Path, PathBuf},
};
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
    /// A local-only Manager release must be tied to a successful exact-head
    /// GitHub CI run; a passed build at another revision is not release proof.
    fn require_manager_ci(&self, commit: &str, targets: &[&str]) -> Result<String> {
        let id = env::var("SHINKI_EMU_RELEASE_CI_RUN_ID")
            .context("set SHINKI_EMU_RELEASE_CI_RUN_ID to the successful exact-head ci.yml run")?;
        let number = id.parse::<u64>()?;
        let run = self
            .get_optional(&format!("{API}/actions/runs/{number}"))?
            .context("CI run not found")?;
        if run["path"] != ".github/workflows/ci.yml"
            || run["head_sha"] != commit
            || run["status"] != "completed"
            || run["conclusion"] != "success"
        {
            bail!("CI run {id} must be completed/successful ci.yml at product commit {commit}");
        }
        let mut jobs = Vec::new();
        for page in 1.. {
            let response = self
                .get_optional(&format!("{API}/actions/runs/{number}/jobs?per_page=100&page={page}"))?
                .context("CI run jobs not found")?;
            let rows = response["jobs"].as_array().context("CI jobs")?;
            jobs.extend(rows.iter().cloned());
            if rows.len() < 100 {
                break;
            }
        }
        for target in targets {
            if !jobs.iter().any(|job| {
                job["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("component (") && name.contains("manager") && name.contains(target))
                    && job["conclusion"] == "success"
            }) {
                bail!("CI run {id} has no successful Manager build for {target}");
            }
        }
        if !jobs.iter().any(|job| job["name"] == "tools" && job["conclusion"] == "success") {
            bail!("CI run {id} has no successful tooling gate");
        }
        Ok(run["html_url"].as_str().context("CI run URL")?.to_owned())
    }
    /// Upload into the draft, replacing a same-name asset an interrupted run left.
    fn upload(&self, release: &Value, path: &Path) -> Result<Value> {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .context("asset filename")?;
        package::portable(name)?;
        let size = fs::metadata(path)?.len();
        let id = release["id"].as_u64().context("release id")?;
        let assets = self
            .get_optional(&format!("{API}/releases/{id}/assets?per_page=100"))?
            .context("release asset list")?;
        for existing in assets
            .as_array()
            .context("assets")?
            .iter()
            .filter(|a| a["name"] == name)
        {
            let asset = existing["id"].as_u64().context("asset id")?;
            self.request(
                reqwest::Method::DELETE,
                &format!("{API}/releases/assets/{asset}"),
            )
            .send()?
            .error_for_status()?;
        }
        let base = release["upload_url"]
            .as_str()
            .context("upload URL")?
            .split('{')
            .next()
            .unwrap();
        let asset: Value = self
            .request(reqwest::Method::POST, base)
            .query(&[("name", name)])
            .header("Content-Type", "application/zip")
            .header("Content-Length", size)
            .body(fs::File::open(path)?)
            .send()?
            .error_for_status()?
            .json()?;
        if asset["size"].as_u64() != Some(size) {
            bail!("GitHub stored {name} with a different size than the local file");
        }
        Ok(asset)
    }
    /// The commit an existing tag names, peeling annotated tags.
    fn tag_commit(&self, tag: &str) -> Result<Option<String>> {
        let Some(reference) = self.get_optional(&format!("{API}/git/ref/tags/{tag}"))? else {
            return Ok(None);
        };
        let mut object = reference["object"].clone();
        while object["type"] == "tag" {
            let sha = object["sha"].as_str().context("tag object SHA")?;
            object = self
                .get_optional(&format!("{API}/git/tags/{sha}"))?
                .context("annotated tag object missing")?["object"]
                .clone();
        }
        if object["type"] != "commit" {
            bail!("tag {tag} does not name a commit");
        }
        Ok(Some(
            object["sha"].as_str().context("tag commit SHA")?.to_owned(),
        ))
    }
    /// Every release carrying `tag`, drafts included: `releases/tags/{tag}` returns
    /// published releases only.
    fn releases_for_tag(&self, tag: &str) -> Result<Vec<Value>> {
        let mut found = Vec::new();
        for page in 1.. {
            let list = self
                .get_optional(&format!("{API}/releases?per_page=100&page={page}"))?
                .context("release list")?;
            let list = list.as_array().context("releases")?;
            found.extend(list.iter().filter(|r| r["tag_name"] == tag).cloned());
            if list.len() < 100 {
                break;
            }
        }
        Ok(found)
    }
    /// Create the release's catalog branch from `main`, or reuse it when it
    /// only touches this core's catalog directory.
    fn catalog_branch(&self, plugin_id: &str, tag: &str) -> Result<String> {
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
            let prefix = format!("Metadata/engines/cores/{plugin_id}/");
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
        Ok(branch)
    }
    fn catalog_put(&self, branch: &str, path: &str, entry: &Value, tag: &str) -> Result<()> {
        let mut content_url = reqwest::Url::parse(&format!("{API}/contents/{path}"))?;
        content_url.query_pairs_mut().append_pair("ref", branch);
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
        Ok(())
    }
    /// Open the catalog PR after every platform entry is on the branch, so a
    /// refused PR never leaves platforms out of the catalog.
    fn catalog_open_pr(&self, branch: &str, tag: &str) -> Result<()> {
        let mut prs_url = reqwest::Url::parse(&format!("{API}/pulls"))?;
        prs_url
            .query_pairs_mut()
            .append_pair("state", "open")
            .append_pair("head", &format!("Project-Shinkirou:{branch}"));
        let prs = self.get_optional(prs_url.as_str())?.context("PR list")?;
        if prs.as_array().context("PRs")?.is_empty() {
            self.post("pulls",&json!({"title":format!("Catalog {tag}"),"head":branch,"base":"main","body":"Catalog generated from the published Release records. Review license, platform, hash and size before merging."}))
                .with_context(|| format!("GitHub refused the catalog PR (Actions may create pull requests only when the organization allows it); every catalog entry is on branch {branch}: open its PR against main manually"))?;
        }
        Ok(())
    }
}

/// Catalog URLs use the final tag; draft upload responses carry `untagged-*` URLs.
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

/// The draft this run may reuse, or `None` to create one. GitHub publishes a
/// release at an existing tag regardless of `target_commitish`, so a tag at another
/// commit, a published release and a draft opened from another commit are refused.
fn release_target(
    releases: &[Value],
    tag_commit: Option<&str>,
    commit: &str,
) -> Result<Option<Value>> {
    if releases.iter().any(|r| r["draft"] != true) {
        bail!("release tag already published; refusing to modify published assets");
    }
    if let Some(tagged) = tag_commit.filter(|tagged| *tagged != commit) {
        bail!(
            "the tag already names commit {tagged}, not this run's commit {commit}; refusing to publish these artifacts under it"
        );
    }
    match releases {
        [] => Ok(None),
        [draft] if draft["target_commitish"] == commit => Ok(Some(draft.clone())),
        [draft] => bail!(
            "the draft release targets {}, not this run's commit {commit}; delete the draft and publish again",
            draft["target_commitish"]
        ),
        _ => bail!("several draft releases carry this tag; delete the extras and publish again"),
    }
}

/// The product revision and core pin every target of one Release must share.
struct Identity {
    tag: String,
    commit: String,
    core_source: Value,
    root: PathBuf,
}

fn identity(inv: &Inventory, component: &str, version: &str) -> Result<Identity> {
    semver::Version::parse(version)?;
    let (tag, core_source) = if component == "manager" {
        (format!("manager-v{version}"), Value::Null)
    } else {
        let core = inv.core(component)?;
        (
            format!("core-{}-v{version}", slug(component)),
            serde_json::to_value(&core.source)?,
        )
    };
    package::portable(&tag)?;
    let commit = String::from_utf8(dependencies::capture(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&inv.root)
            .args(["rev-parse", "HEAD"]),
    )?)?
    .trim()
    .to_owned();
    Ok(Identity {
        root: inv.root.join(".work/release").join(&tag),
        tag,
        commit,
        core_source,
    })
}

/// Records from another product revision or core pin cannot join this Release.
fn require_same_release(
    record: &Value,
    id: &Identity,
    component: &str,
    version: &str,
    path: &Path,
) -> Result<()> {
    if record["schema"] != "shinki.emu.release.v1"
        || record["component"] != component
        || record["version"] != version
        || record["source_commit"] != id.commit.as_str()
        || record["core_source"] != id.core_source
    {
        bail!(
            "{} was prepared from another component, version, product commit or core pin; remove it and prepare again",
            path.display()
        );
    }
    Ok(())
}

/// Package one target's binary and corresponding source, and write the record
/// `publish` aggregates. Blocked and publish-blocked targets produce nothing.
pub fn prepare(inv: &Inventory, target: &str, component: &str, version: &str) -> Result<()> {
    let spec = inv.target(target)?;
    let id = identity(inv, component, version)?;
    if let Some(reason) = inv.blocked(component, target)? {
        println!("{component}/{target}: blocked: {reason}; artifact=null");
        return Ok(());
    }
    if component != "manager" {
        let restriction = &inv.core(component)?.publish_blocked;
        if !restriction.is_empty() {
            println!("{component}: publish_blocked: {restriction}; artifact=null");
            return Ok(());
        }
    }
    let dirty = dependencies::capture(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&inv.root)
            .args(["status", "--porcelain", "--untracked-files=all"]),
    )?;
    if !dirty.is_empty() {
        bail!(
            "release requires committed product sources so corresponding-source archives match the binaries"
        );
    }
    let root = id.root.join(target);
    let record_path = root.join("release.json");
    if record_path.exists() {
        let record: Value = serde_json::from_slice(&fs::read(&record_path)?)?;
        require_same_release(&record, &id, component, version, &record_path)?;
        println!("{}", record_path.display());
        return Ok(());
    }
    let receipt = crate::build::load_receipt(inv, target, component)?;
    let expected_source = id.core_source["rev"].as_str().unwrap_or(&id.commit);
    if receipt.source_dirty || receipt.source_commit != expected_source {
        bail!(
            "release requires a clean build receipt at the configured source revision; rebuild after committing/pinning sources"
        );
    }
    let platform = if component == "manager" && target.contains("android") {
        "android-universal".to_owned()
    } else if spec.platform == "ios" {
        format!("ios-{}-{}-test", spec.architecture, spec.variant)
    } else if component == "manager" && spec.platform == "ohos" {
        format!("ohos-{}-debug-test-signed", spec.architecture)
    } else {
        format!("{}-{}", spec.platform, spec.architecture)
    };
    let kind = if component == "manager" {
        "manager"
    } else {
        "core"
    };
    let extension =
        if component == "manager" && (target.contains("android") || target.ends_with("ohos")) {
            if target.ends_with("ohos") {
                "hap"
            } else {
                "apk"
            }
        } else {
            "zip"
        };
    let filename = format!(
        "shinki-emu-{kind}-{}-{version}-{platform}.{extension}",
        slug(component)
    );
    let sources_platform = if component == "manager" && target.contains("android") {
        format!("android-{}", spec.architecture)
    } else {
        platform.clone()
    };
    let sources = format!(
        "shinki-emu-{}-{version}-{sources_platform}-sources.zip",
        slug(component)
    );
    package::atomic_directory(&root, |work| {
        let staged = work.join("package");
        let package_dir = package::package(inv, target, component, version, &staged)?;
        let publish_blocked = if component == "manager" && target.contains("android") {
            let android: Value =
                serde_json::from_slice(&fs::read(package_dir.join("android.json"))?)?;
            if android["signing"] != "release" {
                "This bundle contains a debug-signed CI test APK; it is not a Release installer."
            } else if target == "x86_64-linux-android" {
                "The universal Android APK is published once from the aarch64 build row."
            } else {
                ""
            }
        } else {
            ""
        };
        let archive = work.join(&filename);
        if component == "manager" && (target.contains("android") || target.ends_with("ohos")) {
            let (metadata, key) = if target.ends_with("ohos") {
                ("ohos.json", "hap")
            } else {
                ("android.json", "apk")
            };
            let package: Value = serde_json::from_slice(&fs::read(package_dir.join(metadata))?)?;
            let name = package[key].as_str().context("mobile package filename")?;
            fs::copy(package_dir.join(name), &archive)?;
        } else {
            package::zip_entries(&package::tree(&package_dir)?, &archive)?;
        }
        fs::remove_dir_all(&staged)?;
        crate::sources::bundle(inv, component, target, &work.join(&sources))?;
        let record = json!({"schema":"shinki.emu.release.v1","component":component,"version":version,"target":target,"platform":platform,
            "source_commit":id.commit,"core_source":id.core_source,
            "archive":{"name":filename,"sha256":package::sha256(&archive)?,"size":fs::metadata(&archive)?.len()},
            "sources":{"name":sources,"sha256":package::sha256(&work.join(&sources))?},"publish_blocked":publish_blocked});
        fs::write(
            work.join("release.json"),
            serde_json::to_vec_pretty(&record)?,
        )?;
        Ok(())
    })?;
    println!("{}", record_path.display());
    Ok(())
}

/// Publish every prepared target of `component` as one GitHub Release, then
/// propose the core catalog entries.
pub fn publish(inv: &Inventory, component: &str, version: &str) -> Result<()> {
    let id = identity(inv, component, version)?;
    let tag = id.tag.as_str();
    if env::var("GITHUB_EVENT_NAME").ok().as_deref() == Some("push")
        && env::var("GITHUB_REF_TYPE").ok().as_deref() == Some("tag")
        && env::var("GITHUB_REF_NAME").ok().as_deref() != Some(tag)
    {
        bail!("tag does not match component/version");
    }
    let local_manager = component == "manager"
        && inv
            .targets
            .iter()
            .any(|t| t.manager_local_build && t.manager_supported);
    let mut records = Vec::new();
    let mut blocked = Vec::new();
    for t in &inv.targets {
        if let Some(reason) = inv.blocked(component, &t.triple)? {
            blocked.push(format!("- {}: {reason} (artifact=null)", t.triple));
            continue;
        }
        let directory = id.root.join(&t.triple);
        let path = directory.join("release.json");
        if !path.is_file() {
            if local_manager && !(component == "manager" && t.triple == "x86_64-linux-android") {
                bail!("Manager release requires a prepared exact-head artifact for {}", t.triple);
            }
            blocked.push(format!(
                "- {}: no successful build/package in this release run (artifact=null)",
                t.triple
            ));
            continue;
        }
        let record: Value = serde_json::from_slice(&fs::read(&path)?)?;
        require_same_release(&record, &id, component, version, &path)?;
        for key in ["archive", "sources"] {
            let name = record[key]["name"].as_str().context("release asset name")?;
            let expected = record[key]["sha256"].as_str().context("release asset SHA256")?;
            let actual = package::sha256(&directory.join(name))?;
            if actual != expected {
                bail!("{name}: release asset hash differs from prepared record");
            }
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
    let gh = GitHub::new()?;
    let ci = if local_manager {
        let targets = inv
            .targets
            .iter()
            .filter(|t| t.manager_supported && t.runner != "local-only" && t.triple != "x86_64-linux-android")
            .map(|t| t.triple.as_str())
            .collect::<Vec<_>>();
        let url = gh.require_manager_ci(&id.commit, &targets)?;
        format!("\nExact-head GitHub CI: {url}.\n")
    } else {
        String::new()
    };
    let notes = format!(
        "{component} {version}\n\nArtifacts:\n{artifacts}\n\nBlocked platforms:\n{}\n\nEach `-sources.zip` holds the exact product/fork revisions, the copyleft Rust crate sources, and the native library source archives and recipes needed to modify and relink. The universal Android Release APK is uploaded once; matching Actions matrix rows do not duplicate it.{ci}",
        blocked.join("\n")
    );
    let notes = if local_manager {
        format!("{notes}\nThe OHOS HAP is experimental, signed with the OpenHarmony SDK test PKI. Its signature and native dependency closure were verified by the local DevEco packager. Device/emulator launch and retail installability were not verified.\n")
    } else {
        notes
    };
    let existing = release_target(
        &gh.releases_for_tag(tag)?,
        gh.tag_commit(tag)?.as_deref(),
        &id.commit,
    )?;
    // A new draft records the run's commit; publishing it creates the tag there.
    let release = match existing {
        Some(draft) => draft,
        None => gh.post(
            "releases",
            &json!({"tag_name":tag,"target_commitish":id.commit,"name":format!("{component} {version}"),"body":notes,"draft":true,"prerelease":!semver::Version::parse(version)?.pre.is_empty()}),
        )?,
    };
    let release_id = release["id"].as_u64().context("release id")?;
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
        assets.push((record, asset));
    }
    gh.request(
        reqwest::Method::PATCH,
        &format!("{API}/releases/{release_id}"),
    )
    .json(&json!({"draft":false,"body":notes}))
    .send()?
    .error_for_status()?;
    if component != "manager" {
        let c = inv.core(component)?;
        let branch = gh.catalog_branch(component, tag)?;
        for (record, asset) in assets {
            let url = published_asset_url(tag, asset["name"].as_str().context("asset name")?)?;
            let hash = record["archive"]["sha256"]
                .as_str()
                .context("archive hash")?;
            let size = record["archive"]["size"].as_u64().context("archive size")?;
            let platform = record["platform"].as_str().context("release platform")?;
            let entry = json!({"plugin_id":component,"display_name":c.display_name,"family_name":c.family_name,"version":version,"abi":"shinki.emu.native.c.v3","platform":platform,"url":url,"sha256":hash,"size":size,"license":c.license,"release_tag":tag});
            let dto: shinki_emu_metadata::CoreCatalogEntry = serde_json::from_value(entry.clone())?;
            dto.validate()?;
            gh.catalog_put(
                &branch,
                &format!("Metadata/engines/cores/{component}/{platform}/{version}.json"),
                &entry,
                tag,
            )?;
        }
        gh.catalog_open_pr(&branch, tag)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_urls_use_final_tags_and_escape_asset_path_segments() {
        let url = published_asset_url("core-example-v0.1.0-beta.2", "core-example.zip").unwrap();
        assert_eq!(
            url,
            "https://github.com/Project-Shinkirou/ShinkiEMU/releases/download/core-example-v0.1.0-beta.2/core-example.zip"
        );
        assert!(published_asset_url("tag", "../asset.zip").is_err());
    }

    #[test]
    fn publishing_reuses_only_a_draft_at_the_run_commit() {
        let commit = "1".repeat(40);
        let other = "2".repeat(40);
        let draft =
            |at: &str| json!({"tag_name":"manager-v1.0.0","draft":true,"target_commitish":at});
        // No release or tag yet: create a draft that GitHub tags at the commit on publish.
        assert!(release_target(&[], None, &commit).unwrap().is_none());
        // A tag pushed for this commit, or a dispatch rerun of an interrupted publish.
        assert!(
            release_target(&[], Some(&commit), &commit)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            release_target(&[draft(&commit)], None, &commit).unwrap(),
            Some(draft(&commit))
        );
        // GitHub would keep an existing tag at its commit and ignore target_commitish.
        assert!(release_target(&[], Some(&other), &commit).is_err());
        assert!(release_target(&[draft(&other)], None, &commit).is_err());
        assert!(release_target(&[draft(&commit), draft(&commit)], None, &commit).is_err());
        let published =
            json!({"tag_name":"manager-v1.0.0","draft":false,"target_commitish":commit});
        assert!(release_target(&[published], Some(&commit), &commit).is_err());
    }
}
