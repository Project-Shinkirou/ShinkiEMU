//! Compile-in store contracts. Secrets and provider-specific plans remain in process.
use std::{any::Any, future::Future, path::Path, pin::Pin, sync::Arc};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StoreError>> + Send + 'a>>;
pub type LoginUpdates = Arc<dyn Fn(LoginUpdate) + Send + Sync>;
pub type ProgressUpdates = Arc<dyn Fn(Progress) + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TitleId(pub String);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginKind { Qr, Credentials, BrowserCookies, LocalImport }
#[derive(Clone, Copy, Debug, Default)]
pub struct Capabilities { pub branches: bool, pub depots: bool, pub verify: bool }
pub struct ProviderDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub login_methods: &'static [LoginKind],
    pub capabilities: Capabilities,
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct StoredLogin { pub account: String, pub secret: String }
pub trait SecretStore: Send + Sync {
    fn load(&self, provider: &str) -> Result<Option<StoredLogin>, StoreError>;
    fn save(&self, provider: &str, login: &StoredLogin) -> Result<(), StoreError>;
    fn clear(&self, provider: &str) -> Result<(), StoreError>;
}
// Deliberately no Debug: passwords, cookies and codes must not enter diagnostics.
pub enum LoginRequest {
    Qr,
    Credentials { account: String, password: Zeroizing<String> },
    BrowserCookies { cookies: Zeroizing<String> },
    LocalImport,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginUpdate {
    Connecting, EmailCode, MobileCode, MobileConfirmation, QrCode(String),
    Browser { url: String }, InvalidCode, SignedIn(String),
}

#[derive(Clone)]
pub struct CancelToken { sender: watch::Sender<bool> }
impl Default for CancelToken { fn default() -> Self { Self::new() } }
impl CancelToken {
    pub fn new() -> Self { Self { sender: watch::channel(false).0 } }
    pub fn cancel(&self) { self.sender.send_replace(true); }
    pub fn is_cancelled(&self) -> bool { *self.sender.borrow() }
    pub fn subscribe(&self) -> watch::Receiver<bool> { self.sender.subscribe() }
    pub async fn cancelled(&self) {
        let mut receiver = self.subscribe();
        let _ = receiver.wait_for(|value| *value).await;
    }
}

#[derive(Clone, Debug, Default)]
pub struct OwnedTitle {
    pub id: TitleId,
    pub name: String,
    pub platforms: Vec<String>,
    pub languages: Vec<String>,
    pub size_bytes: u64,
    /// Titles that cannot be downloaded remain visible with the store's reason.
    pub unavailable_reason: Option<String>,
}
impl Default for TitleId { fn default() -> Self { Self(String::new()) } }
#[derive(Clone, Debug, Default)]
pub struct Branch { pub id: String, pub name: String, pub password_required: bool }
#[derive(Clone, Debug, Default)]
pub struct Depot {
    pub id: String, pub name: String, pub platform: String, pub language: String,
    pub sizes: std::collections::BTreeMap<String, u64>,
}
#[derive(Clone, Debug, Default)]
pub struct TitleDetail { pub title: OwnedTitle, pub branches: Vec<Branch>, pub depots: Vec<Depot> }
#[derive(Default)]
pub struct PlanOptions {
    pub branch: String, pub depots: Vec<String>, pub branch_password: Zeroizing<String>,
    pub verify: bool,
}
/// Payload belongs to the issuing provider. It must not contain credentials or be serialized.
pub struct DownloadPlan {
    pub title: OwnedTitle, pub branch: String, pub depots: Vec<String>,
    pub payload: Box<dyn Any + Send + Sync>,
}
#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub bytes_done: u64, pub bytes_total: u64, pub files_done: u64, pub files_total: u64,
    pub speed_bytes_per_second: u64, pub eta_seconds: u64,
}
#[derive(Debug, Default)]
pub struct TransferSummary {
    pub files_completed: u64, pub files_skipped: u64, pub bytes_downloaded: u64, pub cancelled: bool,
}

