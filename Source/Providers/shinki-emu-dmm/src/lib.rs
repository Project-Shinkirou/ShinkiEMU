//! DMM GAMES / FANZA GAMES provider.
//!
//! Catalog and detail endpoints are intentionally not guessed: the maintained
//! reference client exposes `/v5/userinfo` but does not expose an owned-game
//! response parser. A live capture is required before those methods can be
//! enabled. The grounded file-list and transfer path is available through
//! `Session::plan_from_file_list`.
mod protocol;
mod transfer;

#[cfg(windows)]
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
#[cfg(windows)]
use base64::{Engine, engine::general_purpose::STANDARD};
#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData},
};

use protocol::{Manifest, parse_file_list};
use reqwest::{
    Client, Url,
    header::{COOKIE, HeaderMap, HeaderValue, LOCATION, SET_COOKIE},
    redirect::Policy,
};
use serde::{Deserialize, Serialize};
use shinki_emu_store::{
    BoxFuture, CancelToken, Capabilities, CookieJar, DownloadPlan, LoginKind, LoginRequest,
    LoginUpdate, LoginUpdates, OwnedTitle, PlanOptions, ProgressUpdates, ProviderDescriptor,
    SecretStore, StoreError, StoreProvider, StoreSession, StoredLogin, TitleDetail, TitleId,
    TransferSummary, UpdateState, WebLogin,
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};
const API_BASE: &str = "https://apidgp-gameplayer.games.dmm.com";
const STORE_ID: &str = "dmm";
/// DMM accounts issues both only once the password (and any two-step check)
/// is accepted; DGP's login URL then redirects to its callback code.
const SESSION_COOKIES: &[&str] = &["login_secure_id", "login_session_id"];
static WEB_LOGIN: WebLogin = WebLogin {
    start_url: "https://accounts.dmm.com/service/login/password",
    cookie_urls: &["https://accounts.dmm.com/", "https://www.dmm.com/"],
    session_cookies: SESSION_COOKIES,
    completion_urls: &[],
};
static LOGIN_METHODS: &[LoginKind] = &[LoginKind::Web(&WEB_LOGIN), LoginKind::LocalImport];
static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    id: STORE_ID,
    display_name: "DMM GAMES / FANZA GAMES",
    login_methods: LOGIN_METHODS,
    capabilities: Capabilities {
        branches: false,
        depots: false,
        verify: true,
    },
};

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct AuthState {
    #[serde(rename = "accessToken")]
    access_token: String,
}

struct SessionState {
    auth: AuthState,
    client: Client,
    signatures: Mutex<HashMap<String, Zeroizing<String>>>,
}

#[derive(Deserialize)]
struct ApiResponse<T> {
    result_code: u32,
    data: T,
}

#[derive(Deserialize)]
struct LoginUrl {
    url: String,
}
#[derive(Deserialize)]
struct AccessToken {
    access_token: String,
}

fn http_client() -> Result<Client, StoreError> {
    Client::builder()
        .redirect(Policy::none())
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|_| StoreError::Network)
}

fn dgp_headers(access_token: Option<&str>) -> Result<HeaderMap, StoreError> {
    let mut headers = HeaderMap::new();
    headers.insert("Client-App", HeaderValue::from_static("DMMGamePlayer5"));
    headers.insert("Client-Version", HeaderValue::from_static("5.3.25"));
    headers.insert(
        "User-Agent",
        HeaderValue::from_static("DMMGamePlayer5-Win/5.3.25 Electron/34.3.0"),
    );
    if let Some(token) = access_token {
        headers.insert(
            "actauth",
            HeaderValue::try_from(token).map_err(|_| StoreError::SecretStore)?,
        );
    }
    Ok(headers)
}

