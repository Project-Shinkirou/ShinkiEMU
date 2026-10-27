//! Opt-in live Steam QR acceptance: no account, scan, or saved credentials.
use shinki_emu_steam::Provider;
use shinki_emu_store::{
    CancelToken, LoginRequest, LoginUpdate, SecretStore, StoreError, StoreProvider, StoredLogin,
};
use std::{sync::Arc, time::Duration};

struct NoSecrets;
impl SecretStore for NoSecrets {
    fn load(&self, _: &str) -> Result<Option<StoredLogin>, StoreError> {
        panic!("QR challenge must not read saved credentials")
    }
    fn save(&self, _: &str, _: &StoredLogin) -> Result<(), StoreError> {
        panic!("Unapproved QR must not persist credentials")
    }
    fn clear(&self, _: &str) -> Result<(), StoreError> {
        panic!("QR challenge must not delete credentials")
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requests a real short-lived QR challenge from Valve; requires network"]
async fn live_qr_challenge_arrives_without_account() {
    let (_, mut codes) = tokio::sync::mpsc::channel(1);
    let (challenges, mut challenge) = tokio::sync::mpsc::unbounded_channel();
    let login = Provider.login(
        LoginRequest::Qr,
        &NoSecrets,
        &mut codes,
        Arc::new(move |update| {
            if let LoginUpdate::QrCode(url) = update {
                challenges.send(url).unwrap();
            }
        }),
        CancelToken::new(),
    );
    tokio::pin!(login);
    let url = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::select! {
            url = challenge.recv() => url.expect("QR challenge sender closed"),
            result = &mut login => match result {
                Ok(_) => panic!("QR login completed without mobile approval"),
                Err(error) => panic!("QR login failed before producing a challenge: {error}"),
            },
        }
    })
    .await
    .expect("Steam QR challenge must arrive within 20 seconds");
    assert!(
        !url.strip_prefix("https://s.team/q/")
            .expect("official Steam QR URL")
            .is_empty()
    );
}
