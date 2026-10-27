mod transfer;

use std::{collections::HashSet, path::Path, sync::Arc};

use reqwest::{
    Client, StatusCode,
    header::{COOKIE, LOCATION},
};
use serde::{Deserialize, Serialize};
use shinki_emu_store::{
    BoxFuture, CancelToken, Capabilities, DownloadPlan, LoginKind, LoginRequest, LoginUpdate,
    LoginUpdates, OwnedTitle, PlanOptions, ProgressUpdates, ProviderDescriptor, SecretStore,
    StoreError, StoreProvider, StoreSession, StoredLogin, TitleDetail, TitleId, TransferSummary,
};
use tokio::sync::mpsc;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const ID: &str = "dlsite";
const PLAY: &str = "https://play.dlsite.com/api/v3";
const DOWNLOAD: &str = "https://play.dlsite.com/api/v3/download";
const PRODUCT: &str = "https://www.dlsite.com/home/api/=/product.json";
const METHODS: &[LoginKind] = &[LoginKind::BrowserCookies];
static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    id: ID,
    display_name: "DLsite",
    login_methods: METHODS,
    browser_login_url: Some("https://login.dlsite.com/login"),
    capabilities: Capabilities {
        branches: false,
        depots: false,
        verify: true,
    },
};

#[derive(Clone, Debug, Deserialize)]
struct Count {
    user: u64,
}
#[derive(Clone, Debug, Deserialize)]
struct Sale {
    workno: String,
    #[serde(default)]
    sales_date: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
struct Works {
    #[serde(default)]
    works: Vec<Work>,
}
#[derive(Clone, Debug, Deserialize)]
struct Work {
    #[serde(alias = "workno")]
    id: String,
    #[serde(default)]
    name: Name,
    #[serde(default)]
    content_size: Option<u64>,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(untagged)]
enum Name {
    Text(String),
    Localized(std::collections::BTreeMap<String, String>),
    #[default]
    Missing,
}
impl Name {
    fn text(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Localized(m) => m
                .get("en_US")
                .or_else(|| m.get("ja_JP"))
                .or_else(|| m.values().next())
                .cloned()
                .unwrap_or_default(),
            Self::Missing => String::new(),
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
struct PublicWork {
    #[serde(default, alias = "product_id")]
    id: String,
    #[serde(default, alias = "work_name")]
    name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct Auth {
    cookie_header: String,
    account: String,
}
struct Http {
    client: Client,
    cookie: Arc<Zeroizing<String>>,
}
struct PlannedFile {
    url: reqwest::Url,
}
struct PlanData {
    files: Vec<PlannedFile>,
}

impl Http {
    fn new(cookie: Zeroizing<String>) -> Result<Self, StoreError> {
        let client = Client::builder()
            .user_agent("ShinkiEMU")
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| StoreError::Network)?;
        Ok(Self { client, cookie: Arc::new(cookie) })
    }
    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T, StoreError> {
        let r = self
            .client
            .get(url)
            .header(COOKIE, self.cookie.as_str())
            .send()
            .await
            .map_err(|_| StoreError::Network)?;
        if r.status() == StatusCode::UNAUTHORIZED {
            return Err(StoreError::SessionExpired);
        }
        r.error_for_status()
            .map_err(|_| StoreError::Network)?
            .json()
            .await
            .map_err(|_| StoreError::Metadata)
    }
    async fn library(&self) -> Result<Vec<Work>, StoreError> {
        let count: Count = self
            .get_json(&format!("{PLAY}/content/count?last=0"))
            .await?;
        if count.user == 0 {
            return Ok(Vec::new());
        }
        let sales: Vec<Sale> = self
            .get_json(&format!("{PLAY}/content/sales?last=0"))
            .await?;
        let ids: Vec<String> = sales.into_iter().map(|s| s.workno).collect();
        let mut seen = HashSet::new();
        let mut works = Vec::new();
        for batch in ids.chunks(100) {
            let r = self
                .client
                .post(format!("{PLAY}/content/works"))
                .header(COOKIE, self.cookie.as_str())
                .json(batch)
                .send()
                .await
                .map_err(|_| StoreError::Network)?;
            if r.status() == StatusCode::UNAUTHORIZED {
                return Err(StoreError::SessionExpired);
            }
            let mut page: Works = r
                .error_for_status()
                .map_err(|_| StoreError::Network)?
                .json()
                .await
                .map_err(|_| StoreError::Metadata)?;
            for work in page.works.drain(..) {
                if seen.insert(work.id.clone()) {
                    works.push(work);
                }
            }
        }
        Ok(works)
    }
    async fn metadata(&self, id: &str) -> Result<Option<PublicWork>, StoreError> {
        let r = self
            .client
            .get(PRODUCT)
            .query(&[("workno", id)])
            .header(COOKIE, self.cookie.as_str())
            .send()
            .await
            .map_err(|_| StoreError::Network)?;
        if !r.status().is_success() {
            return Ok(None);
        }
        let values: Vec<PublicWork> = r.json().await.map_err(|_| StoreError::Metadata)?;
        Ok(values.into_iter().find(|w| w.id == id))
    }
    async fn download_plan(&self, id: &str) -> Result<PlanData, StoreError> {
        let response = self.client.get(DOWNLOAD).query(&[("workno", id)])
            .header(COOKIE, self.cookie.as_str()).send().await.map_err(|_| StoreError::Network)?;
        if response.status() == StatusCode::UNAUTHORIZED { return Err(StoreError::SessionExpired); }
        if !response.status().is_redirection() { return Err(StoreError::Unavailable); }
        let location = response.headers().get(LOCATION).and_then(|v| v.to_str().ok()).ok_or(StoreError::Metadata)?;
        let url = response.url().join(location).map_err(|_| StoreError::Metadata)?;
        let links = match classify_download_redirect(&url) {
            DownloadKind::Direct => vec![url],
            DownloadKind::Split => {
                let html = self.fetch_text(url.as_str()).await?;
                let mut links: Vec<_> = extract_links(&html, &url).into_iter()
                    .filter(|link| classify_download_redirect(link) == DownloadKind::Direct && part_number(link).is_some())
                    .collect();
                links.sort_by_key(|link| part_number(link));
                links
            }
            DownloadKind::Serial => {
                let html = self.fetch_text(url.as_str()).await?;
                extract_links(&html, &url).into_iter()
                    .filter(|link| classify_download_redirect(link) == DownloadKind::Direct).take(1).collect()
            }
            DownloadKind::Unavailable => return Err(StoreError::Unavailable),
        };
        if links.is_empty() { return Err(StoreError::Metadata); }
        let mut files = Vec::with_capacity(links.len());
        for link in links {
            if !link.path().contains("product_id") || !link.path().contains(id) { return Err(StoreError::Metadata); }
            files.push(PlannedFile { url: link });
        }
        Ok(PlanData { files })
    }
    async fn fetch_text(&self, url: &str) -> Result<String, StoreError> {
        let mut url = reqwest::Url::parse(url).map_err(|_| StoreError::Metadata)?;
        for _ in 0..8 {
            if url.scheme() != "https" || url.host_str() != Some("www.dlsite.com") {
                return Err(StoreError::Metadata);
            }
            let response = self.client.get(url).header(COOKIE, self.cookie.as_str())
                .send().await.map_err(|_| StoreError::Network)?;
            if response.status().is_redirection() {
                let location = response.headers().get(LOCATION).and_then(|v| v.to_str().ok())
                    .ok_or(StoreError::Metadata)?;
                url = response.url().join(location).map_err(|_| StoreError::Metadata)?;
                continue;
            }
            if response.status() == StatusCode::UNAUTHORIZED { return Err(StoreError::SessionExpired); }
            let response = response.error_for_status().map_err(|_| StoreError::Metadata)?;
            if response.content_length().is_some_and(|len| len > 512 * 1024) { return Err(StoreError::Metadata); }
            let body = response.bytes().await.map_err(|_| StoreError::Network)?;
            if body.len() > 512 * 1024 { return Err(StoreError::Metadata); }
            return String::from_utf8(body.to_vec()).map_err(|_| StoreError::Metadata);
        }
        Err(StoreError::TransferFailed)
    }
}
fn extract_links(html: &str, base: &reqwest::Url) -> Vec<reqwest::Url> {
    let mut links = Vec::new();
    for quote in ['"', '\''] {
        let mut start = None;
        for (index, character) in html.char_indices() {
            if character != quote { continue; }
            if let Some(open) = start.take() {
                let escaped = html[open..index].replace("&amp;", "&");
                if let Ok(url) = base.join(&escaped) {
                    if url.scheme() == "https" && url.host_str() == Some("www.dlsite.com") &&
                        url.path().starts_with("/home/download") && url.path().contains("product_id")
                    { links.push(url); }
                }
            } else { start = Some(index + character.len_utf8()); }
        }
    }
    links.sort();
    links.dedup();
    links
}
fn part_number(url: &reqwest::Url) -> Option<u32> {
    let mut segments = url.path_segments()?;
    while let Some(segment) = segments.next() {
        if segment == "number" { return segments.next()?.parse().ok(); }
    }
    None
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DownloadKind { Direct, Split, Serial, Unavailable }
fn classify_download_redirect(url: &reqwest::Url) -> DownloadKind {
    let host = url.host_str().unwrap_or_default();
    let path = url.path();
    if host != "www.dlsite.com" { return DownloadKind::Unavailable; }
    if path.starts_with("/home/download/split") || path.starts_with("/home/split") { DownloadKind::Split }
    else if path.starts_with("/home/download/serial") || path.starts_with("/home/serial") { DownloadKind::Serial }
    else if path.starts_with("/home/download") { DownloadKind::Direct }
    else { DownloadKind::Unavailable }
}


pub struct Provider;
impl StoreProvider for Provider {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &DESCRIPTOR
    }
    fn login<'a>(
        &'a self,
        request: LoginRequest,
        secrets: &'a dyn SecretStore,
        _codes: &'a mut mpsc::Receiver<String>,
        updates: LoginUpdates,
        cancel: CancelToken,
    ) -> BoxFuture<'a, Box<dyn StoreSession>> {
        Box::pin(async move {
            let (cookie, account) = match request {
                LoginRequest::BrowserCookies { cookies } if !cookies.trim().is_empty() => {
                    (cookies, "DLsite account".to_owned())
                }
                LoginRequest::BrowserCookies { .. } => return Err(StoreError::CookiesRequired),
                _ => return Err(StoreError::UnsupportedLogin),
            };
            if cancel.is_cancelled() {
                return Err(StoreError::Cancelled);
            }
            updates(LoginUpdate::Connecting);
            let http = Http::new(cookie.clone())?;
            http.library().await?;
            let auth = Auth {
                cookie_header: cookie.to_string(),
                account: account.clone(),
            };
            let secret = serde_json::to_string(&auth).map_err(|_| StoreError::SecretStore)?;
            secrets
                .save(
                    ID,
                    &StoredLogin {
                        account: account.clone(),
                        secret,
                    },
                )
                .map_err(|_| StoreError::SecretStore)?;
            updates(LoginUpdate::SignedIn(account.clone()));
            Ok(Box::new(Session { account, http }) as Box<dyn StoreSession>)
        })
    }
    fn restore<'a>(
        &'a self,
        secrets: &'a dyn SecretStore,
        cancel: CancelToken,
    ) -> BoxFuture<'a, Option<Box<dyn StoreSession>>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Ok(None);
            }
            let Some(stored) = secrets.load(ID).map_err(|_| StoreError::SecretStore)? else {
                return Ok(None);
            };
            let auth: Auth =
                serde_json::from_str(&stored.secret).map_err(|_| StoreError::SessionExpired)?;
            let http = Http::new(Zeroizing::new(auth.cookie_header.clone()))?;
            http.library().await?;
            Ok(Some(Box::new(Session {
                account: auth.account.clone(),
                http,
            }) as Box<dyn StoreSession>))
        })
    }
}
pub struct Session {
    account: String,
    http: Http,
}
impl Session {
    async fn find(&self, id: &TitleId) -> Result<Work, StoreError> {
        self.http
            .library()
            .await?
            .into_iter()
            .find(|w| w.id == id.0)
            .ok_or(StoreError::AccessDenied)
    }
}
impl StoreSession for Session {
    fn account(&self) -> &str {
        &self.account
    }
    fn library(&self) -> BoxFuture<'_, Vec<OwnedTitle>> {
        Box::pin(async move {
            let titles = self
                .http
                .library()
                .await?
                .into_iter()
                .map(|w| OwnedTitle {
                    id: TitleId(w.id),
                    name: w.name.text(),
                    platforms: vec!["Windows".into()],
                    languages: Vec::new(),
                    size_bytes: w.content_size.unwrap_or_default(),
                    unavailable_reason: None,
                })
                .collect();
            Ok(titles)
        })
    }
    fn detail<'a>(&'a self, id: &'a TitleId) -> BoxFuture<'a, TitleDetail> {
        Box::pin(async move {
            let w = self.find(id).await?;
            let m = self.http.metadata(&w.id).await?;
            let title = OwnedTitle {
                id: id.clone(),
                name: m
                    .as_ref()
                    .map(|p| p.name.clone())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| w.name.text()),
                platforms: vec!["Windows".into()],
                languages: vec![],
                size_bytes: w.content_size.unwrap_or_default(),
                unavailable_reason: None,
            };
            Ok(TitleDetail {
                title,
                branches: vec![],
                depots: vec![],
            })
        })
    }
    fn plan<'a>(&'a self, id: &'a TitleId, _options: PlanOptions) -> BoxFuture<'a, DownloadPlan> {
        Box::pin(async move {
            let w = self.find(id).await?;
            let mut title = OwnedTitle {
                id: id.clone(),
                name: w.name.text(),
                platforms: vec!["Windows".into()],
                languages: vec![],
                size_bytes: w.content_size.unwrap_or_default(),
                unavailable_reason: None,
            };
            let data = match self.http.download_plan(&w.id).await {
                Ok(data) => data,
                Err(e) => {
                    title.unavailable_reason = Some(
                        "DLsite did not provide a verified PC download plan for this work.".into(),
                    );
                    return Err(e);
                }
            };
            Ok(DownloadPlan {
                title,
                branch: String::new(),
                depots: vec![],
                payload: Box::new(data),
            })
        })
    }
    fn download<'a>(
        &'a self,
        plan: &'a DownloadPlan,
        dest: &'a Path,
        work_dir: &'a Path,
        progress: ProgressUpdates,
        cancel: CancelToken,
    ) -> BoxFuture<'a, TransferSummary> {
        Box::pin(async move {
            let data = plan.payload.downcast_ref::<PlanData>().ok_or(StoreError::Metadata)?;
            transfer::download_parts(&self.http, data, &plan.title, dest, work_dir, &progress, &cancel).await
        })
    }
    fn has_update<'a>(
        &'a self,
        _id: &'a TitleId,
        _installed_dir: &'a Path,
        _options: PlanOptions,
    ) -> BoxFuture<'a, shinki_emu_store::UpdateState> {
        Box::pin(async { Ok(shinki_emu_store::UpdateState::Unknown) })
    }
    fn logout<'a>(&'a self, secrets: &'a dyn SecretStore) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            secrets.clear(ID).map_err(|_| StoreError::SecretStore)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn count_sales_and_works_real_shapes_decode() {
        let c: Count =
            serde_json::from_str(r#"{"user":2,"production":2,"page_limit":50,"concurrency":500}"#)
                .unwrap();
        let s: Vec<Sale> =
            serde_json::from_str(r#"[{"workno":"RJ123456","sales_date":"2024-01-01T00:00:00Z"}]"#)
                .unwrap();
        let w:Works=serde_json::from_str(r#"{"works":[{"workno":"RJ123456","name":{"ja_JP":"作品"},"maker":{"id":"RG1","name":{"ja_JP":"Circle"}},"work_type":"SOU","age_category":"r18","work_files":{},"upgrade_date":"2025-01-01"}]}"#).unwrap();
        assert_eq!(c.user, 2);
        assert_eq!(s[0].workno, "RJ123456");
        assert_eq!(w.works[0].id, "RJ123456");
    }
    #[test]
    fn split_part_order_is_numeric() {
        let mut urls = vec![
            "https://www.dlsite.com/home/download/split/number/10/product_id/RJ1".to_owned(),
            "https://www.dlsite.com/home/download/split/number/2/product_id/RJ1".to_owned(),
        ];
        urls.sort_by(|a, b| natural_cmp(a, b));
        assert!(urls[0].contains("/number/2/"));
    }
    #[test]
    fn play_only_redirect_is_not_a_pc_download() {
        let play = reqwest::Url::parse("https://play.dlsite.com/reader/RJ123456").unwrap();
        assert_eq!(classify_download_redirect(&play), DownloadKind::Unavailable);
        let pc = reqwest::Url::parse("https://www.dlsite.com/home/download/=/product_id/RJ123456.html").unwrap();
        assert_eq!(classify_download_redirect(&pc), DownloadKind::Direct);
    }
    #[test]
    fn fixed_error_display_does_not_echo_secrets() {
        let e = StoreError::TransferFailed;
        assert!(!e.to_string().contains("cookie"));
        assert!(!e.to_string().contains("token"));
    }
}