async fn post_api<T: serde::de::DeserializeOwned>(
    client: &Client,
    path: &str,
    body: serde_json::Value,
    token: Option<&str>,
    cancel: &CancelToken,
) -> Result<T, StoreError> {
    if cancel.is_cancelled() {
        return Err(StoreError::Cancelled);
    }
    let request = client
        .post(format!("{API_BASE}{path}"))
        .headers(dgp_headers(token)?)
        .json(&body);
    let response = tokio::select! {
        result = request.send() => result.map_err(map_request)?,
        () = cancel.cancelled() => return Err(StoreError::Cancelled),
    };
    if !response.status().is_success() {
        return Err(map_status(response.status()));
    }
    let parsed: ApiResponse<T> = response.json().await.map_err(|_| StoreError::Metadata)?;
    if parsed.result_code != 100 {
        return Err(StoreError::SessionExpired);
    }
    Ok(parsed.data)
}

fn allowed_login_host(url: &Url) -> bool {
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some("accounts.dmm.com" | "www.dmm.com" | "webdgp-gameplayer.games.dmm.com")
        )
}

fn callback_code(url: &Url) -> Option<String> {
    if url.host_str() != Some("webdgp-gameplayer.games.dmm.com") || url.path() != "/login/success" {
        return None;
    }
    url.query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
}

/// Follow DGP's login URL with the signed-in DMM cookies until it hands out
/// the callback code, then exchange the code for a DGP access token.
async fn login_with_cookies(
    mut jar: CookieJar,
    cancel: &CancelToken,
) -> Result<AuthState, StoreError> {
    let client = http_client()?;
    let login: LoginUrl = post_api(
        &client,
        "/v5/auth/login/url",
        serde_json::json!({"prompt": ""}),
        None,
        cancel,
    )
    .await?;
    let mut url = Url::parse(&login.url).map_err(|_| StoreError::Metadata)?;
    let mut code = None;
    for _ in 0..20 {
        if cancel.is_cancelled() {
            return Err(StoreError::Cancelled);
        }
        if !allowed_login_host(&url) {
            return Err(StoreError::UnsupportedLogin);
        }
        if let Some(value) = callback_code(&url) {
            code = Some(value);
            break;
        }
        let mut request = client.get(url.clone());
        if let Some(cookie) = jar.header(&url) {
            request = request.header(
                COOKIE,
                HeaderValue::try_from(cookie.as_str())
                    .map_err(|_| StoreError::WebLoginIncomplete)?,
            );
        }
        let response = tokio::select! {
            result = request.send() => result.map_err(map_request)?,
            () = cancel.cancelled() => return Err(StoreError::Cancelled),
        };
        jar.update(
            response.url(),
            response
                .headers()
                .get_all(SET_COOKIE)
                .iter()
                .filter_map(|field| field.to_str().ok()),
        );
        if !response.status().is_redirection() {
            return Err(StoreError::UnsupportedLogin);
        }
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(StoreError::Metadata)?;
        url = url.join(location).map_err(|_| StoreError::Metadata)?;
    }
    let code = code.ok_or(StoreError::UnsupportedLogin)?;
    let issued: AccessToken = post_api(
        &client,
        "/v5/auth/accesstoken/issue",
        serde_json::json!({"code": code}),
        None,
        cancel,
    )
    .await?;
    if issued.access_token.is_empty() {
        return Err(StoreError::SessionExpired);
    }
    let auth = AuthState {
        access_token: issued.access_token,
    };
    validate_token(&client, &auth, cancel).await?;
    Ok(auth)
}

async fn validate_token(
    client: &Client,
    auth: &AuthState,
    cancel: &CancelToken,
) -> Result<(), StoreError> {
    let _: serde_json::Value = post_api(
        client,
        "/v5/userinfo",
        serde_json::json!({}),
        Some(&auth.access_token),
        cancel,
    )
    .await?;
    Ok(())
}

