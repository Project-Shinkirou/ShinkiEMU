use super::*;
use serde::{Deserialize, Serialize};
use shinki_emu_manager_ui_slint::{
    ChoiceViewModel, SteamAction, SteamDepotViewModel, SteamGameViewModel, SteamLoadState,
    SteamLoginState, SteamTransferState, SteamTransferViewModel, SteamViewModel,
};
use shinki_emu_steam::{self as provider, SecretStore};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use tokio::sync::{mpsc as async_mpsc, watch};

const SECRET_REFERENCE: &str = "steam.login";

struct SteamSecrets {
    /// Presence only: neither account names nor tokens leave the secret store.
    marker: PathBuf,
}
impl SecretStore for SteamSecrets {
    fn load(&self) -> Result<Option<provider::StoredLogin>, provider::Error> {
        match self.marker.try_exists()? {
            false => Ok(None),
            true => {
                let store = ManagerSecretStore::open().map_err(|_| provider::Error::SecretStore)?;
                let secret = zeroize::Zeroizing::new(
                    store
                        .resolve(SECRET_REFERENCE)
                        .map_err(|_| provider::Error::SecretStore)?,
                );
                serde_json::from_str(&secret)
                    .map(Some)
                    .map_err(|_| provider::Error::SecretStore)
            }
        }
    }
    fn save(&self, login: &provider::StoredLogin) -> Result<(), provider::Error> {
        let encoded = zeroize::Zeroizing::new(
            serde_json::to_string(login).map_err(|_| provider::Error::SecretStore)?,
        );
        let store = ManagerSecretStore::open().map_err(|_| provider::Error::SecretStore)?;
        store
            .store(SECRET_REFERENCE, &encoded)
            .map_err(|_| provider::Error::SecretStore)?;
        if let Err(error) = std::fs::write(&self.marker, []) {
            store
                .delete(SECRET_REFERENCE)
                .map_err(|_| provider::Error::SecretStore)?;
            return Err(error.into());
        }
        Ok(())
    }
    fn clear(&self) -> Result<(), provider::Error> {
        let existed = self.marker.try_exists()?;
        let store = ManagerSecretStore::open().map_err(|_| provider::Error::SecretStore)?;
        match store.delete(SECRET_REFERENCE) {
            Ok(()) => {}
            Err(_) if !existed => return Ok(()),
            Err(_) => return Err(provider::Error::SecretStore),
        }
        if existed {
            std::fs::remove_file(&self.marker)?;
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Installation {
    app_id: u32,
    branch: String,
    depots: Vec<u32>,
    directory: PathBuf,
}

struct CompletedInstallation {
    name: String,
    directory: PathBuf,
}

struct Shared {
    view: Mutex<SteamViewModel>,
    changed: AtomicBool,
    busy: AtomicBool,
    wake: Mutex<Option<HostWake>>,
}
impl Shared {
    fn update(&self, update: impl FnOnce(&mut SteamViewModel)) {
        update(&mut self.view.lock().unwrap_or_else(|e| e.into_inner()));
        self.changed.store(true, Ordering::Release);
        if let Some(wake) = self.wake.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            wake();
        }
    }
}

enum SteamCommand {
    Action(SteamAction),
    Disconnect,
}

pub(super) struct SteamRuntime {
    shared: Arc<Shared>,
    commands: async_mpsc::Sender<SteamCommand>,
    codes: async_mpsc::Sender<String>,
    cancel: watch::Sender<bool>,
    completed: mpsc::Receiver<CompletedInstallation>,
    pending: Option<Worker>,
    imports: VecDeque<PathBuf>,
}
struct Worker {
    shared: Arc<Shared>,
    commands: async_mpsc::Receiver<SteamCommand>,
    codes: async_mpsc::Receiver<String>,
    cancel: watch::Receiver<bool>,
    completed: mpsc::SyncSender<CompletedInstallation>,
    session: Option<provider::Session>,
    games: Vec<provider::Game>,
    installations: Vec<Installation>,
    path: PathBuf,
    secrets: SteamSecrets,
}

impl SteamRuntime {
    pub(super) fn new(data_dir: &Path) -> Self {
        let shared = Arc::new(Shared {
            view: Mutex::new(SteamViewModel {
                can_import_local: cfg!(any(
                    target_os = "windows",
                    target_os = "linux",
                    target_os = "macos"
                )),
                selected_branch: "public".into(),
                ..Default::default()
            }),
            changed: AtomicBool::new(false),
            busy: AtomicBool::new(false),
            wake: Mutex::new(None),
        });
        let (commands, receiver) = async_mpsc::channel(8);
        let (codes, code_receiver) = async_mpsc::channel(2);
        let (cancel, cancel_receiver) = watch::channel(false);
        let (completed, completion_receiver) = mpsc::sync_channel(8);
        let worker = Worker {
            shared: shared.clone(),
            commands: receiver,
            codes: code_receiver,
            cancel: cancel_receiver,
            completed,
            session: None,
            games: Vec::new(),
            installations: Vec::new(),
            path: data_dir.join("steam-installations.json"),
            secrets: SteamSecrets {
                marker: data_dir.join("steam-login-present"),
            },
        };
        Self {
            shared,
            commands,
            codes,
            cancel,
            completed: completion_receiver,
            pending: Some(worker),
            imports: VecDeque::new(),
        }
    }
    pub(super) fn view(&self) -> SteamViewModel {
        let mut view = self
            .shared
            .view
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let query = view.query.to_lowercase();
        view.games.retain(|game| {
            game.name.to_lowercase().contains(&query) || game.app_id.to_string().contains(&query)
        });
        view
    }
    pub(super) fn cancel(&self) {
        if self.shared.busy.load(Ordering::Acquire) {
            self.cancel.send_replace(true);
            self.shared
                .update(|v| v.transfer.state = SteamTransferState::Cancelling);
        }
    }
    pub(super) fn suspend(&self) -> Result<(), String> {
        self.cancel();
        if self.pending.is_none() {
            self.commands.try_send(SteamCommand::Disconnect)
                .map_err(|_| "无法停止 Steam 连接，请重新打开管理器 / Cannot stop the Steam connection; restart Manager".to_owned())?;
        }
        Ok(())
    }
    pub(super) fn set_wake(&self, wake: HostWake) {
        *self.shared.wake.lock().unwrap_or_else(|e| e.into_inner()) = Some(wake);
    }
    pub(super) fn changed(&self) -> bool {
        self.shared.changed.swap(false, Ordering::AcqRel)
    }
    fn completed(&self) -> Option<CompletedInstallation> {
        self.completed.try_recv().ok()
    }
    pub(super) fn dispatch(&mut self, action: SteamAction) -> Result<(), String> {
        if let SteamAction::SubmitGuard { code } = action {
            return self
                .codes
                .try_send(code)
                .map_err(|_| "请等待验证码提交完成 / Wait for the current code submission".into());
        }
        if matches!(action, SteamAction::Cancel) {
            self.cancel();
            return Ok(());
        }
        if matches!(action, SteamAction::Logout) {
            self.cancel.send_replace(true);
        } else if let SteamAction::Search { query } = action {
            self.shared.update(|v| v.query = query);
            return Ok(());
        } else if self.shared.busy.swap(true, Ordering::AcqRel) {
            return Err("请等待当前 Steam 操作完成，或先取消 / Wait for the current Steam operation or cancel it".into());
        } else {
            self.cancel.send_replace(false);
        }
        if let Some(worker) = self.pending.take() {
            let shared = worker.shared.clone();
            std::thread::Builder::new()
                .name("steam-provider".into())
                .spawn(move || {
                    match tokio::runtime::Builder::new_multi_thread()
                        .worker_threads(2)
                        .enable_all()
                        .build()
                    {
                        Ok(runtime) => runtime.block_on(worker.run()),
                        Err(error) => shared.update(|v| {
                            v.error = error.to_string();
                            v.login_state = SteamLoginState::Error;
                        }),
                    }
                })
                .map_err(|e| e.to_string())?;
        }
        self.commands
            .try_send(SteamCommand::Action(action))
            .map_err(|_| {
                self.shared.busy.store(false, Ordering::Release);
                "Steam 工作线程不可用，请重新打开管理器 / Steam worker unavailable; restart Manager"
                    .into()
            })
    }
}
impl Drop for SteamRuntime {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
    }
}

impl Worker {
    async fn run(mut self) {
        match std::fs::read(&self.path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(installs) => self.installations = installs,
                Err(_) => self.shared.update(|v| v.error = "Steam 安装记录损坏，请重新选择安装目录 / Installation records are damaged; select the installation folder again".into()),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(error) => self.shared.update(|v| v.error = error.to_string()),
        }
        while let Some(command) = self.commands.recv().await {
            let action = match command {
                SteamCommand::Action(action) => action,
                SteamCommand::Disconnect => {
                    self.session = None;
                    self.shared.update(|v| {
                        v.login_state = SteamLoginState::SignedOut;
                        v.qr_url.clear();
                        v.branch_password.clear();
                    });
                    self.shared.busy.store(false, Ordering::Release);
                    continue;
                }
            };
            let logout = matches!(action, SteamAction::Logout);
            let auth = matches!(
                action,
                SteamAction::Open
                    | SteamAction::Login { .. }
                    | SteamAction::QrLogin
                    | SteamAction::ImportLocal
            );
            let download = matches!(action, SteamAction::Download | SteamAction::Verify);
            let mut cancel = self.cancel.clone();
            let result = if logout {
                self.execute(action).await
            } else if *cancel.borrow() {
                Ok(())
            } else if download {
                // The provider drains every outstanding chunk write on cancel.
                self.execute(action).await
            } else {
                tokio::select! {
                    result = self.execute(action) => result,
                    // Dispatch resets this watch to false for every new action.
                    // A version change alone does not mean cancellation.
                    _ = cancel.wait_for(|cancelled| *cancelled) => Ok(()),
                }
            };
            if let Err(error) = result {
                let expired = matches!(error, provider::Error::SessionExpired);
                if expired {
                    self.session = None;
                }
                self.shared.update(|v| {
                    v.error = error.to_string();
                    if (auth || expired || logout) && self.session.is_none() {
                        v.login_state = SteamLoginState::Error;
                    }
                    if v.library_state == SteamLoadState::Loading {
                        v.library_state = SteamLoadState::Error;
                    }
                    if download {
                        v.transfer.state = SteamTransferState::Error;
                        v.transfer.error = error.to_string();
                    }
                });
            } else if *self.cancel.borrow() && !logout {
                if auth {
                    self.session = None;
                }
                self.shared.update(|v| {
                    if auth {
                        v.login_state = SteamLoginState::SignedOut;
                        v.qr_url.clear();
                    }
                    v.transfer.state = SteamTransferState::Cancelled;
                    if v.library_state == SteamLoadState::Loading {
                        v.library_state = SteamLoadState::Idle;
                    }
                });
            }
            self.shared.busy.store(false, Ordering::Release);
            self.shared.update(|_| {});
        }
    }

