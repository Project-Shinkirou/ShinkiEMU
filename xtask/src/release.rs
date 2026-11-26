use crate::{
    build,
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
const ANDROID_SECONDARY: &str = "x86_64-linux-android";

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

    fn release_row(self, target: &Target, component: &str) -> bool {
        !target.is_local_qa()
            && self.includes(&target.platform)
            && (component != "manager" || target.triple != ANDROID_SECONDARY)
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
        if scope.release_row(target, component) && inv.blocked(component, &target.triple)?.is_none()
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

/// Release version, tag and published catalog identity.
struct Identity {
    tag: String,
    commit: String,
    core_source: Value,
    root: PathBuf,
    version: String,
    identity: Option<shinki_emu_metadata::CoreIdentity>,
}

pub fn resolve_version(inv: &Inventory, component: &str, supplied: &str) -> Result<String> {
    if component != "manager" && inv.core(component)?.attribution.is_some() {
        let identity = inv
            .core(component)?
            .identity(&inv.root)?
            .context("release requires a locked adapter Git revision")?;
        let version = identity.version();
        if !supplied.is_empty() && supplied != version {
            bail!(
                "external core version is derived from upstream and adapter pins; omit --version or use {version}"
            );
        }
        return Ok(version);
    }
    semver::Version::parse(supplied)
        .context("Manager and self-owned cores require an explicit product --version (SemVer)")?;
    Ok(supplied.to_owned())
}

pub fn dry_run(inv: &Inventory, component: &str, version: &str, scope: Scope) -> Result<()> {
    let targets = release_targets(inv, component, scope)?;
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
        inv.core(component)?.identity(&inv.root)?
    };
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
            serde_json::to_value(&core.attribution)?,
        )
    };
    package::portable(&tag)?;
    let commit = crate::build::source_commit(inv)?;
    Ok(Identity {
        root: inv.root.join(".work/release").join(&tag),
        tag,
        commit,
        core_source,
        version,
        identity: external,
    })
}

fn require_public_manager_variant(features: &[String]) -> Result<()> {
    if features
        .iter()
        .any(|feature| feature.starts_with("static-"))
    {
        bail!("static Manager variants are not dynamic release packages");
    }
    Ok(())
}