pub struct Provider;
impl Provider {
    pub const fn new() -> Self {
        Self
    }
}
impl Default for Provider {
    fn default() -> Self {
        Self::new()
    }
}
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
            if cancel.is_cancelled() {
                return Err(StoreError::Cancelled);
            }
            updates(LoginUpdate::Connecting);
            let client = http_client()?;
            let auth = match request {
                LoginRequest::Web { cookies } => {
                    let jar = CookieJar::new(cookies);
                    if !SESSION_COOKIES.iter().all(|name| jar.contains(name)) {
                        return Err(StoreError::WebLoginIncomplete);
                    }
                    login_with_cookies(jar, &cancel).await?
                }
                LoginRequest::LocalImport => {
                    let auth = import_local()?;
                    validate_token(&client, &auth, &cancel).await?;
                    auth
                }
                LoginRequest::Qr | LoginRequest::Credentials { .. } => {
                    return Err(StoreError::UnsupportedLogin);
                }
            };
            let login = StoredLogin {
                account: "DMM account".to_owned(),
                secret: serde_json::to_string(&auth).map_err(|_| StoreError::SecretStore)?,
            };
            secrets.save(STORE_ID, &login)?;
            updates(LoginUpdate::SignedIn(login.account.clone()));
            Ok(Box::new(Session {
                state: Arc::new(SessionState {
                    auth,
                    client,
                    signatures: Mutex::default(),
                }),
            }) as Box<dyn StoreSession>)
        })
    }
    fn restore<'a>(
        &'a self,
        secrets: &'a dyn SecretStore,
        cancel: CancelToken,
    ) -> BoxFuture<'a, Option<Box<dyn StoreSession>>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(StoreError::Cancelled);
            }
            let Some(stored) = secrets.load(STORE_ID)? else {
                return Ok(None);
            };
            let auth = serde_json::from_str::<AuthState>(&stored.secret)
                .map_err(|_| StoreError::SecretStore)?;
            if auth.access_token.is_empty() {
                return Ok(None);
            }
            let client = http_client()?;
            validate_token(&client, &auth, &cancel).await?;
            Ok(Some(Box::new(Session {
                state: Arc::new(SessionState {
                    auth,
                    client,
                    signatures: Mutex::default(),
                }),
            }) as Box<dyn StoreSession>))
        })
    }
}

pub struct Session {
    state: Arc<SessionState>,
}
impl Session {
    /// Fetches the exact file-list URL returned by DMM's launch/package metadata.
    /// The URL and CloudFront signing fields are grounded in FastLauncher's
    /// `download(sign, filelist_url, output)` implementation.
    pub async fn fetch_manifest(&self, filelist_url: &str) -> Result<Manifest, StoreError> {
        // Upstream resolves this URL relative to API_DGP and never sends CDN signatures to the API.
        if !filelist_url.starts_with('/') || filelist_url.starts_with("//") {
            return Err(StoreError::Metadata);
        }
        let url =
            Url::parse(&format!("{API_BASE}{filelist_url}")).map_err(|_| StoreError::Metadata)?;
        if url.scheme() != "https" || url.host_str() != Some("apidgp-gameplayer.games.dmm.com") {
            return Err(StoreError::Metadata);
        }
        let response = self
            .state
            .client
            .get(url)
            .headers(self.headers()?)
            .send()
            .await
            .map_err(map_request)?;
        if !response.status().is_success() {
            return Err(map_status(response.status()));
        }
        let body = response.bytes().await.map_err(map_request)?;
        parse_file_list(&body).map_err(|_| StoreError::Metadata)
    }
    /// `sign` comes from an authenticated DGP download response; keep it in memory, not in the plan.
    pub fn plan_from_file_list(
        &self,
        id: TitleId,
        manifest: Manifest,
        sign: &str,
    ) -> Result<DownloadPlan, StoreError> {
        let query = transfer::signed_query(sign);
        if query.len() != 3
            || !["Policy", "Signature", "Key-Pair-Id"]
                .iter()
                .all(|key| query.iter().filter(|(name, _)| name == key).count() == 1)
        {
            return Err(StoreError::Metadata);
        }
        self.state
            .signatures
            .lock()
            .map_err(|_| StoreError::SecretStore)?
            .insert(id.0.clone(), Zeroizing::new(sign.to_owned()));
        let title = OwnedTitle {
            id: id.clone(),
            name: id.0.clone(),
            platforms: vec!["windows".into()],
            languages: Vec::new(),
            size_bytes: manifest.files.iter().map(|file| file.size).sum(),
            unavailable_reason: None,
        };
        Ok(DownloadPlan {
            title,
            branch: String::new(),
            depots: Vec::new(),
            payload: Box::new(transfer::DmmPlan { manifest }),
        })
    }
    fn headers(&self) -> Result<HeaderMap, StoreError> {
        dgp_headers(Some(&self.state.auth.access_token))
    }
}

