//! Steam downloads without a Steam installation or a GPL runtime dependency.
mod auth;
mod catalog;
#[cfg(test)]
mod live_tests;
mod transfer;

use catalog::Game;
use shinki_emu_store::StoreError;
mod provider;
pub use provider::Provider;
use std::future::Future;
use std::sync::{Arc, Mutex};
use steamroom::client::{LoggedIn, SteamClient};
struct Session {
    client: SteamClient<LoggedIn>,
    wire: Arc<Mutex<auth::WireState>>,
    heartbeat: tokio::task::JoinHandle<()>,
    owned: tokio::sync::Mutex<Option<std::collections::HashSet<u32>>>,
    pub account_name: String,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.heartbeat.abort();
    }
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("网络连接失败，请检查连接后重试 / Connection failed; check your network and retry")]
    Network,
    #[error("Steam 请求超时，请重试 / Steam request timed out; retry")]
    Timeout,
    #[error("账号或密码错误，请重新输入 / Incorrect account or password")]
    InvalidPassword,
    #[error("验证码错误，请重新输入 / Incorrect Steam Guard code")]
    InvalidGuardCode,
    #[error("登录已过期，请重新登录 / Session expired; sign in again")]
    SessionExpired,
    #[error(
        "未找到可导入的 Steam 客户端登录，请使用密码或扫码登录 / No importable Steam login; use password or QR login"
    )]
    NoLocalLogin,
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    #[error("此设备不支持导入 Steam 客户端登录 / Local Steam login import is desktop-only")]
    ImportUnsupported,
    #[error("请至少选择一个内容仓库 / Select at least one depot")]
    DepotRequired,
    #[error(
        "Steam 未返回所需内容，请刷新游戏信息 / Steam metadata is incomplete; refresh game details"
    )]
    Metadata,
    #[error(
        "没有此内容仓库的访问权限，请检查账号和分支密码 / Check depot ownership and branch password"
    )]
    AccessDenied,
    #[error(
        "内容校验失败，请重新下载或修复 / Content verification failed; download again or repair"
    )]
    Integrity,
    #[error("目录无法读写，请检查权限和剩余空间 / Check folder permissions and free space: {0}")]
    Io(#[from] std::io::Error),
    #[error("下载失败，请检查连接、剩余空间后重试 / Download failed; check network and free space")]
    TransferFailed,
}

impl From<steamroom::Error> for Error {
    fn from(error: steamroom::Error) -> Self {
        match error {
            steamroom::Error::Connection(steamroom::error::ConnectionError::DepotAccessDenied(
                _,
            )) => Self::AccessDenied,
            steamroom::Error::Connection(steamroom::error::ConnectionError::LogonFailed(_)) => {
                Self::SessionExpired
            }
            _ => Self::Network,
        }
    }
}

pub(crate) async fn timed<T>(
    future: impl Future<Output = Result<T, steamroom::Error>>,
) -> Result<T, Error> {
    tokio::time::timeout(std::time::Duration::from_secs(60), future)
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(Error::from)
}
