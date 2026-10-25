//! Opt-in live Steam QR acceptance: no account, scan, or saved credentials.
use shinki_emu_steam::{login, Error, LoginMethod, LoginUpdate, SecretStore, StoredLogin};
use std::time::Duration;

struct NoSecrets;
impl SecretStore for NoSecrets {
    fn load(&self) -> Result<Option<StoredLogin>, Error> {
        panic!("QR challenge creation must not read saved credentials")
    }
    fn save(&self, _: &StoredLogin) -> Result<(), Error> {
        panic!("unapproved QR challenge must not persist credentials")
    }
    fn clear(&self) -> Result<(), Error> {
        panic!("QR challenge creation must not delete credentials")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requests a real short-lived QR challenge from Valve; requires network"]
async fn live_qr_challenge_arrives_without_account() {
    let started = std::time::Instant::now();
    let (_, mut codes) = tokio::sync::mpsc::channel(1);
    let (challenges, mut challenge) = tokio::sync::mpsc::unbounded_channel();
    let login = login(LoginMethod::Qr, &NoSecrets, &mut codes, |update| {
        println!("login update after {:?}: {update:?}", started.elapsed());
        if let LoginUpdate::QrCode(url) = update {
            challenges.send(url).unwrap();
        }
    });
    tokio::pin!(login);
    let url = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::select! {
            url = challenge.recv() => url.expect("QR challenge sender closed"),
            result = &mut login => match result {
                Ok(_) => panic!("QR login completed without mobile approval"),
                Err(error) => panic!("QR login failed before producing a challenge: {error:?}"),
            },
        }
    })
    .await
    .expect("Steam QR challenge must arrive within 20 seconds");
    let challenge_path = url
        .strip_prefix("https://s.team/q/")
        .expect("Steam returned its public QR challenge URL");
    assert!(!challenge_path.is_empty());
    println!(
        "Live Steam QR challenge after {:?}: {url}",
        started.elapsed()
    );
    // Drop the pending login without scanning or authenticating an account.
}