impl StoreSession for Session {
    fn account(&self) -> &str {
        "DMM account"
    }
    fn library(&self) -> BoxFuture<'_, Vec<OwnedTitle>> {
        Box::pin(async { Err(StoreError::CatalogCaptureRequired) })
    }
    fn detail<'a>(&'a self, _id: &'a TitleId) -> BoxFuture<'a, TitleDetail> {
        Box::pin(async { Err(StoreError::CatalogCaptureRequired) })
    }
    fn plan<'a>(&'a self, _id: &'a TitleId, _options: PlanOptions) -> BoxFuture<'a, DownloadPlan> {
        Box::pin(async { Err(StoreError::CatalogCaptureRequired) })
    }
    fn download<'a>(
        &'a self,
        plan: &'a DownloadPlan,
        dest: &'a Path,
        work_dir: &'a Path,
        progress: ProgressUpdates,
        cancel: CancelToken,
    ) -> BoxFuture<'a, TransferSummary> {
        let sign = self
            .state
            .signatures
            .lock()
            .map_err(|_| StoreError::SecretStore)
            .and_then(|signatures| {
                signatures
                    .get(&plan.title.id.0)
                    .cloned()
                    .ok_or(StoreError::Metadata)
            });
        Box::pin(async move {
            let sign = sign?;
            let mut query = transfer::signed_query(&sign);
            let outcome = transfer::download(
                &self.state.client,
                plan,
                &query,
                dest,
                work_dir,
                progress,
                cancel,
            )
            .await;
            query.zeroize();
            outcome
        })
    }
    fn has_update<'a>(
        &'a self,
        _id: &'a TitleId,
        _installed_dir: &'a Path,
        _options: PlanOptions,
    ) -> BoxFuture<'a, UpdateState> {
        Box::pin(async { Ok(UpdateState::Unknown) })
    }
    fn logout<'a>(&'a self, secrets: &'a dyn SecretStore) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.state
                .signatures
                .lock()
                .map_err(|_| StoreError::SecretStore)?
                .clear();
            secrets.clear(STORE_ID)
        })
    }
}

fn map_request(error: reqwest::Error) -> StoreError {
    if error.is_timeout() {
        StoreError::Timeout
    } else {
        StoreError::Network
    }
}
fn map_status(status: reqwest::StatusCode) -> StoreError {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        StoreError::SessionExpired
    } else if status == reqwest::StatusCode::FORBIDDEN {
        StoreError::AccessDenied
    } else {
        StoreError::Network
    }
}