/// Package one target and record the asset for publication.
pub fn prepare(
    inv: &Inventory,
    target: &str,
    component: &str,
    version: &str,
    scope: Scope,
) -> Result<()> {
    let spec = inv.target(target)?;
    if !scope.release_row(spec, component) {
        bail!(
            "{component}/{target} is not a release row in scope {}",
            scope.name()
        );
    }
    if component == "manager" {
        require_public_manager_variant(&build::requested_manager_features()?)?;
    }
    let id = identity(inv, component, version)?;
    let version = id.version.as_str();
    if let Some(reason) = inv.blocked(component, target)? {
        println!("{component}/{target}: blocked: {reason}; artifact=null");
        return Ok(());
    }
    let root = id.root.join(target);
    let record_path = root.join("release.json");
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
    package::atomic_directory(&root, |work| {
        let staged = work.join("package");
        let package_dir = package::package(inv, target, component, version, &staged)?;
        let ios = (component == "manager"
            && crate::apple::ios_frameworks::target_supported(target))
        .then(|| {
            crate::apple::ios_frameworks::read_manifest(
                inv,
                &package_dir,
                target,
                version,
                &id.commit,
            )
        })
        .transpose()?;
        let archive = work.join(&filename);
        let mut ohos = None;
        let mut android_signing = None;
        if component == "manager" && (target.contains("android") || target.ends_with("ohos")) {
            let (metadata, key) = if target.ends_with("ohos") {
                ("ohos.json", "hap")
            } else {
                ("android.json", "apk")
            };
            let package: Value = serde_json::from_slice(&fs::read(package_dir.join(metadata))?)?;
            let name = package[key].as_str().context("mobile package filename")?;
            fs::copy(package_dir.join(name), &archive)?;
            if key == "hap" {
                ohos = Some(package);
            } else {
                android_signing = package["signing"].as_str().map(str::to_owned);
            }
        } else {
            package::zip_entries(&package::tree(&package_dir)?, &archive)?;
        }
        if let Some(manifest) = &ios {
            manifest.verify_release_archive(&archive)?;
        }
        fs::remove_dir_all(&staged)?;
        let sha256 = package::sha256(&archive)?;
        let mut record = json!({
            "component": component, "version": version, "target": target, "platform": platform,
            "source_commit": id.commit,
            "release_scope": scope.name(),
            "archive": {"name": filename, "sha256": sha256, "size": fs::metadata(&archive)?.len()},
        });
        if let Some(signing) = android_signing {
            record["android_signing"] = json!(signing);
        }
        if let Some(ios) = ios {
            record["ios"] = serde_json::to_value(ios)?;
        }
        if let Some(mut ohos) = ohos {
            if ohos["source_commit"] != id.commit
                || ohos["version"] != version
                || ohos["target"] != target
                || ohos["sha256"] != sha256
                || !ohos["bundled_core"].is_object()
            {
                bail!("signed HAP and OHOS package receipt disagree");
            }
            record["bundled_core"] = ohos["bundled_core"].take();
        }
        fs::write(
            work.join("release.json"),
            serde_json::to_vec_pretty(&record)?,
        )?;
        Ok(())
    })?;
    println!("{}", record_path.display());
    Ok(())
}

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
    let path = directory.join(name);
    if package::sha256(&path)? != expected
        || record[key]["size"].as_u64() != Some(fs::metadata(&path)?.len())
    {
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
    let require_all = scope == Scope::WindowsAndroid || scope.local_manager(inv, component);
    let mut records = Vec::new();
    let mut blocked = Vec::new();
    for t in &inv.targets {
        if !scope.release_row(t, component) {
            continue;
        }
        if let Some(reason) = inv.blocked(component, &t.triple)? {
            blocked.push(format!("- {}: {reason} (artifact=null)", t.triple));
            continue;
        }
        let ios =
            component == "manager" && crate::apple::ios_frameworks::target_supported(&t.triple);
        let directory = id.root.join(&t.triple);
        let path = directory.join("release.json");
        if !path.is_file() {
            if require_all || ios {
                bail!(
                    "{component} release requires a prepared package for {} in scope {}",
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
        if record["component"] != component
            || record["version"] != id.version
            || record["target"] != t.triple
            || record["release_scope"] != scope.name()
        {
            bail!("{} is not a package for this release", path.display());
        }
        if record["source_commit"] != id.commit {
            bail!(
                "{} was not prepared from product commit {}",
                path.display(),
                id.commit
            );
        }
        verify_asset(&directory, &record, "archive")?;
        if ios {
            let manifest: crate::apple::ios_frameworks::Manifest =
                serde_json::from_value(record["ios"].clone())
                    .context("iOS release requires the exact-head Framework manifest")?;
            manifest.validate(inv, &t.triple, &id.version, &id.commit)?;
            manifest.verify_release_archive(&directory.join(asset_name(&record, "archive")?))?;
        }
        if component == "manager"
            && t.platform == "android"
            && record["android_signing"] != "release"
        {
            bail!(
                "{component}/{} requires a release-signed Android APK",
                t.triple
            );
        }
        records.push((directory, record));
    }
    if records.is_empty() {
        bail!("zero publishable artifacts; no Release created");
    }
    Ok(PreparedRelease { records, blocked })
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
    let tag = id.tag.as_str();
    let PreparedRelease { records, blocked } = collect_records(inv, &id, component, scope)?;
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
    let notes = format!(
        "{component} {label}\n\nArtifacts:\n{artifacts}\n\nBlocked platforms:\n{}",
        blocked.join("\n")
    );
    let mut notes = if local_manager {
        format!("{notes}\nThe experimental OHOS HAP is signed with the OpenHarmony SDK test PKI.\n")
    } else {
        notes
    };
    if component == "manager"
        && records
            .iter()
            .any(|(_, record)| record["platform"] == "android-universal")
    {
        notes.push_str(
            "\nAndroid requires Android 9 (API 28) or later. Install separately released cores from the official catalog.\n\nAndroid 需要 Android 9（API 28）或更新版本，核心从官方目录单独安装。\n\nAndroid 9（API 28）以降が必要です。コアは公式カタログから別途インストールしてください。\n",
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
                    if !records.iter().any(|(_, record)| record["archive"]["name"] == name) {
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
    fn static_core_manager_variants_are_rejected_before_release_packaging() {
        assert!(require_public_manager_variant(&["store-steam".into()]).is_ok());
        for feature in ["static-adv32", "static-cmvs"] {
            assert!(
                require_public_manager_variant(&["store-steam".into(), feature.into()]).is_err()
            );
        }
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

    #[test]
    fn publishing_reuses_only_a_draft_at_the_run_commit() {
        let commit = "1".repeat(40);
        let other = "2".repeat(40);
        let draft =
            |at: &str| json!({"tag_name":"manager-v1.0.0","draft":true,"target_commitish":at});
        // No release or tag yet: create a draft that GitHub tags at the commit on publish.
        assert!(release_target(&[], None, &commit).unwrap().is_none());
        // An existing tag is valid only when it names this exact commit.
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
        let primary = "aarch64-linux-android";
        assert_eq!(
            targets("manager", scope),
            ["x86_64-pc-windows-msvc", primary]
        );
        assert_eq!(
            targets("siglus_rs", scope),
            ["x86_64-pc-windows-msvc", primary, ANDROID_SECONDARY]
        );
        assert!(!scope.local_manager(&inv, "manager"));
        assert!(Scope::default().local_manager(&inv, "manager"));
        assert!(targets("manager", Scope::default()).contains(&"aarch64-unknown-linux-ohos"));
    }

    #[test]
    fn private_ohos_qa_is_excluded_from_release() {
        let inv = Inventory::load(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
        let qa = shinki_emu_build::ohos::Target::X64.rust_target();
        let (companion, _) = crate::ohos::companion(&inv, qa).unwrap();
        for component in ["manager", &companion.id] {
            assert!(
                release_targets(&inv, component, Scope::All)
                    .unwrap()
                    .iter()
                    .all(|target| target.triple != qa)
            );
            let error = prepare(&inv, qa, component, "1.0.0", Scope::All).unwrap_err();
            assert!(error.to_string().contains("not a release row"), "{error}");
        }
    }

    #[test]
    fn all_platform_manager_requires_same_commit_and_publishable_packages() {
        let inv = Inventory::load(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
        let out = tempfile::tempdir().unwrap();
        let mut id = identity(&inv, "manager", "1.0.0").unwrap();
        id.root = out.path().to_owned();
        let targets = release_targets(&inv, "manager", Scope::All).unwrap();
        let android = id.root.join("aarch64-linux-android/release.json");
        for target in &targets {
            let dir = id.root.join(&target.triple);
            fs::create_dir(&dir).unwrap();
            let archive = dir.join("asset");
            let ios = if target.platform == "ios" {
                Some(
                    crate::apple::ios_frameworks::tests::release_archive_fixture(
                        &inv,
                        &target.triple,
                        &id.version,
                        &id.commit,
                        &archive,
                    ),
                )
            } else {
                fs::write(&archive, b"test release asset").unwrap();
                None
            };
            fs::write(
                dir.join("release.json"),
                serde_json::to_vec(&json!({
                    "component": "manager",
                    "version": id.version,
                    "target": target.triple,
                    "release_scope": "all",
                    "source_commit": id.commit,
                    "android_signing": (target.platform == "android").then_some("release"),
                    "ios": ios,
                    "archive": {
                        "name": "asset",
                        "sha256": package::sha256(&archive).unwrap(),
                        "size": fs::metadata(&archive).unwrap().len(),
                    },
                }))
                .unwrap(),
            )
            .unwrap();
        }
        let target_count = targets.len();
        let qa = shinki_emu_build::ohos::Target::X64.rust_target();
        let qa_dir = id.root.join(qa);
        fs::create_dir(&qa_dir).unwrap();
        fs::write(qa_dir.join("release.json"), b"untrusted local QA record").unwrap();
        let prepared = collect_records(&inv, &id, "manager", Scope::All).unwrap();
        assert_eq!(prepared.records.len(), target_count);
        assert!(prepared.blocked.iter().all(|row| !row.contains(qa)));
        let mut record: Value = serde_json::from_slice(&fs::read(&android).unwrap()).unwrap();
        let different_digit = if id.commit.starts_with('0') { "1" } else { "0" };
        record["source_commit"] = json!(different_digit.repeat(40));
        fs::write(&android, serde_json::to_vec(&record).unwrap()).unwrap();
        let Err(error) = collect_records(&inv, &id, "manager", Scope::All) else {
            panic!("all-platform publication accepted a record from another commit");
        };
        assert!(
            error
                .to_string()
                .contains("was not prepared from product commit"),
            "{error}"
        );

        record["source_commit"] = json!(id.commit);
        record["android_signing"] = json!("debug");
        fs::write(&android, serde_json::to_vec(&record).unwrap()).unwrap();
        let Err(error) = collect_records(&inv, &id, "manager", Scope::All) else {
            panic!("all-platform publication accepted a debug Android record");
        };
        assert!(
            error
                .to_string()
                .contains("requires a release-signed Android APK"),
            "{error}"
        );
    }
}
