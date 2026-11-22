use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{sync::Mutex, time::Instant};
use vn::{
    http::Endpoint,
    model::{
        Response,
        visual_novel::{VisualNovelDeveloper, VisualNovelImage, VisualNovelTag},
    },
};

use crate::{
    CoverAsset, CoverFetcher, CoverPolicy, LocalizedTitle, MetadataError, MetadataLicenseManifest,
    MetadataProvider, MetadataProviderId, MetadataRecord, MetadataRelease, MetadataSearchQuery,
    RemoteCover,
};

const SHINKI_EMU_USER_AGENT: &str =
    "RoukakuEngine/ShinkiEMU (https://github.com/Project-Shinkirou/ShinkiEMU)";
const WORK_FIELDS: &str = concat!(
    "title,description,alttitle,aliases,titles{lang,title,latin,official,main},released,platforms,",
    "developers{name,original},tags.name,image{url,thumbnail,dims,thumbnail_dims,sexual,violence}",
);

#[derive(Debug, Clone)]
pub struct VndbProviderConfig {
    pub license: MetadataLicenseManifest,
    pub timeout: std::time::Duration,
    pub minimum_request_delay: std::time::Duration,
}

pub struct VndbProvider {
    client: reqwest12::Client,
    next_request: Mutex<Instant>,
    cover_fetcher: CoverFetcher,
    config: VndbProviderConfig,
}

impl VndbProvider {
    pub fn new(config: VndbProviderConfig) -> Result<Self, MetadataError> {
        config.license.permit_vndb()?;
        tracing::info!(
            target: "shinki_emu_metadata::vndb",
            event = "emu.metadata.provider.create",
            provider_id = "vndb",
            timeout_ms = config.timeout.as_millis(),
            minimum_request_delay_ms = config.minimum_request_delay.as_millis(),
            "metadata provider created"
        );
        let client = reqwest12::Client::builder()
            .use_rustls_tls()
            .https_only(true)
            .timeout(config.timeout)
            .user_agent(SHINKI_EMU_USER_AGENT)
            .build()
            .map_err(map_vndb_error)?;
        Ok(Self {
            client,
            next_request: Mutex::new(Instant::now()),
            cover_fetcher: CoverFetcher::new(CoverPolicy::default())?,
            config,
        })
    }

    fn authorize(&self) -> Result<(), MetadataError> {
        self.config.license.permit_vndb()
    }

    async fn query<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: Endpoint,
        body: Value,
    ) -> Result<Response<T>, MetadataError> {
        let mut next_request = self.next_request.lock().await;
        tokio::time::sleep_until(*next_request).await;
        let response = self
            .client
            .post(endpoint.url())
            .json(&body)
            .send()
            .await
            .and_then(reqwest12::Response::error_for_status)
            .map_err(map_vndb_error);
        *next_request = Instant::now() + self.config.minimum_request_delay;
        response?.json().await.map_err(map_vndb_error)
    }

    async fn query_works(
        &self,
        filter: Value,
        results: u8,
    ) -> Result<Vec<MetadataRecord>, MetadataError> {
        let response: Response<KanaWork> = self
            .query(
                Endpoint::VisualNovel,
                json!({"filters": filter, "fields": WORK_FIELDS, "results": results}),
            )
            .await?;
        Ok(response.into_iter().map(convert_record).collect())
    }

    /// Query the concrete releases (rIDs) of a VN, newest first, so the local
    /// installation can be pinned to a specific version.
    async fn query_releases(&self, vn_id: &str) -> Result<Vec<MetadataRelease>, MetadataError> {
        let response: Response<vn::model::release::Release> = self
            .query(
                Endpoint::Release,
                json!({
                    "filters": release_filter(vn_id),
                    "fields": "id,title,alttitle,released,platforms",
                    "sort": "released", "reverse": true, "results": 100,
                }),
            )
            .await?;
        let mut releases = response
            .results
            .into_iter()
            .map(convert_release)
            .collect::<Vec<_>>();
        releases
            .sort_by_key(|release| std::cmp::Reverse(release.released.clone().unwrap_or_default()));
        Ok(releases)
    }
}

