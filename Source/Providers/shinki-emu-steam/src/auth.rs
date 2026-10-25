use crate::{Error, Session};
use bytes::Bytes;
use prost::Message;
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use steamroom::{
    client::{Ready, SteamClient},
    connection::{CmServer, Protocol},
    generated::CMsgClientLicenseList,
    messages::{EMsg, header::PacketHeader},
    transport::{Transport, websocket::WebSocketTransport},
};
use steamroom_client::login::{
    ApprovedAuth, CredentialsLoginFlow, GuardType, LoginError, PreparedLoginBuilder,
};
use tokio::sync::mpsc;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub trait SecretStore: Send + Sync {
    fn load(&self) -> Result<Option<StoredLogin>, Error>;
    fn save(&self, login: &StoredLogin) -> Result<(), Error>;
    fn clear(&self) -> Result<(), Error>;
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct StoredLogin {
    pub account: String,
    pub refresh_token: String,
}

pub enum LoginMethod {
    Anonymous,
    Credentials {
        account: String,
        password: zeroize::Zeroizing<String>,
    },
    Qr,
    Token(StoredLogin),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginUpdate {
    Connecting,
    EmailCode,
    MobileCode,
    MobileConfirmation,
    QrCode(String),
    InvalidCode,
    SignedIn(String),
}

/// Small side-channel: steamroom 0.3 can discard unrelated messages, including
/// licenses bundled with the logon response. Capture only licenses and session
/// identity before its parser consumes the packet; never capture auth payloads.
#[derive(Default)]
pub(crate) struct WireState {
    pub licenses: Option<Vec<(u32, u64)>>,
    pub steam_id: Option<u64>,
    pub session_id: Option<i32>,
}

struct LicenseTransport {
    inner: WebSocketTransport,
    wire: Arc<Mutex<WireState>>,
}

impl Transport for LicenseTransport {
    fn send(
        &self,
        data: &[u8],
    ) -> Pin<Box<dyn Future<Output = Result<(), steamroom::Error>> + Send + '_>> {
        self.inner.send(data)
    }
    fn recv(&self) -> Pin<Box<dyn Future<Output = Result<Bytes, steamroom::Error>> + Send + '_>> {
        Box::pin(async move {
            let packet = self.inner.recv().await?;
            capture(&packet, &self.wire, 0);
            Ok(packet)
        })
    }
}

fn capture(packet: &[u8], wire: &Mutex<WireState>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(PacketHeader::Protobuf { header, body }) = PacketHeader::parse(packet) else {
        return;
    };
    if header.emsg == EMsg::MULTI {
        if let Ok(messages) = steamroom::client::multi::unpack_multi(&body) {
            for message in messages {
                capture(&message, wire, depth + 1);
            }
        }
    } else if header.emsg == EMsg::CLIENT_LICENSE_LIST {
        if let Ok(list) = CMsgClientLicenseList::decode(&*body)
            && list.eresult == Some(1)
        {
            wire.lock().unwrap_or_else(|e| e.into_inner()).licenses = Some(
                list.licenses
                    .into_iter()
                    .filter_map(|l| l.package_id.map(|id| (id, l.access_token.unwrap_or(0))))
                    .collect(),
            );
        }
    } else if header.emsg == EMsg::CLIENT_LOG_ON_RESPONSE
        && let Ok(header) = header.decode_header()
    {
        let mut state = wire.lock().unwrap_or_else(|e| e.into_inner());
        state.steam_id = header.steamid;
        state.session_id = header.client_sessionid;
    }
}

async fn connect() -> Result<(SteamClient<Ready>, Arc<Mutex<WireState>>), Error> {
    let servers = tokio::time::timeout(Duration::from_secs(30), CmServer::fetch())
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(Error::from)?;
    let mut last_error = Error::Network;
    for server in servers
        .iter()
        .filter(|s| s.protocol == Protocol::WebSocket)
        .take(5)
    {
        let transport = match tokio::time::timeout(
            Duration::from_secs(15),
            WebSocketTransport::connect(server),
        )
        .await
        {
            Ok(Ok(transport)) => transport,
            Ok(Err(error)) => {
                last_error = error.into();
                continue;
            }
            Err(_) => {
                last_error = Error::Timeout;
                continue;
            }
        };
        let wire = Arc::new(Mutex::new(WireState::default()));
        let (client, _) = SteamClient::connect_ws(LicenseTransport {
            inner: transport,
            wire: wire.clone(),
        })
        .await?;
        return Ok((client.prepare().await?, wire));
    }
    Err(last_error)
}

fn login_error(error: LoginError) -> Error {
    match error {
        LoginError::InvalidPassword => Error::InvalidPassword,
        LoginError::InvalidGuardCode => Error::InvalidGuardCode,
        LoginError::LogonFailed(_) => Error::SessionExpired,
        _ => Error::Network,
    }
}