    async fn authenticate(&mut self, method: provider::LoginMethod) -> Result<(), provider::Error> {
        self.session = None;
        self.games.clear();
        self.shared.update(|v| {
            v.games.clear();
            v.depots.clear();
            v.selected_app_id = 0;
            v.error.clear();
            v.qr_url.clear();
        });
        while self.codes.try_recv().is_ok() {}
        self.session = Some(
            provider::login(method, &self.secrets, &mut self.codes, |update| {
                self.shared.update(|v| apply_login(v, update));
            })
            .await?,
        );
        self.refresh().await
    }

    async fn refresh(&mut self) -> Result<(), provider::Error> {
        self.shared.update(|v| {
            v.library_state = SteamLoadState::Loading;
            v.error.clear();
        });
        self.games = self
            .session
            .as_ref()
            .ok_or(provider::Error::SessionExpired)?
            .owned_games()
            .await?;
        let games = self
            .games
            .iter()
            .map(|game| game_view(game, &self.installations))
            .collect();
        self.shared.update(|v| {
            v.games = games;
            v.library_state = SteamLoadState::Ready;
        });
        Ok(())
    }

    async fn execute(&mut self, action: SteamAction) -> Result<(), provider::Error> {
        match action {
            SteamAction::Open => {
                if self.session.is_none() {
                    if let Some(login) = self.secrets.load()? {
                        self.authenticate(provider::LoginMethod::Token(login))
                            .await?;
                    }
                }
            }
            SteamAction::Login { account, password } => {
                self.authenticate(provider::LoginMethod::Credentials {
                    account,
                    password: zeroize::Zeroizing::new(password),
                })
                .await?
            }
            SteamAction::QrLogin => self.authenticate(provider::LoginMethod::Qr).await?,
            SteamAction::ImportLocal => {
                self.authenticate(provider::LoginMethod::Token(provider::import_local()?))
                    .await?
            }
            SteamAction::Logout => {
                self.session = None;
                self.games.clear();
                self.shared.update(|v| {
                    *v = SteamViewModel {
                        can_import_local: v.can_import_local,
                        library_root: v.library_root.clone(),
                        selected_branch: "public".into(),
                        ..Default::default()
                    };
                });
                self.secrets.clear()?;
            }
            SteamAction::RefreshLibrary => self.refresh().await?,
            SteamAction::SelectGame { app_id } => {
                self.shared.update(|v| {
                    v.library_state = SteamLoadState::Loading;
                    v.error.clear();
                });
                let game = self
                    .session
                    .as_ref()
                    .ok_or(provider::Error::SessionExpired)?
                    .game(app_id)
                    .await?;
                let installed = self.installations.iter().find(|i| i.app_id == app_id);
                self.shared.update(|v| {
                    v.selected_app_id = app_id;
                    v.branches = game
                        .branches
                        .iter()
                        .map(|b| ChoiceViewModel {
                            id: b.name.clone(),
                            label: b.name.clone(),
                        })
                        .collect();
                    v.selected_branch =
                        installed.map_or_else(|| "public".into(), |i| i.branch.clone());
                    v.branch_password.clear();
                    v.depots = game
                        .depots
                        .iter()
                        .map(|d| SteamDepotViewModel {
                            depot_id: d.depot_id,
                            name: d.name.clone(),
                            platform: d.platform.clone(),
                            language: d.language.clone(),
                            size_bytes: d.sizes.get(&v.selected_branch).copied().unwrap_or(0),
                            selected: installed.map_or_else(
                                || {
                                    (d.platform.is_empty()
                                        || d.platform.split(',').any(|p| p == "windows"))
                                        && (d.language.is_empty() || d.language == "english")
                                },
                                |i| i.depots.contains(&d.depot_id),
                            ),
                        })
                        .collect();
                    if let Some(installation) = installed {
                        v.library_root = installation
                            .directory
                            .parent()
                            .map_or_else(String::new, |p| p.to_string_lossy().into_owned());
                    }
                    v.library_state = SteamLoadState::Ready;
                });
            }
            SteamAction::SelectDepot { depot_id, selected } => self.shared.update(|v| {
                if let Some(depot) = v.depots.iter_mut().find(|d| d.depot_id == depot_id) {
                    depot.selected = selected;
                }
            }),
            SteamAction::SelectBranch { branch, password } => {
                let selected = self
                    .shared
                    .view
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .selected_app_id;
                let game = self.games.iter().find(|game| game.app_id == selected);
                self.shared.update(|v| {
                    for depot in &mut v.depots {
                        depot.size_bytes = game
                            .and_then(|game| {
                                game.depots.iter().find(|d| d.depot_id == depot.depot_id)
                            })
                            .and_then(|depot| depot.sizes.get(&branch))
                            .copied()
                            .unwrap_or(0);
                    }
                    v.selected_branch = branch;
                    v.branch_password = password;
                });
            }
            SteamAction::SetLibraryRoot { path } => self.shared.update(|v| v.library_root = path),
            SteamAction::Download | SteamAction::Verify => {
                self.download(matches!(action, SteamAction::Verify)).await?
            }
            SteamAction::CheckUpdates => {
                let session = self
                    .session
                    .as_ref()
                    .ok_or(provider::Error::SessionExpired)?;
                self.shared
                    .update(|v| v.library_state = SteamLoadState::Loading);
                for installed in &self.installations {
                    let mut game = session.game(installed.app_id).await?;
                    let view = self
                        .shared
                        .view
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone();
                    if view.selected_app_id == installed.app_id
                        && view.selected_branch == installed.branch
                    {
                        let password = zeroize::Zeroizing::new(view.branch_password);
                        session
                            .unlock_branch(&mut game, &installed.branch, &password)
                            .await?;
                    }
                    if installed.depots.iter().any(|id| {
                        game.depots
                            .iter()
                            .find(|d| d.depot_id == *id)
                            .is_none_or(|depot| !depot.manifests.contains_key(&installed.branch))
                    }) {
                        return Err(provider::Error::AccessDenied);
                    }
                    let update = provider::has_update(
                        &installed.directory,
                        &game,
                        &installed.depots,
                        &installed.branch,
                    );
                    self.shared.update(|v| {
                        if let Some(view) =
                            v.games.iter_mut().find(|g| g.app_id == installed.app_id)
                        {
                            view.update_available = update;
                        }
                    });
                }
                self.shared
                    .update(|v| v.library_state = SteamLoadState::Ready);
            }
            SteamAction::Search { .. }
            | SteamAction::SubmitGuard { .. }
            | SteamAction::Cancel
            | SteamAction::ChooseLibraryRoot => {}
        }
        Ok(())
    }