impl MetadataProvider for VndbProvider {
    fn provider_id(&self) -> MetadataProviderId {
        MetadataProviderId::Vndb
    }

    async fn search(
        &self,
        query: &MetadataSearchQuery,
    ) -> Result<Vec<MetadataRecord>, MetadataError> {
        self.authorize()?;
        query.validate()?;
        self.query_works(json!(["search", "=", &query.title]), query.limit)
            .await
    }

    async fn fetch_by_id(&self, remote_id: &str) -> Result<MetadataRecord, MetadataError> {
        self.authorize()?;
        if !is_vndb_id(remote_id) {
            return Err(MetadataError::InvalidRemoteId(remote_id.to_owned()));
        }
        let mut records = self.query_works(json!(["id", "=", remote_id]), 10).await?;
        if records.len() != 1 {
            return Err(MetadataError::NotFound(remote_id.to_owned()));
        }
        Ok(records.remove(0))
    }

    async fn fetch_cover(
        &self,
        record: &MetadataRecord,
        allow_sensitive: bool,
    ) -> Result<CoverAsset, MetadataError> {
        self.authorize()?;
        if record.provider != MetadataProviderId::Vndb {
            return Err(MetadataError::InvalidRequest("provider"));
        }
        self.cover_fetcher.fetch(record, allow_sensitive).await
    }

    async fn fetch_releases(&self, remote_id: &str) -> Result<Vec<MetadataRelease>, MetadataError> {
        self.authorize()?;
        if !is_vndb_id(remote_id) {
            return Err(MetadataError::InvalidRemoteId(remote_id.to_owned()));
        }
        self.query_releases(remote_id).await
    }
}

fn is_vndb_id(value: &str) -> bool {
    value.strip_prefix('v').is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn convert_release(value: vn::model::release::Release) -> MetadataRelease {
    MetadataRelease {
        release_id: value.id.to_string(),
        title: value.title.or(value.alttitle),
        released: value.released,
        platforms: value.platforms.unwrap_or_default(),
    }
}

// vn's VisualNovelTitle normalizes zh-Hans and zh-Hant to the same enum.
// Decode the queried work fields directly to retain the provider's exact tags.
#[derive(Deserialize)]
struct KanaWork {
    id: vn::VisualNovelId,
    title: Option<String>,
    description: Option<String>,
    alttitle: Option<String>,
    aliases: Option<Vec<String>>,
    titles: Option<Vec<KanaTitle>>,
    released: Option<String>,
    platforms: Option<Vec<String>>,
    developers: Option<Vec<VisualNovelDeveloper>>,
    tags: Option<Vec<VisualNovelTag>>,
    image: Option<VisualNovelImage>,
}

#[derive(Deserialize)]
struct KanaTitle {
    lang: Option<String>,
    title: Option<String>,
    latin: Option<String>,
    main: Option<bool>,
    official: Option<bool>,
}

fn convert_record(value: KanaWork) -> MetadataRecord {
    let mut alternate_titles = value.aliases.unwrap_or_default();
    if let Some(title) = value.alttitle {
        alternate_titles.push(title);
    }
    let mut localized_titles = Vec::new();
    if let Some(titles) = value.titles {
        for title in titles {
            if let Some(text) = title.title {
                if let Some(language) = title.lang.filter(|lang| !lang.is_empty())
                    && !text.trim().is_empty()
                {
                    localized_titles.push(LocalizedTitle {
                        language,
                        title: text.clone(),
                        official: title.official.unwrap_or(false),
                        main: title.main.unwrap_or(false),
                    });
                }
                alternate_titles.push(text);
            }
            if let Some(latin) = title.latin {
                alternate_titles.push(latin);
            }
        }
    }
    alternate_titles.sort();
    alternate_titles.dedup();
    let developers = value
        .developers
        .unwrap_or_default()
        .into_iter()
        .filter_map(|developer| developer.producer.name.or(developer.producer.original))
        .collect();
    let mut tags = value
        .tags
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tag| tag.tag.name)
        .collect::<Vec<_>>();
    tags.sort();
    tags.dedup();
    let cover = value.image.map(|image| RemoteCover {
        url: image.thumbnail.or(image.url).unwrap_or_default(),
        width: image.thumbnail_dims.or(image.dims).map(|dims| dims[0]),
        height: image.thumbnail_dims.or(image.dims).map(|dims| dims[1]),
        sexual: image.sexual,
        violence: image.violence,
    });
    let sensitive = cover.as_ref().is_some_and(|cover| {
        cover.sexual.unwrap_or(2.0) > 0.0 || cover.violence.unwrap_or(2.0) > 0.0
    });
    MetadataRecord {
        provider: MetadataProviderId::Vndb,
        remote_id: value.id.to_string(),
        title: value.title.unwrap_or_default(),
        localized_titles,
        description: value.description.map(|text| plain_description(&text)),
        alternate_titles,
        tags,
        developers,
        release_date: value.released,
        platforms: value.platforms.unwrap_or_default(),
        engine: None,
        cover: cover.filter(|cover| !cover.url.is_empty()),
        sensitive,
    }
}

