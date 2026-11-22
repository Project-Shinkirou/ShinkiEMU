use crate::{
    cores::{Inventory, Target, slug},
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
const ANDROID_PRIMARY: &str = "aarch64-linux-android";
const ANDROID_SECONDARY: &str = "x86_64-linux-android";
const ANDROID_RELEASE_CERT: &str =
    "c415bd462d26574758ed455055ff528a3bd6036e39d5ac6887308f8cf69e97fc";
const LAST_PUBLIC_ANDROID_VERSION_CODE: u32 = 86;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, clap::ValueEnum)]
pub enum Scope {
    #[default]
    All,
    WindowsAndroid,
}

impl Scope {
    fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::WindowsAndroid => "windows-android",
        }
    }

    pub fn includes(self, platform: &str) -> bool {
        self == Self::All || matches!(platform, "windows" | "android")
    }

    fn local_manager(self, inv: &Inventory, component: &str) -> bool {
        component == "manager"
            && inv.targets.iter().any(|target| {
                self.includes(&target.platform)
                    && target.manager_local_build
                    && target.manager_supported
            })
    }
}

fn release_targets<'a>(
    inv: &'a Inventory,
    component: &str,
    scope: Scope,
) -> Result<Vec<&'a Target>> {
    let mut targets = Vec::new();
    for target in &inv.targets {
        if scope.includes(&target.platform)
            && inv.blocked(component, &target.triple)?.is_none()
            && (component != "manager" || target.triple != ANDROID_SECONDARY)
        {
            targets.push(target);
        }
    }
    if targets.is_empty() {
        bail!(
            "{component} has no release targets in scope {}",
            scope.name()
        );
    }
    Ok(targets)
}

fn require_scoped_ci_jobs(inv: &Inventory, jobs: &[Value]) -> Result<()> {
    let scope = Scope::WindowsAndroid;
    let passed = |name: &str| {
        jobs.iter()
            .any(|job| job["name"] == name && job["conclusion"] == "success")
    };
    if !passed("release-scope (windows-android)") {
        bail!("CI run has no successful windows-android release scope receipt");
    }
    for row in inv.job_matrix(true, None)?.include {
        if scope.includes(&row.platform) {
            let prefix = format!("component ({}, {},", row.target, row.component);
            if !jobs.iter().any(|job| {
                job["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with(&prefix))
                    && job["conclusion"] == "success"
            }) {
                bail!(
                    "CI run has no successful {} build for {}",
                    row.component,
                    row.target
                );
            }
        }
    }
    Ok(())
}

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
    /// Only a successful exact-head run with the applicable release matrix is proof.
    fn require_ci(&self, inv: &Inventory, commit: &str, scope: Scope) -> Result<String> {
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
                .get_optional(&format!(
                    "{API}/actions/runs/{number}/jobs?per_page=100&page={page}"
                ))?
                .context("CI run jobs not found")?;
            let rows = response["jobs"].as_array().context("CI jobs")?;
            jobs.extend(rows.iter().cloned());
            if rows.len() < 100 {
                break;
            }
        }
        if scope == Scope::WindowsAndroid {
            require_scoped_ci_jobs(inv, &jobs).with_context(|| format!("CI run {id}"))?;
        } else {
            for target in release_targets(inv, "manager", scope)?
                .into_iter()
                .filter(|target| target.runner != "local-only")
            {
                if !jobs.iter().any(|job| {
                    job["name"].as_str().is_some_and(|name| {
                        name.starts_with("component (")
                            && name.contains("manager")
                            && name.contains(&target.triple)
                    }) && job["conclusion"] == "success"
                }) {
                    bail!(
                        "CI run {id} has no successful Manager build for {}",
                        target.triple
                    );
                }
            }
        }
        if !jobs
            .iter()
            .any(|job| job["name"] == "tools" && job["conclusion"] == "success")
        {
            bail!("CI run {id} has no successful tooling gate");
        }
        Ok(run["html_url"].as_str().context("CI run URL")?.to_owned())
    }
    fn require_ohos_handoff_ci(&self, commit: &str) -> Result<String> {
        let id = env::var("SHINKI_EMU_RELEASE_HANDOFF_RUN_ID").context(
            "set SHINKI_EMU_RELEASE_HANDOFF_RUN_ID to the exact-head release.yml handoff run",
        )?;
        let number = id.parse::<u64>()?;
        let run = self
            .get_optional(&format!("{API}/actions/runs/{number}"))?
            .context("release handoff run not found")?;
        if run["path"] != ".github/workflows/release.yml"
            || run["head_sha"] != commit
            || run["status"] != "completed"
            || run["conclusion"] != "success"
        {
            bail!("handoff run {id} must be completed/successful release.yml at {commit}");
        }
        let response = self
            .get_optional(&format!("{API}/actions/runs/{number}/jobs?per_page=100"))?
            .context("release handoff jobs not found")?;
        if !response["jobs"]
            .as_array()
            .context("handoff jobs")?
            .iter()
            .any(|job| job["name"] == "verify-ohos-handoff" && job["conclusion"] == "success")
        {
            bail!("release run {id} did not verify the staged OHOS artifact");
        }
        Ok(run["html_url"]
            .as_str()
            .context("handoff run URL")?
            .to_owned())
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
    version: String,
    identity: Option<shinki_emu_metadata::CoreIdentity>,
}

