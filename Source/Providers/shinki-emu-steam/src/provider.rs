use crate::{Error, Game, Session, auth, transfer};
use shinki_emu_store::*;
use std::path::Path;

pub struct Provider;
static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor {
    browser_login_url: None,
    id: "steam",
    display_name: "Steam",
    login_methods: &[
        LoginKind::Qr,
        LoginKind::Credentials,
        LoginKind::LocalImport,
    ],
    capabilities: Capabilities {
        branches: true,
        depots: true,
        verify: true,
    },
};
impl StoreProvider for Provider {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        &DESCRIPTOR
    }
    fn login<'a>(
        &'a self,
        request: LoginRequest,
        secrets: &'a dyn SecretStore,
        codes: &'a mut tokio::sync::mpsc::Receiver<String>,
        updates: LoginUpdates,
        cancel: CancelToken,
    ) -> BoxFuture<'a, Box<dyn StoreSession>> {
        Box::pin(async move {
            let method = match request {
                LoginRequest::Qr => auth::LoginMethod::Qr,
                LoginRequest::Credentials { account, password } => {
                    auth::LoginMethod::Credentials { account, password }
                }
                LoginRequest::LocalImport => auth::LoginMethod::Token(auth::import_local()?),
                LoginRequest::BrowserCookies { .. } => return Err(StoreError::UnsupportedLogin),
            };
            tokio::select! {
                session = auth::login(method, secrets, codes, move |update| updates(update)) => {
                    Ok(Box::new(session?) as Box<dyn StoreSession>)
                }
                _ = cancel.cancelled() => Err(StoreError::Cancelled),
            }
        })
    }
    fn restore<'a>(
        &'a self,
        secrets: &'a dyn SecretStore,
        cancel: CancelToken,
    ) -> BoxFuture<'a, Option<Box<dyn StoreSession>>> {
        Box::pin(async move {
            let Some(saved) = secrets.load("steam")? else {
                return Ok(None);
            };
            let (_, mut codes) = tokio::sync::mpsc::channel(1);
            tokio::select! {
                session = auth::login(auth::LoginMethod::Token(saved), secrets, &mut codes, |_| {}) =>
                    Ok(Some(Box::new(session?) as Box<dyn StoreSession>)),
                _ = cancel.cancelled() => Err(StoreError::Cancelled),
            }
        })
    }
}
fn app_id(id: &TitleId) -> Result<u32, StoreError> {
    id.0.parse().map_err(|_| StoreError::Metadata)
}
fn title(game: &Game) -> OwnedTitle {
    OwnedTitle {
        id: TitleId(game.app_id.to_string()),
        name: game.name.clone(),
        platforms: game.platforms.clone(),
        languages: game.languages.clone(),
        size_bytes: game
            .depots
            .iter()
            .map(|d| d.sizes.get("public").copied().unwrap_or(0))
            .sum(),
        unavailable_reason: None,
    }
}
impl Session {
    async fn owned_game(&self, id: &TitleId) -> Result<Game, StoreError> {
        let id = app_id(id)?;
        let mut owned = self.owned.lock().await;
        if owned.is_none() {
            *owned = Some(
                self.owned_games()
                    .await?
                    .into_iter()
                    .map(|g| g.app_id)
                    .collect(),
            );
        }
        if !owned.as_ref().is_some_and(|ids| ids.contains(&id)) {
            return Err(StoreError::AccessDenied);
        }
        drop(owned);
        Ok(self.game(id).await?)
    }
    async fn prepare(&self, id: &TitleId, options: &PlanOptions) -> Result<Game, StoreError> {
        let mut game = self.owned_game(id).await?;
        self.unlock_branch(&mut game, &options.branch, &options.branch_password)
            .await?;
        Ok(game)
    }
}
impl StoreSession for Session {
    fn account(&self) -> &str {
        &self.account_name
    }
    fn library(&self) -> BoxFuture<'_, Vec<OwnedTitle>> {
        Box::pin(async move {
            let games = self.owned_games().await?;
            *self.owned.lock().await = Some(games.iter().map(|g| g.app_id).collect());
            Ok(games.iter().map(title).collect())
        })
    }
    fn detail<'a>(&'a self, id: &'a TitleId) -> BoxFuture<'a, TitleDetail> {
        Box::pin(async move {
            let game = self.owned_game(id).await?;
            Ok(TitleDetail {
                title: title(&game),
                branches: game
                    .branches
                    .iter()
                    .map(|b| Branch {
                        id: b.name.clone(),
                        name: b.name.clone(),
                        password_required: b.password_required,
                    })
                    .collect(),
                depots: game
                    .depots
                    .iter()
                    .map(|d| Depot {
                        id: d.depot_id.to_string(),
                        name: d.name.clone(),
                        platform: d.platform.clone(),
                        language: d.language.clone(),
                        sizes: d.sizes.clone(),
                    })
                    .collect(),
            })
        })
    }
    fn plan<'a>(
        &'a self,
        id: &'a TitleId,
        mut options: PlanOptions,
    ) -> BoxFuture<'a, DownloadPlan> {
        Box::pin(async move {
            if options.branch.is_empty() {
                options.branch = "public".into();
            }
            let game = self.prepare(id, &options).await?;
            if options.depots.is_empty() {
                return Err(StoreError::DepotRequired);
            }
            let depot_ids = options
                .depots
                .iter()
                .map(|id| id.parse().map_err(|_| StoreError::Metadata))
                .collect::<Result<Vec<u32>, _>>()?;
            let result = DownloadPlan {
                title: title(&game),
                branch: options.branch.clone(),
                depots: options.depots,
                payload: Box::new(transfer::DownloadPlan {
                    game,
                    depot_ids,
                    branch: options.branch,
                    verify: options.verify,
                }),
            };
            Ok(result)
        })
    }
    fn download<'a>(
        &'a self,
        plan: &'a DownloadPlan,
        dest: &'a Path,
        _work_dir: &'a Path,
        progress: ProgressUpdates,
        cancel: CancelToken,
    ) -> BoxFuture<'a, TransferSummary> {
        Box::pin(async move {
            let payload = plan
                .payload
                .downcast_ref::<transfer::DownloadPlan>()
                .ok_or(StoreError::Metadata)?;
            Ok(
                Session::download(self, payload, dest, cancel.subscribe(), move |p| {
                    progress(p)
                })
                .await?,
            )
        })
    }
    fn has_update<'a>(
        &'a self,
        id: &'a TitleId,
        installed_dir: &'a Path,
        mut options: PlanOptions,
    ) -> BoxFuture<'a, UpdateState> {
        Box::pin(async move {
            if options.branch.is_empty() {
                options.branch = "public".into();
            }
            let game = self.prepare(id, &options).await?;
            let depots = options
                .depots
                .iter()
                .map(|id| id.parse().map_err(|_| StoreError::Metadata))
                .collect::<Result<Vec<u32>, _>>()?;
            if depots.iter().any(|id| {
                game.depots
                    .iter()
                    .find(|d| d.depot_id == *id)
                    .is_none_or(|d| !d.manifests.contains_key(&options.branch))
            }) {
                return Err(StoreError::AccessDenied);
            }
            Ok(
                if transfer::has_update(installed_dir, &game, &depots, &options.branch) {
                    UpdateState::Available
                } else {
                    UpdateState::UpToDate
                },
            )
        })
    }
    fn logout<'a>(&'a self, secrets: &'a dyn SecretStore) -> BoxFuture<'a, ()> {
        Box::pin(async move { secrets.clear("steam") })
    }
}
impl From<Error> for StoreError {
    fn from(error: Error) -> Self {
        match error {
            Error::Store(error) => error,
            Error::Network => Self::Network,
            Error::Timeout => Self::Timeout,
            Error::InvalidPassword => Self::InvalidPassword,
            Error::InvalidGuardCode => Self::InvalidGuardCode,
            Error::SessionExpired => Self::SessionExpired,
            Error::NoLocalLogin => Self::NoLocalLogin,
            #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
            Error::ImportUnsupported => Self::ImportUnsupported,
            Error::DepotRequired => Self::DepotRequired,
            Error::Metadata => Self::Metadata,
            Error::AccessDenied => Self::AccessDenied,
            Error::Integrity => Self::Integrity,
            Error::Io(_) => Self::Io,
            Error::TransferFailed => Self::TransferFailed,
        }
    }
}
