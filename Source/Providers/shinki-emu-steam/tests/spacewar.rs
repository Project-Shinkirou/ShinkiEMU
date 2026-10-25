//! Opt-in network acceptance; never requires or reads account credentials.
use shinki_emu_steam::{
    DownloadPlan, Error, LoginMethod, SecretStore, StoredLogin, has_update, login,
};
use std::sync::Mutex;
use steamroom::depot::{DepotId, DepotKey, ManifestId};
use steamroom_client::depot_config::DepotConfig;

struct NoSecrets;
impl SecretStore for NoSecrets {
    fn load(&self) -> Result<Option<StoredLogin>, Error> {
        Ok(None)
    }
    fn save(&self, _: &StoredLogin) -> Result<(), Error> {
        panic!("anonymous login must not persist credentials")
    }
    fn clear(&self) -> Result<(), Error> {
        panic!("anonymous test must not touch the secret store")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads real Spacewar content from Valve; requires network"]
async fn anonymous_spacewar_download_verify_repair_and_simulated_update() {
    let temp = tempfile::tempdir().unwrap();
    let (_, mut codes) = tokio::sync::mpsc::channel(1);
    let session = login(LoginMethod::Anonymous, &NoSecrets, &mut codes, |_| {})
        .await
        .expect("anonymous Steam login");
    let game = session.game(480).await.expect("Spacewar product info");
    let plan = DownloadPlan {
        game,
        depot_ids: vec![481],
        branch: "public".into(),
        install_dir: temp.path().to_owned(),
        verify: true,
    };
    let (_, cancel) = tokio::sync::watch::channel(false);
    let progress = Mutex::new(shinki_emu_steam::Progress::default());
    let downloaded = session
        .download(&plan, cancel.clone(), |value| {
            *progress.lock().unwrap() = value
        })
        .await
        .expect("Spacewar depot download");
    assert!(!downloaded.cancelled);
    assert!(downloaded.files_completed > 0);
    let progress = progress.into_inner().unwrap();
    assert!(progress.bytes_total > 0);
    assert_eq!(progress.bytes_done, progress.bytes_total);
    assert_eq!(progress.files_done, progress.files_total);
    assert!(!has_update(temp.path(), &plan.game, &[481], "public"));
    let verified = session
        .download(&plan, cancel.clone(), |_| {})
        .await
        .expect("verify Spacewar");
    assert_eq!(
        verified.bytes_downloaded, 0,
        "verified files should be reused locally"
    );
    let mut config = DepotConfig::load(temp.path());
    let (id, key) = config.get_installed(DepotId(481)).unwrap();
    let manifest_path = DepotConfig::config_dir(temp.path())
        .join("manifests")
        .join(format!("481_{}.zip", id.0));
    let mut manifest =
        steamroom_client::manifest::parse_cdn_manifest(&std::fs::read(manifest_path).unwrap())
            .unwrap();
    if manifest.filenames_encrypted {
        manifest.decrypt_filenames(&key).unwrap();
    }
    let file = manifest
        .files
        .iter()
        .find(|f| f.size > 0 && f.link_target.is_none() && !f.chunks.is_empty())
        .unwrap();
    let path = temp.path().join(file.normalized_path());
    let original = std::fs::read(&path).unwrap();
    let mut damaged = original.clone();
    damaged[0] ^= 0xff;
    std::fs::write(&path, damaged).unwrap();
    let repaired = session
        .download(&plan, cancel, |_| {})
        .await
        .expect("repair corrupted Spacewar file");
    assert!(repaired.bytes_downloaded > 0);
    assert_eq!(std::fs::read(path).unwrap(), original);
    config.set_installed(
        DepotId(481),
        ManifestId(id.0.wrapping_sub(1)),
        &DepotKey(key.0),
    );
    config.save(temp.path()).unwrap();
    assert!(
        has_update(temp.path(), &plan.game, &[481], "public"),
        "simulated older installed manifest must be detected"
    );
}