/// Fixed messages only: never carry HTTP bodies, URLs, tokens or OS error text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("Connection failed; check your network and retry")] Network,
    #[error("Request timed out; retry")] Timeout,
    #[error("Incorrect account or password")] InvalidPassword,
    #[error("Incorrect verification code")] InvalidGuardCode,
    #[error("Session expired; sign in again")] SessionExpired,
    #[error("Unlock the platform secret store and retry")] SecretStore,
    #[error("No importable local client login")] NoLocalLogin,
    #[error("Local client import is unsupported on this device")] ImportUnsupported,
    #[error("Select a title first")] GameRequired,
    #[error("Select at least one content depot")] DepotRequired,
    #[error("Choose a writable library folder")] LibraryRootRequired,
    #[error("Store metadata is incomplete; refresh title details")] Metadata,
    #[error("Check ownership and branch password")] AccessDenied,
    #[error("Content verification failed; download again or repair")] Integrity,
    #[error("Check folder permissions and free space")] Io,
    #[error("Download failed; check network and free space")] TransferFailed,
    #[error("This title is not offered as a PC download")] Unavailable,
    #[error("This login method is not supported")] UnsupportedLogin,
    #[error("Operation cancelled")] Cancelled,
    #[error("Provider identifiers must be unique")] DuplicateProvider,
}
impl StoreError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Network => "network", Self::Timeout => "timeout", Self::InvalidPassword => "invalid_password",
            Self::InvalidGuardCode => "invalid_code", Self::SessionExpired => "session_expired",
            Self::SecretStore => "secret_store", Self::NoLocalLogin => "no_local_login",
            Self::ImportUnsupported => "import_unsupported", Self::GameRequired => "title_required",
            Self::DepotRequired => "depot_required", Self::LibraryRootRequired => "library_root_required",
            Self::Metadata => "metadata", Self::AccessDenied => "access_denied", Self::Integrity => "integrity",
            Self::Io => "io", Self::TransferFailed => "transfer_failed", Self::Unavailable => "unavailable",
            Self::UnsupportedLogin => "unsupported_login", Self::Cancelled => "cancelled",
            Self::DuplicateProvider => "duplicate_provider",
        }
    }
}
impl From<std::io::Error> for StoreError { fn from(_: std::io::Error) -> Self { Self::Io } }

pub trait StoreProvider: Send + Sync {
    fn descriptor(&self) -> &'static ProviderDescriptor;
    fn login<'a>(&'a self, request: LoginRequest, secrets: &'a dyn SecretStore,
        codes: &'a mut mpsc::Receiver<String>, updates: LoginUpdates, cancel: CancelToken)
        -> BoxFuture<'a, Box<dyn StoreSession>>;
    fn restore<'a>(&'a self, secrets: &'a dyn SecretStore, cancel: CancelToken)
        -> BoxFuture<'a, Option<Box<dyn StoreSession>>>;
}
pub trait StoreSession: Send + Sync {
    fn account(&self) -> &str;
    fn library(&self) -> BoxFuture<'_, Vec<OwnedTitle>>;
    fn detail<'a>(&'a self, id: &'a TitleId) -> BoxFuture<'a, TitleDetail>;
    fn plan<'a>(&'a self, id: &'a TitleId, options: PlanOptions) -> BoxFuture<'a, DownloadPlan>;
    /// dest is the final game directory; work_dir is Manager-owned .shinki/store/<provider>/.
    fn download<'a>(&'a self, plan: &'a DownloadPlan, dest: &'a Path, work_dir: &'a Path,
        progress: ProgressUpdates, cancel: CancelToken) -> BoxFuture<'a, TransferSummary>;
    fn has_update<'a>(&'a self, id: &'a TitleId, installed_dir: &'a Path,
        options: PlanOptions) -> BoxFuture<'a, bool>;
    fn logout<'a>(&'a self, secrets: &'a dyn SecretStore) -> BoxFuture<'a, ()>;
}
#[derive(Default)]
pub struct Registry { providers: Vec<Arc<dyn StoreProvider>> }
impl Registry {
    pub fn new() -> Self { Self::default() }
    pub fn register(&mut self, provider: impl StoreProvider + 'static) -> Result<(), StoreError> {
        let id = provider.descriptor().id;
        if self.get(id).is_some() { return Err(StoreError::DuplicateProvider); }
        self.providers.push(Arc::new(provider));
        Ok(())
    }
    pub fn providers(&self) -> &[Arc<dyn StoreProvider>] { &self.providers }
    pub fn get(&self, id: &str) -> Option<Arc<dyn StoreProvider>> {
        self.providers.iter().find(|p| p.descriptor().id == id).cloned()
    }
}