/// Prefer the mobile authenticator when Steam offers both code mechanisms.
pub fn guard_kind(kinds: &[GuardType]) -> Result<GuardType, Error> {
    if kinds.contains(&GuardType::DeviceCode) {
        Ok(GuardType::DeviceCode)
    } else if kinds.contains(&GuardType::EmailCode) {
        Ok(GuardType::EmailCode)
    } else {
        Err(Error::Metadata)
    }
}

async fn finish(
    approved: ApprovedAuth,
    store: &dyn SecretStore,
) -> Result<(SteamClient<steamroom::client::LoggedIn>, String), Error> {
    let tokens = approved.tokens();
    let saved = StoredLogin {
        account: tokens.account_name.clone().ok_or(Error::Metadata)?,
        refresh_token: tokens.refresh_token.clone(),
    };
    let client = approved.finish().await.map_err(login_error)?;
    store.save(&saved)?;
    Ok((client, saved.account.clone()))
}

pub async fn login(
    method: LoginMethod,
    store: &dyn SecretStore,
    codes: &mut mpsc::Receiver<String>,
    notify: impl Fn(LoginUpdate),
) -> Result<Session, Error> {
    notify(LoginUpdate::Connecting);
    let (ready, wire) = connect().await?;
    let builder = PreparedLoginBuilder::new(ready).device_name("ShinkiEMU");
    let (client, account) = match method {
        LoginMethod::Anonymous => (
            builder.anonymous().login().await.map_err(login_error)?,
            String::new(),
        ),
        LoginMethod::Token(saved) => {
            let client = builder
                .with_refresh_token(saved.account.clone(), saved.refresh_token.clone())
                .login()
                .await
                .map_err(login_error)?;
            store.save(&saved)?;
            (client, saved.account.clone())
        }
        LoginMethod::Qr => {
            let flow = builder.with_qr().begin().await.map_err(login_error)?;
            notify(LoginUpdate::QrCode(flow.challenge_url().to_owned()));
            finish(flow.wait_for_scan().await.map_err(login_error)?, store).await?
        }
        LoginMethod::Credentials { account, password } => {
            let flow = builder
                .with_credentials(account, password.as_str())
                .begin()
                .await
                .map_err(login_error)?;
            match flow {
                CredentialsLoginFlow::Approved(approved) => finish(approved, store).await?,
                CredentialsLoginFlow::NeedsMobileConfirm(challenge) => {
                    notify(LoginUpdate::MobileConfirmation);
                    finish(
                        challenge
                            .wait_for_confirmation()
                            .await
                            .map_err(login_error)?,
                        store,
                    )
                    .await?
                }
                CredentialsLoginFlow::NeedsGuardCode(mut challenge) => {
                    let kind = guard_kind(challenge.allowed_kinds())?;
                    notify(if kind == GuardType::DeviceCode {
                        LoginUpdate::MobileCode
                    } else {
                        LoginUpdate::EmailCode
                    });
                    loop {
                        let code = zeroize::Zeroizing::new(
                            codes.recv().await.ok_or(Error::SessionExpired)?,
                        );
                        match challenge.submit_code(&code, kind).await {
                            Ok(approved) => break finish(approved, store).await?,
                            Err((retry, LoginError::InvalidGuardCode)) => {
                                challenge = retry;
                                notify(LoginUpdate::InvalidCode);
                            }
                            Err((_, error)) => return Err(login_error(error)),
                        }
                    }
                }
                _ => return Err(Error::Metadata),
            }
        }
    };
    notify(LoginUpdate::SignedIn(account.clone()));
    let heart = client.clone();
    let heartbeat = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(20));
        loop {
            interval.tick().await;
            if heart.send_heartbeat().await.is_err() {
                break;
            }
        }
    });
    Ok(Session {
        client,
        wire,
        heartbeat,
        account_name: account,
    })
}

pub fn import_local() -> Result<StoredLogin, Error> {
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    {
        let dir = steamroom_client::steam_creds::steam_dir().ok_or(Error::NoLocalLogin)?;
        let account =
            steamroom_client::steam_creds::detect_username(&dir).ok_or(Error::NoLocalLogin)?;
        let cached =
            steamroom_client::steam_creds::extract_token(&account).ok_or(Error::NoLocalLogin)?;
        Ok(StoredLogin {
            account: cached.account_name,
            refresh_token: cached.refresh_token,
        })
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Err(Error::ImportUnsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guard_challenge_uses_supported_mobile_or_email_mechanism() {
        assert_eq!(
            guard_kind(&[GuardType::EmailCode]).unwrap(),
            GuardType::EmailCode
        );
        assert_eq!(
            guard_kind(&[GuardType::EmailCode, GuardType::DeviceCode]).unwrap(),
            GuardType::DeviceCode
        );
        assert!(guard_kind(&[GuardType::DeviceConfirmation]).is_err());
    }
}