fn import_local() -> Result<AuthState, StoreError> {
    #[cfg(not(windows))]
    {
        return Err(StoreError::ImportUnsupported);
    }
    #[cfg(windows)]
    {
        let appdata = std::env::var_os("APPDATA").ok_or(StoreError::NoLocalLogin)?;
        let root = PathBuf::from(appdata).join("dmmgameplayer5");
        let state =
            std::fs::read(root.join("Local State")).map_err(|_| StoreError::NoLocalLogin)?;
        let state: serde_json::Value =
            serde_json::from_slice(&state).map_err(|_| StoreError::NoLocalLogin)?;
        let encoded = state
            .pointer("/os_crypt/encrypted_key")
            .and_then(|value| value.as_str())
            .ok_or(StoreError::NoLocalLogin)?;
        let encrypted = STANDARD
            .decode(encoded)
            .map_err(|_| StoreError::NoLocalLogin)?;
        let protected = encrypted
            .strip_prefix(b"DPAPI")
            .ok_or(StoreError::NoLocalLogin)?;
        let key = Zeroizing::new(dpapi_unprotect(protected).ok_or(StoreError::NoLocalLogin)?);
        let encrypted_auth = Zeroizing::new(
            std::fs::read(root.join("authAccessTokenData.enc"))
                .map_err(|_| StoreError::NoLocalLogin)?,
        );
        if encrypted_auth.len() < 31 || &encrypted_auth[..3] != b"v10" {
            return Err(StoreError::NoLocalLogin);
        }
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| StoreError::NoLocalLogin)?;
        let plaintext = Zeroizing::new(
            cipher
                .decrypt(
                    Nonce::from_slice(&encrypted_auth[3..15]),
                    &encrypted_auth[15..],
                )
                .map_err(|_| StoreError::NoLocalLogin)?,
        );
        let auth: AuthState =
            serde_json::from_slice(&plaintext).map_err(|_| StoreError::NoLocalLogin)?;
        if auth.access_token.is_empty() {
            return Err(StoreError::NoLocalLogin);
        }
        Ok(auth)
    }
}

#[cfg(windows)]
fn dpapi_unprotect(input: &[u8]) -> Option<Vec<u8>> {
    let mut source = input.to_vec();
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: source.len() as u32,
        pbData: source.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &input_blob,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
    let result = bytes.to_vec();
    bytes.fill(0);
    unsafe {
        LocalFree(output.pbData as _);
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_errors_are_fixed_and_secret_free() {
        assert_eq!(
            map_status(reqwest::StatusCode::UNAUTHORIZED).to_string(),
            "Session expired; sign in again"
        );
        assert_eq!(
            map_status(reqwest::StatusCode::FORBIDDEN).code(),
            "access_denied"
        );
    }

    #[tokio::test]
    async fn web_login_without_dmm_session_is_rejected_before_secret_storage() {
        struct UnusedSecrets;
        impl SecretStore for UnusedSecrets {
            fn load(&self, _: &str) -> Result<Option<StoredLogin>, StoreError> {
                Err(StoreError::SecretStore)
            }
            fn save(&self, _: &str, _: &StoredLogin) -> Result<(), StoreError> {
                Err(StoreError::SecretStore)
            }
            fn clear(&self, _: &str) -> Result<(), StoreError> {
                Err(StoreError::SecretStore)
            }
        }
        let (_sender, mut codes) = mpsc::channel(1);
        let response = Provider
            .login(
                LoginRequest::Web {
                    cookies: vec![shinki_emu_store::WebCookie {
                        name: "login_secure_id".into(),
                        value: "value".into(),
                        domain: "dmm.com".into(),
                        host_only: false,
                        path: "/".into(),
                        secure: true,
                        http_only: true,
                    }],
                },
                &UnusedSecrets,
                &mut codes,
                Arc::new(|_| {}),
                CancelToken::new(),
            )
            .await;
        assert!(matches!(response, Err(StoreError::WebLoginIncomplete)));
    }

    #[test]
    fn browser_callback_only_accepts_dgp_success_origin() {
        let success =
            Url::parse("https://webdgp-gameplayer.games.dmm.com/login/success?code=example")
                .unwrap();
        let attacker = Url::parse("https://example.com/login/success?code=example").unwrap();
        assert_eq!(callback_code(&success).as_deref(), Some("example"));
        assert_eq!(callback_code(&attacker), None);
    }
}