/// Kana `/release` filter selecting every release of one visual novel. The
/// filter is named `vn`; `vns` is only a response field and is rejected.
fn release_filter(vn_id: &str) -> serde_json::Value {
    serde_json::json!(["vn", "=", ["id", "=", vn_id]])
}

const DESCRIPTION_TAGS: [&str; 7] = ["b", "i", "u", "s", "quote", "raw", "code"];

enum DescriptionTag {
    Keep,
    SpoilerOpen,
    SpoilerClose,
}

/// Parse one recognised VNDB formatting tag at the start of `text`, returning
/// its kind and byte length. Unknown bracket text is not a tag.
fn description_tag(text: &str) -> Option<(DescriptionTag, usize)> {
    let end = text.find(']')?;
    let inner = &text[1..end];
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(name) => (true, name),
        None => (false, inner),
    };
    let (name, argument) = match inner.split_once('=') {
        Some((name, argument)) => (name, Some(argument)),
        None => (inner, None),
    };
    let named = |tag: &str| name.eq_ignore_ascii_case(tag);
    let kind = if named("url") && (!closing || argument.is_none()) {
        DescriptionTag::Keep
    } else if argument.is_some() {
        return None;
    } else if named("spoiler") {
        if closing {
            DescriptionTag::SpoilerClose
        } else {
            DescriptionTag::SpoilerOpen
        }
    } else if DESCRIPTION_TAGS.iter().any(|tag| named(tag)) {
        DescriptionTag::Keep
    } else {
        return None;
    };
    Some((kind, end + 1))
}

fn strip_description_tags(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    let mut spoiler_depth = 0usize;
    while let Some(start) = rest.find('[') {
        if spoiler_depth == 0 {
            output.push_str(&rest[..start]);
        }
        rest = &rest[start..];
        let Some((kind, length)) = description_tag(rest) else {
            if spoiler_depth == 0 {
                output.push('[');
            }
            rest = &rest[1..];
            continue;
        };
        rest = &rest[length..];
        match kind {
            DescriptionTag::Keep => {}
            DescriptionTag::SpoilerOpen => spoiler_depth += 1,
            DescriptionTag::SpoilerClose => {
                if spoiler_depth == 1 && (output.is_empty() || output.ends_with('\n')) {
                    rest = rest.strip_prefix('\n').unwrap_or(rest);
                }
                spoiler_depth = spoiler_depth.saturating_sub(1);
            }
        }
    }
    if spoiler_depth == 0 {
        output.push_str(rest);
    }
    output
}

/// Convert VNDB description formatting to plain text: formatting tags and
/// `[url=…]` wrappers keep their content, spoilers (including an unclosed
/// one) are removed, line breaks are kept and blank-line runs collapse to one.
pub fn plain_description(text: &str) -> String {
    let mut current = text.replace("\r\n", "\n");
    loop {
        let stripped = strip_description_tags(&current);
        let mut lines = Vec::new();
        for line in stripped.lines().map(str::trim_end) {
            if !(line.is_empty() && lines.last().is_some_and(|last: &&str| last.is_empty())) {
                lines.push(line);
            }
        }
        let next = lines.join("\n").trim().to_owned();
        if next == current {
            return next;
        }
        current = next;
    }
}

