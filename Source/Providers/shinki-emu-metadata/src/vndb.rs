use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex, MutexGuard},
    time::Instant,
};
use vn::{
    Vndb,
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
    "title,description,alttitle,aliases,titles.lang,titles.title,titles.latin,",
    "titles.official,titles.main,released,platforms,developers.name,developers.original,",
    "tags.name,image.url,image.thumbnail,image.dims,image.thumbnail_dims,image.sexual,image.violence",
);

#[derive(Debug, Clone)]
pub struct VndbProviderConfig {
    pub license: MetadataLicenseManifest,
    pub timeout: std::time::Duration,
    pub minimum_request_delay: std::time::Duration,
}

pub struct VndbProvider {
    work_client: reqwest12::Client,
    work_url: reqwest12::Url,
    release_client: Arc<Vndb>,
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
        let work_client = reqwest12::Client::builder()
            .use_rustls_tls()
            .https_only(true)
            .timeout(config.timeout)
            .user_agent(SHINKI_EMU_USER_AGENT)
            .build()
            .map_err(|error| map_vndb_error(error.into()))?;
        let release_client = Vndb::builder()
            .max_concurrent_requests(1)
            .timeout(config.timeout)
            .user_agent(SHINKI_EMU_USER_AGENT)
            .build();
        Ok(Self {
            work_client,
            work_url: Endpoint::VisualNovel.url(),
            release_client,
            next_request: Mutex::new(Instant::now()),
            cover_fetcher: CoverFetcher::new(CoverPolicy::default())?,
            config,
        })
    }

    fn authorize(&self) -> Result<(), MetadataError> {
        self.config.license.permit_vndb()
    }

    // Both transports share a single admission slot and cooldown. With the
    // typed release client, success is visible only after body decoding.
    async fn wait_for_request(&self) -> MutexGuard<'_, Instant> {
        let next_request = self.next_request.lock().await;
        tokio::time::sleep_until(*next_request).await;
        next_request
    }

    async fn query_works(
        &self,
        filter: Value,
        limit: Option<u8>,
    ) -> Result<Vec<MetadataRecord>, MetadataError> {
        let mut body = json!({"filters": filter, "fields": WORK_FIELDS});
        if let Some(limit) = limit {
            body["results"] = json!(limit);
        }
        let mut next_request = self.wait_for_request().await;
        let response = self
            .work_client
            .post(self.work_url.clone())
            .json(&body)
            .send()
            .await
            .and_then(reqwest12::Response::error_for_status)
            .map_err(|error| map_vndb_error(error.into()))?;
        *next_request = Instant::now() + self.config.minimum_request_delay;
        drop(next_request);
        let response: Response<KanaWork> = response.json().await.map_err(|error| {
            if error.is_decode() {
                MetadataError::SchemaMismatch("vndb-kana")
            } else {
                map_vndb_error(error.into())
            }
        })?;
        Ok(response.into_iter().map(convert_record).collect())
    }

    /// Query the concrete releases (rIDs) of a VN, newest first, so the local
    /// installation can be pinned to a specific version.
    async fn query_releases(
        &self,
        vn_id: &vn::VisualNovelId,
    ) -> Result<Vec<MetadataRelease>, MetadataError> {
        let mut next_request = self.wait_for_request().await;
        let response = self
            .release_client
            .post()
            .release()
            .filters(vn::http::JsonQueryFilter::new(release_filter(
                &vn_id.to_string(),
            )))
            .raw_fields([
                "id".to_owned(),
                "title".to_owned(),
                "alttitle".to_owned(),
                "released".to_owned(),
                "platforms".to_owned(),
            ])
            .sort(vn::model::release::SortReleaseBy::Released)
            .reverse()
            .results(100)
            .send()
            .await
            .map_err(map_vndb_error);
        if response.is_ok() || matches!(&response, Err(MetadataError::SchemaMismatch("vndb-kana")))
        {
            *next_request = Instant::now() + self.config.minimum_request_delay;
        }
        drop(next_request);
        let response = response?;
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
        self.query_works(json!(["search", "=", &query.title]), Some(query.limit))
            .await
    }

    async fn fetch_by_id(&self, remote_id: &str) -> Result<MetadataRecord, MetadataError> {
        self.authorize()?;
        if !is_vndb_id(remote_id) {
            return Err(MetadataError::InvalidRemoteId(remote_id.to_owned()));
        }
        let mut records = self
            .query_works(json!(["id", "=", remote_id]), None)
            .await?;
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
        let id = vn::VisualNovelId::new(remote_id)
            .ok_or_else(|| MetadataError::InvalidRemoteId(remote_id.to_owned()))?;
        self.query_releases(&id).await
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

fn map_vndb_error(error: vn::error::Error) -> MetadataError {
    if let vn::error::Error::RequestFailed {
        status: Some(status),
        ..
    } = &error
    {
        match status.as_u16() {
            400 => return MetadataError::RequestRejected("vndb"),
            401 => return MetadataError::Unauthorized,
            429 => return MetadataError::RateLimited,
            _ => {}
        }
    }
    let message = error.to_string();
    if message.contains("429") {
        MetadataError::RateLimited
    } else if message.contains("401") {
        MetadataError::Unauthorized
    } else if message.contains("deserialize") || message.contains("decode") {
        MetadataError::SchemaMismatch("vndb-kana")
    } else {
        MetadataError::Upstream(format!("vndb:{message}"))
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
        assert!(
            record
                .alternate_titles
                .iter()
                .any(|title| title == "Gendai")
        );
        assert!(
            record
                .alternate_titles
                .iter()
                .any(|title| title == "Search alias")
        );
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

    #[tokio::test]
    async fn work_queries_preserve_kana_wire_contract_and_request_pacing() {
        use crate::ReleaseUse;
        use std::time::Duration;
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
        use tokio_rustls::{TlsAcceptor, rustls};

        let delay = Duration::from_millis(120);
        let config = VndbProviderConfig {
            license: MetadataLicenseManifest {
                release_use: ReleaseUse::NonCommercial,
                vndb_commercial_license_id: None,
            },
            timeout: Duration::from_secs(3),
            minimum_request_delay: delay,
        };
        let mut provider = VndbProvider::new(config).unwrap();
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
        let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.cert.der().clone()], key.into())
        .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(tls));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        provider.work_url.set_host(Some("localhost")).unwrap();
        provider
            .work_url
            .set_port(Some(listener.local_addr().unwrap().port()))
            .unwrap();
        provider.work_client = reqwest12::Client::builder()
            .use_rustls_tls()
            .https_only(true)
            .no_proxy()
            .resolve("localhost", listener.local_addr().unwrap())
            .timeout(provider.config.timeout)
            .user_agent(SHINKI_EMU_USER_AGENT)
            .add_root_certificate(
                reqwest12::Certificate::from_pem(cert.cert.pem().as_bytes()).unwrap(),
            )
            .build()
            .unwrap();

        let server = tokio::spawn(async move {
            let response = json!({"more": false, "results": [{
                "id": "v17", "title": "Provider default", "platforms": ["win"],
                "titles": [
                    {"lang": "zh-Hant", "title": "繁體標題", "official": true, "main": false},
                    {"lang": "zh-Hans", "title": "简体标题", "official": true, "main": false}
                ],
                "developers": [{"id": "p1", "name": "Developer", "original": null}],
                "image": {"url": "https://unused.invalid/img", "thumbnail": "https://unused.invalid/thumb",
                    "dims": [640, 480], "thumbnail_dims": [320, 240], "sexual": 0, "violence": 0}
            }]}).to_string();
            let mut receipt = Vec::new();
            for (status, payload) in [
                ("200 OK", response.as_str()),
                ("400 Bad Request", ""),
                ("401 Unauthorized", ""),
                ("429 Too Many Requests", ""),
                ("200 OK", "{not-json"),
            ] {
                let (stream, _) = listener.accept().await.unwrap();
                let mut stream = BufReader::new(acceptor.accept(stream).await.unwrap());
                let mut line = String::new();
                stream.read_line(&mut line).await.unwrap();
                assert_eq!(line.trim_end(), "POST /kana/vn HTTP/1.1");
                let mut length = None;
                let mut has_user_agent = false;
                loop {
                    line.clear();
                    assert_ne!(stream.read_line(&mut line).await.unwrap(), 0);
                    if line == "\r\n" {
                        break;
                    }
                    let (name, value) = line.split_once(':').unwrap();
                    assert!(!name.eq_ignore_ascii_case("authorization"));
                    if name.eq_ignore_ascii_case("user-agent") {
                        assert_eq!(value.trim(), SHINKI_EMU_USER_AGENT);
                        has_user_agent = true;
                    }
                    if name.eq_ignore_ascii_case("content-length") {
                        length = Some(value.trim().parse().unwrap());
                    }
                }
                assert!(has_user_agent);
                let mut body = vec![0; length.unwrap()];
                stream.read_exact(&mut body).await.unwrap();
                receipt.push((
                    Instant::now(),
                    serde_json::from_slice::<Value>(&body).unwrap(),
                ));
                let wire_response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                stream
                    .get_mut()
                    .write_all(wire_response.as_bytes())
                    .await
                    .unwrap();
                stream.get_mut().shutdown().await.unwrap();
            }
            receipt
        });

        let mut search = MetadataSearchQuery {
            title: "License denied".into(),
            aliases: Vec::new(),
            developer: None,
            release_date: None,
            limit: 7,
        };
        provider.config.license.release_use = ReleaseUse::Commercial;
        assert!(matches!(
            provider.search(&search).await,
            Err(MetadataError::LicenseBlocked(_))
        ));
        provider.config.license.release_use = ReleaseUse::NonCommercial;
        search.title = "Game".into();
        let results = provider.search(&search).await.unwrap();
        assert_eq!(results.len(), 1);
        let work = &results[0];
        assert_eq!(work.remote_id, "v17");
        assert_eq!(work.localized_title("zh-CN"), Some("简体标题"));
        assert_eq!(work.localized_title("zh-Hant"), Some("繁體標題"));
        assert_eq!(work.developers, ["Developer"]);
        assert_eq!(work.cover.as_ref().unwrap().width, Some(320));
        let deadline = *provider.next_request.lock().await;
        assert!(matches!(
            provider.fetch_by_id("v17").await,
            Err(MetadataError::RequestRejected("vndb"))
        ));
        assert_eq!(*provider.next_request.lock().await, deadline);
        assert!(matches!(
            provider.fetch_by_id("v17").await,
            Err(MetadataError::Unauthorized)
        ));
        assert_eq!(*provider.next_request.lock().await, deadline);
        assert!(matches!(
            provider.fetch_by_id("v17").await,
            Err(MetadataError::RateLimited)
        ));
        assert_eq!(*provider.next_request.lock().await, deadline);
        let malformed = provider.fetch_by_id("v17").await;
        assert!(
            matches!(malformed, Err(MetadataError::SchemaMismatch("vndb-kana"))),
            "unexpected malformed-response result: {malformed:?}"
        );

        let receipt = tokio::time::timeout(provider.config.timeout, server)
            .await
            .unwrap()
            .unwrap();
        let mut fields = receipt[0].1["fields"]
            .as_str()
            .unwrap()
            .split(',')
            .collect::<Vec<_>>();
        fields.sort_unstable();
        assert_eq!(
            fields,
            [
                "aliases",
                "alttitle",
                "description",
                "developers.name",
                "developers.original",
                "image.dims",
                "image.sexual",
                "image.thumbnail",
                "image.thumbnail_dims",
                "image.url",
                "image.violence",
                "platforms",
                "released",
                "tags.name",
                "title",
                "titles.lang",
                "titles.latin",
                "titles.main",
                "titles.official",
                "titles.title",
            ]
        );
        assert_eq!(receipt[0].1["filters"], json!(["search", "=", "Game"]));
        assert_eq!(receipt[0].1["results"], 7);
        assert!(receipt[0].1.get("sort").is_none());
        assert!(receipt[1].0 >= deadline && receipt[1].0 - receipt[0].0 >= delay);
        for (_, body) in &receipt[1..] {
            assert_eq!(body["filters"], json!(["id", "=", "v17"]));
            assert_eq!(body["fields"], receipt[0].1["fields"]);
            assert!(body.get("sort").is_none());
            assert!(body.get("results").is_none());
        }
    }
}