    async fn download(&mut self, verify: bool) -> Result<(), provider::Error> {
        let view = self
            .shared
            .view
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if view.library_root.trim().is_empty() {
            return Err(provider::Error::LibraryRootRequired);
        }
        let session = self
            .session
            .as_ref()
            .ok_or(provider::Error::SessionExpired)?;
        if view.selected_app_id == 0 {
            return Err(provider::Error::GameRequired);
        }
        let password = zeroize::Zeroizing::new(view.branch_password);
        let prepare = async {
            let mut game = session.game(view.selected_app_id).await?;
            session
                .unlock_branch(&mut game, &view.selected_branch, &password)
                .await?;
            Ok::<_, provider::Error>(game)
        };
        let mut cancel = self.cancel.clone();
        let game = tokio::select! {
            result = prepare => result?,
            _ = cancel.changed() => return Ok(()),
        };
        self.shared.update(|v| {
            v.branch_password.clear();
            v.error.clear();
            v.transfer = SteamTransferViewModel {
                app_id: game.app_id,
                state: if verify {
                    SteamTransferState::Verifying
                } else {
                    SteamTransferState::Preparing
                },
                ..Default::default()
            };
        });
        let directory = self
            .installations
            .iter()
            .find(|i| {
                i.app_id == game.app_id
                    && i.directory.parent() == Some(Path::new(&view.library_root))
            })
            .map_or_else(
                || Path::new(&view.library_root).join(format!("steam-{}", game.app_id)),
                |i| i.directory.clone(),
            );
        let plan = provider::DownloadPlan {
            game,
            branch: view.selected_branch,
            depot_ids: view
                .depots
                .iter()
                .filter(|d| d.selected)
                .map(|d| d.depot_id)
                .collect(),
            install_dir: directory.clone(),
            verify: true,
        };
        let result = session
            .download(&plan, self.cancel.clone(), |progress| {
                self.shared.update(|v| {
                    if !verify {
                        v.transfer.state = SteamTransferState::Downloading;
                    }
                    v.transfer.bytes_done = progress.bytes_done;
                    v.transfer.bytes_total = progress.bytes_total;
                    v.transfer.files_done = progress.files_done;
                    v.transfer.files_total = progress.files_total;
                    v.transfer.speed_bytes_per_second = progress.speed_bytes_per_second;
                    v.transfer.eta_seconds = progress.eta_seconds;
                    v.transfer.progress = (progress.bytes_total > 0)
                        .then(|| progress.bytes_done as f32 / progress.bytes_total as f32);
                });
            })
            .await?;
        if result.cancelled {
            self.shared
                .update(|v| v.transfer.state = SteamTransferState::Cancelled);
            return Ok(());
        }
        let mut depots = plan.depot_ids;
        if let Some(previous) = self
            .installations
            .iter()
            .find(|i| i.app_id == plan.game.app_id && i.directory == directory)
        {
            for id in &previous.depots {
                if !depots.contains(id) {
                    depots.push(*id);
                }
            }
        }
        self.installations
            .retain(|i| i.app_id != plan.game.app_id || i.directory != directory);
        self.installations.push(Installation {
            app_id: plan.game.app_id,
            branch: plan.branch,
            depots,
            directory: directory.clone(),
        });
        let encoded =
            serde_json::to_vec(&self.installations).map_err(|_| provider::Error::Metadata)?;
        // Persist metadata only. Credentials are stored exclusively by SteamSecrets.
        std::fs::write(&self.path, encoded)?;
        self.completed
            .send(CompletedInstallation {
                name: plan.game.name,
                directory,
            })
            .map_err(|_| provider::Error::SessionExpired)?;
        self.shared.update(|v| {
            v.transfer.state = SteamTransferState::Complete;
            v.transfer.progress = Some(1.0);
            v.transfer.status = "下载完成，正在识别核心 / Download complete, probing cores".into();
            if let Some(game) = v.games.iter_mut().find(|g| g.app_id == plan.game.app_id) {
                game.installed = true;
                game.update_available = false;
            }
        });
        Ok(())
    }
}

fn apply_login(view: &mut SteamViewModel, update: provider::LoginUpdate) {
    view.error.clear();
    view.login_state = match update {
        provider::LoginUpdate::Connecting => SteamLoginState::Connecting,
        provider::LoginUpdate::EmailCode => SteamLoginState::EmailCode,
        provider::LoginUpdate::MobileCode => SteamLoginState::MobileCode,
        provider::LoginUpdate::MobileConfirmation => SteamLoginState::MobileConfirmation,
        provider::LoginUpdate::QrCode(url) => {
            view.qr_url = url;
            SteamLoginState::QrCode
        }
        provider::LoginUpdate::InvalidCode => {
            view.error = provider::Error::InvalidGuardCode.to_string();
            view.login_state
        }
        provider::LoginUpdate::SignedIn(account) => {
            view.account_name = account;
            view.qr_url.clear();
            SteamLoginState::SignedIn
        }
    };
}
fn game_view(game: &provider::Game, installed: &[Installation]) -> SteamGameViewModel {
    SteamGameViewModel {
        app_id: game.app_id,
        name: game.name.clone(),
        platforms: game.platforms.clone(),
        languages: game.languages.clone(),
        size_bytes: game
            .depots
            .iter()
            .map(|d| d.sizes.get("public").copied().unwrap_or(0))
            .sum(),
        installed: installed.iter().any(|i| i.app_id == game.app_id),
        update_available: installed
            .iter()
            .filter(|i| i.app_id == game.app_id)
            .any(|i| provider::has_update(&i.directory, game, &i.depots, &i.branch)),
    }
}

impl ShinkiEmuManagerController {
    pub(super) fn steam_action(&mut self, action: SteamAction) -> Result<ManagerViewModel, String> {
        if !self.network_allowed
            && !matches!(
                action,
                SteamAction::Logout
                    | SteamAction::Cancel
                    | SteamAction::Search { .. }
                    | SteamAction::SetLibraryRoot { .. }
                    | SteamAction::SelectDepot { .. }
                    | SteamAction::SelectBranch { .. }
            )
        {
            return Err("请先在设置中允许网络访问 / Allow network access in settings first".into());
        }
        self.steam.dispatch(action)?;
        self.model()
    }
    pub(super) fn poll_steam(&mut self) -> Result<bool, String> {
        let mut changed = self.steam.changed();
        while let Some(completed) = self.steam.completed() {
            let path = std::fs::canonicalize(completed.directory).map_err(|e| e.to_string())?;
            let id = path_id(&path);
            if self.library.game(&id).map_err(|e| e.to_string())?.is_none() {
                self.library
                    .add_game(&GameRecord {
                        game_id: id.clone(),
                        title: completed.name,
                        user_title: None,
                        location: path.to_str().ok_or("SHINKI_EMU_GAME_PATH_UTF8")?.into(),
                        family_id: None,
                        content_fingerprint: None,
                        added_at_unix_ms: unix_time_ms()?,
                    })
                    .map_err(|e| e.to_string())?;
            }
            self.selected_case_id = Some(id);
            self.steam.imports.push_back(path);
            changed = true;
        }
        if self.active.is_none() && !self.startup_busy() && self.patch_job.is_none() {
            if let Some(path) = self.steam.imports.front().cloned() {
                self.add_game_directory(&path)?;
                self.steam.imports.pop_front();
                changed = true;
            }
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guard_retry_preserves_challenge_and_success_clears_qr() {
        let mut view = SteamViewModel::default();
        apply_login(&mut view, provider::LoginUpdate::Connecting);
        assert_eq!(view.login_state, SteamLoginState::Connecting);
        apply_login(&mut view, provider::LoginUpdate::EmailCode);
        apply_login(&mut view, provider::LoginUpdate::InvalidCode);
        assert_eq!(view.login_state, SteamLoginState::EmailCode);
        assert!(!view.error.is_empty());
        apply_login(
            &mut view,
            provider::LoginUpdate::QrCode("https://s.team/q/test".into()),
        );
        assert!(!view.qr_url.is_empty());
        apply_login(&mut view, provider::LoginUpdate::SignedIn("account".into()));
        assert_eq!(view.login_state, SteamLoginState::SignedIn);
        assert!(view.qr_url.is_empty());
        assert!(view.error.is_empty());
    }

    #[test]
    fn completed_download_is_added_even_without_a_matching_core() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("downloaded");
        std::fs::create_dir(&directory).unwrap();
        let mut app = ShinkiEmuManagerController::open_library(
            root.path().join("manager"),
            FrameMailbox::new(),
            Vec::new(),
        )
        .unwrap();
        app.steam
            .pending
            .as_ref()
            .unwrap()
            .completed
            .send(CompletedInstallation {
                name: "Downloaded Steam game".into(),
                directory: directory.clone(),
            })
            .unwrap();
        assert!(app.poll_steam().unwrap());
        super::super::probe_state_tests::settle_background(&mut app);
        let id = path_id(&std::fs::canonicalize(directory).unwrap());
        let game = app.library.game(&id).unwrap().unwrap();
        assert_eq!(game.title, "Downloaded Steam game");
        assert!(game.family_id.is_none());
        assert!(app.steam.imports.is_empty());
    }

    #[test]
    fn search_filters_owned_games_without_starting_network_work() {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = SteamRuntime::new(root.path());
        runtime.shared.update(|v| {
            v.games = vec![
                SteamGameViewModel {
                    app_id: 480,
                    name: "Spacewar".into(),
                    ..Default::default()
                },
                SteamGameViewModel {
                    app_id: 20,
                    name: "Other".into(),
                    ..Default::default()
                },
            ]
        });
        runtime
            .dispatch(SteamAction::Search {
                query: "SPACE".into(),
            })
            .unwrap();
        assert_eq!(runtime.view().games.len(), 1);
        assert_eq!(runtime.view().games[0].app_id, 480);
        assert!(runtime.pending.is_some());
        runtime
            .dispatch(SteamAction::Search { query: "20".into() })
            .unwrap();
        assert_eq!(runtime.view().games[0].app_id, 20);
    }

    #[test]
    #[ignore = "contacts the metadata service and requests a real Steam QR challenge"]
    fn live_startup_network_clients_and_qr_challenge_wake_host() {
        super::super::initialize_network_crypto();
        let http_runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let metadata = shinki_emu_metadata::MetadataSyncClient::new(
            shinki_emu_metadata::DEFAULT_METADATA_BASE_URL,
            std::time::Duration::from_secs(20),
        ).unwrap();
        let synchronized = http_runtime.block_on(metadata.sync(
            None, &std::collections::BTreeMap::new(),
        )).expect("Manager metadata HTTPS must work after startup crypto initialization");
        match synchronized {
            shinki_emu_metadata::SyncOutcome::Updated { index, files, .. } => {
                assert!(!index.files.is_empty());
                assert_eq!(files.len(), index.files.len());
            }
            shinki_emu_metadata::SyncOutcome::Unchanged => panic!("no cached ETag was supplied"),
        }
        #[cfg(not(target_os = "android"))]
        let _bangumi = shinki_emu_metadata::BangumiProvider::new(
            shinki_emu_metadata::BangumiProviderConfig {
                access_token: None,
                timeout: std::time::Duration::from_secs(20),
            },
        ).expect("Manager's reqwest 0.12 client must initialize alongside reqwest 0.13");
        let _translation = shinki_emu_translation::TranslationClient::new(
            shinki_emu_translation::TranslationProfile {
                model: "startup-tls-check".into(),
                ..Default::default()
            },
            Arc::new(crate::platform_secret::ManagerSecretStore::open().unwrap()),
        ).expect("Manager translation HTTPS client must initialize without reading a credential");
        let root = tempfile::tempdir().unwrap();
        let mut runtime = SteamRuntime::new(root.path());
        let (wake, wakeups) = mpsc::channel();
        runtime.set_wake(Arc::new(move || {
            let _ = wake.send(());
        }));
        let started = std::time::Instant::now();
        runtime.dispatch(SteamAction::QrLogin).unwrap();
        let deadline = started + std::time::Duration::from_secs(20);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            wakeups.recv_timeout(remaining)
                .expect("Manager worker must wake the host with a QR challenge within 20 seconds");
            let view = runtime.view();
            assert_ne!(view.login_state, SteamLoginState::Error, "{}", view.error);
            if view.login_state == SteamLoginState::QrCode {
                assert!(view.qr_url.starts_with("https://s.team/q/"));
                println!("Manager worker QR challenge after {:?}: {}", started.elapsed(), view.qr_url);
                runtime.cancel();
                break;
            }
        }
    }
}