fn vndb_status_error(status: reqwest12::StatusCode) -> Option<MetadataError> {
    match status.as_u16() {
        400 => Some(MetadataError::RequestRejected("vndb")),
        401 => Some(MetadataError::Unauthorized),
        429 => Some(MetadataError::RateLimited),
        _ => None,
    }
}

fn map_vndb_error(error: reqwest12::Error) -> MetadataError {
    if let Some(error) = error.status().and_then(vndb_status_error) {
        error
    } else if error.is_decode() {
        MetadataError::SchemaMismatch("vndb-kana")
    } else {
        MetadataError::Upstream(format!("vndb:{error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_response_keeps_exact_title_languages_and_search_aliases() {
        let response: Response<KanaWork> = serde_json::from_value(json!({
            "more": false,
            "results": [{
                "id": "v17",
                "title": "Provider default",
                "alttitle": "原題",
                "aliases": ["Search alias"],
                "titles": [
                    {"lang": "zh-Hant", "title": "繁體標題", "official": true, "main": false},
                    {"lang": "zh-Hans", "title": "简体标题", "official": false, "main": false},
                    {"lang": "ja", "title": "原題", "latin": "Gendai", "official": true, "main": true},
                    {"lang": "en", "title": "English Title", "official": true, "main": false}
                ]
            }]
        }))
        .unwrap();
        let record = convert_record(response.into_iter().next().unwrap());
        assert_eq!(record.title, "Provider default");
        assert_eq!(record.localized_title("zh-CN"), Some("简体标题"));
        assert_eq!(record.localized_title("zh-Hant"), Some("繁體標題"));
        assert_eq!(record.localized_title("ja"), Some("原題"));
        assert_eq!(record.localized_title("en"), Some("English Title"));
        assert!(record.localized_titles[2].main && record.localized_titles[2].official);
        assert!(record.alternate_titles.iter().any(|title| title == "Gendai"));
        assert!(record.alternate_titles.iter().any(|title| title == "Search alias"));
    }

    #[test]
    fn validates_remote_ids_without_network() {
        assert!(is_vndb_id("v17"));
        assert!(!is_vndb_id("17"));
        assert!(!is_vndb_id("v17/path"));
    }

    #[test]
    fn release_filter_uses_the_vn_filter() {
        assert_eq!(
            release_filter("v17"),
            serde_json::json!(["vn", "=", ["id", "=", "v17"]])
        );
    }

    #[test]
    fn plain_description_strips_vndb_formatting() {
        let source = "The story follows [b]Kazuki[/b], a [i]bored [u]student[/u][/i].\n\n\
[spoiler]He is the culprit.[/spoiler]\n\n\
[quote]Remember the [s]promise[/s].[/quote]\n\
[From [url=https://en.wikipedia.org/wiki/Air_(visual_novel)]Wikipedia[/url]]";
        let plain = plain_description(source);
        assert_eq!(
            plain,
            "The story follows Kazuki, a bored student.\n\nRemember the promise.\n[From Wikipedia]"
        );
        assert_eq!(plain_description(&plain), plain);
    }

    #[test]
    fn plain_description_removes_inline_and_unclosed_spoilers() {
        assert_eq!(
            plain_description("Ending: [spoiler]she [b]dies[/b][/spoiler] maybe.\r\nNext line"),
            "Ending:  maybe.\nNext line"
        );
        assert_eq!(
            plain_description("Line one\n[spoiler]secret\n\nmore"),
            "Line one"
        );
        assert_eq!(
            plain_description("[b]bold, never closed"),
            "bold, never closed"
        );
    }

    #[test]
    fn plain_description_leaves_unknown_brackets_alone() {
        let text = "Arrays like a[0] and [Edited from the official site] stay; [url] too.";
        assert_eq!(
            plain_description(text),
            "Arrays like a[0] and [Edited from the official site] stay;  too."
        );
        assert_eq!(plain_description("[url=x]a[/url=y]"), "a[/url=y]");
        assert_eq!(plain_description("[[b]b]"), "");
    }

    #[test]
    fn rejected_request_maps_to_a_stable_code() {
        assert!(matches!(
            vndb_status_error(reqwest12::StatusCode::BAD_REQUEST),
            Some(MetadataError::RequestRejected("vndb"))
        ));
    }
}