const RELEASE_PRODUCT_VERSION: &str = "SHINKI_EMU_RELEASE_PRODUCT_VERSION";
const RELEASE_SOURCE_COMMIT: &str = "SHINKI_EMU_RELEASE_SOURCE_COMMIT";

fn batch_input(name: &str) -> Result<Option<String>> {
    env::var_os(name)
        .map(|value| {
            value
                .into_string()
                .map_err(|_| anyhow::anyhow!("{name} is not UTF-8"))
        })
        .transpose()
}

pub(crate) fn check_product_version(version: &str) -> Result<()> {
    if let Some(expected) = batch_input(RELEASE_PRODUCT_VERSION)?
        && version != expected
    {
        bail!("product version {version} differs from frozen release version {expected}");
    }
    Ok(())
}

pub(crate) fn check_product_source(commit: Option<&str>) -> Result<()> {
    if let Some(expected) = batch_input(RELEASE_SOURCE_COMMIT)?
        && (!crate::build::valid_source_commit(&expected) || commit != Some(expected.as_str()))
    {
        bail!("expected a clean checkout at frozen product commit {expected}");
    }
    Ok(())
}

fn require_product_batch(id: &Identity, component: &str) -> Result<()> {
    if component != "manager" && id.identity.is_none() {
        if batch_input(RELEASE_PRODUCT_VERSION)?.is_none()
            || batch_input(RELEASE_SOURCE_COMMIT)?.is_none()
        {
            bail!("self-owned core releases require the frozen Manager batch inputs {RELEASE_PRODUCT_VERSION} and {RELEASE_SOURCE_COMMIT}");
        }
        check_product_version(&id.version)?;
        check_product_source(Some(&id.commit))?;
    }
    Ok(())
}

pub fn resolve_version(inv: &Inventory, component: &str, supplied: &str) -> Result<String> {
    if component != "manager"
        && let Some(source) = &inv.core(component)?.source
    {
        let identity = source.identity();
        let version = identity.version();
        if !supplied.is_empty() && supplied != version {
            bail!(
                "external core version is derived from its verified upstream/adapter pins; omit --version or use {version}"
            );
        }
        return Ok(version);
    }
    semver::Version::parse(supplied)
        .context("Manager and self-owned cores require the same explicit product --version (SemVer)")?;
    check_product_version(supplied)?;
    Ok(supplied.to_owned())
}

pub fn dry_run(inv: &Inventory, component: &str, version: &str, scope: Scope) -> Result<()> {
    let targets = release_targets(inv, component, scope)?;
    if component != "manager" {
        crate::sources::verify_provenance(inv, inv.core(component)?)?;
    }
    let id = identity(inv, component, version)?;
    println!(
        "{}",
        serde_json::to_string(&json!({
            "component":component, "version":id.version, "release_tag":id.tag,
            "source_commit":id.commit, "core_source":id.core_source, "identity":id.identity,
            "release_scope":scope.name(), "local_manager":scope.local_manager(inv, component),
            "targets":targets.iter().map(|target| target.triple.as_str()).collect::<Vec<_>>(),
        }))?
    );
    Ok(())
}

fn identity(inv: &Inventory, component: &str, version: &str) -> Result<Identity> {
    let version = resolve_version(inv, component, version)?;
    let external = if component == "manager" {
        None
    } else {
        inv.core(component)?
            .source
            .as_ref()
            .map(crate::cores::CoreSource::identity)
    };
    if let Some(identity) = &external {
        crate::metadata::require_release_sequence(&inv.root.join("Metadata"), component, identity)?;
    }
    let (tag, core_source) = if component == "manager" {
        (format!("manager-v{version}"), Value::Null)
    } else {
        let core = inv.core(component)?;
        (
            if external.is_some() {
                format!("core-{}-{version}", slug(component))
            } else {
                format!("core-{}-v{version}", slug(component))
            },
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
    check_product_source(Some(&commit))?;
    Ok(Identity {
        root: inv.root.join(".work/release").join(&tag),
        tag,
        commit,
        core_source,
        version,
        identity: external,
    })
}

/// Records from another product revision or core pin cannot join this Release.
fn require_same_release(
    record: &Value,
    id: &Identity,
    component: &str,
    target: &str,
    path: &Path,
    scope: Scope,
) -> Result<()> {
    if record["schema"] != "shinki.emu.release.v1"
        || record["target"] != target
        || !(record["release_scope"] == scope.name()
            || (scope == Scope::All && record.get("release_scope").is_none()))
        || record["component"] != component
        || record["version"] != id.version
        || (id.identity.is_none() && record["product_version"] != id.version)
        || record["source_commit"] != id.commit.as_str()
        || record["core_source"] != id.core_source
        || record.get("identity").unwrap_or(&Value::Null) != &serde_json::to_value(&id.identity)?
    {
        bail!(
            "{} does not match the release component, version, source, target or scope {}; prepare it again",
            path.display(),
            scope.name()
        );
    }
    Ok(())
}

/// Package one target's binary and corresponding source, and write the record
/// `publish` aggregates. Blocked and publish-blocked targets produce nothing.
pub fn prepare(
    inv: &Inventory,
    target: &str,
    component: &str,
    version: &str,
    scope: Scope,
) -> Result<()> {
    let spec = inv.target(target)?;
    if !scope.includes(&spec.platform)
        || (scope == Scope::WindowsAndroid && component == "manager" && target == ANDROID_SECONDARY)
    {
        bail!(
            "{component}/{target} is not a release row in scope {}",
            scope.name()
        );
    }
    let id = identity(inv, component, version)?;
    let version = id.version.as_str();
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
    require_product_batch(&id, component)?;
    if component != "manager" {
        crate::sources::verify_provenance(inv, inv.core(component)?)?;
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
        require_same_release(&record, &id, component, target, &record_path, scope)?;
        println!("{}", record_path.display());
        return Ok(());
    }
    let receipt = crate::build::load_receipt(inv, target, component)?;
    if id.identity.is_none() {
        receipt.require_product_version(version)?;
        if crate::apple::ios_frameworks::enabled(target, &receipt.features) {
            bail!("the iOS Framework Manager is for private QA; rebuild without ios-framework-cores for release");
        }
        if crate::ohos::manager_mode(target, &receipt.features)? {
            bail!("the bundled Musica OHOS HAP is for local device acceptance only");
        }
    }
    let expected_source = id.core_source["rev"].as_str().unwrap_or(&id.commit);
    receipt.require_product_source(expected_source)?;
    let product_commit = if id.identity.is_none() {
        receipt.source_commit.as_str()
    } else {
        id.commit.as_str()
    };
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
        let mut android_targets = Value::Null;
        let publish_blocked = if component == "manager" && target.contains("android") {
            let android: Value =
                serde_json::from_slice(&fs::read(package_dir.join("android.json"))?)?;
            android_targets = android["targets"].clone();
            if android["signing"] != "release" {
                "This bundle contains a debug-signed CI test APK; it is not a Release installer."
            } else if target == ANDROID_SECONDARY {
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
        crate::sources::bundle(
            inv,
            component,
            target,
            product_commit,
            receipt.product_version.as_deref(),
            &work.join(&sources),
        )?;
        let mut android_x86_64_sources = Value::Null;
        if scope == Scope::WindowsAndroid && component == "manager" && target == ANDROID_PRIMARY {
            let name = format!("shinki-emu-manager-{version}-android-x86_64-sources.zip");
            let path = work.join(&name);
            crate::sources::bundle(
                inv,
                component,
                ANDROID_SECONDARY,
                product_commit,
                receipt.product_version.as_deref(),
                &path,
            )?;
            android_x86_64_sources = json!({"name":name,"sha256":package::sha256(&path)?});
            if publish_blocked.is_empty() {
                verify_android_release(&archive, version)?;
            }
        }
        let record = json!({"schema":"shinki.emu.release.v1","component":component,"version":version,"target":target,"platform":platform,
            "source_commit":id.commit,"core_source":id.core_source,
            "release_scope":scope.name(),
            "android_targets":android_targets,"android_x86_64_sources":android_x86_64_sources,
            "identity":id.identity,
            "product_version":receipt.product_version,
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

/// Hosted integrity/native/source gate for the exact locally signed HAP.
/// Signature verification itself is performed by the DevEco packager.
pub fn verify_ohos_handoff(inv: &Inventory, component: &str, version: &str) -> Result<()> {
    if component != "manager" {
        bail!("the local OHOS handoff is only defined for manager");
    }
    let target = "aarch64-unknown-linux-ohos";
    let id = identity(inv, component, version)?;
    let directory = id.root.join(target);
    let record_path = directory.join("release.json");
    let record: Value = serde_json::from_slice(&fs::read(&record_path)?)?;
    require_same_release(&record, &id, component, target, &record_path, Scope::All)?;
    for key in ["archive", "sources"] {
        let name = record[key]["name"].as_str().context("handoff asset name")?;
        package::portable(name)?;
        if package::sha256(&directory.join(name))?
            != record[key]["sha256"].as_str().context("handoff SHA256")?
        {
            bail!("{name}: handoff SHA256 mismatch");
        }
    }
    let hap = directory.join(record["archive"]["name"].as_str().unwrap());
    let extracted = crate::temporary::directory()?;
    let mut archive = zip::ZipArchive::new(fs::File::open(&hap)?)?;
    for required in [
        "module.json",
        "ets/modules.abc",
        "resources/rawfile/bundled/bundled.manifest",
        "resources/rawfile/bundled/metadata/index.json",
    ] {
        archive
            .by_name(required)
            .with_context(|| format!("HAP lacks {required}"))?;
    }
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name()?.into_owned();
        if name.starts_with("libs/") && name.ends_with(".so") {
            package::portable(&name)?;
            let path = extracted.path().join(&name);
            fs::create_dir_all(path.parent().unwrap())?;
            std::io::copy(&mut entry, &mut fs::File::create(path)?)?;
        }
    }
    let libs = extracted.path().join("libs/arm64-v8a");
    if !libs.join("libshinki_emu_manager.so").is_file() {
        bail!("HAP lacks the arm64 Manager library");
    }
    for (path, _) in package::tree(&libs)? {
        crate::binary::closure(&path, std::slice::from_ref(&libs), target)?;
    }
    let sources = directory.join(record["sources"]["name"].as_str().unwrap());
    let mut archive = zip::ZipArchive::new(fs::File::open(sources)?)?;
    archive.by_name("BUILD-FROM-SOURCE.md")?;
    let source = archive.by_name("source.tar")?;
    let mut tar = tar::Archive::new(source);
    let mut references = None;
    for entry in tar.entries()? {
        let mut entry = entry?;
        if entry.path()?.as_ref() == Path::new("./source-references.json")
            || entry.path()?.as_ref() == Path::new("source-references.json")
        {
            references = Some(serde_json::from_reader::<_, Value>(&mut entry)?);
        }
    }
    let references = references.context("source archive lacks source-references.json")?;
    if references["product"]["rev"] != id.commit || references["target"] != target {
        bail!("corresponding source does not match the OHOS build revision/target");
    }
    println!(
        "OHOS handoff verified: source={}, HAP SHA256={}",
        id.commit,
        package::sha256(&hap)?
    );
    Ok(())
}

fn android_tool(name: &str) -> Result<std::process::Command> {
    let sdk = env::var_os("ANDROID_HOME")
        .or_else(|| env::var_os("ANDROID_SDK_ROOT"))
        .context("set ANDROID_HOME to verify the production APK")?;
    let tools = PathBuf::from(sdk).join("build-tools/37.0.0");
    if cfg!(windows) && name == "apksigner" {
        let mut command = std::process::Command::new("cmd");
        command.arg("/c").arg(tools.join("apksigner.bat"));
        Ok(command)
    } else {
        let name = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_owned()
        };
        Ok(std::process::Command::new(tools.join(name)))
    }
}

fn badging_attribute<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace().find_map(|field| {
        let (name, value) = field.split_once('=')?;
        (name == key).then_some(value.trim_matches('\''))
    })
}

fn require_android_upgrade(signing: &str, badging: &str, version: &str) -> Result<()> {
    let mut digests = signing
        .lines()
        .filter(|line| line.starts_with("Signer #"))
        .filter_map(|line| line.split_once(" certificate SHA-256 digest: "))
        .map(|(_, digest)| digest.trim());
    if digests.next() != Some(ANDROID_RELEASE_CERT) || digests.next().is_some() {
        bail!("Android APK must use the existing org.shinkirou.shinkiemu production certificate");
    }
    let package = badging
        .lines()
        .find(|line| line.starts_with("package: "))
        .context("Android APK package metadata missing")?;
    let code: u32 = badging_attribute(package, "versionCode")
        .context("Android APK versionCode missing")?
        .parse()?;
    if badging_attribute(package, "name") != Some("org.shinkirou.shinkiemu")
        || badging_attribute(package, "versionName") != Some(version)
        || code <= LAST_PUBLIC_ANDROID_VERSION_CODE
        || !badging.lines().any(|line| line.trim() == "sdkVersion:'28'")
    {
        bail!(
            "Android APK must use org.shinkirou.shinkiemu, versionName {version}, versionCode above {LAST_PUBLIC_ANDROID_VERSION_CODE}, and minSdk 28"
        );
    }
    Ok(())
}

fn verify_android_release(apk: &Path, version: &str) -> Result<()> {
    let signing = dependencies::capture(
        android_tool("apksigner")?
            .args(["verify", "--print-certs"])
            .arg(apk),
    )?;
    let badging = dependencies::capture(android_tool("aapt2")?.args(["dump", "badging"]).arg(apk))?;
    require_android_upgrade(
        std::str::from_utf8(&signing)?,
        std::str::from_utf8(&badging)?,
        version,
    )
}

const EXTRA_ANDROID_SOURCES: &str = "android_x86_64_sources";

fn asset_name<'a>(record: &'a Value, key: &str) -> Result<&'a str> {
    let name = record[key]["name"].as_str().context("release asset name")?;
    package::portable(name)?;
    if name.contains('/') {
        bail!("{name}: release asset name must be a filename");
    }
    Ok(name)
}

fn verify_asset(directory: &Path, record: &Value, key: &str) -> Result<()> {
    let name = asset_name(record, key)?;
    let expected = record[key]["sha256"]
        .as_str()
        .context("release asset SHA256")?;
    if package::sha256(&directory.join(name))? != expected {
        bail!("{name}: release asset hash differs from prepared record");
    }
    Ok(())
}

struct PreparedRelease {
    records: Vec<(PathBuf, Value)>,
    blocked: Vec<String>,
}

fn collect_records(
    inv: &Inventory,
    id: &Identity,
    component: &str,
    scope: Scope,
) -> Result<PreparedRelease> {
    let local_manager = scope.local_manager(inv, component);
    let strict = scope == Scope::WindowsAndroid;
    let mut records = Vec::new();
    let mut blocked = Vec::new();
    for t in &inv.targets {
        let secondary = component == "manager" && t.triple == ANDROID_SECONDARY;
        if !scope.includes(&t.platform) || (strict && secondary) {
            continue;
        }
        if let Some(reason) = inv.blocked(component, &t.triple)? {
            blocked.push(format!("- {}: {reason} (artifact=null)", t.triple));
            continue;
        }
        let directory = id.root.join(&t.triple);
        let path = directory.join("release.json");
        if !path.is_file() {
            if strict || (local_manager && !secondary) {
                bail!(
                    "{component} release requires a prepared exact-head artifact for {} in scope {}",
                    t.triple,
                    scope.name()
                );
            }
            blocked.push(format!(
                "- {}: no successful build/package in this release run (artifact=null)",
                t.triple
            ));
            continue;
        }
        let record: Value = serde_json::from_slice(&fs::read(&path)?)?;
        require_same_release(&record, id, component, &t.triple, &path, scope)?;
        for key in ["archive", "sources"] {
            verify_asset(&directory, &record, key)?;
        }
        if let Some(reason) = record["publish_blocked"].as_str().filter(|s| !s.is_empty()) {
            if strict {
                bail!("{component}/{} cannot be published: {reason}", t.triple);
            }
            blocked.push(format!(
                "- {}: publish_blocked: {reason} (artifact=null)",
                t.triple
            ));
            continue;
        }
        if strict && component == "manager" && t.triple == ANDROID_PRIMARY {
            if record["android_targets"] != json!([ANDROID_PRIMARY, ANDROID_SECONDARY]) {
                bail!("the universal Android APK requires receipts for both Android ABI builds");
            }
            verify_asset(&directory, &record, EXTRA_ANDROID_SOURCES)
                .context("the universal Android APK requires x86_64 corresponding source")?;
            verify_android_release(
                &directory.join(asset_name(&record, "archive")?),
                &id.version,
            )?;
        }
        records.push((directory, record));
    }
    if records.is_empty() {
        bail!("zero publishable artifacts; no Release created");
    }
    Ok(PreparedRelease { records, blocked })
}

/// Self-owned cores may publish ahead of Manager, but need the matching
/// verified Manager build from the same frozen product release.
fn require_manager_batch(
    inv: &Inventory,
    id: &Identity,
    records: &[(PathBuf, Value)],
    scope: Scope,
) -> Result<()> {
    let manager = identity(inv, "manager", &id.version)?;
    let mut verified = std::collections::BTreeSet::new();
    for (_, record) in records {
        let target = record["target"].as_str().context("core release target")?;
        // The primary row owns the universal Manager APK in either scope.
        let target = if target == ANDROID_SECONDARY {
            ANDROID_PRIMARY
        } else {
            target
        };
        if !verified.insert(target) {
            continue;
        }
        let directory = manager.root.join(target);
        let path = directory.join("release.json");
        let record: Value = serde_json::from_slice(&fs::read(&path).with_context(|| {
            format!("self-owned core publication requires the matching Manager batch receipt {}", path.display())
        })?)?;
        require_same_release(&record, &manager, "manager", target, &path, scope)?;
        if record["publish_blocked"].as_str().is_none_or(|reason| !reason.is_empty()) {
            bail!("matching Manager batch artifact for {target} is not publishable");
        }
        for key in ["archive", "sources"] {
            verify_asset(&directory, &record, key)?;
        }
    }
    Ok(())
}

/// Publish every prepared target of `component` as one GitHub Release, then
/// propose the core catalog entries.
pub fn publish(inv: &Inventory, component: &str, version: &str, scope: Scope) -> Result<()> {
    let id = identity(inv, component, version)?;
    let version = id.version.as_str();
    let label = id.identity.as_ref().map_or_else(
        || version.to_owned(),
        shinki_emu_metadata::CoreIdentity::display_version,
    );
    let local_manager = scope.local_manager(inv, component);
    if component != "manager" {
        let core = inv.core(component)?;
        if !core.publish_blocked.is_empty() {
            bail!("{component} is publish_blocked: {}", core.publish_blocked);
        }
    }
    let tag = id.tag.as_str();
    if env::var("GITHUB_EVENT_NAME").ok().as_deref() == Some("push")
        && env::var("GITHUB_REF_TYPE").ok().as_deref() == Some("tag")
        && env::var("GITHUB_REF_NAME").ok().as_deref() != Some(tag)
    {
        bail!("tag does not match component/version");
    }
    require_product_batch(&id, component)?;
    if id.identity.is_none() {
        let source = crate::sources::state(&inv.root)?;
        if source.dirty || source.commit != id.commit {
            bail!("product release requires the clean frozen checkout at {}", id.commit);
        }
    }
    let PreparedRelease { records, blocked } = collect_records(inv, &id, component, scope)?;
    if component != "manager" && id.identity.is_none() {
        require_manager_batch(inv, &id, &records, scope)?;
    }
    if component != "manager" {
        crate::sources::verify_provenance(inv, inv.core(component)?)?;
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
    let ci = if scope == Scope::WindowsAndroid {
        let url = gh.require_ci(inv, &id.commit, scope)?;
        format!(
            "\nRelease scope: Windows and Android.\nExact-head Windows/Android GitHub CI: {url}.\n"
        )
    } else if local_manager {
        let url = gh.require_ci(inv, &id.commit, scope)?;
        verify_ohos_handoff(inv, component, version)?;
        let handoff = gh.require_ohos_handoff_ci(&id.commit)?;
        format!("\nExact-head GitHub CI: {url}.\nOHOS artifact handoff gate: {handoff}.\n")
    } else {
        String::new()
    };
    let notes = format!(
        "{component} {label}\n\nArtifacts:\n{artifacts}\n\nBlocked platforms:\n{}\n\nEach `-sources.zip` holds the exact product/fork revisions, the copyleft Rust crate sources, and the native library source archives and recipes needed to modify and relink. The universal Android Release APK is uploaded once; matching Actions matrix rows do not duplicate it.{ci}",
        blocked.join("\n")
    );
    let mut notes = if local_manager {
        format!(
            "{notes}\nThe OHOS HAP is experimental, signed with the OpenHarmony SDK test PKI. Its signature and native dependency closure were verified by the local DevEco packager. Device/emulator launch and retail installability were not verified.\n"
        )
    } else {
        notes
    };
    if component == "manager" {
        notes.push_str(
            "\nKnown previous-release issue: the official Manager beta.7 Windows binary self-reports beta.6. Published beta.7 tags and assets remain unchanged. This release binds its internal version to the compile-time product version and requires matching build, package and release identities.\n\nbeta.7 已知问题：官方 Windows 二进制内部自报 beta.6。已公开 beta.7 tag 和资产保持不变。本版通过编译期产品版本统一内部版本，并要求构建、安装包和发行记录一致。\n\nbeta.7 の既知の問題：公式 Windows バイナリは内部版を beta.6 と報告します。公開済み beta.7 タグと添付ファイルは変更しません。本版はコンパイル時の製品版を内部版に使い、ビルド・パッケージ・リリースの識別を一致させます。\n",
        );
        let android = inv
            .cores
            .iter()
            .filter(|core| {
                core.static_manager_targets
                    .iter()
                    .any(|target| target == "aarch64-linux-android")
            })
            .map(|core| core.id.as_str())
            .collect::<Vec<_>>();
        let cores = android.join(", ");
        notes.push_str(&format!(
            "\nAndroid Manager embeds {} cores: {cores}.\nAndroid Manager 内置 {} 个核心：{cores}。\nAndroid Manager は{}コアを組み込みます：{cores}。\n",
            android.len(),
            android.len(),
            android.len(),
        ));
        notes.push_str(
            "\nAndroid requires Android 9 (API 28) or later; Android 8.0/8.1 are no longer supported.\n\nAndroid 最低要求 Android 9（API 28），不再支持 Android 8.0/8.1。\n\nAndroid 9（API 28）以降が必要です。Android 8.0/8.1 はサポート対象外です。\n",
        );
        if !android.contains(&"kirikiroid2-web") {
            notes.push_str(
                "\nKiriKiri is not included in this Android Manager; static embedding remains in development for a later Manager release. A standalone Android core ZIP does not establish Manager support.\n\n本版 Android Manager 不包含 KiriKiri；其静态接入仍在开发，将随后续 Manager 版本交付。独立 Android 核心 ZIP 不代表 Manager 可用。\n\n本版 Android Manager に KiriKiri は含まれません。静的組み込みは後続の Manager リリースに向けて開発中です。単独の Android コア ZIP は Manager での利用を保証しません。\n",
            );
        }
        notes.push_str(
            "\nNew Musica patches require Musica core 0.1.0-beta.5 or later. Manager prompts you to update the core when its native capabilities are missing. Older Managers continue using the legacy v1 patch catalog.\n\n新 Musica 补丁需要 Musica 核心 beta.5 或更高版本；缺少所需原生能力时，Manager 会提示更新核心。旧 Manager 继续使用 v1 补丁目录。\n\n新しい Musica パッチには Musica コア 0.1.0-beta.5 以降が必要です。必要なネイティブ機能が不足する場合、Manager はコアの更新を案内します。旧 Manager は引き続き v1 パッチカタログを使用します。\n",
        );
    }
    if id.identity.is_some() {
        notes.push_str(
            "\nCompatibility: these upstream-identity packages require ShinkiEMU Manager 0.1.0-beta.6 or later. Earlier Managers use the legacy packages and catalogs.\n\n兼容性：这些上游身份核心包需要 ShinkiEMU Manager 0.1.0-beta.6 或以上版本。旧版 Manager 继续使用旧核心包和目录。\n\n互換性：この upstream 識別子付きコアパッケージには ShinkiEMU Manager 0.1.0-beta.6 以降が必要です。それ以前の Manager では従来のパッケージとカタログを使用してください。\n",
        );
    }
    let existing = release_target(
        &gh.releases_for_tag(tag)?,
        gh.tag_commit(tag)?.as_deref(),
        &id.commit,
    )?;
    // A new draft records the run's commit; publishing it creates the tag there.
    let release = match existing {
        Some(draft) => {
            if scope == Scope::WindowsAndroid {
                let assets = draft["assets"].as_array().context("draft assets")?;
                for asset in assets {
                    let name = asset["name"].as_str().context("draft asset name")?;
                    if !records.iter().any(|(_, record)| {
                        ["archive", "sources", EXTRA_ANDROID_SOURCES]
                            .iter()
                            .any(|key| record[*key]["name"] == name)
                    }) {
                        bail!("draft {tag} already has an asset outside scope: {name}");
                    }
                }
            }
            draft
        }
        None => gh.post(
            "releases",
            &json!({"tag_name":tag,"target_commitish":id.commit,"name":format!("{component} {label}"),"body":notes,"draft":true,"prerelease":id.identity.as_ref().map_or_else(|| semver::Version::parse(version).map(|v| !v.pre.is_empty()).unwrap_or(false), |i| i.upstream.ref_kind == shinki_emu_metadata::UpstreamRefKind::Branch)}),
        )?,
    };
    let release_id = release["id"].as_u64().context("release id")?;
    let mut assets = Vec::new();
    for (directory, record) in &records {
        gh.upload(&release, &directory.join(asset_name(record, "sources")?))?;
        if !record[EXTRA_ANDROID_SOURCES].is_null() {
            gh.upload(
                &release,
                &directory.join(asset_name(record, EXTRA_ANDROID_SOURCES)?),
            )?;
        }
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
            let mut entry = json!({"plugin_id":component,"display_name":c.display_name,"family_name":c.family_name,"version":version,"abi":"shinki.emu.native.c.v3","platform":platform,"url":url,"sha256":hash,"size":size,"license":c.license,"release_tag":tag});
            if let Some(identity) = &id.identity {
                entry["identity"] = serde_json::to_value(identity)?;
            }
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

    #[test]
    fn historical_external_pins_build_but_cannot_be_republished_as_new_releases() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inv = Inventory::load(root).unwrap();
        let source = inv.core("rfvp").unwrap().source.as_ref().unwrap();
        let frozen_version = source.identity().version();
        let mut published = source.identity();
        published.release_sequence += 1;
        let published_version = published.version();
        let tag = format!("core-rfvp-{published_version}");
        let entry = json!({
            "plugin_id":"rfvp", "display_name":"RFVP", "family_name":"RealLive",
            "platform":"windows-x86_64", "abi":"shinki.emu.native.c.v3",
            "license":"MIT", "identity":published, "version":published_version,
            "release_tag":tag,
            "url":format!("https://github.com/Project-Shinkirou/ShinkiEMU/releases/download/{tag}/core.zip"),
            "sha256":"a".repeat(64), "size":1,
        });
        let temp = crate::temporary::directory().unwrap();
        let catalog = temp
            .path()
            .join("Metadata/engines/cores/rfvp/windows-x86_64");
        fs::create_dir_all(&catalog).unwrap();
        fs::write(
            catalog.join("published.json"),
            serde_json::to_vec(&entry).unwrap(),
        )
        .unwrap();
        inv.root = temp.path().to_owned();
        assert_eq!(resolve_version(&inv, "rfvp", "").unwrap(), frozen_version);
        let error = identity(&inv, "rfvp", "").err().unwrap();
        assert!(
            error
                .to_string()
                .contains("external release_sequence must increase"),
            "{error:#}"
        );
    }

    #[test]
    fn release_receipts_and_external_versions_follow_pins_not_manager_beta() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inv = Inventory::load(root).unwrap();
        let id = identity(&inv, "rfvp", "").unwrap();
        assert!(resolve_version(&inv, "rfvp", "0.1.0-beta.5").is_err());
        let record = json!({
            "schema":"shinki.emu.release.v1","component":"rfvp","version":id.version,
            "source_commit":id.commit,"core_source":id.core_source,"identity":id.identity,
            "target":"x86_64-pc-windows-msvc",
        });
        let target = "x86_64-pc-windows-msvc";
        let path = Path::new("release.json");
        require_same_release(&record, &id, "rfvp", target, path, Scope::All).unwrap();
        let mut other = record.clone();
        other["identity"]["adapter"]["commit"] = "f".repeat(40).into();
        assert!(require_same_release(&other, &id, "rfvp", target, path, Scope::All).is_err());
        let mut changed = inv.clone();
        let source = changed
            .cores
            .iter_mut()
            .find(|core| core.id == "rfvp")
            .unwrap()
            .source
            .as_mut()
            .unwrap();
        source.rev = "f".repeat(40);
        source.release_sequence += 1;
        let updated = identity(&changed, "rfvp", "").unwrap();
        assert_ne!(updated.version, id.version);
        assert_ne!(updated.tag, id.tag);
    }

    #[test]
    fn product_release_rejects_mislabeled_or_mixed_receipts() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inv = Inventory::load(root).unwrap();
        for component in ["manager", "shinki.emu.cmvs", "shinki.emu.musica"] {
            let id = identity(&inv, component, "0.1.0-beta.8").unwrap();
            let target = "x86_64-pc-windows-msvc";
            let path = Path::new("release.json");
            let mut record = json!({
                "schema":"shinki.emu.release.v1", "component":component, "version":id.version,
                "source_commit":id.commit, "core_source":null, "identity":null, "target":target,
            });
            let verify =
                |record| require_same_release(record, &id, component, target, path, Scope::All);
            assert!(verify(&record).is_err());
            record["product_version"] = "0.1.0-beta.6".into();
            assert!(verify(&record).is_err());
            record["product_version"] = "0.1.0-beta.8".into();
            verify(&record).unwrap();
            let other = if id.commit.starts_with('a') { "b" } else { "a" };
            record["source_commit"] = other.repeat(40).into();
            assert!(verify(&record).is_err());
        }
    }

    #[test]
    fn scoped_release_keeps_standalone_android_rows_and_all_keeps_local_manager() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inv = Inventory::load(root).unwrap();
        let targets = |component, scope| {
            release_targets(&inv, component, scope)
                .unwrap()
                .into_iter()
                .map(|target| target.triple.as_str())
                .collect::<Vec<_>>()
        };
        let scope = Scope::WindowsAndroid;
        assert_eq!(
            targets("manager", scope),
            ["x86_64-pc-windows-msvc", ANDROID_PRIMARY]
        );
        assert_eq!(
            targets("siglus_rs", scope),
            ["x86_64-pc-windows-msvc", ANDROID_PRIMARY, ANDROID_SECONDARY]
        );
        assert!(!scope.local_manager(&inv, "manager"));
        assert!(Scope::default().local_manager(&inv, "manager"));
        assert!(targets("manager", Scope::default()).contains(&"aarch64-unknown-linux-ohos"));
    }

    #[test]
    fn scoped_ci_rejects_runs_without_scope_or_every_job() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inv = Inventory::load(root).unwrap();
        let mut jobs = inv
            .job_matrix(true, None)
            .unwrap()
            .include
            .into_iter()
            .filter(|row| Scope::WindowsAndroid.includes(&row.platform))
            .map(|row| {
                json!({
                    "name":format!("component ({}, {}, {}, {})", row.target, row.component,
                        row.external, row.runner),
                    "conclusion":"success"
                })
            })
            .collect::<Vec<_>>();
        assert!(require_scoped_ci_jobs(&inv, &jobs).is_err());
        jobs.push(json!({"name":"release-scope (windows-android)","conclusion":"success"}));
        require_scoped_ci_jobs(&inv, &jobs).unwrap();
        jobs[0]["conclusion"] = "skipped".into();
        assert!(require_scoped_ci_jobs(&inv, &jobs).is_err());
    }

    #[test]
    fn scoped_publish_requires_both_android_sources_and_disallows_legacy_receipts() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inv = Inventory::load(root).unwrap();
        let mut id = identity(&inv, "manager", "0.1.0-beta.9").unwrap();
        let temp = crate::temporary::directory().unwrap();
        id.root = temp.path().into();
        let scope = Scope::WindowsAndroid;
        let error = |scope| {
            let err = collect_records(&inv, &id, "manager", scope).err().unwrap();
            format!("{err:#}")
        };
        let write_record = |target: &str, scope: Option<Scope>| {
            let dir = id.root.join(target);
            fs::create_dir(&dir).unwrap();
            for name in ["binary.zip", "sources.zip"] {
                fs::write(dir.join(name), name).unwrap();
            }
            let mut record = json!({
                "schema":"shinki.emu.release.v1", "component":"manager", "target":target,
                "version":id.version, "product_version":id.version, "source_commit":id.commit,
                "core_source":null, "identity":null,
                "archive":{"name":"binary.zip", "sha256":package::sha256(&dir.join("binary.zip")).unwrap()},
                "sources":{"name":"sources.zip", "sha256":package::sha256(&dir.join("sources.zip")).unwrap()},
                "android_targets":[ANDROID_PRIMARY, ANDROID_SECONDARY], "publish_blocked":""
            });
            if let Some(scope) = scope {
                record["release_scope"] = scope.name().into();
            }
            let path = dir.join("release.json");
            fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
            path
        };
        let windows = "x86_64-pc-windows-msvc";
        assert!(error(scope).contains(windows));
        let legacy = write_record(windows, None);
        assert!(error(scope).contains("does not match"));
        assert!(error(Scope::default()).contains("x86_64-unknown-linux-gnu"));
        fs::remove_dir_all(legacy.parent().unwrap()).unwrap();
        write_record(windows, Some(scope));
        assert!(error(scope).contains(ANDROID_PRIMARY));
        write_record(ANDROID_PRIMARY, Some(scope));
        assert!(error(scope).contains("requires x86_64 corresponding source"));
    }

    #[test]
    fn android_production_apk_must_upgrade_the_public_version() {
        let version = "0.1.0-beta.9";
        let signing = format!("Signer #1 certificate SHA-256 digest: {ANDROID_RELEASE_CERT}");
        let badging = format!(
            "package: name='org.shinkirou.shinkiemu' versionCode='87' versionName='{version}'\nsdkVersion:'28'\n"
        );
        require_android_upgrade(&signing, &badging, version).unwrap();
        assert!(require_android_upgrade(&signing, &badging.replace("87", "86"), version).is_err());
        assert!(
            require_android_upgrade(&signing, &badging.replace("'28'", "'27'"), version).is_err()
        );
        assert!(require_android_upgrade(&signing, &badging, "0.1.0-beta.8").is_err());
        let wrong_signer = format!("Signer #1 certificate SHA-256 digest: {}", "f".repeat(64));
        assert!(require_android_upgrade(&wrong_signer, &badging, version).is_err());
    }
}
